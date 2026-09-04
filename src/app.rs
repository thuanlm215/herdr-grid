use crate::{
    herdr::{ApplyProgress, HerdrClient, Snapshot, Transaction},
    model::{
        is_draft_pane, Edge, Geometry, LayoutNode, PaneId, PresetKind, Rect, SplitPath,
        TemplateNode, DRAFT_PANE_PREFIX,
    },
    saved::{CatalogError, SavedCatalog, SavedLayout, MAX_LAYOUT_NAME_CHARS, MAX_SAVED_LAYOUTS},
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKind {
    Error,
    Success,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppMessage {
    pub kind: MessageKind,
    pub text: String,
    pub expires_at: Option<Instant>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropPreview {
    pub pane_id: PaneId,
    pub edge: Option<Edge>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresetPage {
    BuiltIn,
    Saved,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresetPicker {
    pub selected: usize,
    pub page: PresetPage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NamePromptKind {
    Save,
    Rename { index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamePrompt {
    pub kind: NamePromptKind,
    pub value: String,
}

pub struct App {
    pub snapshot: Snapshot,
    pub preview: LayoutNode,
    pub undo: Vec<LayoutNode>,
    pub selected: PaneId,
    pub carrying: Option<PaneId>,
    pub drop_edge: Option<Edge>,
    pub selected_split: SplitPath,
    pub message: Option<AppMessage>,
    pub show_help: bool,
    pub dragging: Option<PaneId>,
    pub drop_preview: Option<DropPreview>,
    pub progress: Option<ApplyProgress>,
    pub preset_picker: Option<PresetPicker>,
    pub saved_catalog: SavedCatalog,
    pub name_prompt: Option<NamePrompt>,
    pub delete_confirm: Option<usize>,
    next_draft: u64,
    catalog_backup: Option<SavedCatalog>,
}
impl App {
    pub fn new(snapshot: Snapshot) -> Self {
        Self::with_catalog(snapshot, SavedCatalog::default())
    }

    pub fn with_catalog(snapshot: Snapshot, saved_catalog: SavedCatalog) -> Self {
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
            preset_picker: None,
            saved_catalog,
            name_prompt: None,
            delete_confirm: None,
            next_draft: 1,
            catalog_backup: None,
        }
    }
    fn edit(&mut self, f: impl FnOnce(&mut LayoutNode) -> Result<(), crate::model::ModelError>) {
        let old = self.preview.clone();
        match f(&mut self.preview) {
            Ok(()) => self.undo.push(old),
            Err(e) => self.set_error(e),
        }
    }
    pub fn set_error(&mut self, error: impl ToString) {
        self.message = Some(AppMessage {
            kind: MessageKind::Error,
            text: error.to_string(),
            expires_at: None,
        });
    }
    pub fn set_success(&mut self, message: impl Into<String>) {
        self.message = Some(AppMessage {
            kind: MessageKind::Success,
            text: message.into(),
            expires_at: Some(Instant::now() + Duration::from_secs(3)),
        });
    }
    pub fn expire_message(&mut self) {
        if self
            .message
            .as_ref()
            .and_then(|message| message.expires_at)
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.message = None;
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
    pub fn open_preset_picker(&mut self) {
        let count = self.preview.pane_ids().len();
        let selected = PresetKind::ALL
            .iter()
            .position(|preset| preset.slots() >= count)
            .unwrap_or(0);
        self.preset_picker = Some(PresetPicker {
            selected,
            page: PresetPage::BuiltIn,
        });
    }
    pub fn close_preset_picker(&mut self) {
        self.preset_picker = None;
    }
    pub fn move_preset_selection(&mut self, delta: isize) {
        let len = self.current_preset_count();
        let Some(picker) = &mut self.preset_picker else {
            return;
        };
        if len == 0 {
            picker.selected = 0;
            return;
        }
        let len = len as isize;
        picker.selected = (picker.selected as isize + delta).rem_euclid(len) as usize;
    }
    pub fn select_preset(&mut self, index: usize) {
        let len = self.current_preset_count();
        if let Some(picker) = &mut self.preset_picker {
            if index < len {
                picker.selected = index;
            }
        }
    }
    pub fn toggle_preset_collection(&mut self) {
        let Some(picker) = &mut self.preset_picker else {
            return;
        };
        picker.page = match picker.page {
            PresetPage::BuiltIn => PresetPage::Saved,
            PresetPage::Saved => PresetPage::BuiltIn,
        };
        picker.selected = 0;
    }

    pub fn set_preset_collection(&mut self, page: PresetPage) {
        if let Some(picker) = &mut self.preset_picker {
            picker.page = page;
            picker.selected = 0;
        }
    }

    pub fn current_preset_count(&self) -> usize {
        match self.preset_picker.as_ref().map(|picker| picker.page) {
            Some(PresetPage::BuiltIn) | None => PresetKind::ALL.len(),
            Some(PresetPage::Saved) => self.saved_catalog.layouts.len().min(MAX_SAVED_LAYOUTS),
        }
    }

    pub fn saved_layout_at_picker(&self) -> Option<(usize, &SavedLayout)> {
        let picker = self.preset_picker.as_ref()?;
        let PresetPage::Saved = picker.page else {
            return None;
        };
        let index = picker.selected;
        self.saved_catalog
            .layouts
            .get(index)
            .map(|layout| (index, layout))
    }
    pub fn preset_source_count(&self) -> usize {
        self.preview.pane_ids().len()
    }
    pub fn preset_enabled(&self, preset: PresetKind) -> bool {
        self.preset_source_count() <= preset.slots()
    }
    pub fn saved_preset_enabled(&self, layout: &SavedLayout) -> bool {
        self.preset_source_count() <= layout.slots()
    }
    pub fn accept_selected_preset(&mut self) {
        let Some(picker) = self.preset_picker.clone() else {
            return;
        };
        if let PresetPage::Saved = picker.page {
            self.accept_selected_saved_preset();
            return;
        }
        let Some(preset) = PresetKind::ALL.get(picker.selected).copied() else {
            return;
        };
        if !self.preset_enabled(preset) {
            self.set_error(format!(
                "{} has {} slots but the current preview has {} panes",
                preset.title(),
                preset.slots(),
                self.preset_source_count()
            ));
            return;
        }
        let result = self.apply_current_tab_preset(preset);
        match result {
            Ok(()) => self.preset_picker = None,
            Err(error) => self.set_error(error),
        }
    }
    fn accept_selected_saved_preset(&mut self) {
        let Some((_, layout)) = self.saved_layout_at_picker() else {
            return;
        };
        let layout = layout.clone();
        if !self.saved_preset_enabled(&layout) {
            self.set_error(format!(
                "{} has {} slots but the current preview has {} panes",
                layout.name,
                layout.slots(),
                self.preset_source_count()
            ));
            return;
        }
        let result = self.apply_current_saved_preset(&layout);
        match result {
            Ok(()) => self.preset_picker = None,
            Err(error) => self.set_error(error),
        }
    }

    fn apply_current_saved_preset(
        &mut self,
        layout: &SavedLayout,
    ) -> Result<(), crate::model::TemplateError> {
        let source = self.preview.clone();
        let selected = self.selected.clone();
        let mut next_draft = self.next_draft;
        let target =
            layout
                .tree
                .instantiate_current(layout.anchor_slot, &source, &selected, || {
                    let id = format!("{DRAFT_PANE_PREFIX}{next_draft}");
                    next_draft += 1;
                    id
                })?;
        self.next_draft = next_draft;
        self.undo.push(source);
        self.preview = target;
        self.selected = selected;
        self.repair_selection();
        Ok(())
    }

    pub fn open_save_prompt(&mut self) {
        self.message = None;
        self.name_prompt = Some(NamePrompt {
            kind: NamePromptKind::Save,
            value: String::new(),
        });
    }

    pub fn open_rename_prompt(&mut self) {
        let Some((index, layout)) = self.saved_layout_at_picker() else {
            return;
        };
        self.name_prompt = Some(NamePrompt {
            kind: NamePromptKind::Rename { index },
            value: layout.name.clone(),
        });
    }

    pub fn request_delete_saved(&mut self) {
        if let Some((index, _)) = self.saved_layout_at_picker() {
            self.delete_confirm = Some(index);
        }
    }

    pub fn append_prompt_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        if let Some(prompt) = &mut self.name_prompt {
            if prompt.value.chars().count() < MAX_LAYOUT_NAME_CHARS {
                prompt.value.push(ch);
            }
        }
    }

    pub fn backspace_prompt(&mut self) {
        if let Some(prompt) = &mut self.name_prompt {
            prompt.value.pop();
        }
    }

    pub fn cancel_prompt(&mut self) {
        self.name_prompt = None;
    }

    pub fn commit_name_prompt(&mut self) -> Result<bool, CatalogError> {
        let Some(prompt) = self.name_prompt.clone() else {
            return Ok(false);
        };
        let old = self.saved_catalog.clone();
        match prompt.kind {
            NamePromptKind::Save => {
                let (tree, anchor) = TemplateNode::capture(&self.preview, &self.selected)?;
                self.saved_catalog.add(&prompt.value, tree, anchor)?;
            }
            NamePromptKind::Rename { index } => {
                self.saved_catalog.rename(index, &prompt.value)?;
            }
        }
        self.catalog_backup = Some(old);
        self.name_prompt = None;
        self.set_success(match prompt.kind {
            NamePromptKind::Save => "Custom layout saved",
            NamePromptKind::Rename { .. } => "Custom layout renamed",
        });
        Ok(true)
    }

    pub fn confirm_delete_saved(&mut self) -> Result<bool, CatalogError> {
        let Some(index) = self.delete_confirm.take() else {
            return Ok(false);
        };
        let old = self.saved_catalog.clone();
        self.saved_catalog.delete(index)?;
        self.catalog_backup = Some(old);
        if let Some(picker) = &mut self.preset_picker {
            let count = match picker.page {
                PresetPage::BuiltIn => PresetKind::ALL.len(),
                PresetPage::Saved => self.saved_catalog.layouts.len().min(MAX_SAVED_LAYOUTS),
            };
            picker.selected = picker.selected.min(count.saturating_sub(1));
        }
        self.set_success("Custom layout deleted");
        Ok(true)
    }

    pub fn has_catalog_change(&self) -> bool {
        self.catalog_backup.is_some()
    }

    pub fn catalog_saved(&mut self) {
        self.catalog_backup = None;
    }

    pub fn catalog_save_failed(&mut self, error: impl ToString) {
        if let Some(previous) = self.catalog_backup.take() {
            self.saved_catalog = previous;
        }
        self.set_error(format!(
            "Could not save custom layouts: {}",
            error.to_string()
        ));
    }
    fn apply_current_tab_preset(
        &mut self,
        preset: PresetKind,
    ) -> Result<(), crate::model::ModelError> {
        let source = self.preview.clone();
        let source_selected = self.selected.clone();
        let mut ids = source.pane_ids();
        if preset.has_main() {
            if let Some(index) = ids.iter().position(|id| id == &source_selected) {
                let main = ids.remove(index);
                ids.insert(0, main);
            }
        }
        while ids.len() < preset.slots() {
            ids.push(self.fresh_draft_id());
        }
        let target = preset.build(&ids)?;
        self.undo.push(source);
        self.preview = target;
        self.selected = source_selected;
        self.repair_selection();
        Ok(())
    }
    fn fresh_draft_id(&mut self) -> PaneId {
        let id = format!("{DRAFT_PANE_PREFIX}{}", self.next_draft);
        self.next_draft += 1;
        id
    }
    pub fn add_draft(&mut self, target: &str, edge: Edge) {
        let id = self.fresh_draft_id();
        let old = self.preview.clone();
        match self.preview.insert_at_edge(target, id.clone(), edge, 0.5) {
            Ok(_) => {
                self.undo.push(old);
                self.selected = id;
            }
            Err(error) => self.set_error(error),
        }
    }
    pub fn remove_selected_draft(&mut self) {
        if !is_draft_pane(&self.selected) {
            self.set_error("Only new draft panes can be deleted here");
            return;
        }
        let selected = self.selected.clone();
        let old = self.preview.clone();
        match self.preview.detach_pane(&selected) {
            Ok(_) => {
                self.undo.push(old);
                self.repair_selection();
            }
            Err(error) => self.set_error(error),
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
