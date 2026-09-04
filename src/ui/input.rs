use crate::{
    app::{App, DropPreview, PresetPage},
    model::{AddZone, Edge, Geometry, Hit, UiAction},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

pub enum Action {
    Continue,
    Apply,
    Cancel,
}
pub fn key(app: &mut App, k: KeyEvent) -> Action {
    if k.kind == KeyEventKind::Release {
        return Action::Continue;
    }
    if app.name_prompt.is_some() {
        return match k.code {
            KeyCode::Esc => {
                app.cancel_prompt();
                Action::Continue
            }
            KeyCode::Enter => {
                if let Err(error) = app.commit_name_prompt() {
                    app.set_error(error);
                }
                Action::Continue
            }
            KeyCode::Backspace => {
                app.backspace_prompt();
                Action::Continue
            }
            KeyCode::Char(ch) => {
                app.append_prompt_char(ch);
                Action::Continue
            }
            _ => Action::Continue,
        };
    }
    if app.delete_confirm.is_some() {
        return match k.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Err(error) = app.confirm_delete_saved() {
                    app.set_error(error);
                }
                Action::Continue
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                app.delete_confirm = None;
                Action::Continue
            }
            _ => Action::Continue,
        };
    }
    if app.show_help {
        if matches!(
            k.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        ) {
            app.show_help = false;
        }
        return Action::Continue;
    }
    if app.preset_picker.is_some() {
        return match k.code {
            KeyCode::Esc | KeyCode::Char('p') => {
                app.close_preset_picker();
                Action::Continue
            }
            KeyCode::Char('/') => {
                app.toggle_preset_collection();
                Action::Continue
            }
            KeyCode::Char('?') => {
                app.show_help = true;
                Action::Continue
            }
            KeyCode::Char('r') => {
                app.open_rename_prompt();
                Action::Continue
            }
            KeyCode::Char('d') => {
                app.request_delete_saved();
                Action::Continue
            }
            KeyCode::Enter => {
                app.accept_selected_preset();
                Action::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.move_preset_selection(-1);
                Action::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                app.move_preset_selection(1);
                Action::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.move_preset_selection(-3);
                Action::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.move_preset_selection(3);
                Action::Continue
            }
            _ => Action::Continue,
        };
    }
    match k.code {
        KeyCode::Char('?') => {
            app.show_help = true;
            Action::Continue
        }
        KeyCode::Esc if app.message.take().is_some() => Action::Continue,
        KeyCode::Esc if app.carrying.is_some() => {
            app.carrying = None;
            app.drop_edge = None;
            app.drop_preview = None;
            Action::Continue
        }
        KeyCode::Esc | KeyCode::Char('q') => Action::Cancel,
        KeyCode::Enter => Action::Apply,
        KeyCode::Char('p') => {
            app.open_preset_picker();
            Action::Continue
        }
        KeyCode::Char('s') => {
            app.open_save_prompt();
            Action::Continue
        }
        KeyCode::Char('u') => {
            app.undo();
            Action::Continue
        }
        KeyCode::Char('r') => {
            app.reset();
            Action::Continue
        }
        KeyCode::Char('=') => {
            app.balance_splits();
            Action::Continue
        }
        KeyCode::Char('d') if crate::model::is_draft_pane(&app.selected) => {
            app.remove_selected_draft();
            Action::Continue
        }
        KeyCode::Char('[') => {
            app.resize_selected_split(-0.05);
            Action::Continue
        }
        KeyCode::Char(']') => {
            app.resize_selected_split(0.05);
            Action::Continue
        }
        KeyCode::Char(' ') => {
            app.toggle_carry();
            Action::Continue
        }
        KeyCode::Left | KeyCode::Char('h') => nav(app, -1, Some(Edge::Left)),
        KeyCode::Right | KeyCode::Char('l') => nav(app, 1, Some(Edge::Right)),
        KeyCode::Up | KeyCode::Char('k') => nav(app, -1, Some(Edge::Top)),
        KeyCode::Down | KeyCode::Char('j') => nav(app, 1, Some(Edge::Bottom)),
        _ => Action::Continue,
    }
}
fn nav(app: &mut App, _d: isize, e: Option<Edge>) -> Action {
    if app.carrying.is_some() {
        app.drop_edge = e;
    }
    if let Some(edge) = e {
        app.move_selection_spatial(edge);
    }
    Action::Continue
}
#[derive(Clone, Debug, PartialEq)]
pub enum DragState {
    Pane(String),
    Divider(crate::model::Divider),
    AddButton(AddZone),
}

pub fn mouse(app: &mut App, m: MouseEvent, g: &Geometry, drag: &mut Option<DragState>) -> Action {
    if app.show_help {
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && g.hit_action(m.column, m.row) == Some(UiAction::Help)
        {
            app.show_help = false;
        }
        *drag = None;
        return Action::Continue;
    }
    if app.name_prompt.is_some() || app.delete_confirm.is_some() {
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some(action @ (UiAction::DialogConfirm | UiAction::DialogCancel)) =
                g.hit_action(m.column, m.row)
            {
                *drag = None;
                return toolbar_action(app, action);
            }
        }
        *drag = None;
        return Action::Continue;
    }
    if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
        if let Some(action) = g.hit_action(m.column, m.row) {
            *drag = None;
            return toolbar_action(app, action);
        }
    }
    if app.preset_picker.is_some() {
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some(index) = g.hit_preset_card(m.column, m.row) {
                app.select_preset(index);
            }
        }
        *drag = None;
        return Action::Continue;
    }
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(zone) = g.hit_add_zone(m.column, m.row) {
                *drag = Some(DragState::AddButton(zone.clone()));
                return Action::Continue;
            }
            match g.hit(m.column, m.row) {
                Some(Hit::Pane(id) | Hit::Edge(id, _)) => {
                    app.selected = id.clone();
                    app.drop_preview = None;
                    *drag = Some(DragState::Pane(id));
                }
                Some(Hit::Divider(path)) => {
                    if let Some(divider) = g.dividers.iter().find(|d| d.path == path) {
                        app.selected_split = path;
                        *drag = Some(DragState::Divider(divider.clone()));
                    }
                }
                None => {}
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => match drag.clone() {
            Some(DragState::Divider(divider)) => {
                let ratio = match divider.direction {
                    crate::model::Direction::Horizontal => {
                        (m.column.saturating_sub(divider.bounds.x)) as f64
                            / divider.bounds.width.max(1) as f64
                    }
                    crate::model::Direction::Vertical => {
                        (m.row.saturating_sub(divider.bounds.y)) as f64
                            / divider.bounds.height.max(1) as f64
                    }
                };
                app.set_split_ratio(divider.path, ratio);
            }
            Some(DragState::Pane(source)) => {
                app.dragging = Some(source.clone());
                app.drop_preview = drop_preview(g.hit(m.column, m.row), &source);
            }
            Some(DragState::AddButton(zone)) => {
                let source = zone.pane_id;
                *drag = Some(DragState::Pane(source.clone()));
                app.dragging = Some(source.clone());
                app.drop_preview = drop_preview(g.hit(m.column, m.row), &source);
            }
            None => {}
        },
        MouseEventKind::Up(MouseButton::Left) => match drag.take() {
            Some(DragState::AddButton(zone)) => {
                if g.hit_add_zone(m.column, m.row).is_some_and(|target| {
                    target.pane_id == zone.pane_id && target.edge == zone.edge
                }) {
                    app.add_draft(&zone.pane_id, zone.edge);
                }
            }
            Some(DragState::Pane(src)) => {
                let target = app
                    .drop_preview
                    .take()
                    .or_else(|| drop_preview(g.hit(m.column, m.row), &src));
                if let Some(target) = target {
                    match target.edge {
                        Some(edge) => app.reparent(&src, &target.pane_id, edge),
                        None => app.swap(&src, &target.pane_id),
                    }
                }
                app.dragging = None;
            }
            Some(DragState::Divider(_)) | None => {}
        },
        _ => {}
    }
    Action::Continue
}

fn toolbar_action(app: &mut App, action: UiAction) -> Action {
    match action {
        UiAction::ModeEditor => {
            app.close_preset_picker();
        }
        UiAction::ModePresets => {
            app.open_preset_picker();
        }
        UiAction::Balance => {
            app.close_preset_picker();
            app.balance_splits();
        }
        UiAction::Undo => {
            app.close_preset_picker();
            app.undo();
        }
        UiAction::DeleteDraft => {
            app.remove_selected_draft();
        }
        UiAction::Save => {
            app.close_preset_picker();
            app.open_save_prompt();
        }
        UiAction::Apply => return Action::Apply,
        UiAction::Cancel => return Action::Cancel,
        UiAction::Help => {
            app.show_help = true;
        }
        UiAction::PresetBuiltIn => app.set_preset_collection(PresetPage::BuiltIn),
        UiAction::PresetSaved => app.set_preset_collection(PresetPage::Saved),
        UiAction::PresetPreview => app.accept_selected_preset(),
        UiAction::PresetRename => app.open_rename_prompt(),
        UiAction::PresetDelete => app.request_delete_saved(),
        UiAction::DialogConfirm => {
            let result = if app.name_prompt.is_some() {
                app.commit_name_prompt()
            } else {
                app.confirm_delete_saved()
            };
            if let Err(error) = result {
                app.set_error(error);
            }
        }
        UiAction::DialogCancel => {
            app.cancel_prompt();
            app.delete_confirm = None;
        }
    }
    Action::Continue
}

fn drop_preview(hit: Option<Hit>, source: &str) -> Option<DropPreview> {
    match hit {
        Some(Hit::Pane(pane_id)) if pane_id != source => Some(DropPreview {
            pane_id,
            edge: None,
        }),
        Some(Hit::Edge(pane_id, edge)) if pane_id != source => Some(DropPreview {
            pane_id,
            edge: Some(edge),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        herdr::Snapshot,
        model::{ActionZone, AddZone, Direction, LayoutNode, Rect, UiAction},
    };
    use std::collections::HashMap;

    fn press(app: &mut App, code: KeyCode) -> Action {
        key(
            app,
            KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        )
    }

    fn app() -> App {
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
        App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "a".into(),
            tree,
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        })
    }

    #[test]
    fn carrying_navigation_keeps_source_and_moves_target() {
        let mut app = app();
        app.toggle_carry();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Right, crossterm::event::KeyModifiers::NONE),
        );
        assert_eq!(app.carrying.as_deref(), Some("a"));
        assert_eq!(app.selected, "b");
        assert_eq!(app.drop_edge, Some(Edge::Right));
        app.toggle_carry();
        assert_eq!(app.preview.pane_ids(), ["b", "a"]);
    }

    #[test]
    fn arrows_follow_visual_neighbors() {
        let pane = |id: &str| LayoutNode::Pane { pane_id: id.into() };
        let column = |top: LayoutNode, bottom: LayoutNode| LayoutNode::Split {
            direction: Direction::Vertical,
            ratio: 0.5,
            first: Box::new(top),
            second: Box::new(bottom),
        };
        let tree = LayoutNode::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(column(pane("a"), pane("c"))),
            second: Box::new(column(pane("b"), pane("d"))),
        };
        let mut app = App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "a".into(),
            tree,
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        });

        app.move_selection_spatial(Edge::Right);
        assert_eq!(app.selected, "b");
        app.move_selection_spatial(Edge::Bottom);
        assert_eq!(app.selected, "d");
        app.move_selection_spatial(Edge::Left);
        assert_eq!(app.selected, "c");
        app.move_selection_spatial(Edge::Top);
        assert_eq!(app.selected, "a");
    }

    #[test]
    fn help_and_carry_can_be_dismissed_without_closing() {
        let mut app = app();
        assert!(matches!(
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char('?'), crossterm::event::KeyModifiers::NONE)
            ),
            Action::Continue
        ));
        assert!(app.show_help);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE),
        );
        assert!(!app.show_help);

        app.open_preset_picker();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), crossterm::event::KeyModifiers::NONE),
        );
        assert!(app.show_help);
        assert!(app.preset_picker.is_some());
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), crossterm::event::KeyModifiers::NONE),
        );
        assert!(!app.show_help);
        assert!(app.preset_picker.is_some());
        app.close_preset_picker();

        app.toggle_carry();
        assert!(matches!(
            key(
                &mut app,
                KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE)
            ),
            Action::Continue
        ));
        assert!(app.carrying.is_none());
    }

    #[test]
    fn click_selects_without_showing_drag_state() {
        let mut app = app();
        let geometry = Geometry::calculate(
            &app.preview,
            crate::model::Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
        );
        let mut drag = None;

        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 30,
                row: 10,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &geometry,
            &mut drag,
        );

        assert_eq!(app.selected, "b");
        assert!(app.dragging.is_none());

        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 30,
                row: 10,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &geometry,
            &mut drag,
        );
        assert!(app.dragging.is_none());
    }

    #[test]
    fn pane_turns_into_drag_state_only_after_movement() {
        let mut app = app();
        let geometry = Geometry::calculate(
            &app.preview,
            crate::model::Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
        );
        let mut drag = None;
        let event = |kind, column| MouseEvent {
            kind,
            column,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        mouse(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), 10),
            &geometry,
            &mut drag,
        );
        assert!(app.dragging.is_none());

        mouse(
            &mut app,
            event(MouseEventKind::Drag(MouseButton::Left), 30),
            &geometry,
            &mut drag,
        );
        assert_eq!(app.dragging.as_deref(), Some("a"));
    }

    #[test]
    fn clicking_selected_pane_handle_creates_and_deletes_a_draft() {
        let mut app = app();
        let mut geometry = Geometry::calculate(
            &app.preview,
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
        );
        geometry.add_zones.push(AddZone {
            pane_id: "a".into(),
            edge: Edge::Right,
            rect: Rect {
                x: 8,
                y: 8,
                width: 4,
                height: 3,
            },
        });
        let mut drag = None;
        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 9,
                row: 9,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &geometry,
            &mut drag,
        );
        assert_eq!(app.preview.pane_ids().len(), 2);
        assert!(matches!(drag, Some(DragState::AddButton(_))));
        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 9,
                row: 9,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &geometry,
            &mut drag,
        );

        assert_eq!(app.preview.pane_ids().len(), 3);
        assert!(crate::model::is_draft_pane(&app.selected));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), crossterm::event::KeyModifiers::NONE),
        );
        assert_eq!(app.preview.pane_ids(), ["a", "b"]);
    }

    #[test]
    fn dragging_from_an_add_handle_moves_the_pane_instead_of_adding() {
        let mut app = app();
        let mut geometry = Geometry::calculate(
            &app.preview,
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
        );
        geometry.add_zones.push(AddZone {
            pane_id: "a".into(),
            edge: Edge::Right,
            rect: Rect {
                x: 8,
                y: 8,
                width: 4,
                height: 3,
            },
        });
        let event = |kind, column| MouseEvent {
            kind,
            column,
            row: 9,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let mut drag = None;
        mouse(
            &mut app,
            event(MouseEventKind::Down(MouseButton::Left), 9),
            &geometry,
            &mut drag,
        );
        mouse(
            &mut app,
            event(MouseEventKind::Drag(MouseButton::Left), 30),
            &geometry,
            &mut drag,
        );
        assert!(matches!(drag, Some(DragState::Pane(ref id)) if id == "a"));
        assert_eq!(app.preview.pane_ids().len(), 2);
    }

    #[test]
    fn balance_is_available_in_editor() {
        let mut normal = app();
        normal.preview.set_ratio(&[], 0.8).unwrap();
        key(
            &mut normal,
            KeyEvent::new(KeyCode::Char('='), crossterm::event::KeyModifiers::NONE),
        );
        assert_eq!(normal.preview.ratio_at(&[]), Some(0.5));
        assert_eq!(normal.undo.len(), 1);
    }

    #[test]
    fn preset_picker_builds_missing_slots_and_keeps_selected_pane_as_main() {
        let mut app = app();
        app.selected = "b".into();

        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), crossterm::event::KeyModifiers::NONE),
        );
        let main_left = crate::model::PresetKind::ALL
            .iter()
            .position(|preset| *preset == crate::model::PresetKind::MainLeft)
            .unwrap();
        app.select_preset(main_left);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
        );

        assert!(app.preset_picker.is_none());
        assert_eq!(app.preview.pane_ids()[0], "b");
        assert_eq!(app.preview.pane_ids().len(), 3);
        assert!(app
            .preview
            .pane_ids()
            .iter()
            .any(|id| crate::model::is_draft_pane(id)));
    }

    #[test]
    fn save_prompt_captures_preview_without_applying_it() {
        let mut app = app();
        app.preview.set_ratio(&[], 0.7).unwrap();
        app.selected = "b".into();

        press(&mut app, KeyCode::Char('s'));
        for ch in "Wide right".chars() {
            press(&mut app, KeyCode::Char(ch));
        }
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.saved_catalog.layouts.len(), 1);
        let saved = &app.saved_catalog.layouts[0];
        assert_eq!(saved.name, "Wide right");
        assert_eq!(saved.anchor_slot, 2);
        assert_eq!(saved.tree.preview_tree().unwrap().ratio_at(&[]), Some(0.7));
        assert!(app.has_catalog_change());
        assert_eq!(app.snapshot.tree.ratio_at(&[]), Some(0.5));
    }

    #[test]
    fn saved_picker_maps_selection_to_anchor_and_creates_missing_panes() {
        let mut app = app();
        app.add_draft("b", Edge::Bottom);
        app.selected = "b".into();
        app.open_save_prompt();
        app.name_prompt.as_mut().unwrap().value = "Right stack".into();
        app.commit_name_prompt().unwrap();
        app.catalog_saved();
        app.reset();
        app.selected = "a".into();

        app.open_preset_picker();
        app.toggle_preset_collection();
        app.accept_selected_preset();

        assert!(app.preset_picker.is_none());
        assert_eq!(app.preview.pane_ids()[1], "a");
        assert_eq!(app.preview.pane_ids().len(), 3);
        assert!(app
            .preview
            .pane_ids()
            .iter()
            .any(|id| crate::model::is_draft_pane(id)));
    }

    #[test]
    fn saved_layouts_can_be_renamed_and_deleted() {
        let mut app = app();
        app.open_save_prompt();
        app.name_prompt.as_mut().unwrap().value = "Pair".into();
        app.commit_name_prompt().unwrap();
        app.catalog_saved();

        app.open_preset_picker();
        app.toggle_preset_collection();
        press(&mut app, KeyCode::Char('r'));
        while !app.name_prompt.as_ref().unwrap().value.is_empty() {
            press(&mut app, KeyCode::Backspace);
        }
        for ch in "Fresh pair".chars() {
            press(&mut app, KeyCode::Char(ch));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.saved_catalog.layouts[0].name, "Fresh pair");
        app.catalog_saved();

        press(&mut app, KeyCode::Char('d'));
        assert!(app.delete_confirm.is_some());
        press(&mut app, KeyCode::Char('y'));
        assert!(app.saved_catalog.layouts.is_empty());
    }

    #[test]
    fn slash_toggles_between_built_in_and_saved_presets() {
        let mut app = app();
        app.open_preset_picker();
        assert_eq!(
            app.preset_picker.as_ref().unwrap().page,
            crate::app::PresetPage::BuiltIn
        );

        press(&mut app, KeyCode::Char('/'));
        assert_eq!(
            app.preset_picker.as_ref().unwrap().page,
            crate::app::PresetPage::Saved
        );

        press(&mut app, KeyCode::Char('/'));
        assert_eq!(
            app.preset_picker.as_ref().unwrap().page,
            crate::app::PresetPage::BuiltIn
        );
    }

    #[test]
    fn toolbar_switches_mouse_modes_and_exposes_apply() {
        let mut app = app();
        let geometry = Geometry {
            action_zones: vec![
                ActionZone {
                    action: UiAction::ModeEditor,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 8,
                        height: 3,
                    },
                },
                ActionZone {
                    action: UiAction::Apply,
                    rect: Rect {
                        x: 10,
                        y: 0,
                        width: 8,
                        height: 3,
                    },
                },
            ],
            ..Default::default()
        };
        let click = |column| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 1,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let mut drag = None;

        assert!(matches!(
            mouse(&mut app, click(2), &geometry, &mut drag),
            Action::Continue
        ));
        assert!(app.preset_picker.is_none());
        assert!(matches!(
            mouse(&mut app, click(12), &geometry, &mut drag),
            Action::Apply
        ));
    }

    #[test]
    fn editor_click_selects_a_pane_before_dragging() {
        let mut app = app();
        let geometry = Geometry::calculate(
            &app.preview,
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
        );
        let mut drag = None;
        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 30,
                row: 10,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &geometry,
            &mut drag,
        );
        assert_eq!(app.selected, "b");
        assert!(matches!(drag, Some(DragState::Pane(ref id)) if id == "b"));
        assert!(app.dragging.is_none());
    }

    #[test]
    fn save_dialog_confirm_button_is_clickable() {
        let mut app = app();
        app.open_save_prompt();
        app.name_prompt.as_mut().unwrap().value = "Mouse layout".into();
        let geometry = Geometry {
            action_zones: vec![ActionZone {
                action: UiAction::DialogConfirm,
                rect: Rect {
                    x: 10,
                    y: 10,
                    width: 10,
                    height: 3,
                },
            }],
            ..Default::default()
        };
        let mut drag = None;
        mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 12,
                row: 11,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &geometry,
            &mut drag,
        );
        assert!(app.name_prompt.is_none());
        assert_eq!(app.saved_catalog.layouts[0].name, "Mouse layout");
    }
}
