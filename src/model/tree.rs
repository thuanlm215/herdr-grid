use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub type PaneId = String;
pub type SplitPath = Vec<bool>; // false=first, true=second
pub const DRAFT_PANE_PREFIX: &str = "__herdr_grid_draft:";

pub fn is_draft_pane(id: &str) -> bool {
    id.starts_with(DRAFT_PANE_PREFIX)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LayoutNode {
    Pane {
        pane_id: PaneId,
    },
    Split {
        direction: Direction,
        ratio: f64,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum ModelError {
    #[error("pane not found: {0}")]
    NotFound(PaneId),
    #[error("source and target must differ")]
    SamePane,
    #[error("duplicate pane: {0}")]
    Duplicate(PaneId),
    #[error("ratio must be between 0.05 and 0.95")]
    InvalidRatio,
    #[error("split path not found")]
    InvalidPath,
}

impl LayoutNode {
    pub fn validate(&self) -> Result<(), ModelError> {
        fn visit(n: &LayoutNode, ids: &mut HashSet<String>) -> Result<(), ModelError> {
            match n {
                LayoutNode::Pane { pane_id } => {
                    if ids.insert(pane_id.clone()) {
                        Ok(())
                    } else {
                        Err(ModelError::Duplicate(pane_id.clone()))
                    }
                }
                LayoutNode::Split {
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    if !(0.05..=0.95).contains(ratio) {
                        return Err(ModelError::InvalidRatio);
                    }
                    visit(first, ids)?;
                    visit(second, ids)
                }
            }
        }
        visit(self, &mut HashSet::new())
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut out = vec![];
        self.walk(&mut |id| out.push(id.clone()));
        out
    }
    fn walk(&self, f: &mut impl FnMut(&PaneId)) {
        match self {
            Self::Pane { pane_id } => f(pane_id),
            Self::Split { first, second, .. } => {
                first.walk(f);
                second.walk(f);
            }
        }
    }
    fn replace_id(&mut self, from: &str, to: String) -> bool {
        match self {
            Self::Pane { pane_id } if pane_id == from => {
                *pane_id = to;
                true
            }
            Self::Pane { .. } => false,
            Self::Split { first, second, .. } => {
                first.replace_id(from, to.clone()) || second.replace_id(from, to)
            }
        }
    }
    pub fn replace_pane_id(&mut self, from: &str, to: PaneId) -> Result<(), ModelError> {
        if from != to && self.pane_ids().iter().any(|id| id == &to) {
            return Err(ModelError::Duplicate(to));
        }
        self.replace_id(from, to.clone())
            .then_some(())
            .ok_or_else(|| ModelError::NotFound(from.into()))
    }
    pub fn swap(&mut self, a: &str, b: &str) -> Result<(), ModelError> {
        if a == b {
            return Err(ModelError::SamePane);
        }
        let ids = self.pane_ids();
        if !ids.iter().any(|x| x == a) {
            return Err(ModelError::NotFound(a.into()));
        }
        if !ids.iter().any(|x| x == b) {
            return Err(ModelError::NotFound(b.into()));
        }
        let marker = format!("\0herdr-grid-{a}");
        self.replace_id(a, marker.clone());
        self.replace_id(b, a.into());
        self.replace_id(&marker, b.into());
        Ok(())
    }
    fn take(&mut self, id: &str) -> Option<LayoutNode> {
        match self {
            Self::Pane { .. } => None,
            Self::Split { first, second, .. } => {
                if matches!(&**first, Self::Pane{pane_id} if pane_id==id) {
                    let keep = (**second).clone();
                    let found = std::mem::replace(&mut **first, keep.clone());
                    *self = keep;
                    return Some(found);
                }
                if matches!(&**second, Self::Pane{pane_id} if pane_id==id) {
                    let keep = (**first).clone();
                    let found = std::mem::replace(&mut **second, keep.clone());
                    *self = keep;
                    return Some(found);
                }
                first.take(id).or_else(|| second.take(id))
            }
        }
    }
    pub fn detach_pane(&mut self, id: &str) -> Result<LayoutNode, ModelError> {
        self.take(id).ok_or_else(|| ModelError::NotFound(id.into()))
    }
    fn target_mut(&mut self, id: &str) -> Option<&mut LayoutNode> {
        match self {
            Self::Pane { pane_id } if pane_id == id => Some(self),
            Self::Pane { .. } => None,
            Self::Split { first, second, .. } => {
                first.target_mut(id).or_else(|| second.target_mut(id))
            }
        }
    }
    pub fn reparent(&mut self, source: &str, target: &str, edge: Edge) -> Result<(), ModelError> {
        if source == target {
            return Err(ModelError::SamePane);
        }
        if !self.pane_ids().iter().any(|x| x == source) {
            return Err(ModelError::NotFound(source.into()));
        }
        let moved = self
            .take(source)
            .ok_or_else(|| ModelError::NotFound(source.into()))?;
        let target_node = self
            .target_mut(target)
            .ok_or_else(|| ModelError::NotFound(target.into()))?;
        let old = target_node.clone();
        let (direction, before) = match edge {
            Edge::Left => (Direction::Horizontal, true),
            Edge::Right => (Direction::Horizontal, false),
            Edge::Top => (Direction::Vertical, true),
            Edge::Bottom => (Direction::Vertical, false),
        };
        let (first, second) = if before { (moved, old) } else { (old, moved) };
        *target_node = Self::Split {
            direction,
            ratio: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        };
        Ok(())
    }
    pub fn set_ratio(&mut self, path: &[bool], ratio: f64) -> Result<(), ModelError> {
        if !(0.05..=0.95).contains(&ratio) {
            return Err(ModelError::InvalidRatio);
        }
        let mut n = self;
        for second in path {
            n = match n {
                Self::Split {
                    first, second: s, ..
                } => {
                    if *second {
                        s
                    } else {
                        first
                    }
                }
                _ => return Err(ModelError::InvalidPath),
            };
        }
        match n {
            Self::Split { ratio: r, .. } => {
                *r = ratio;
                Ok(())
            }
            _ => Err(ModelError::InvalidPath),
        }
    }
    pub fn ratio_at(&self, path: &[bool]) -> Option<f64> {
        let mut node = self;
        for take_second in path {
            node = match node {
                Self::Split { first, second, .. } => {
                    if *take_second {
                        second
                    } else {
                        first
                    }
                }
                Self::Pane { .. } => return None,
            };
        }
        match node {
            Self::Split { ratio, .. } => Some(*ratio),
            Self::Pane { .. } => None,
        }
    }
    pub fn balance_splits(&mut self) -> bool {
        fn visit(node: &mut LayoutNode) -> bool {
            match node {
                LayoutNode::Pane { .. } => false,
                LayoutNode::Split {
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    let first_changed = visit(first);
                    let second_changed = visit(second);
                    let ratio_changed = (*ratio - 0.5).abs() > f64::EPSILON;
                    *ratio = 0.5;
                    first_changed || second_changed || ratio_changed
                }
            }
        }
        visit(self)
    }
    pub fn insert_second(
        &mut self,
        target: &str,
        pane: PaneId,
        direction: Direction,
        ratio: f64,
    ) -> Result<(), ModelError> {
        if self.pane_ids().iter().any(|id| id == &pane) {
            return Err(ModelError::Duplicate(pane));
        }
        if !(0.05..=0.95).contains(&ratio) {
            return Err(ModelError::InvalidRatio);
        }
        let target_node = self
            .target_mut(target)
            .ok_or_else(|| ModelError::NotFound(target.into()))?;
        let first = target_node.clone();
        *target_node = Self::Split {
            direction,
            ratio,
            first: Box::new(first),
            second: Box::new(Self::Pane { pane_id: pane }),
        };
        Ok(())
    }

    pub fn insert_at_edge(
        &mut self,
        target: &str,
        pane: PaneId,
        edge: Edge,
        ratio: f64,
    ) -> Result<(), ModelError> {
        if self.pane_ids().iter().any(|id| id == &pane) {
            return Err(ModelError::Duplicate(pane));
        }
        if !(0.05..=0.95).contains(&ratio) {
            return Err(ModelError::InvalidRatio);
        }
        let target_node = self
            .target_mut(target)
            .ok_or_else(|| ModelError::NotFound(target.into()))?;
        let old = target_node.clone();
        let new = Self::Pane { pane_id: pane };
        let (direction, first, second) = match edge {
            Edge::Left => (Direction::Horizontal, new, old),
            Edge::Right => (Direction::Horizontal, old, new),
            Edge::Top => (Direction::Vertical, new, old),
            Edge::Bottom => (Direction::Vertical, old, new),
        };
        *target_node = Self::Split {
            direction,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tree() -> LayoutNode {
        LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane {
                pane_id: "a".into(),
            }),
            second: Box::new(LayoutNode::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane {
                    pane_id: "b".into(),
                }),
                second: Box::new(LayoutNode::Pane {
                    pane_id: "c".into(),
                }),
            }),
        }
    }
    #[test]
    fn swap_preserves() {
        let mut t = tree();
        t.swap("a", "c").unwrap();
        assert_eq!(t.pane_ids(), ["c", "b", "a"]);
        t.validate().unwrap()
    }
    #[test]
    fn reparent_preserves() {
        let mut t = tree();
        t.reparent("c", "a", Edge::Left).unwrap();
        assert_eq!(t.pane_ids(), ["c", "a", "b"]);
        t.validate().unwrap()
    }
    #[test]
    fn ratios_checked() {
        let mut t = tree();
        assert!(t.set_ratio(&[], 0.8).is_ok());
        assert!(t.set_ratio(&[], 1.0).is_err())
    }

    #[test]
    fn inserts_new_pane_on_requested_edge() {
        let mut t = LayoutNode::Pane {
            pane_id: "a".into(),
        };
        t.insert_at_edge("a", "draft".into(), Edge::Left, 0.5)
            .unwrap();
        assert_eq!(t.pane_ids(), ["draft", "a"]);
        assert!(matches!(
            t,
            LayoutNode::Split {
                direction: Direction::Horizontal,
                ..
            }
        ));
    }

    #[test]
    fn balance_sets_every_split_to_half() {
        let mut t = tree();
        t.set_ratio(&[], 0.8).unwrap();
        t.set_ratio(&[true], 0.7).unwrap();
        assert!(t.balance_splits());
        assert_eq!(t.ratio_at(&[]), Some(0.5));
        assert_eq!(t.ratio_at(&[true]), Some(0.5));
        assert!(!t.balance_splits());
        t.validate().unwrap();
    }
}
