use crate::{
    herdr::{ApplyProgress, HerdrClient, Snapshot, Transaction},
    model::{Edge, Geometry, LayoutNode, PaneId, Rect, SplitPath},
};

#[derive(Clone, Debug, PartialEq)]
pub struct DropPreview {
    pub pane_id: PaneId,
    pub edge: Option<Edge>,
}

pub struct App {
    pub snapshot: Snapshot,
    pub preview: LayoutNode,
    pub undo: Vec<LayoutNode>,
    pub selected: PaneId,
    pub carrying: Option<PaneId>,
    pub drop_edge: Option<Edge>,
    pub selected_split: SplitPath,
    pub message: Option<String>,
    pub show_help: bool,
    pub dragging: Option<PaneId>,
    pub drop_preview: Option<DropPreview>,
    pub progress: Option<ApplyProgress>,
}
impl App {
    pub fn new(snapshot: Snapshot) -> Self {
        let selected = snapshot.focused_pane_id.clone();
        let preview = snapshot.tree.clone();
        Self {
            snapshot,
            preview,
            undo: vec![],
            selected,
            carrying: None,
            drop_edge: None,
            selected_split: vec![],
            message: None,
            show_help: false,
            dragging: None,
            drop_preview: None,
            progress: None,
        }
    }
    fn edit(&mut self, f: impl FnOnce(&mut LayoutNode) -> Result<(), crate::model::ModelError>) {
        let old = self.preview.clone();
        match f(&mut self.preview) {
            Ok(()) => self.undo.push(old),
            Err(e) => self.message = Some(e.to_string()),
        }
    }
    pub fn swap(&mut self, a: &str, b: &str) {
        self.edit(|t| t.swap(a, b))
    }
    pub fn reparent(&mut self, a: &str, b: &str, e: Edge) {
        self.edit(|t| t.reparent(a, b, e))
    }
    pub fn undo(&mut self) {
        if let Some(t) = self.undo.pop() {
            self.preview = t
        }
    }
    pub fn reset(&mut self) {
        if self.preview != self.snapshot.tree {
            self.undo.push(self.preview.clone());
            self.preview = self.snapshot.tree.clone()
        }
    }
    pub fn resize_selected_split(&mut self, delta: f64) {
        let path = self.selected_split.clone();
        let mut node = &self.preview;
        for second in &path {
            node = match node {
                LayoutNode::Split {
                    first, second: b, ..
                } => {
                    if *second {
                        b
                    } else {
                        first
                    }
                }
                LayoutNode::Pane { .. } => return,
            };
        }
        let LayoutNode::Split { ratio, .. } = node else {
            return;
        };
        let ratio = (*ratio + delta).clamp(0.05, 0.95);
        self.edit(|t| t.set_ratio(&path, ratio));
    }
    pub fn set_split_ratio(&mut self, path: SplitPath, ratio: f64) {
        self.selected_split = path.clone();
        self.edit(|t| t.set_ratio(&path, ratio.clamp(0.05, 0.95)));
    }
    pub fn move_selection(&mut self, delta: isize) {
        let ids = self.preview.pane_ids();
        let i = ids.iter().position(|p| p == &self.selected).unwrap_or(0) as isize;
        self.selected = ids[(i + delta).rem_euclid(ids.len() as isize) as usize].clone()
    }
    pub fn move_selection_spatial(&mut self, edge: Edge) {
        let geometry = Geometry::calculate(
            &self.preview,
            Rect {
                x: 0,
                y: 0,
                width: 1_000,
                height: 1_000,
            },
        );
        let Some(current) = geometry
            .panes
            .iter()
            .find(|pane| pane.pane_id == self.selected)
        else {
            return;
        };
        let center = |rect: Rect| {
            (
                rect.x as i32 * 2 + rect.width as i32,
                rect.y as i32 * 2 + rect.height as i32,
            )
        };
        let (cx, cy) = center(current.rect);
        let best = geometry
            .panes
            .iter()
            .filter(|pane| pane.pane_id != self.selected)
            .filter_map(|pane| {
                let (x, y) = center(pane.rect);
                let (primary, secondary) = match edge {
                    Edge::Left if x < cx => (cx - x, (cy - y).abs()),
                    Edge::Right if x > cx => (x - cx, (cy - y).abs()),
                    Edge::Top if y < cy => (cy - y, (cx - x).abs()),
                    Edge::Bottom if y > cy => (y - cy, (cx - x).abs()),
                    _ => return None,
                };
                Some((primary * 10_000 + secondary, pane.pane_id.clone()))
            })
            .min_by_key(|(score, _)| *score);
        if let Some((_, pane_id)) = best {
            self.selected = pane_id;
        }
    }
    pub fn toggle_carry(&mut self) {
        if let Some(src) = self.carrying.take() {
            let target = self.selected.clone();
            if src != target {
                if let Some(edge) = self.drop_edge.take() {
                    self.reparent(&src, &target, edge)
                } else {
                    self.swap(&src, &target)
                }
            }
        } else {
            self.carrying = Some(self.selected.clone())
        }
    }
    pub async fn apply<C: HerdrClient>(&mut self, c: &C) -> anyhow::Result<()> {
        Transaction {
            client: c,
            snapshot: &self.snapshot,
        }
        .apply(&self.preview)
        .await
    }
}
