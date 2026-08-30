use super::{Direction, LayoutNode, ModelError, PaneId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresetKind {
    MainLeft,
    MainRight,
    MainTop,
    MainBottom,
    Grid2x2,
    MainGrid,
    Grid3x2,
    Grid2x3,
    Grid3x3,
}

impl PresetKind {
    pub const ALL: [Self; 9] = [
        Self::MainLeft,
        Self::MainRight,
        Self::MainTop,
        Self::MainBottom,
        Self::Grid2x2,
        Self::MainGrid,
        Self::Grid3x2,
        Self::Grid2x3,
        Self::Grid3x3,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::MainLeft => "Main left + stack",
            Self::MainRight => "Stack + main right",
            Self::MainTop => "Main top + pair",
            Self::MainBottom => "Pair + main bottom",
            Self::Grid2x2 => "Grid 2×2",
            Self::MainGrid => "Main + grid 2×2",
            Self::Grid3x2 => "Grid 3×2",
            Self::Grid2x3 => "Grid 2×3",
            Self::Grid3x3 => "Grid 3×3",
        }
    }

    pub fn slots(self) -> usize {
        match self {
            Self::MainLeft | Self::MainRight | Self::MainTop | Self::MainBottom => 3,
            Self::Grid2x2 => 4,
            Self::MainGrid => 5,
            Self::Grid3x2 | Self::Grid2x3 => 6,
            Self::Grid3x3 => 9,
        }
    }

    pub fn has_main(self) -> bool {
        matches!(
            self,
            Self::MainLeft | Self::MainRight | Self::MainTop | Self::MainBottom | Self::MainGrid
        )
    }

    pub fn preview_divisors(self) -> (u16, u16) {
        match self {
            Self::MainLeft | Self::MainRight | Self::MainTop | Self::MainBottom => (2, 2),
            Self::Grid2x2 => (2, 2),
            Self::MainGrid => (4, 2),
            Self::Grid3x2 => (3, 2),
            Self::Grid2x3 => (2, 3),
            Self::Grid3x3 => (3, 3),
        }
    }

    pub fn build(self, ids: &[PaneId]) -> Result<LayoutNode, ModelError> {
        if ids.len() != self.slots() {
            return Err(ModelError::WrongPaneCount {
                expected: self.slots(),
                actual: ids.len(),
            });
        }
        let panes = ids.iter().cloned().map(pane).collect::<Vec<_>>();
        Ok(match self {
            Self::MainLeft => split(
                Direction::Horizontal,
                0.5,
                panes[0].clone(),
                even_line(panes[1..].to_vec(), Direction::Vertical),
            ),
            Self::MainRight => split(
                Direction::Horizontal,
                0.5,
                even_line(panes[1..].to_vec(), Direction::Vertical),
                panes[0].clone(),
            ),
            Self::MainTop => split(
                Direction::Vertical,
                0.5,
                panes[0].clone(),
                even_line(panes[1..].to_vec(), Direction::Horizontal),
            ),
            Self::MainBottom => split(
                Direction::Vertical,
                0.5,
                even_line(panes[1..].to_vec(), Direction::Horizontal),
                panes[0].clone(),
            ),
            Self::Grid2x2 => grid(panes, 2, 2),
            Self::MainGrid => split(
                Direction::Horizontal,
                0.5,
                panes[0].clone(),
                grid(panes[1..].to_vec(), 2, 2),
            ),
            Self::Grid3x2 => grid(panes, 3, 2),
            Self::Grid2x3 => grid(panes, 2, 3),
            Self::Grid3x3 => grid(panes, 3, 3),
        })
    }
}

fn pane(pane_id: PaneId) -> LayoutNode {
    LayoutNode::Pane { pane_id }
}

fn split(direction: Direction, ratio: f64, first: LayoutNode, second: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        direction,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn even_line(mut nodes: Vec<LayoutNode>, direction: Direction) -> LayoutNode {
    if nodes.len() == 1 {
        return nodes.pop().unwrap();
    }
    let first = nodes.remove(0);
    let ratio = 1.0 / (nodes.len() + 1) as f64;
    split(direction, ratio, first, even_line(nodes, direction))
}

fn grid(nodes: Vec<LayoutNode>, columns: usize, rows: usize) -> LayoutNode {
    debug_assert_eq!(nodes.len(), columns * rows);
    let rows = nodes
        .chunks(columns)
        .map(|row| even_line(row.to_vec(), Direction::Horizontal))
        .collect();
    even_line(rows, Direction::Vertical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(count: usize) -> Vec<String> {
        (1..=count).map(|index| format!("p{index}")).collect()
    }

    #[test]
    fn every_preset_builds_a_valid_tree_with_each_slot_once() {
        for preset in PresetKind::ALL {
            let ids = ids(preset.slots());
            let tree = preset.build(&ids).unwrap();
            let mut actual = tree.pane_ids();
            let mut expected = ids;
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected, "{}", preset.title());
            tree.validate().unwrap();
        }
    }

    #[test]
    fn three_by_three_uses_equal_global_rows_and_columns() {
        let tree = PresetKind::Grid3x3.build(&ids(9)).unwrap();
        assert!((tree.ratio_at(&[]).unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(tree.ratio_at(&[true]), Some(0.5));
        assert!((tree.ratio_at(&[false]).unwrap() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn two_by_two_renders_four_equal_cells() {
        let tree = PresetKind::Grid2x2.build(&ids(4)).unwrap();
        let geometry = crate::model::Geometry::calculate(
            &tree,
            crate::model::Rect {
                x: 0,
                y: 0,
                width: 120,
                height: 60,
            },
        );
        assert_eq!(geometry.panes.len(), 4);
        assert!(geometry
            .panes
            .iter()
            .all(|pane| pane.rect.width == 60 && pane.rect.height == 30));
    }

    #[test]
    fn main_templates_keep_slot_one_in_the_main_region() {
        let ids = ids(3);
        for preset in [
            PresetKind::MainLeft,
            PresetKind::MainRight,
            PresetKind::MainTop,
            PresetKind::MainBottom,
        ] {
            assert_eq!(
                preset
                    .build(&ids)
                    .unwrap()
                    .pane_ids()
                    .iter()
                    .filter(|id| *id == "p1")
                    .count(),
                1
            );
        }
    }
}
