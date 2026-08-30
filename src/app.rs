use crate::{
    herdr::{ApplyProgress, HerdrClient, Snapshot, Transaction},
    model::{
        is_draft_pane, Edge, Geometry, LayoutNode, PaneId, Rect, SplitPath, DRAFT_PANE_PREFIX,
    },
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
    pub add_mode: bool,
    pub add_target: Option<PaneId>,
    next_draft: u64,
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
            add_mode: false,
            add_target: None,
            next_draft: 1,
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
            self.preview = t;
            self.repair_selection();
        }
    }
    pub fn reset(&mut self) {
        if self.preview != self.snapshot.tree {
            self.undo.push(self.preview.clone());
            self.preview = self.snapshot.tree.clone()
        }
        self.repair_selection();
    }
    pub fn balance_splits(&mut self) {
        let old = self.preview.clone();
        if self.preview.balance_splits() {
            self.undo.push(old);
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
    pub fn toggle_add_mode(&mut self) {
        self.add_mode = !self.add_mode;
        self.add_target = None;
        self.carrying = None;
        self.dragging = None;
        self.drop_preview = None;
    }
    pub fn exit_add_mode(&mut self) {
        self.add_mode = false;
        self.add_target = None;
    }
    pub fn cancel_add_mode(&mut self) {
        self.preview = self.snapshot.tree.clone();
        self.undo.clear();
        self.carrying = None;
        self.dragging = None;
        self.drop_preview = None;
        self.exit_add_mode();
        self.repair_selection();
    }
    pub fn select_add_target(&mut self, pane_id: PaneId) {
        self.selected = pane_id.clone();
        self.add_target = Some(pane_id);
    }
    pub fn add_draft(&mut self, target: &str, edge: Edge) {
        let id = format!("{DRAFT_PANE_PREFIX}{}", self.next_draft);
        self.next_draft += 1;
        let old = self.preview.clone();
        match self.preview.insert_at_edge(target, id.clone(), edge, 0.5) {
            Ok(_) => {
                self.undo.push(old);
                self.selected = id.clone();
                self.add_target = Some(id);
            }
            Err(error) => self.message = Some(error.to_string()),
        }
    }
    pub fn remove_selected_draft(&mut self) {
        if !is_draft_pane(&self.selected) {
            self.message = Some("Only new draft panes can be deleted here".into());
            return;
        }
        let selected = self.selected.clone();
        let old = self.preview.clone();
        match self.preview.detach_pane(&selected) {
            Ok(_) => {
                self.undo.push(old);
                self.repair_selection();
                self.add_target = None;
            }
            Err(error) => self.message = Some(error.to_string()),
        }
    }
    fn repair_selection(&mut self) {
        let ids = self.preview.pane_ids();
        if !ids.iter().any(|id| id == &self.selected) {
            self.selected = ids
                .iter()
                .find(|id| !is_draft_pane(id))
                .or_else(|| ids.first())
                .cloned()
                .unwrap_or_default();
        }
        if self
            .add_target
            .as_ref()
            .is_some_and(|id| !ids.iter().any(|pane| pane == id))
        {
            self.add_target = None;
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
