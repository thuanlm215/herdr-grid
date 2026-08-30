use super::{plan, rebuild_plan, HerdrClient, Operation, Snapshot};
use crate::model::LayoutNode;
use fs2::FileExt;
use std::collections::HashMap;
use std::fs::OpenOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyProgress {
    Validating,
    Applying { current: usize, total: usize },
    Verifying,
    Recovering,
    Done,
}

pub struct ApplyGuard {
    _file: std::fs::File,
}
impl ApplyGuard {
    pub fn acquire() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join("herdr-grid.apply.lock");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.try_lock_exclusive()
            .map_err(|_| anyhow::anyhow!("another herdr-grid apply is running"))?;
        Ok(Self { _file: file })
    }
}
pub struct Transaction<'a, C: HerdrClient> {
    pub client: &'a C,
    pub snapshot: &'a Snapshot,
}
impl<C: HerdrClient> Transaction<'_, C> {
    pub async fn apply(&self, target: &LayoutNode) -> anyhow::Result<()> {
        self.apply_with_progress(target, &mut |_| {}).await
    }

    pub async fn apply_with_progress(
        &self,
        target: &LayoutNode,
        progress: &mut dyn FnMut(ApplyProgress),
    ) -> anyhow::Result<()> {
        progress(ApplyProgress::Validating);
        let _lock = ApplyGuard::acquire()?;
        let live = self.client.snapshot().await?;
        if live.workspace_id != self.snapshot.workspace_id
            || live.tab_id != self.snapshot.tab_id
            || live.revisions != self.snapshot.revisions
            || !equivalent(&live.tree, &self.snapshot.tree)
            || sorted_ids(&live.tree) != sorted_ids(&self.snapshot.tree)
        {
            anyhow::bail!("session changed since editor opened; no changes applied")
        };
        let p = plan(&self.snapshot.tree, target)?;
        if p.structural {
            return self.apply_structural(&p.operations, target, progress).await;
        };
        let mut expected = self.snapshot.tree.clone();
        let total = p.operations.len();
        for (index, op) in p.operations.into_iter().enumerate() {
            progress(ApplyProgress::Applying {
                current: index + 1,
                total,
            });
            let executed: anyhow::Result<()> = match &op {
                Operation::Swap { source, target } => self.client.swap(source, target).await,
                Operation::SetRatio { path, ratio } => {
                    self.client
                        .set_ratio(&self.snapshot.tab_id, path, *ratio)
                        .await
                }
                _ => unreachable!(),
            };
            match executed {
                Ok(()) => {}
                Err(cause) => {
                    progress(ApplyProgress::Recovering);
                    let rollback = self.recover_ambiguous_original().await;
                    return match rollback {
                        Ok(()) => Err(anyhow::anyhow!(
                            "apply step failed ({cause}); earlier steps were rolled back"
                        )),
                        Err(e) => Err(anyhow::anyhow!(
                            "apply step failed ({cause}); rollback also failed: {e}"
                        )),
                    };
                }
            }
            match &op {
                Operation::Swap { source, target } => expected.swap(source, target)?,
                Operation::SetRatio { path, ratio } => expected.set_ratio(path, *ratio)?,
                _ => unreachable!(),
            }
            let after = self.client.snapshot().await;
            if !matches!(&after, Ok(s) if s.workspace_id == self.snapshot.workspace_id && s.tab_id == self.snapshot.tab_id && equivalent(&s.tree, &expected) && sorted_ids(&s.tree) == sorted_ids(&expected))
            {
                let cause = after
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "post-step layout mismatch".into());
                progress(ApplyProgress::Recovering);
                let rollback = self.recover_ambiguous_original().await;
                return match rollback {
                    Ok(()) => Err(anyhow::anyhow!(
                        "apply failed ({cause}); completed steps were rolled back"
                    )),
                    Err(e) => Err(anyhow::anyhow!(
                        "apply failed ({cause}); rollback also failed: {e}"
                    )),
                };
            }
        }
        progress(ApplyProgress::Verifying);
        progress(ApplyProgress::Done);
        Ok(())
    }

    async fn apply_structural(
        &self,
        operations: &[Operation],
        target: &LayoutNode,
        progress: &mut dyn FnMut(ApplyProgress),
    ) -> anyhow::Result<()> {
        let mut ids: HashMap<String, String> = self
            .snapshot
            .tree
            .pane_ids()
            .into_iter()
            .map(|id| (id.clone(), id))
            .collect();
        let mut tabs: HashMap<String, String> = ids
            .keys()
            .map(|id| (id.clone(), self.snapshot.tab_id.clone()))
            .collect();
        let mut result = self
            .execute_structural(operations, target, &mut ids, &mut tabs, progress, true)
            .await;
        if result.is_ok() {
            progress(ApplyProgress::Verifying);
            let anchor = self.snapshot.tree.pane_ids()[0].clone();
            result = match self.client.layout_for(&ids[&anchor]).await {
                Ok(live) => match to_logical(&live, &ids) {
                    Ok(logical) if equivalent(&logical, target) => Ok(()),
                    Ok(_) => Err(anyhow::anyhow!("structural final verification mismatch")),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            };
        }
        if let Err(cause) = result {
            progress(ApplyProgress::Recovering);
            let reconciliation = self.reconcile(&mut ids, &mut tabs).await;
            if let Err(error) = reconciliation {
                return Err(anyhow::anyhow!("structural apply failed ({cause}); authoritative reconciliation failed ({error}); last known pane locations: {}", format_locations(&ids, &tabs)));
            }
            let rollback_plan = plan(&self.snapshot.tree, &self.snapshot.tree)?.operations;
            let rollback = self
                .recover_original(&rollback_plan, &mut ids, &mut tabs)
                .await;
            return match rollback {
                Ok(()) => Err(anyhow::anyhow!("structural apply failed ({cause}); original layout restored")),
                Err(recovery) => Err(anyhow::anyhow!("structural apply failed ({cause}); recovery failed ({recovery}); preserved pane locations: {}", format_locations(&ids, &tabs))),
            };
        }
        progress(ApplyProgress::Done);
        Ok(())
    }

    async fn execute_structural(
        &self,
        operations: &[Operation],
        target: &LayoutNode,
        ids: &mut HashMap<String, String>,
        tabs: &mut HashMap<String, String>,
        progress: &mut dyn FnMut(ApplyProgress),
        report_steps: bool,
    ) -> anyhow::Result<()> {
        let anchor = self.snapshot.tree.pane_ids()[0].clone();
        let mut expected = LayoutNode::Pane {
            pane_id: anchor.clone(),
        };
        for (index, op) in operations.iter().enumerate() {
            if report_steps {
                progress(ApplyProgress::Applying {
                    current: index + 1,
                    total: operations.len(),
                });
            }
            match op {
                Operation::Park { pane } => {
                    let outcome = self
                        .client
                        .park_pane(&ids[pane], &self.snapshot.workspace_id)
                        .await?;
                    if outcome.target_tree.pane_ids() != [outcome.pane_id.clone()] {
                        anyhow::bail!("scratch tab contains unexpected panes")
                    }
                    ids.insert(pane.clone(), outcome.pane_id.clone());
                    tabs.insert(pane.clone(), outcome.tab_id);
                }
                Operation::Move {
                    pane,
                    target: target_pane,
                    direction,
                    ratio,
                } => {
                    let outcome = self
                        .client
                        .move_pane(
                            &ids[pane],
                            &self.snapshot.tab_id,
                            &ids[target_pane],
                            *direction,
                            *ratio,
                        )
                        .await?;
                    ids.insert(pane.clone(), outcome.pane_id.clone());
                    tabs.insert(pane.clone(), outcome.tab_id);
                    expected.insert_second(target_pane, pane.clone(), *direction, *ratio)?;
                    let logical = to_logical(&outcome.target_tree, ids)?;
                    if !equivalent(&logical, &expected) {
                        anyhow::bail!("pane.move target layout differs from simulated plan")
                    }
                }
                Operation::Swap { source, target } => {
                    self.client.swap(&ids[source], &ids[target]).await?;
                    expected.swap(source, target)?;
                    let live = self.client.layout_for(&ids[&anchor]).await?;
                    if !equivalent(&to_logical(&live, ids)?, &expected) {
                        anyhow::bail!("pane.swap layout differs from simulated plan")
                    }
                }
                Operation::SetRatio { path, ratio } => {
                    self.client
                        .set_ratio(&self.snapshot.tab_id, path, *ratio)
                        .await?;
                    expected.set_ratio(path, *ratio)?;
                    let live = self.client.layout_for(&ids[&anchor]).await?;
                    if !equivalent(&to_logical(&live, ids)?, &expected) {
                        anyhow::bail!("split ratio layout differs from simulated plan")
                    }
                }
            }
        }
        if !equivalent(&expected, target) {
            anyhow::bail!("structural simulation did not produce target")
        }
        Ok(())
    }

    async fn recover_original(
        &self,
        _unused: &[Operation],
        ids: &mut HashMap<String, String>,
        tabs: &mut HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let original = &self.snapshot.tree;
        let anchor = original.pane_ids()[0].clone();
        for logical in original.pane_ids().into_iter().filter(|id| id != &anchor) {
            if tabs[&logical] == self.snapshot.tab_id {
                let outcome = self
                    .client
                    .park_pane(&ids[&logical], &self.snapshot.workspace_id)
                    .await?;
                ids.insert(logical.clone(), outcome.pane_id);
                tabs.insert(logical, outcome.tab_id);
            }
        }
        let recovery_plan = rebuild_plan(original, &anchor)?;
        let mut noop = |_| {};
        self.execute_structural(&recovery_plan, original, ids, tabs, &mut noop, false)
            .await?;
        let live = self.client.layout_for(&ids[&anchor]).await?;
        if !equivalent(&to_logical(&live, ids)?, original) {
            anyhow::bail!("restored layout verification mismatch")
        }
        Ok(())
    }

    async fn reconcile(
        &self,
        ids: &mut HashMap<String, String>,
        tabs: &mut HashMap<String, String>,
    ) -> anyhow::Result<LayoutNode> {
        let inventory = self
            .client
            .pane_locations(&self.snapshot.workspace_id)
            .await?;
        for (logical, actual) in ids.clone() {
            let resolved = if inventory.contains_key(&actual) {
                actual
            } else if inventory.contains_key(&logical) {
                logical.clone()
            } else {
                anyhow::bail!("pane {logical} ({actual}) is absent from authoritative inventory")
            };
            tabs.insert(logical.clone(), inventory[&resolved].clone());
            ids.insert(logical, resolved);
        }
        let anchor = self.snapshot.tree.pane_ids()[0].clone();
        self.client.layout_for(&ids[&anchor]).await
    }

    async fn recover_ambiguous_original(&self) -> anyhow::Result<()> {
        let mut ids: HashMap<String, String> = self
            .snapshot
            .tree
            .pane_ids()
            .into_iter()
            .map(|id| (id.clone(), id))
            .collect();
        let mut tabs = HashMap::new();
        self.reconcile(&mut ids, &mut tabs).await?;
        self.recover_original(&[], &mut ids, &mut tabs).await
    }
}

fn to_logical(tree: &LayoutNode, ids: &HashMap<String, String>) -> anyhow::Result<LayoutNode> {
    let reverse: HashMap<_, _> = ids
        .iter()
        .map(|(logical, actual)| (actual.as_str(), logical.as_str()))
        .collect();
    fn map(node: &LayoutNode, reverse: &HashMap<&str, &str>) -> anyhow::Result<LayoutNode> {
        Ok(match node {
            LayoutNode::Pane { pane_id } => LayoutNode::Pane {
                pane_id: reverse
                    .get(pane_id.as_str())
                    .ok_or_else(|| anyhow::anyhow!("unknown response pane id {pane_id}"))?
                    .to_string(),
            },
            LayoutNode::Split {
                direction,
                ratio,
                first,
                second,
            } => LayoutNode::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(map(first, reverse)?),
                second: Box::new(map(second, reverse)?),
            },
        })
    }
    map(tree, &reverse)
}

fn format_locations(ids: &HashMap<String, String>, tabs: &HashMap<String, String>) -> String {
    let mut entries = ids
        .iter()
        .map(|(logical, actual)| {
            format!(
                "{logical}={actual}@{}",
                tabs.get(logical).map(String::as_str).unwrap_or("unknown")
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.join(", ")
}

fn sorted_ids(tree: &LayoutNode) -> Vec<String> {
    let mut ids = tree.pane_ids();
    ids.sort();
    ids
}

fn equivalent(a: &LayoutNode, b: &LayoutNode) -> bool {
    match (a, b) {
        (LayoutNode::Pane { pane_id: a }, LayoutNode::Pane { pane_id: b }) => a == b,
        (
            LayoutNode::Split {
                direction: ad,
                ratio: ar,
                first: af,
                second: as_,
            },
            LayoutNode::Split {
                direction: bd,
                ratio: br,
                first: bf,
                second: bs,
            },
        ) => ad == bd && (ar - br).abs() <= 0.02 && equivalent(af, bf) && equivalent(as_, bs),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{herdr::PaneMetadata, model::Direction};
    use async_trait::async_trait;
    use std::{collections::HashMap, sync::Mutex};
    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn tree(ids: [&str; 3]) -> LayoutNode {
        LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane {
                pane_id: ids[0].into(),
            }),
            second: Box::new(LayoutNode::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane {
                    pane_id: ids[1].into(),
                }),
                second: Box::new(LayoutNode::Pane {
                    pane_id: ids[2].into(),
                }),
            }),
        }
    }
    fn snapshot(tree: LayoutNode) -> Snapshot {
        let metadata = tree
            .pane_ids()
            .into_iter()
            .map(|id| {
                (
                    id.clone(),
                    PaneMetadata {
                        pane_id: id,
                        revision: 1,
                        ..Default::default()
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let revisions = metadata
            .iter()
            .map(|(id, m)| (id.clone(), m.revision))
            .collect();
        Snapshot {
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            focused_pane_id: "a".into(),
            tree,
            metadata,
            revisions,
        }
    }
    struct StructuralFake {
        state: Mutex<StructuralState>,
        fail_at: Option<usize>,
        committed_error_at: Option<usize>,
    }
    struct StructuralState {
        original_tab: String,
        tabs: HashMap<String, LayoutNode>,
        calls: usize,
        next_tab: usize,
    }
    impl StructuralFake {
        fn outcome(
            state: &StructuralState,
            pane: &str,
            tab: &str,
            created: Option<String>,
        ) -> super::super::MoveOutcome {
            super::super::MoveOutcome {
                pane_id: pane.into(),
                tab_id: tab.into(),
                created_tab_id: created,
                target_tree: state.tabs[tab].clone(),
            }
        }
        fn maybe_fail(&self, state: &mut StructuralState) -> anyhow::Result<()> {
            state.calls += 1;
            if self.fail_at == Some(state.calls) {
                anyhow::bail!("injected move failure")
            }
            Ok(())
        }
        fn committed_error(&self, state: &StructuralState) -> anyhow::Result<()> {
            if self.committed_error_at == Some(state.calls) {
                anyhow::bail!("committed-then-error")
            }
            Ok(())
        }
    }
    #[async_trait]
    impl HerdrClient for StructuralFake {
        async fn snapshot(&self) -> anyhow::Result<Snapshot> {
            let s = self.state.lock().unwrap();
            Ok(snapshot(s.tabs[&s.original_tab].clone()))
        }
        async fn layout_for(&self, pane: &str) -> anyhow::Result<LayoutNode> {
            let s = self.state.lock().unwrap();
            Ok(s.tabs
                .values()
                .find(|tree| tree.pane_ids().iter().any(|id| id == pane))
                .unwrap()
                .clone())
        }
        async fn pane_locations(
            &self,
            _workspace: &str,
        ) -> anyhow::Result<HashMap<String, String>> {
            let s = self.state.lock().unwrap();
            Ok(s.tabs
                .iter()
                .flat_map(|(tab, tree)| tree.pane_ids().into_iter().map(|pane| (pane, tab.clone())))
                .collect())
        }
        async fn swap(&self, source: &str, target: &str) -> anyhow::Result<()> {
            let mut s = self.state.lock().unwrap();
            self.maybe_fail(&mut s)?;
            let tab = s
                .tabs
                .iter()
                .find(|(_, t)| {
                    let ids = t.pane_ids();
                    ids.iter().any(|x| x == source) && ids.iter().any(|x| x == target)
                })
                .map(|(id, _)| id.clone())
                .unwrap();
            s.tabs.get_mut(&tab).unwrap().swap(source, target)?;
            self.committed_error(&s)?;
            Ok(())
        }
        async fn set_ratio(
            &self,
            tab: &str,
            path: &crate::model::SplitPath,
            ratio: f64,
        ) -> anyhow::Result<()> {
            let mut s = self.state.lock().unwrap();
            self.maybe_fail(&mut s)?;
            s.tabs.get_mut(tab).unwrap().set_ratio(path, ratio)?;
            self.committed_error(&s)?;
            Ok(())
        }
        async fn park_pane(
            &self,
            pane: &str,
            _workspace: &str,
        ) -> anyhow::Result<super::super::MoveOutcome> {
            let mut s = self.state.lock().unwrap();
            self.maybe_fail(&mut s)?;
            let source = s
                .tabs
                .iter()
                .find(|(_, t)| t.pane_ids().iter().any(|id| id == pane))
                .map(|(id, _)| id.clone())
                .unwrap();
            let detached = s.tabs.get_mut(&source).unwrap().detach_pane(pane)?;
            s.next_tab += 1;
            let tab = format!("w1:t{}", s.next_tab);
            s.tabs.insert(tab.clone(), detached);
            self.committed_error(&s)?;
            Ok(Self::outcome(&s, pane, &tab, Some(tab.clone())))
        }
        async fn move_pane(
            &self,
            pane: &str,
            tab: &str,
            target: &str,
            direction: crate::model::Direction,
            ratio: f64,
        ) -> anyhow::Result<super::super::MoveOutcome> {
            let mut s = self.state.lock().unwrap();
            self.maybe_fail(&mut s)?;
            let source = s
                .tabs
                .iter()
                .find(|(_, t)| t.pane_ids().iter().any(|id| id == pane))
                .map(|(id, _)| id.clone())
                .unwrap();
            let moved = s.tabs.remove(&source).unwrap();
            assert_eq!(moved.pane_ids(), [pane]);
            s.tabs
                .get_mut(tab)
                .unwrap()
                .insert_second(target, pane.into(), direction, ratio)?;
            self.committed_error(&s)?;
            Ok(Self::outcome(&s, pane, tab, None))
        }
    }
    fn structural_fake(original: LayoutNode, fail_at: Option<usize>) -> StructuralFake {
        let mut tabs = HashMap::new();
        tabs.insert("w1:t1".into(), original);
        StructuralFake {
            state: Mutex::new(StructuralState {
                original_tab: "w1:t1".into(),
                tabs,
                calls: 0,
                next_tab: 1,
            }),
            fail_at,
            committed_error_at: None,
        }
    }

    #[tokio::test]
    async fn structural_reparent_reaches_exact_target() {
        let _serial = TEST_LOCK.lock().await;
        let original = tree(["a", "b", "c"]);
        let target = LayoutNode::Split {
            direction: Direction::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane {
                    pane_id: "a".into(),
                }),
                second: Box::new(LayoutNode::Pane {
                    pane_id: "c".into(),
                }),
            }),
            second: Box::new(LayoutNode::Pane {
                pane_id: "b".into(),
            }),
        };
        let fake = structural_fake(original.clone(), None);
        let before = snapshot(original);
        let mut updates = Vec::new();
        Transaction {
            client: &fake,
            snapshot: &before,
        }
        .apply_with_progress(&target, &mut |update| updates.push(update))
        .await
        .unwrap();
        assert_eq!(updates.first(), Some(&ApplyProgress::Validating));
        assert_eq!(updates.last(), Some(&ApplyProgress::Done));
        assert!(updates
            .iter()
            .any(|update| matches!(update, ApplyProgress::Applying { .. })));
        assert!(equivalent(
            &fake.state.lock().unwrap().tabs["w1:t1"],
            &target
        ));
    }

    #[tokio::test]
    async fn structural_failure_restores_original_and_preserves_all_panes() {
        let _serial = TEST_LOCK.lock().await;
        let original = tree(["a", "b", "c"]);
        let target = LayoutNode::Split {
            direction: Direction::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane {
                pane_id: "c".into(),
            }),
            second: Box::new(LayoutNode::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane {
                    pane_id: "a".into(),
                }),
                second: Box::new(LayoutNode::Pane {
                    pane_id: "b".into(),
                }),
            }),
        };
        let boundaries = plan(&original, &target).unwrap().operations.len();
        for boundary in 1..=boundaries {
            let fake = structural_fake(original.clone(), Some(boundary));
            let err = Transaction {
                client: &fake,
                snapshot: &snapshot(original.clone()),
            }
            .apply(&target)
            .await
            .unwrap_err();
            assert!(
                err.to_string().contains("original layout restored"),
                "boundary {boundary}: {err}"
            );
            let s = fake.state.lock().unwrap();
            assert!(
                equivalent(&s.tabs["w1:t1"], &original),
                "boundary {boundary}"
            );
            let mut ids = s
                .tabs
                .values()
                .flat_map(LayoutNode::pane_ids)
                .collect::<Vec<_>>();
            ids.sort();
            assert_eq!(ids, ["a", "b", "c"], "boundary {boundary}");
        }
    }

    #[tokio::test]
    async fn committed_then_error_is_reconciled_for_park_move_and_ratio() {
        let _serial = TEST_LOCK.lock().await;
        let original = tree(["a", "b", "c"]);
        let target = LayoutNode::Split {
            direction: Direction::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane {
                pane_id: "c".into(),
            }),
            second: Box::new(LayoutNode::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane {
                    pane_id: "a".into(),
                }),
                second: Box::new(LayoutNode::Pane {
                    pane_id: "b".into(),
                }),
            }),
        };
        for boundary in [1, 3] {
            let mut fake = structural_fake(original.clone(), None);
            fake.committed_error_at = Some(boundary);
            let err = Transaction {
                client: &fake,
                snapshot: &snapshot(original.clone()),
            }
            .apply(&target)
            .await
            .unwrap_err();
            assert!(err.to_string().contains("original layout restored"));
            assert!(equivalent(
                &fake.state.lock().unwrap().tabs["w1:t1"],
                &original
            ));
        }
        let mut ratio_target = original.clone();
        ratio_target.set_ratio(&[], 0.7).unwrap();
        let mut fake = structural_fake(original.clone(), None);
        fake.committed_error_at = Some(1);
        Transaction {
            client: &fake,
            snapshot: &snapshot(original.clone()),
        }
        .apply(&ratio_target)
        .await
        .unwrap_err();
        assert!(equivalent(
            &fake.state.lock().unwrap().tabs["w1:t1"],
            &original
        ));
    }
}
