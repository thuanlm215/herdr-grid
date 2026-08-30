use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub type PaneId = String;
pub type SplitPath = Vec<bool>; // false=first, true=second

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
}
