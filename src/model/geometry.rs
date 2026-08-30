use super::{Direction, Edge, LayoutNode, PaneId, SplitPath};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PaneRect {
    pub pane_id: PaneId,
    pub rect: Rect,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub path: SplitPath,
    pub rect: Rect,
    pub bounds: Rect,
    pub direction: Direction,
}
#[derive(Clone, Debug, PartialEq)]
pub struct AddZone {
    pub pane_id: PaneId,
    pub edge: Edge,
    pub rect: Rect,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PresetCardZone {
    pub index: usize,
    pub rect: Rect,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
    pub add_zones: Vec<AddZone>,
    pub preset_cards: Vec<PresetCardZone>,
    pub preset_destination: Option<Rect>,
    pub preset_apply: Option<Rect>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Pane(PaneId),
    Edge(PaneId, Edge),
    Divider(SplitPath),
}

impl Geometry {
    pub fn calculate(tree: &LayoutNode, area: Rect) -> Self {
        fn go(n: &LayoutNode, r: Rect, p: &mut SplitPath, g: &mut Geometry) {
            match n {
                LayoutNode::Pane { pane_id } => g.panes.push(PaneRect {
                    pane_id: pane_id.clone(),
                    rect: r,
                }),
                LayoutNode::Split {
                    direction,
                    ratio,
                    first,
                    second,
                } => match direction {
                    Direction::Horizontal => {
                        if r.width < 2 {
                            go(first, r, p, g);
                            go(second, r, p, g);
                            return;
                        }
                        let a = (((r.width as f64) * ratio).round() as u16).clamp(1, r.width - 1);
                        g.dividers.push(Divider {
                            path: p.clone(),
                            rect: Rect {
                                x: r.x + a,
                                y: r.y,
                                width: 1,
                                height: r.height,
                            },
                            bounds: r,
                            direction: *direction,
                        });
                        p.push(false);
                        go(
                            first,
                            Rect {
                                x: r.x,
                                y: r.y,
                                width: a,
                                height: r.height,
                            },
                            p,
                            g,
                        );
                        p.pop();
                        p.push(true);
                        go(
                            second,
                            Rect {
                                x: r.x.saturating_add(a),
                                y: r.y,
                                width: r.width.saturating_sub(a),
                                height: r.height,
                            },
                            p,
                            g,
                        );
                        p.pop();
                    }
                    Direction::Vertical => {
                        if r.height < 2 {
                            go(first, r, p, g);
                            go(second, r, p, g);
                            return;
                        }
                        let a = (((r.height as f64) * ratio).round() as u16).clamp(1, r.height - 1);
                        g.dividers.push(Divider {
                            path: p.clone(),
                            rect: Rect {
                                x: r.x,
                                y: r.y + a,
                                width: r.width,
                                height: 1,
                            },
                            bounds: r,
                            direction: *direction,
                        });
                        p.push(false);
                        go(
                            first,
                            Rect {
                                x: r.x,
                                y: r.y,
                                width: r.width,
                                height: a,
                            },
                            p,
                            g,
                        );
                        p.pop();
                        p.push(true);
                        go(
                            second,
                            Rect {
                                x: r.x,
                                y: r.y.saturating_add(a),
                                width: r.width,
                                height: r.height.saturating_sub(a),
                            },
                            p,
                            g,
                        );
                        p.pop();
                    }
                },
            }
        }
        let mut g = Self::default();
        go(tree, area, &mut vec![], &mut g);
        g
    }
    pub fn hit(&self, x: u16, y: u16) -> Option<Hit> {
        if let Some(d) = self.dividers.iter().find(|d| inside(d.rect, x, y)) {
            return Some(Hit::Divider(d.path.clone()));
        }
        let p = self.panes.iter().find(|p| inside(p.rect, x, y))?;
        let rx = x - p.rect.x;
        let ry = y - p.rect.y;
        let edge_x = (p.rect.width / 4).max(1);
        let edge_y = (p.rect.height / 4).max(1);
        let e = if rx < edge_x {
            Some(Edge::Left)
        } else if rx >= p.rect.width.saturating_sub(edge_x) {
            Some(Edge::Right)
        } else if ry < edge_y {
            Some(Edge::Top)
        } else if ry >= p.rect.height.saturating_sub(edge_y) {
            Some(Edge::Bottom)
        } else {
            None
        };
        Some(e.map_or_else(
            || Hit::Pane(p.pane_id.clone()),
            |e| Hit::Edge(p.pane_id.clone(), e),
        ))
    }
    pub fn hit_add_zone(&self, x: u16, y: u16) -> Option<&AddZone> {
        self.add_zones.iter().find(|zone| inside(zone.rect, x, y))
    }
    pub fn hit_preset_card(&self, x: u16, y: u16) -> Option<usize> {
        self.preset_cards
            .iter()
            .find(|zone| inside(zone.rect, x, y))
            .map(|zone| zone.index)
    }
    pub fn hit_preset_destination(&self, x: u16, y: u16) -> bool {
        self.preset_destination
            .is_some_and(|rect| inside(rect, x, y))
    }
    pub fn hit_preset_apply(&self, x: u16, y: u16) -> bool {
        self.preset_apply.is_some_and(|rect| inside(rect, x, y))
    }
}
fn inside(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && y >= r.y && x < r.x.saturating_add(r.width) && y < r.y.saturating_add(r.height)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn center_and_edges() {
        let t = LayoutNode::Pane {
            pane_id: "p".into(),
        };
        let g = Geometry::calculate(
            &t,
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 10,
            },
        );
        assert_eq!(g.hit(10, 5), Some(Hit::Pane("p".into())));
        assert_eq!(g.hit(0, 5), Some(Hit::Edge("p".into(), Edge::Left)));
    }
    #[test]
    fn tiny_areas_do_not_underflow() {
        let t = LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.95,
            first: Box::new(LayoutNode::Pane {
                pane_id: "a".into(),
            }),
            second: Box::new(LayoutNode::Pane {
                pane_id: "b".into(),
            }),
        };
        let g = Geometry::calculate(
            &t,
            Rect {
                x: u16::MAX - 1,
                y: u16::MAX - 1,
                width: 1,
                height: 1,
            },
        );
        assert_eq!(g.panes.len(), 2);
        assert!(g.dividers.is_empty());
        let _ = g.hit(u16::MAX - 1, u16::MAX - 1);
    }

    #[test]
    fn split_uses_child_border_as_resize_target_without_a_gap() {
        let tree = LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane {
                pane_id: "a".into(),
            }),
            second: Box::new(LayoutNode::Pane {
                pane_id: "b".into(),
            }),
        };
        let geometry = Geometry::calculate(
            &tree,
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 10,
            },
        );

        assert_eq!(
            geometry.panes[0].rect.x + geometry.panes[0].rect.width,
            geometry.panes[1].rect.x
        );
        assert_eq!(geometry.dividers[0].rect.x, geometry.panes[1].rect.x);
        assert_eq!(
            geometry.hit(geometry.panes[1].rect.x, 5),
            Some(Hit::Divider(vec![]))
        );
    }
}
