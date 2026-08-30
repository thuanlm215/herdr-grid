use crate::model::{Direction, LayoutNode, PaneId};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
pub struct WireRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WirePane {
    pub pane_id: PaneId,
    pub rect: WireRect,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WireLayout {
    pub workspace_id: String,
    pub tab_id: String,
    pub focused_pane_id: PaneId,
    pub zoomed: bool,
    pub panes: Vec<WirePane>,
    #[serde(default)]
    pub splits: Vec<WireSplit>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WireSplit {
    pub direction: WireDirection,
    pub ratio: f64,
    pub rect: WireRect,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WireDirection {
    Right,
    Down,
}
impl From<WireDirection> for Direction {
    fn from(value: WireDirection) -> Self {
        match value {
            WireDirection::Right => Direction::Horizontal,
            WireDirection::Down => Direction::Vertical,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct LayoutEnvelope {
    pub result: LayoutResult,
}
#[derive(Clone, Debug, Deserialize)]
pub struct LayoutResult {
    pub layout: WireLayout,
}
#[derive(Clone, Debug, Deserialize)]
pub struct PaneListEnvelope {
    pub result: PaneListResult,
}
#[derive(Clone, Debug, Deserialize)]
pub struct PaneListResult {
    pub panes: Vec<PaneMetadata>,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct PaneMetadata {
    pub pane_id: PaneId,
    pub tab_id: String,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub revision: u64,
    #[serde(skip)]
    pub process_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProcessEnvelope {
    pub result: ProcessResult,
}
#[derive(Debug, Deserialize)]
pub struct ProcessResult {
    pub process_info: ProcessInfo,
}
#[derive(Debug, Deserialize)]
pub struct ProcessInfo {
    #[serde(default)]
    pub foreground_processes: Vec<ForegroundProcess>,
}
#[derive(Debug, Deserialize)]
pub struct ForegroundProcess {
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub workspace_id: String,
    pub tab_id: String,
    pub focused_pane_id: PaneId,
    pub tree: LayoutNode,
    pub metadata: HashMap<PaneId, PaneMetadata>,
    pub revisions: HashMap<PaneId, u64>,
}

pub fn tree_from_rects(mut panes: Vec<WirePane>) -> anyhow::Result<LayoutNode> {
    fn build(p: &mut [WirePane]) -> anyhow::Result<LayoutNode> {
        if p.len() == 1 {
            return Ok(LayoutNode::Pane {
                pane_id: p[0].pane_id.clone(),
            });
        }
        let min_x = p.iter().map(|v| v.rect.x).min().unwrap();
        let max_x = p.iter().map(|v| v.rect.x + v.rect.width).max().unwrap();
        for cut in (min_x + 1)..max_x {
            let left = p.iter().filter(|v| v.rect.x + v.rect.width <= cut).count();
            let right = p.iter().filter(|v| v.rect.x >= cut).count();
            if left > 0 && left + right == p.len() {
                p.sort_by_key(|v| v.rect.x >= cut);
                let (a, b) = p.split_at_mut(left);
                let total = (max_x - min_x) as f64;
                return Ok(LayoutNode::Split {
                    direction: Direction::Horizontal,
                    ratio: (cut - min_x) as f64 / total,
                    first: Box::new(build(a)?),
                    second: Box::new(build(b)?),
                });
            }
        }
        let min_y = p.iter().map(|v| v.rect.y).min().unwrap();
        let max_y = p.iter().map(|v| v.rect.y + v.rect.height).max().unwrap();
        for cut in (min_y + 1)..max_y {
            let top = p.iter().filter(|v| v.rect.y + v.rect.height <= cut).count();
            let bottom = p.iter().filter(|v| v.rect.y >= cut).count();
            if top > 0 && top + bottom == p.len() {
                p.sort_by_key(|v| v.rect.y >= cut);
                let (a, b) = p.split_at_mut(top);
                let total = (max_y - min_y) as f64;
                return Ok(LayoutNode::Split {
                    direction: Direction::Vertical,
                    ratio: (cut - min_y) as f64 / total,
                    first: Box::new(build(a)?),
                    second: Box::new(build(b)?),
                });
            }
        }
        anyhow::bail!("pane rectangles do not form a binary split tree")
    }
    if panes.is_empty() {
        anyhow::bail!("active tab has no panes")
    }
    build(&mut panes)
}

pub fn tree_from_layout(layout: &WireLayout) -> anyhow::Result<LayoutNode> {
    if layout.splits.is_empty() {
        return tree_from_rects(layout.panes.clone());
    }
    fn bounds(panes: &[WirePane]) -> WireRect {
        let x = panes.iter().map(|p| p.rect.x).min().unwrap();
        let y = panes.iter().map(|p| p.rect.y).min().unwrap();
        let right = panes.iter().map(|p| p.rect.x + p.rect.width).max().unwrap();
        let bottom = panes
            .iter()
            .map(|p| p.rect.y + p.rect.height)
            .max()
            .unwrap();
        WireRect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
    fn build(panes: &[WirePane], splits: &[WireSplit]) -> anyhow::Result<LayoutNode> {
        if panes.len() == 1 {
            return Ok(LayoutNode::Pane {
                pane_id: panes[0].pane_id.clone(),
            });
        }
        let area = bounds(panes);
        let split = splits
            .iter()
            .find(|s| {
                s.rect.x == area.x
                    && s.rect.y == area.y
                    && s.rect.width == area.width
                    && s.rect.height == area.height
            })
            .ok_or_else(|| anyhow::anyhow!("authoritative split missing for subtree bounds"))?;
        let expected = match split.direction {
            WireDirection::Right => area.x as f64 + area.width as f64 * split.ratio,
            WireDirection::Down => area.y as f64 + area.height as f64 * split.ratio,
        };
        let range = match split.direction {
            WireDirection::Right => (area.x + 1)..(area.x + area.width),
            WireDirection::Down => (area.y + 1)..(area.y + area.height),
        };
        let cut = range
            .filter(|cut| {
                let first = panes
                    .iter()
                    .filter(|p| match split.direction {
                        WireDirection::Right => p.rect.x + p.rect.width <= *cut,
                        WireDirection::Down => p.rect.y + p.rect.height <= *cut,
                    })
                    .count();
                let second = panes
                    .iter()
                    .filter(|p| match split.direction {
                        WireDirection::Right => p.rect.x >= *cut,
                        WireDirection::Down => p.rect.y >= *cut,
                    })
                    .count();
                first > 0 && first + second == panes.len()
            })
            .min_by(|a, b| ((*a as f64 - expected).abs()).total_cmp(&(*b as f64 - expected).abs()))
            .ok_or_else(|| anyhow::anyhow!("panes do not partition along authoritative split"))?;
        let (first, second): (Vec<_>, Vec<_>) =
            panes.iter().cloned().partition(|p| match split.direction {
                WireDirection::Right => p.rect.x + p.rect.width <= cut,
                WireDirection::Down => p.rect.y + p.rect.height <= cut,
            });
        Ok(LayoutNode::Split {
            direction: split.direction.into(),
            ratio: split.ratio,
            first: Box::new(build(&first, splits)?),
            second: Box::new(build(&second, splits)?),
        })
    }
    if layout.panes.is_empty() {
        anyhow::bail!("active tab has no panes")
    }
    build(&layout.panes, &layout.splits)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(id: &str, x: u16, y: u16, width: u16, height: u16) -> WirePane {
        WirePane {
            pane_id: id.into(),
            rect: WireRect {
                x,
                y,
                width,
                height,
            },
        }
    }

    #[test]
    fn reconstructs_nested_rectangles() {
        let t = tree_from_rects(vec![
            p("a", 0, 0, 49, 100),
            p("b", 50, 0, 50, 49),
            p("c", 50, 50, 50, 50),
        ])
        .unwrap();
        assert_eq!(t.pane_ids(), ["a", "b", "c"]);
        t.validate().unwrap();
    }

    #[test]
    fn rejects_overlap() {
        assert!(tree_from_rects(vec![p("a", 0, 0, 10, 10), p("b", 5, 5, 10, 10)]).is_err());
    }
    #[test]
    fn live_right_down_json_deserializes() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w1","tab_id":"w1:t1","focused_pane_id":"w1:p1","zoomed":false,"panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":49,"height":10}},{"pane_id":"w1:p2","rect":{"x":50,"y":0,"width":50,"height":10}}],"splits":[{"id":"s1","direction":"right","ratio":0.25,"rect":{"x":0,"y":0,"width":100,"height":10}}]}}}"#;
        let envelope: LayoutEnvelope = serde_json::from_str(raw).unwrap();
        let tree = tree_from_layout(&envelope.result.layout).unwrap();
        assert!(
            matches!(tree,LayoutNode::Split{direction:Direction::Horizontal,ratio,..} if ratio==0.25)
        );
    }

    #[test]
    fn authoritative_direction_resolves_aligned_grid_grouping() {
        let layout = WireLayout {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "a".into(),
            zoomed: false,
            panes: vec![
                p("a", 0, 0, 49, 49),
                p("b", 50, 0, 50, 49),
                p("c", 0, 50, 49, 50),
                p("d", 50, 50, 50, 50),
            ],
            splits: vec![
                WireSplit {
                    direction: WireDirection::Down,
                    ratio: 0.5,
                    rect: WireRect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                },
                WireSplit {
                    direction: WireDirection::Right,
                    ratio: 0.5,
                    rect: WireRect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 49,
                    },
                },
                WireSplit {
                    direction: WireDirection::Right,
                    ratio: 0.5,
                    rect: WireRect {
                        x: 0,
                        y: 50,
                        width: 100,
                        height: 50,
                    },
                },
            ],
        };
        let tree = tree_from_layout(&layout).unwrap();
        assert!(matches!(
            tree,
            LayoutNode::Split {
                direction: Direction::Vertical,
                ..
            }
        ));
        assert_eq!(tree.pane_ids(), ["a", "b", "c", "d"]);
    }
}
