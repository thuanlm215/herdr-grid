use crate::{
    herdr::{ApplyProgress, HerdrClient, SessionTab, SessionWorkspace, Snapshot, Transaction},
    model::{
        is_draft_pane, DestChip, DestId, Edge, Geometry, LayoutNode, PaneId, PresetKind, Rect,
        Rehome, RehomeDest, SplitPath, TemplateNode, DRAFT_PANE_PREFIX,
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

#[derive(Clone, Debug, PartialEq)]
pub struct UndoFrame {
    preview: LayoutNode,
    rehomes: Vec<Rehome>,
}

pub struct App {
    pub snapshot: Snapshot,
    pub preview: LayoutNode,
    pub rehomes: Vec<Rehome>,
    pub dest_hover: Option<DestId>,
    pub dest_cursor: Option<DestId>,
    pub expanded_workspace: Option<String>,
    pub undo: Vec<UndoFrame>,
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
            rehomes: vec![],
            dest_hover: None,
            dest_cursor: None,
            expanded_workspace: None,
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
    fn checkpoint(&self) -> UndoFrame {
        UndoFrame {
            preview: self.preview.clone(),
            rehomes: self.rehomes.clone(),
        }
    }
    fn edit(&mut self, f: impl FnOnce(&mut LayoutNode) -> Result<(), crate::model::ModelError>) {
        let old = self.checkpoint();
        match f(&mut self.preview) {
            Ok(()) => self.undo.push(old),
            Err(e) => self.set_error(e),
        }
    }
    pub fn is_modified(&self) -> bool {
        self.preview != self.snapshot.tree || !self.rehomes.is_empty()
    }
    pub fn moving_pane(&self) -> Option<&PaneId> {
        self.dragging.as_ref().or(self.carrying.as_ref())
    }
    pub fn highlighted_dest(&self) -> Option<DestId> {
        self.dest_hover.clone().or_else(|| self.dest_cursor.clone())
    }
    pub fn set_error(&mut self, error: impl ToString) {
        self.message = Some(AppMessage {
            kind: MessageKind::Error,
            text: error.to_string(),
            expires_at: Some(Instant::now() + Duration::from_secs(3)),
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
        if let Some(frame) = self.undo.pop() {
            self.preview = frame.preview;
            self.rehomes = frame.rehomes;
            self.repair_selection();
        }
    }
    pub fn reset(&mut self) {
        if self.is_modified() {
            self.undo.push(self.checkpoint());
            self.preview = self.snapshot.tree.clone();
            self.rehomes.clear();
        }
        self.repair_selection();
    }
    pub fn balance_splits(&mut self) {
        let old = self.checkpoint();
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
                LayoutNode::Empty | LayoutNode::Pane { .. } => return,
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
        self.undo.push(UndoFrame {
            preview: source,
            rehomes: self.rehomes.clone(),
        });
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
        self.undo.push(UndoFrame {
            preview: source,
            rehomes: self.rehomes.clone(),
        });
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
        let old = self.checkpoint();
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
        let old = self.checkpoint();
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
        .apply_preview(&self.preview, &self.rehomes)
        .await
    }

    pub fn dest_chips(&self) -> Vec<DestChip> {
        let mut chips = self.workspace_chips();
        chips.extend(self.tab_chips());
        chips
    }

    pub fn workspace_chips(&self) -> Vec<DestChip> {
        let current_ws = &self.snapshot.workspace_id;
        let mut workspaces = self.snapshot.workspaces.clone();
        if !workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == *current_ws)
        {
            workspaces.push(SessionWorkspace {
                workspace_id: current_ws.clone(),
                label: current_ws.clone(),
                active_tab_id: Some(self.snapshot.tab_id.clone()),
            });
        }
        let mut chips: Vec<_> = workspaces
            .iter()
            .map(|workspace| self.workspace_chip(workspace, workspace.workspace_id == *current_ws))
            .collect();
        chips.push(self.action_chip(DestId::NewWorkspace, "+ Workspace"));
        chips
    }

    pub fn tab_chips(&self) -> Vec<DestChip> {
        let workspace_id = self.expanded_workspace_id();
        let tabs: Vec<&SessionTab> = self
            .snapshot
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace_id)
            .collect();
        let mut chips = if tabs.is_empty() && workspace_id == self.snapshot.workspace_id {
            vec![self.tab_chip(&self.snapshot.tab_id, "this", true, false)]
        } else {
            tabs.into_iter()
                .map(|tab| {
                    self.tab_chip(
                        &tab.tab_id,
                        &tab_label(tab),
                        tab.tab_id == self.snapshot.tab_id,
                        tab.zoomed,
                    )
                })
                .collect()
        };
        chips.push(self.action_chip(
            DestId::NewTab {
                workspace_id: workspace_id.clone(),
            },
            "+ Tab",
        ));
        chips
    }

    pub fn expanded_workspace_id(&self) -> String {
        self.expanded_workspace
            .clone()
            .unwrap_or_else(|| self.snapshot.workspace_id.clone())
    }

    pub fn set_dest_hover(&mut self, dest: Option<DestId>) {
        self.dest_hover = dest.clone();
        if self.moving_pane().is_none() {
            if dest.is_none() {
                self.expanded_workspace = None;
            }
            return;
        }
        match dest {
            Some(DestId::Workspace(id)) => self.expanded_workspace = Some(id),
            Some(DestId::Tab(tab_id)) => {
                if let Some(workspace_id) = self.workspace_id_for_tab(&tab_id) {
                    self.expanded_workspace = Some(workspace_id);
                }
            }
            Some(DestId::NewTab { .. }) | Some(DestId::NewWorkspace) | None => {}
        }
    }

    fn workspace_id_for_tab(&self, tab_id: &str) -> Option<String> {
        self.snapshot
            .tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .map(|tab| tab.workspace_id.clone())
    }

    fn tab_chip(&self, tab_id: &str, label: &str, current: bool, zoomed: bool) -> DestChip {
        let id = DestId::Tab(tab_id.to_owned());
        let (enabled, reason) = if current {
            (false, Some("Already on this tab".into()))
        } else if zoomed {
            (false, Some("Unzoom that tab first".into()))
        } else {
            (true, None)
        };
        DestChip {
            badge: self.badge_for(&id),
            label: if current {
                format!("{label}*")
            } else {
                label.into()
            },
            id,
            enabled,
            current,
            reason,
        }
    }

    fn workspace_chip(&self, workspace: &SessionWorkspace, current: bool) -> DestChip {
        let id = DestId::Workspace(workspace.workspace_id.clone());
        let zoomed = workspace
            .active_tab_id
            .as_ref()
            .and_then(|tab_id| {
                self.snapshot
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == *tab_id)
                    .map(|tab| tab.zoomed)
            })
            .unwrap_or(false);
        let (enabled, reason) = if current {
            (false, Some("Already on this workspace".into()))
        } else if zoomed {
            (false, Some("Unzoom that tab first".into()))
        } else {
            (true, None)
        };
        let name = if workspace.label.is_empty() {
            workspace.workspace_id.clone()
        } else {
            workspace.label.clone()
        };
        DestChip {
            badge: self.badge_for(&id),
            id,
            label: name,
            enabled,
            current,
            reason,
        }
    }

    fn action_chip(&self, id: DestId, label: &str) -> DestChip {
        DestChip {
            badge: self.badge_for(&id),
            id,
            label: label.into(),
            enabled: true,
            current: false,
            reason: None,
        }
    }

    fn badge_for(&self, id: &DestId) -> Option<String> {
        let panes: Vec<_> = self
            .rehomes
            .iter()
            .filter(|rehome| rehome.dest.matches_chip(id))
            .map(|rehome| short_pane_id(&rehome.pane_id))
            .collect();
        match panes.len() {
            0 => None,
            1 => Some(format!("+{}", panes[0])),
            n => Some(format!("+{n}")),
        }
    }

    pub fn cycle_dest(&mut self, delta: isize) {
        let chips: Vec<_> = self
            .dest_chips()
            .into_iter()
            .filter(|chip| chip.enabled)
            .map(|chip| chip.id)
            .collect();
        if chips.is_empty() {
            self.dest_cursor = None;
            return;
        }
        let current = self.highlighted_dest();
        let index = current
            .and_then(|id| chips.iter().position(|chip| *chip == id))
            .map(|index| (index as isize + delta).rem_euclid(chips.len() as isize) as usize)
            .unwrap_or(if delta >= 0 { 0 } else { chips.len() - 1 });
        self.dest_cursor = Some(chips[index].clone());
        self.dest_hover = None;
        match &chips[index] {
            DestId::Workspace(id) => self.expanded_workspace = Some(id.clone()),
            DestId::Tab(tab_id) => {
                if let Some(workspace_id) = self.workspace_id_for_tab(tab_id) {
                    self.expanded_workspace = Some(workspace_id);
                }
            }
            DestId::NewTab { .. } => {}
            DestId::NewWorkspace => self.expanded_workspace = None,
        }
    }

    pub fn drop_on_highlighted_dest(&mut self) {
        let Some(pane) = self.moving_pane().cloned() else {
            return;
        };
        let Some(dest) = self.highlighted_dest() else {
            return;
        };
        self.rehome(&pane, dest);
    }

    pub fn rehome(&mut self, pane: &str, dest: DestId) {
        if let Err(error) = self.can_rehome(pane) {
            self.set_error(error);
            self.clear_move_state();
            return;
        }
        let chips = self.dest_chips();
        let Some(chip) = chips.iter().find(|chip| chip.id == dest) else {
            self.clear_move_state();
            return;
        };
        if !chip.enabled {
            self.set_error(
                chip.reason
                    .clone()
                    .unwrap_or_else(|| "Can't send a pane there".into()),
            );
            self.clear_move_state();
            return;
        }
        let resolved = match self.resolve_dest(&dest) {
            Ok(dest) => dest,
            Err(error) => {
                self.set_error(error);
                self.clear_move_state();
                return;
            }
        };
        let old = self.checkpoint();
        match self.preview.detach_pane(pane) {
            Ok(_) => {
                self.undo.push(old);
                self.rehomes.push(Rehome {
                    pane_id: pane.into(),
                    dest: resolved,
                });
                self.clear_move_state();
                self.repair_selection();
            }
            Err(error) => {
                self.set_error(error);
                self.clear_move_state();
            }
        }
    }

    fn can_rehome(&self, pane: &str) -> Result<(), String> {
        if is_draft_pane(pane) {
            return Err("Drafts stay here until Apply".into());
        }
        if !self.preview.pane_ids().iter().any(|id| id == pane) {
            return Err("That pane is not in this tab".into());
        }
        let leftover_drafts = self
            .preview
            .pane_ids()
            .into_iter()
            .filter(|id| is_draft_pane(id))
            .count();
        let live = self
            .preview
            .pane_ids()
            .into_iter()
            .filter(|id| !is_draft_pane(id) && id != pane)
            .count();
        if live == 0 && leftover_drafts > 0 {
            return Err("Can't empty this tab while new shells are waiting".into());
        }
        Ok(())
    }

    fn resolve_dest(&self, dest: &DestId) -> Result<RehomeDest, String> {
        match dest {
            DestId::Tab(tab_id) => {
                let label = self
                    .snapshot
                    .tabs
                    .iter()
                    .find(|tab| tab.tab_id == *tab_id)
                    .map(tab_label)
                    .unwrap_or_else(|| tab_id.clone());
                Ok(RehomeDest::Tab {
                    tab_id: tab_id.clone(),
                    label,
                })
            }
            DestId::NewTab { workspace_id } => Ok(RehomeDest::NewTab {
                workspace_id: workspace_id.clone(),
            }),
            DestId::NewWorkspace => Ok(RehomeDest::NewWorkspace),
            DestId::Workspace(workspace_id) => {
                let workspace = self
                    .snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == *workspace_id);
                if let Some(tab_id) =
                    workspace.and_then(|workspace| workspace.active_tab_id.as_ref())
                {
                    if tab_id != &self.snapshot.tab_id {
                        return self.resolve_dest(&DestId::Tab(tab_id.clone()));
                    }
                }
                Ok(RehomeDest::NewTab {
                    workspace_id: workspace_id.clone(),
                })
            }
        }
    }

    fn clear_move_state(&mut self) {
        self.carrying = None;
        self.dragging = None;
        self.drop_edge = None;
        self.drop_preview = None;
        self.dest_hover = None;
        self.dest_cursor = None;
        self.expanded_workspace = None;
    }

    pub fn adopt_snapshot(&mut self, snapshot: Snapshot) {
        self.selected = snapshot.focused_pane_id.clone();
        self.preview = snapshot.tree.clone();
        self.snapshot = snapshot;
        self.rehomes.clear();
        self.undo.clear();
        self.selected_split.clear();
        self.preset_picker = None;
        self.clear_move_state();
        self.repair_selection();
    }

    pub fn click_dest(&self, dest: &DestId) -> Result<Option<String>, String> {
        if self.is_modified() {
            return Err("Apply or Cancel before switching tabs".into());
        }
        match dest {
            DestId::NewTab { .. } | DestId::NewWorkspace => {
                Err("Drop a pane here to create it".into())
            }
            DestId::Tab(tab_id) if tab_id == &self.snapshot.tab_id => Ok(None),
            DestId::Tab(tab_id) => self.pane_for_tab(tab_id).map(Some),
            DestId::Workspace(workspace_id) if workspace_id == &self.snapshot.workspace_id => {
                Ok(None)
            }
            DestId::Workspace(workspace_id) => {
                let tab_id = self
                    .snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == *workspace_id)
                    .and_then(|workspace| workspace.active_tab_id.as_ref())
                    .cloned()
                    .ok_or_else(|| "That workspace has no tabs".to_string())?;
                if tab_id == self.snapshot.tab_id {
                    return Ok(None);
                }
                self.pane_for_tab(&tab_id).map(Some)
            }
        }
    }

    fn pane_for_tab(&self, tab_id: &str) -> Result<String, String> {
        let tab = self
            .snapshot
            .tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .ok_or_else(|| "Tab not found".to_string())?;
        if tab.zoomed {
            return Err("Unzoom that tab first".into());
        }
        tab.focused_pane_id
            .clone()
            .ok_or_else(|| "That tab has no panes".to_string())
    }
}

fn tab_label(tab: &SessionTab) -> String {
    if tab.label.is_empty() {
        tab.tab_id
            .rsplit(':')
            .next()
            .unwrap_or(&tab.tab_id)
            .to_owned()
    } else {
        tab.label.clone()
    }
}

fn short_pane_id(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}
