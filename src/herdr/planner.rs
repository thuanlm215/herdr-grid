use crate::model::{Direction, LayoutNode, PaneId, SplitPath};

#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    Swap {
        source: PaneId,
        target: PaneId,
    },
    SetRatio {
        path: SplitPath,
        ratio: f64,
    },
    Park {
        pane: PaneId,
    },
    Move {
        pane: PaneId,
        target: PaneId,
        direction: Direction,
        ratio: f64,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub original: LayoutNode,
    pub target: LayoutNode,
    pub operations: Vec<Operation>,
    pub structural: bool,
}

pub fn plan(original: &LayoutNode, target: &LayoutNode) -> anyhow::Result<Plan> {
    original.validate()?;
    target.validate()?;
    let mut a = original.pane_ids();
    let mut b = target.pane_ids();
    a.sort();
    b.sort();
    if a != b {
        anyhow::bail!("target pane set differs from snapshot")
    }
    if same_shape(original, target) {
        let mut work = original.clone();
        let mut ops = vec![];
        for (index, want) in target.pane_ids().into_iter().enumerate() {
            let have = work.pane_ids()[index].clone();
            if want != have {
                ops.push(Operation::Swap {
                    source: have.clone(),
                    target: want.clone(),
                });
                work.swap(&have, &want)?;
            }
        }
        collect_ratios(original, target, &mut vec![], &mut ops);
        return Ok(Plan {
            original: original.clone(),
            target: target.clone(),
            operations: ops,
            structural: false,
        });
    }
    let anchor = original.pane_ids()[0].clone();
    let mut ops = original
        .pane_ids()
        .into_iter()
        .filter(|p| p != &anchor)
        .map(|pane| Operation::Park { pane })
        .collect::<Vec<_>>();
    ops.extend(rebuild_plan(target, &anchor)?);
    Ok(Plan {
        original: original.clone(),
        target: target.clone(),
        operations: ops,
        structural: true,
    })
}
pub fn rebuild_plan(target: &LayoutNode, anchor: &str) -> anyhow::Result<Vec<Operation>> {
    let mut order = vec![anchor.to_owned()];
    order.extend(target.pane_ids().into_iter().filter(|id| id != anchor));
    let mut index = 0;
    let build_target = relabel(target, &order, &mut index);
    let mut operations = rebuild_moves(&build_target);
    operations.extend(plan(&build_target, target)?.operations);
    Ok(operations)
}
fn relabel(node: &LayoutNode, ids: &[PaneId], index: &mut usize) -> LayoutNode {
    match node {
        LayoutNode::Pane { .. } => {
            let pane_id = ids[*index].clone();
            *index += 1;
            LayoutNode::Pane { pane_id }
        }
        LayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } => LayoutNode::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(relabel(first, ids, index)),
            second: Box::new(relabel(second, ids, index)),
        },
    }
}
pub fn rebuild_moves(target: &LayoutNode) -> Vec<Operation> {
    let mut operations = Vec::new();
    emit_build(target, "", &mut operations);
    operations
}
fn same_shape(a: &LayoutNode, b: &LayoutNode) -> bool {
    match (a, b) {
        (LayoutNode::Pane { .. }, LayoutNode::Pane { .. }) => true,
        (
            LayoutNode::Split {
                direction: ad,
                first: af,
                second: as_,
                ..
            },
            LayoutNode::Split {
                direction: bd,
                first: bf,
                second: bs,
                ..
            },
        ) => ad == bd && same_shape(af, bf) && same_shape(as_, bs),
        _ => false,
    }
}
fn collect_ratios(a: &LayoutNode, b: &LayoutNode, path: &mut SplitPath, out: &mut Vec<Operation>) {
    if let (
        LayoutNode::Split {
            ratio: ar,
            first: af,
            second: as_,
            ..
        },
        LayoutNode::Split {
            ratio: br,
            first: bf,
            second: bs,
            ..
        },
    ) = (a, b)
    {
        if (ar - br).abs() > 0.01 {
            out.push(Operation::SetRatio {
                path: path.clone(),
                ratio: *br,
            })
        }
        path.push(false);
        collect_ratios(af, bf, path, out);
        path.pop();
        path.push(true);
        collect_ratios(as_, bs, path, out);
        path.pop();
    }
}
fn emit_build(n: &LayoutNode, _anchor: &str, out: &mut Vec<Operation>) -> Option<PaneId> {
    match n {
        LayoutNode::Pane { pane_id } => Some(pane_id.clone()),
        LayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let left = first.pane_ids()[0].clone();
            let right = second.pane_ids()[0].clone();
            out.push(Operation::Move {
                pane: right,
                target: left.clone(),
                direction: *direction,
                ratio: *ratio,
            });
            emit_build(first, _anchor, out);
            emit_build(second, _anchor, out);
            Some(left)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pane(id: &str) -> LayoutNode {
        LayoutNode::Pane { pane_id: id.into() }
    }
    fn split(direction: Direction, a: LayoutNode, b: LayoutNode) -> LayoutNode {
        LayoutNode::Split {
            direction,
            ratio: 0.5,
            first: Box::new(a),
            second: Box::new(b),
        }
    }

    #[test]
    fn permutation_uses_swaps() {
        let a = split(Direction::Horizontal, pane("a"), pane("b"));
        let b = split(Direction::Horizontal, pane("b"), pane("a"));
        let p = plan(&a, &b).unwrap();
        assert!(!p.structural);
        assert!(matches!(p.operations[0], Operation::Swap { .. }));
    }

    #[test]
    fn changed_shape_creates_scratch_plan() {
        let a = split(
            Direction::Horizontal,
            pane("a"),
            split(Direction::Vertical, pane("b"), pane("c")),
        );
        let b = split(
            Direction::Vertical,
            split(Direction::Horizontal, pane("a"), pane("b")),
            pane("c"),
        );
        let p = plan(&a, &b).unwrap();
        assert!(p.structural);
        assert_eq!(
            p.operations
                .iter()
                .filter(|x| matches!(x, Operation::Park { .. }))
                .count(),
            2
        );
        assert_eq!(
            p.operations
                .iter()
                .filter(|x| matches!(x, Operation::Move { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn differing_sets_are_rejected() {
        assert!(plan(&pane("a"), &pane("b")).is_err());
    }

    #[test]
    fn three_pane_cycle_uses_dynamic_order() {
        let original = split(
            Direction::Horizontal,
            pane("a"),
            split(Direction::Vertical, pane("b"), pane("c")),
        );
        let target = split(
            Direction::Horizontal,
            pane("b"),
            split(Direction::Vertical, pane("c"), pane("a")),
        );
        let p = plan(&original, &target).unwrap();
        let mut simulated = original;
        for op in p.operations {
            if let Operation::Swap { source, target } = op {
                simulated.swap(&source, &target).unwrap();
            }
        }
        assert_eq!(simulated.pane_ids(), target.pane_ids());
    }
}
