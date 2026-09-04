use super::{Direction, Geometry, LayoutNode, PaneId, Rect};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_TEMPLATE_SLOTS: usize = 25;
const MAX_TEMPLATE_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TemplateNode {
    Slot {
        slot: u16,
    },
    Split {
        direction: Direction,
        ratio: f64,
        first: Box<TemplateNode>,
        second: Box<TemplateNode>,
    },
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum TemplateError {
    #[error("selected pane is not part of the layout")]
    MissingAnchor,
    #[error("layout must contain between 1 and {MAX_TEMPLATE_SLOTS} panes")]
    InvalidSlotCount,
    #[error("layout nesting is too deep")]
    TooDeep,
    #[error("split ratio must be between 0.05 and 0.95")]
    InvalidRatio,
    #[error("layout contains duplicate slot {0}")]
    DuplicateSlot(u16),
    #[error("layout slots must be numbered consecutively from 1")]
    NonContiguousSlots,
    #[error("anchor slot {0} does not exist")]
    InvalidAnchor(u16),
    #[error("layout has {slots} slots but the current preview has {panes} panes")]
    TooManyPanes { panes: usize, slots: usize },
    #[error("missing pane assignment for slot {0}")]
    MissingAssignment(u16),
}

impl TemplateNode {
    pub fn capture(tree: &LayoutNode, selected: &str) -> Result<(Self, u16), TemplateError> {
        let ordered = visual_pane_ids(tree);
        if ordered.is_empty() || ordered.len() > MAX_TEMPLATE_SLOTS {
            return Err(TemplateError::InvalidSlotCount);
        }
        let slots = ordered
            .iter()
            .enumerate()
            .map(|(index, pane_id)| (pane_id.clone(), index as u16 + 1))
            .collect::<HashMap<_, _>>();
        let anchor = *slots.get(selected).ok_or(TemplateError::MissingAnchor)?;

        fn convert(node: &LayoutNode, slots: &HashMap<PaneId, u16>) -> TemplateNode {
            match node {
                LayoutNode::Pane { pane_id } => TemplateNode::Slot {
                    slot: slots[pane_id],
                },
                LayoutNode::Split {
                    direction,
                    ratio,
                    first,
                    second,
                } => TemplateNode::Split {
                    direction: *direction,
                    ratio: *ratio,
                    first: Box::new(convert(first, slots)),
                    second: Box::new(convert(second, slots)),
                },
            }
        }

        let template = convert(tree, &slots);
        template.validate(anchor)?;
        Ok((template, anchor))
    }

    pub fn slot_count(&self) -> usize {
        self.slots().len()
    }

    pub fn validate(&self, anchor_slot: u16) -> Result<(), TemplateError> {
        fn visit(
            node: &TemplateNode,
            depth: usize,
            slots: &mut HashSet<u16>,
        ) -> Result<(), TemplateError> {
            if depth > MAX_TEMPLATE_DEPTH {
                return Err(TemplateError::TooDeep);
            }
            match node {
                TemplateNode::Slot { slot } => {
                    if !slots.insert(*slot) {
                        return Err(TemplateError::DuplicateSlot(*slot));
                    }
                }
                TemplateNode::Split {
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    if !ratio.is_finite() || !(0.05..=0.95).contains(ratio) {
                        return Err(TemplateError::InvalidRatio);
                    }
                    visit(first, depth + 1, slots)?;
                    visit(second, depth + 1, slots)?;
                }
            }
            Ok(())
        }

        let mut slots = HashSet::new();
        visit(self, 0, &mut slots)?;
        if slots.is_empty() || slots.len() > MAX_TEMPLATE_SLOTS {
            return Err(TemplateError::InvalidSlotCount);
        }
        if !(1..=slots.len() as u16).all(|slot| slots.contains(&slot)) {
            return Err(TemplateError::NonContiguousSlots);
        }
        if !slots.contains(&anchor_slot) {
            return Err(TemplateError::InvalidAnchor(anchor_slot));
        }
        Ok(())
    }

    pub fn instantiate_current(
        &self,
        anchor_slot: u16,
        source: &LayoutNode,
        selected: &str,
        mut fresh_draft: impl FnMut() -> PaneId,
    ) -> Result<LayoutNode, TemplateError> {
        self.validate(anchor_slot)?;
        let slot_count = self.slot_count();
        let mut panes = visual_pane_ids(source);
        if panes.len() > slot_count {
            return Err(TemplateError::TooManyPanes {
                panes: panes.len(),
                slots: slot_count,
            });
        }
        let selected_index = panes
            .iter()
            .position(|pane_id| pane_id == selected)
            .ok_or(TemplateError::MissingAnchor)?;
        let anchor_pane = panes.remove(selected_index);
        let mut assignments = HashMap::new();
        assignments.insert(anchor_slot, anchor_pane);
        for slot in 1..=slot_count as u16 {
            if slot == anchor_slot {
                continue;
            }
            assignments.insert(
                slot,
                if panes.is_empty() {
                    fresh_draft()
                } else {
                    panes.remove(0)
                },
            );
        }
        self.instantiate(&assignments)
    }

    pub fn preview_tree(&self) -> Result<LayoutNode, TemplateError> {
        let assignments = self
            .slots()
            .into_iter()
            .map(|slot| (slot, slot.to_string()))
            .collect();
        self.instantiate(&assignments)
    }

    fn instantiate(&self, assignments: &HashMap<u16, PaneId>) -> Result<LayoutNode, TemplateError> {
        Ok(match self {
            TemplateNode::Slot { slot } => LayoutNode::Pane {
                pane_id: assignments
                    .get(slot)
                    .cloned()
                    .ok_or(TemplateError::MissingAssignment(*slot))?,
            },
            TemplateNode::Split {
                direction,
                ratio,
                first,
                second,
            } => LayoutNode::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(first.instantiate(assignments)?),
                second: Box::new(second.instantiate(assignments)?),
            },
        })
    }

    fn slots(&self) -> Vec<u16> {
        fn visit(node: &TemplateNode, slots: &mut Vec<u16>) {
            match node {
                TemplateNode::Slot { slot } => slots.push(*slot),
                TemplateNode::Split { first, second, .. } => {
                    visit(first, slots);
                    visit(second, slots);
                }
            }
        }
        let mut slots = Vec::new();
        visit(self, &mut slots);
        slots
    }
}

pub fn visual_pane_ids(tree: &LayoutNode) -> Vec<PaneId> {
    let mut panes = Geometry::calculate(
        tree,
        Rect {
            x: 0,
            y: 0,
            width: 10_000,
            height: 10_000,
        },
    )
    .panes;
    panes.sort_by_key(|pane| (pane.rect.y, pane.rect.x));
    panes.into_iter().map(|pane| pane.pane_id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LayoutNode {
        LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.4,
            first: Box::new(LayoutNode::Pane {
                pane_id: "a".into(),
            }),
            second: Box::new(LayoutNode::Split {
                direction: Direction::Vertical,
                ratio: 0.6,
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
    fn capture_round_trip_preserves_geometry() {
        let source = sample();
        let (template, anchor) = TemplateNode::capture(&source, "b").unwrap();
        assert_eq!(anchor, 2);
        let result = template
            .instantiate_current(anchor, &source, "b", || "draft".into())
            .unwrap();
        assert_eq!(result, source);
    }

    #[test]
    fn anchor_maps_selected_pane_and_missing_slots_become_drafts() {
        let (template, anchor) = TemplateNode::capture(&sample(), "b").unwrap();
        let source = LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane {
                pane_id: "x".into(),
            }),
            second: Box::new(LayoutNode::Pane {
                pane_id: "y".into(),
            }),
        };
        let result = template
            .instantiate_current(anchor, &source, "y", || "new".into())
            .unwrap();
        assert_eq!(result.pane_ids(), vec!["x", "y", "new"]);
    }

    #[test]
    fn extra_panes_are_rejected() {
        let (template, anchor) = TemplateNode::capture(&sample(), "a").unwrap();
        let four = crate::model::PresetKind::Grid2x2
            .build(&["a".into(), "b".into(), "c".into(), "d".into()])
            .unwrap();
        assert!(matches!(
            template.instantiate_current(anchor, &four, "a", || "new".into()),
            Err(TemplateError::TooManyPanes { .. })
        ));
    }
}
