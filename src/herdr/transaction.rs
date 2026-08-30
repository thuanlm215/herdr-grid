use super::{plan, rebuild_plan, HerdrClient, Operation, Snapshot};
use crate::model::{is_draft_pane, Direction, LayoutNode};
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
            || !equivalent(&live.tree, &self.snapshot.tree)
            || sorted_ids(&live.tree) != sorted_ids(&self.snapshot.tree)
        {
            anyhow::bail!("layout changed since editor opened; no changes applied")
        };
        if target.pane_ids().iter().any(|id| is_draft_pane(id)) {
            return self.apply_with_drafts(target, progress).await;
        }
        self.apply_validated(target, progress).await
    }

    async fn apply_validated(
        &self,
        target: &LayoutNode,
        progress: &mut dyn FnMut(ApplyProgress),
    ) -> anyhow::Result<()> {
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

    async fn apply_with_drafts(
        &self,
        target: &LayoutNode,
        progress: &mut dyn FnMut(ApplyProgress),
    ) -> anyhow::Result<()> {
        let drafts = target
            .pane_ids()
            .into_iter()
            .filter(|id| is_draft_pane(id))
            .collect::<Vec<_>>();
        let anchor = self
            .snapshot
            .tree
            .pane_ids()
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("cannot create a pane in an empty layout"))?;
        let mut created = Vec::with_capacity(drafts.len());
        let mut expanded_tree = self.snapshot.tree.clone();

        for (index, draft) in drafts.iter().enumerate() {
            progress(ApplyProgress::Applying {
                current: index + 1,
                total: drafts.len(),
            });
            match self
                .client
                .split_pane(&anchor, Direction::Horizontal, 0.5)
                .await
            {
                Ok(outcome) => {
                    created.push((draft.clone(), outcome.pane_id));
                    expanded_tree = outcome.target_tree;
                    let mut expected_ids = self.snapshot.tree.pane_ids();
                    expected_ids.extend(created.iter().map(|(_, actual)| actual.clone()));
                    expected_ids.sort();
                    if sorted_ids(&expanded_tree) != expected_ids {
                        progress(ApplyProgress::Recovering);
                        let cleanup = self.cleanup_created(&created, &anchor).await;
                        return match cleanup {
                            Ok(()) => Err(anyhow::anyhow!(
                                "pane creation changed an unexpected part of the layout; new panes were removed"
                            )),
                            Err(error) => Err(anyhow::anyhow!(
                                "pane creation changed an unexpected part of the layout; cleanup also failed: {error}"
                            )),
                        };
                    }
                }
                Err(cause) => {
                    progress(ApplyProgress::Recovering);
                    let cleanup = self.cleanup_created(&created, &anchor).await;
                    return match cleanup {
                        Ok(()) => Err(anyhow::anyhow!(
                            "could not create new pane ({cause}); created panes were removed"
                        )),
                        Err(error) => Err(anyhow::anyhow!(
                            "could not create new pane ({cause}); cleanup also failed: {error}"
                        )),
                    };
                }
            }
        }

        let mut mapped_target = target.clone();
        for (draft, actual) in &created {
            mapped_target.replace_pane_id(draft, actual.clone())?;
        }
        let expanded = Snapshot {
            workspace_id: self.snapshot.workspace_id.clone(),
            tab_id: self.snapshot.tab_id.clone(),
            focused_pane_id: anchor.clone(),
            tree: expanded_tree,
            metadata: self.snapshot.metadata.clone(),
            revisions: self.snapshot.revisions.clone(),
        };
        let transaction = Transaction {
            client: self.client,
            snapshot: &expanded,
        };
        match transaction.apply_validated(&mapped_target, progress).await {
            Ok(()) => Ok(()),
            Err(cause) => {
                progress(ApplyProgress::Recovering);
                match self.cleanup_created(&created, &anchor).await {
                    Ok(()) => Err(anyhow::anyhow!(
                        "apply failed ({cause}); new panes were removed and the original layout restored"
                    )),
                    Err(error) => Err(anyhow::anyhow!(
                        "apply failed ({cause}); new-pane cleanup also failed: {error}"
                    )),
                }
            }
        }
    }

    async fn cleanup_created(
        &self,
        created: &[(String, String)],
        anchor: &str,
    ) -> anyhow::Result<()> {
        let mut errors = Vec::new();
        for (_, pane) in created.iter().rev() {
            if let Err(error) = self.client.close_pane(pane).await {
                errors.push(format!("{pane}: {error}"));
            }
        }
        if !errors.is_empty() {
            anyhow::bail!("failed to remove {}", errors.join(", "))
        }
        let live = self.client.layout_for(anchor).await?;
        if !equivalent(&live, &self.snapshot.tree) {
            anyhow::bail!("original layout verification mismatch after removing new panes")
        }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedWorkspace {
    pub workspace_id: String,
    pub tab_id: String,
}

pub async fn create_workspace_layout<C: HerdrClient>(
    client: &C,
    target: &LayoutNode,
    cwd: &str,
    progress: &mut dyn FnMut(ApplyProgress),
) -> anyhow::Result<CreatedWorkspace> {
    progress(ApplyProgress::Validating);
    let _lock = ApplyGuard::acquire()?;
    target.validate()?;
    let logical_ids = target.pane_ids();
    if logical_ids.is_empty() || logical_ids.iter().any(|id| !is_draft_pane(id)) {
        anyhow::bail!("new workspace presets must contain only new shell slots")
    }
    let operations = workspace_creation_plan(target);
    let total = logical_ids.len();
    progress(ApplyProgress::Applying { current: 1, total });
    let workspace = client.create_workspace(cwd, "herdr-grid preset").await?;
    let root_logical = logical_ids[0].clone();
    let mut ids = HashMap::from([(root_logical.clone(), workspace.pane_id.clone())]);

    let build_result: anyhow::Result<()> = async {
        for (index, operation) in operations.iter().enumerate() {
            progress(ApplyProgress::Applying {
                current: index + 2,
                total,
            });
            let anchor = ids
                .get(&operation.anchor)
                .ok_or_else(|| {
                    anyhow::anyhow!("preset anchor {} was not created", operation.anchor)
                })?
                .clone();
            let outcome = client
                .split_pane(&anchor, operation.direction, operation.ratio)
                .await?;
            ids.insert(operation.new_pane.clone(), outcome.pane_id);
            let mut actual = outcome.target_tree.pane_ids();
            let mut expected = ids.values().cloned().collect::<Vec<_>>();
            actual.sort();
            expected.sort();
            if actual != expected {
                anyhow::bail!("pane.split changed an unexpected part of the new workspace")
            }
        }
        progress(ApplyProgress::Verifying);
        let live = client.layout_for(&workspace.pane_id).await?;
        let logical = to_logical(&live, &ids)?;
        if !equivalent(&logical, target) {
            anyhow::bail!("new workspace layout verification mismatch")
        }
        Ok(())
    }
    .await;

    if let Err(cause) = build_result {
        progress(ApplyProgress::Recovering);
        return match client.close_workspace(&workspace.workspace_id).await {
            Ok(()) => Err(anyhow::anyhow!(
                "new workspace creation failed ({cause}); the partial workspace was removed"
            )),
            Err(cleanup) => Err(anyhow::anyhow!(
                "new workspace creation failed ({cause}); cleanup also failed: {cleanup}"
            )),
        };
    }
    let _ = client.focus_workspace(&workspace.workspace_id).await;
    progress(ApplyProgress::Done);
    Ok(CreatedWorkspace {
        workspace_id: workspace.workspace_id,
        tab_id: workspace.tab_id,
    })
}

#[derive(Clone, Debug)]
struct WorkspaceCreateOperation {
    anchor: String,
    new_pane: String,
    direction: Direction,
    ratio: f64,
}

fn workspace_creation_plan(target: &LayoutNode) -> Vec<WorkspaceCreateOperation> {
    fn emit(node: &LayoutNode, operations: &mut Vec<WorkspaceCreateOperation>) {
        if let LayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } = node
        {
            operations.push(WorkspaceCreateOperation {
                anchor: first.pane_ids()[0].clone(),
                new_pane: second.pane_ids()[0].clone(),
                direction: *direction,
                ratio: *ratio,
            });
            emit(first, operations);
            emit(second, operations);
        }
    }
    let mut operations = Vec::new();
    emit(target, &mut operations);
    operations
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
    use crate::{
        herdr::PaneMetadata,
        model::{Direction, DRAFT_PANE_PREFIX},
    };
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
    fn snapshot_at_revision(tree: LayoutNode, revision: u64) -> Snapshot {
        let metadata = tree
            .pane_ids()
            .into_iter()
            .map(|id| {
                (
                    id.clone(),
                    PaneMetadata {
                        pane_id: id,
                        revision,
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

    fn snapshot(tree: LayoutNode) -> Snapshot {
        snapshot_at_revision(tree, 1)
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
        revision: u64,
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
            Ok(snapshot_at_revision(
                s.tabs[&s.original_tab].clone(),
                s.revision,
            ))
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
                revision: 1,
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
    async fn pane_activity_revision_drift_does_not_block_apply() {
        let _serial = TEST_LOCK.lock().await;
        let original = tree(["a", "b", "c"]);
        let mut target = original.clone();
        target.swap("a", "b").unwrap();
        let fake = structural_fake(original.clone(), None);
        fake.state.lock().unwrap().revision = 2;

        Transaction {
            client: &fake,
            snapshot: &snapshot(original),
        }
        .apply(&target)
        .await
        .unwrap();

        assert!(equivalent(
            &fake.state.lock().unwrap().tabs["w1:t1"],
            &target
        ));
    }

    #[tokio::test]
    async fn external_layout_drift_still_blocks_apply_before_writes() {
        let _serial = TEST_LOCK.lock().await;
        let original = tree(["a", "b", "c"]);
        let mut requested = original.clone();
        requested.swap("a", "b").unwrap();
        let fake = structural_fake(original.clone(), None);
        fake.state
            .lock()
            .unwrap()
            .tabs
            .get_mut("w1:t1")
            .unwrap()
            .set_ratio(&[], 0.7)
            .unwrap();

        let error = Transaction {
            client: &fake,
            snapshot: &snapshot(original),
        }
        .apply(&requested)
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "layout changed since editor opened; no changes applied"
        );
        assert_eq!(fake.state.lock().unwrap().calls, 0);
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

    struct DraftFake {
        state: Mutex<(LayoutNode, usize)>,
        fail_split_at: Option<usize>,
    }

    #[async_trait]
    impl HerdrClient for DraftFake {
        async fn snapshot(&self) -> anyhow::Result<Snapshot> {
            Ok(snapshot(self.state.lock().unwrap().0.clone()))
        }
        async fn layout_for(&self, _pane: &str) -> anyhow::Result<LayoutNode> {
            Ok(self.state.lock().unwrap().0.clone())
        }
        async fn swap(&self, source: &str, target: &str) -> anyhow::Result<()> {
            self.state.lock().unwrap().0.swap(source, target)?;
            Ok(())
        }
        async fn set_ratio(
            &self,
            _tab: &str,
            path: &crate::model::SplitPath,
            ratio: f64,
        ) -> anyhow::Result<()> {
            self.state.lock().unwrap().0.set_ratio(path, ratio)?;
            Ok(())
        }
        async fn split_pane(
            &self,
            target: &str,
            direction: Direction,
            ratio: f64,
        ) -> anyhow::Result<super::super::SplitOutcome> {
            let mut state = self.state.lock().unwrap();
            state.1 += 1;
            if self.fail_split_at == Some(state.1) {
                anyhow::bail!("injected split failure")
            }
            let pane_id = format!("new-{}", state.1);
            state
                .0
                .insert_second(target, pane_id.clone(), direction, ratio)?;
            Ok(super::super::SplitOutcome {
                pane_id,
                target_tree: state.0.clone(),
            })
        }
        async fn close_pane(&self, pane: &str) -> anyhow::Result<()> {
            self.state.lock().unwrap().0.detach_pane(pane)?;
            Ok(())
        }
    }

    #[tokio::test]
    async fn draft_is_materialized_as_a_real_pane_only_on_apply() {
        let _serial = TEST_LOCK.lock().await;
        let original = LayoutNode::Pane {
            pane_id: "a".into(),
        };
        let draft = format!("{DRAFT_PANE_PREFIX}1");
        let target = LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(original.clone()),
            second: Box::new(LayoutNode::Pane { pane_id: draft }),
        };
        let fake = DraftFake {
            state: Mutex::new((original.clone(), 0)),
            fail_split_at: None,
        };

        Transaction {
            client: &fake,
            snapshot: &snapshot(original),
        }
        .apply(&target)
        .await
        .unwrap();

        let state = fake.state.lock().unwrap();
        assert_eq!(state.0.pane_ids(), ["a", "new-1"]);
        assert_eq!(state.1, 1);
    }

    #[tokio::test]
    async fn partial_draft_creation_closes_only_new_panes() {
        let _serial = TEST_LOCK.lock().await;
        let original = LayoutNode::Pane {
            pane_id: "a".into(),
        };
        let mut target = original.clone();
        target
            .insert_second(
                "a",
                format!("{DRAFT_PANE_PREFIX}1"),
                Direction::Horizontal,
                0.5,
            )
            .unwrap();
        target
            .insert_second(
                &format!("{DRAFT_PANE_PREFIX}1"),
                format!("{DRAFT_PANE_PREFIX}2"),
                Direction::Vertical,
                0.5,
            )
            .unwrap();
        let fake = DraftFake {
            state: Mutex::new((original.clone(), 0)),
            fail_split_at: Some(2),
        };

        let error = Transaction {
            client: &fake,
            snapshot: &snapshot(original.clone()),
        }
        .apply(&target)
        .await
        .unwrap_err();

        assert!(error.to_string().contains("created panes were removed"));
        assert_eq!(fake.state.lock().unwrap().0, original);
    }

    struct WorkspaceFake {
        state: Mutex<WorkspaceState>,
        fail_split_at: Option<usize>,
    }

    struct WorkspaceState {
        tree: Option<LayoutNode>,
        splits: usize,
        closed: bool,
        focused: bool,
    }

    #[async_trait]
    impl HerdrClient for WorkspaceFake {
        async fn snapshot(&self) -> anyhow::Result<Snapshot> {
            anyhow::bail!("not used by workspace creation")
        }

        async fn layout_for(&self, _pane: &str) -> anyhow::Result<LayoutNode> {
            self.state
                .lock()
                .unwrap()
                .tree
                .clone()
                .ok_or_else(|| anyhow::anyhow!("workspace has no layout"))
        }

        async fn swap(&self, _source: &str, _target: &str) -> anyhow::Result<()> {
            anyhow::bail!("not used by workspace creation")
        }

        async fn set_ratio(
            &self,
            _tab: &str,
            _path: &crate::model::SplitPath,
            _ratio: f64,
        ) -> anyhow::Result<()> {
            anyhow::bail!("not used by workspace creation")
        }

        async fn create_workspace(
            &self,
            _cwd: &str,
            _label: &str,
        ) -> anyhow::Result<super::super::WorkspaceOutcome> {
            self.state.lock().unwrap().tree = Some(LayoutNode::Pane {
                pane_id: "root".into(),
            });
            Ok(super::super::WorkspaceOutcome {
                workspace_id: "new-workspace".into(),
                tab_id: "new-workspace:t1".into(),
                pane_id: "root".into(),
            })
        }

        async fn split_pane(
            &self,
            target: &str,
            direction: Direction,
            ratio: f64,
        ) -> anyhow::Result<super::super::SplitOutcome> {
            let mut state = self.state.lock().unwrap();
            state.splits += 1;
            if self.fail_split_at == Some(state.splits) {
                anyhow::bail!("injected workspace split failure")
            }
            let pane_id = format!("new-{}", state.splits);
            state.tree.as_mut().unwrap().insert_second(
                target,
                pane_id.clone(),
                direction,
                ratio,
            )?;
            Ok(super::super::SplitOutcome {
                pane_id,
                target_tree: state.tree.clone().unwrap(),
            })
        }

        async fn close_workspace(&self, workspace: &str) -> anyhow::Result<()> {
            assert_eq!(workspace, "new-workspace");
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            state.tree = None;
            Ok(())
        }

        async fn focus_workspace(&self, workspace: &str) -> anyhow::Result<()> {
            assert_eq!(workspace, "new-workspace");
            self.state.lock().unwrap().focused = true;
            Ok(())
        }
    }

    fn workspace_fake(fail_split_at: Option<usize>) -> WorkspaceFake {
        WorkspaceFake {
            state: Mutex::new(WorkspaceState {
                tree: None,
                splits: 0,
                closed: false,
                focused: false,
            }),
            fail_split_at,
        }
    }

    #[tokio::test]
    async fn new_workspace_is_built_to_the_exact_preset_and_focused() {
        let _serial = TEST_LOCK.lock().await;
        let ids = (1..=4)
            .map(|index| format!("{DRAFT_PANE_PREFIX}{index}"))
            .collect::<Vec<_>>();
        let target = crate::model::PresetKind::Grid2x2.build(&ids).unwrap();
        let fake = workspace_fake(None);

        let created = create_workspace_layout(&fake, &target, "/tmp", &mut |_| {})
            .await
            .unwrap();

        assert_eq!(created.workspace_id, "new-workspace");
        let state = fake.state.lock().unwrap();
        assert_eq!(state.splits, 3);
        assert!(state.focused);
        assert!(!state.closed);
    }

    #[tokio::test]
    async fn failed_workspace_build_closes_only_the_partial_workspace() {
        let _serial = TEST_LOCK.lock().await;
        let ids = (1..=4)
            .map(|index| format!("{DRAFT_PANE_PREFIX}{index}"))
            .collect::<Vec<_>>();
        let target = crate::model::PresetKind::Grid2x2.build(&ids).unwrap();
        let fake = workspace_fake(Some(2));

        let error = create_workspace_layout(&fake, &target, "/tmp", &mut |_| {})
            .await
            .unwrap_err();

        assert!(error.to_string().contains("partial workspace was removed"));
        let state = fake.state.lock().unwrap();
        assert!(state.closed);
        assert!(state.tree.is_none());
        assert!(!state.focused);
    }
}
