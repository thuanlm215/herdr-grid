use crate::{
    app::{App, DropPreview},
    model::{Edge, Geometry, Hit},
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
            KeyCode::Tab => {
                app.toggle_preset_destination();
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
    if app.add_mode {
        return match k.code {
            KeyCode::Esc if app.message.take().is_some() => Action::Continue,
            KeyCode::Esc => {
                app.cancel_add_mode();
                Action::Continue
            }
            KeyCode::Enter => {
                app.exit_add_mode();
                Action::Continue
            }
            KeyCode::Char('d') => {
                app.remove_selected_draft();
                Action::Continue
            }
            KeyCode::Char('?') => {
                app.show_help = true;
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
        KeyCode::Char('n') => {
            if !app.pending_new_workspace {
                app.toggle_add_mode();
            }
            Action::Continue
        }
        KeyCode::Char('p') => {
            app.open_preset_picker();
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
}

pub fn mouse(app: &mut App, m: MouseEvent, g: &Geometry, drag: &mut Option<DragState>) {
    if app.preset_picker.is_some() {
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some(index) = g.hit_preset_card(m.column, m.row) {
                app.select_preset(index);
            } else if g.hit_preset_destination(m.column, m.row) {
                app.toggle_preset_destination();
            } else if g.hit_preset_apply(m.column, m.row) {
                app.accept_selected_preset();
            }
        }
        *drag = None;
        return;
    }
    if app.add_mode {
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some(zone) = g.hit_add_zone(m.column, m.row) {
                let target = zone.pane_id.clone();
                app.add_draft(&target, zone.edge);
            } else if let Some(Hit::Pane(id) | Hit::Edge(id, _)) = g.hit(m.column, m.row) {
                app.select_add_target(id);
            }
        }
        *drag = None;
        return;
    }
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => match g.hit(m.column, m.row) {
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
        },
        MouseEventKind::Drag(MouseButton::Left) => match drag {
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
                app.set_split_ratio(divider.path.clone(), ratio);
            }
            Some(DragState::Pane(source)) => {
                app.dragging = Some(source.clone());
                app.drop_preview = drop_preview(g.hit(m.column, m.row), source);
            }
            None => {}
        },
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(DragState::Pane(src)) = drag.take() {
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
            } else {
                drag.take();
            }
        }
        _ => {}
    }
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
        model::{AddZone, Direction, LayoutNode, Rect},
    };
    use std::collections::HashMap;

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
    fn add_mode_creates_and_deletes_a_draft_without_leaving_the_mode() {
        let mut app = app();
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('n'), crossterm::event::KeyModifiers::NONE),
        );
        assert!(app.add_mode);

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

        assert!(app.add_mode);
        assert_eq!(app.preview.pane_ids().len(), 3);
        assert!(crate::model::is_draft_pane(&app.selected));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), crossterm::event::KeyModifiers::NONE),
        );
        assert_eq!(app.preview.pane_ids(), ["a", "b"]);
        assert!(app.add_mode);
    }

    #[test]
    fn add_mode_enter_keeps_preview_and_escape_discards_it() {
        let mut app = app();
        app.toggle_add_mode();
        app.add_draft("a", Edge::Right);

        for code in [KeyCode::Char('n'), KeyCode::Char('q')] {
            assert!(matches!(
                key(
                    &mut app,
                    KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
                ),
                Action::Continue
            ));
            assert!(app.add_mode);
        }
        key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
        );
        assert!(!app.add_mode);
        assert_eq!(app.preview.pane_ids().len(), 3);

        app.toggle_add_mode();
        let target = app.preview.pane_ids()[0].clone();
        app.add_draft(&target, Edge::Bottom);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE),
        );
        assert!(!app.add_mode);
        assert_eq!(app.preview.pane_ids(), ["a", "b"]);
        assert!(app.undo.is_empty());
    }

    #[test]
    fn balance_is_available_only_in_normal_mode() {
        let mut normal = app();
        normal.preview.set_ratio(&[], 0.8).unwrap();
        key(
            &mut normal,
            KeyEvent::new(KeyCode::Char('='), crossterm::event::KeyModifiers::NONE),
        );
        assert_eq!(normal.preview.ratio_at(&[]), Some(0.5));
        assert_eq!(normal.undo.len(), 1);

        let mut adding = app();
        adding.preview.set_ratio(&[], 0.8).unwrap();
        adding.toggle_add_mode();
        key(
            &mut adding,
            KeyEvent::new(KeyCode::Char('='), crossterm::event::KeyModifiers::NONE),
        );
        assert_eq!(adding.preview.ratio_at(&[]), Some(0.8));
        assert!(adding.undo.is_empty());
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
        assert!(!app.pending_new_workspace);
    }

    #[test]
    fn new_workspace_preset_can_be_previewed_and_undone_without_touching_source() {
        let mut app = app();
        let original = app.preview.clone();
        app.open_preset_picker();
        app.toggle_preset_destination();
        let grid = crate::model::PresetKind::ALL
            .iter()
            .position(|preset| *preset == crate::model::PresetKind::Grid2x2)
            .unwrap();
        app.select_preset(grid);
        app.accept_selected_preset();

        assert!(app.pending_new_workspace);
        assert_eq!(app.preview.pane_ids().len(), 4);
        assert!(app
            .preview
            .pane_ids()
            .iter()
            .all(|id| crate::model::is_draft_pane(id)));

        app.undo();
        assert!(!app.pending_new_workspace);
        assert_eq!(app.preview, original);
    }
}
