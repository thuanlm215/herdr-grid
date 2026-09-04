use crate::{
    app::{App, DropPreview, MessageKind, NamePromptKind, PresetPage},
    herdr::ApplyProgress,
    model::{
        is_draft_pane, ActionZone, AddZone, Edge, Geometry, PaneRect, PresetCardZone, PresetKind,
        Rect, UiAction,
    },
};
use ratatui::{
    layout::{Alignment, Constraint, Layout},
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

pub fn draw(frame: &mut Frame, app: &App) -> Geometry {
    let outer = frame.area();
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .split(outer);

    let canvas = rows[1];
    let mut geo = Geometry::calculate(
        &app.preview,
        Rect {
            x: canvas.x,
            y: canvas.y,
            width: canvas.width,
            height: canvas.height,
        },
    );
    render_toolbar(frame, app, rows[0], &mut geo);
    for pane in &geo.panes {
        render_pane(frame, app, pane);
    }
    if app.preset_picker.is_none() {
        if let Some(pane) = geo.panes.iter().find(|pane| pane.pane_id == app.selected) {
            geo.add_zones = add_zones(pane);
            render_add_zones(frame, &geo.add_zones);
        }
    }
    render_drop_preview(frame, app, &geo);

    let footer_style = Style::default();
    frame.render_widget(
        Paragraph::new(footer(app)).style(footer_style).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(footer_style),
        ),
        rows[2],
    );

    if app.preset_picker.is_some() {
        render_preset_picker(frame, app, &mut geo, canvas);
    }
    if app.show_help {
        render_help(frame, app);
    }
    if app.message.is_some() {
        render_message(frame, app, canvas);
    }
    if app.name_prompt.is_some() {
        render_name_prompt(frame, app, &mut geo);
    } else if app.delete_confirm.is_some() {
        render_delete_confirm(frame, app, &mut geo);
    }
    geo
}

fn render_message(frame: &mut Frame, app: &App, bounds: ratatui::layout::Rect) {
    let message = app.message.as_ref().unwrap();
    let (color, title) = match message.kind {
        MessageKind::Error => (Color::Red, " Error · Esc dismiss "),
        MessageKind::Success => (Color::LightGreen, " Done · Esc dismiss "),
    };
    let width = (message.text.chars().count() as u16 + 6).clamp(24, bounds.width.max(1));
    let height = 3.min(bounds.height);
    let area = ratatui::layout::Rect::new(
        bounds.x.saturating_add(bounds.width.saturating_sub(width)),
        bounds
            .y
            .saturating_add(bounds.height.saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(message.text.as_str())
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(color))
                    .title(title),
            ),
        area,
    );
}

fn render_toolbar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect, geo: &mut Geometry) {
    let preset_mode = app.preset_picker.is_some();
    let actions: Vec<(UiAction, String, u16, bool)> = if preset_mode {
        let has_selection = app.current_preset_count() > 0;
        let mut actions = vec![(UiAction::PresetPreview, "Preview".into(), 11, has_selection)];
        if app.preset_picker.as_ref().unwrap().page == PresetPage::Saved {
            actions.extend([
                (UiAction::PresetRename, "Rename".into(), 10, has_selection),
                (UiAction::PresetDelete, "Delete".into(), 10, has_selection),
            ]);
        }
        actions.push((UiAction::Help, "?".into(), 5, true));
        actions
    } else {
        let draft_selected = is_draft_pane(&app.selected);
        vec![
            (UiAction::Balance, "Balance".into(), 11, true),
            (UiAction::Undo, "Undo".into(), 8, true),
            (UiAction::Save, "Save".into(), 8, true),
            (UiAction::DeleteDraft, "Delete".into(), 10, draft_selected),
            (UiAction::Apply, "Apply".into(), 9, true),
            (UiAction::Cancel, "Cancel".into(), 10, true),
            (UiAction::Help, "?".into(), 5, true),
        ]
    };
    let mut items = vec![
        (Some(UiAction::ModeEditor), "Editor".to_string(), 12, true),
        (Some(UiAction::ModePresets), "Presets".to_string(), 12, true),
        (None, String::new(), 2, true),
    ];
    items.extend(
        actions
            .into_iter()
            .map(|(action, label, width, enabled)| (Some(action), label, width, enabled)),
    );
    let constraints = items
        .iter()
        .map(|(_, _, width, _)| Constraint::Length(*width))
        .collect::<Vec<_>>();
    let cells = Layout::horizontal(constraints).split(area);
    for ((action, label, _, enabled), rect) in items.into_iter().zip(cells.iter().copied()) {
        let Some(action) = action else {
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                rect,
            );
            continue;
        };
        let tab = matches!(action, UiAction::ModeEditor | UiAction::ModePresets);
        let active = match action {
            UiAction::ModeEditor => !preset_mode,
            UiAction::ModePresets => preset_mode,
            _ => false,
        };
        let text_color = if !enabled {
            Color::DarkGray
        } else if active {
            Color::Cyan
        } else if matches!(action, UiAction::Apply) {
            Color::LightGreen
        } else if matches!(action, UiAction::Cancel) {
            Color::LightRed
        } else {
            Color::Gray
        };
        if tab {
            let border_color = if active { Color::Cyan } else { Color::Gray };
            frame.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(if active {
                        BorderType::Thick
                    } else {
                        BorderType::Plain
                    })
                    .border_style(Style::default().fg(border_color)),
                rect,
            );
            let inner = rect.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(if active {
                        Style::default().fg(Color::Cyan).bold()
                    } else {
                        Style::default().fg(text_color)
                    }),
                inner,
            );
        } else {
            let border_color = if !enabled {
                Color::DarkGray
            } else if matches!(action, UiAction::Apply) {
                Color::LightGreen
            } else if matches!(action, UiAction::Cancel) {
                Color::LightRed
            } else {
                Color::Gray
            };
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(text_color))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(border_color)),
                    ),
                rect,
            );
        }
        if enabled {
            geo.action_zones.push(ActionZone {
                action,
                rect: from_ratatui(rect),
            });
        }
    }
}

fn render_preset_picker(
    frame: &mut Frame,
    app: &App,
    geometry: &mut Geometry,
    bounds: ratatui::layout::Rect,
) {
    let picker = app.preset_picker.as_ref().unwrap();
    let area = bounds;
    frame.render_widget(Clear, area);
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(16)]).split(inner);

    let collection = centered_rect(42, 100, rows[0]);
    let collection_cells =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(collection);
    for (page, action, label, rect) in [
        (
            PresetPage::BuiltIn,
            UiAction::PresetBuiltIn,
            "Built-in",
            collection_cells[0],
        ),
        (
            PresetPage::Saved,
            UiAction::PresetSaved,
            "Saved",
            collection_cells[1],
        ),
    ] {
        let active = picker.page == page;
        let text_color = if active { Color::Cyan } else { Color::Gray };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(Style::default().fg(text_color).add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Gray)),
                ),
            rect,
        );
        geometry.action_zones.push(ActionZone {
            action,
            rect: from_ratatui(rect),
        });
    }

    let card_rows = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Length(1),
        Constraint::Ratio(1, 3),
        Constraint::Length(1),
        Constraint::Ratio(1, 3),
    ])
    .split(rows[1]);
    let card_count = app.current_preset_count();
    for index in 0..card_count {
        let row = index / 3;
        let column = index % 3;
        let columns = Layout::horizontal([
            Constraint::Ratio(1, 3),
            Constraint::Length(2),
            Constraint::Ratio(1, 3),
            Constraint::Length(2),
            Constraint::Ratio(1, 3),
        ])
        .split(card_rows[row * 2]);
        let card = columns[column * 2];
        geometry.preset_cards.push(PresetCardZone {
            index,
            rect: Rect {
                x: card.x,
                y: card.y,
                width: card.width,
                height: card.height,
            },
        });
        let (title, tree, enabled, anchor_slot) = match picker.page {
            PresetPage::BuiltIn => {
                let preset = PresetKind::ALL[index];
                let ids = (1..=preset.slots())
                    .map(|slot| slot.to_string())
                    .collect::<Vec<_>>();
                (
                    preset.title().to_string(),
                    preset.build(&ids).ok(),
                    app.preset_enabled(preset),
                    None,
                )
            }
            PresetPage::Saved => {
                let layout = &app.saved_catalog.layouts[index];
                (
                    layout.name.clone(),
                    layout.tree.preview_tree().ok(),
                    app.saved_preset_enabled(layout),
                    Some(layout.anchor_slot),
                )
            }
        };
        let selected = index == picker.selected;
        let color = if !enabled {
            Color::DarkGray
        } else if selected {
            Color::LightCyan
        } else {
            Color::Gray
        };
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color))
                .title(format!(" {title} ")),
            card,
        );
        if let Some(tree) = tree.filter(|_| card.width > 6 && card.height > 3) {
            let preview = card.inner(Margin {
                horizontal: 2,
                vertical: 1,
            });
            let preview = if let PresetPage::BuiltIn = picker.page {
                preset_preview_rect(preview, PresetKind::ALL[index])
            } else {
                preview
            };
            let mini = Geometry::calculate(
                &tree,
                Rect {
                    x: preview.x,
                    y: preview.y,
                    width: preview.width,
                    height: preview.height,
                },
            );
            for pane in mini.panes {
                let is_anchor = anchor_slot.is_some_and(|slot| pane.pane_id == slot.to_string());
                frame.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(if is_anchor && enabled {
                            Color::LightGreen
                        } else {
                            color
                        }))
                        .title(if is_anchor {
                            format!("{}*", pane.pane_id)
                        } else {
                            pane.pane_id
                        }),
                    to_ratatui(pane.rect),
                );
            }
        }
    }

    if card_count == 0 {
        frame.render_widget(
            Paragraph::new("No saved layouts yet\n\nOpen Editor, then click Save")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            rows[1],
        );
    }
}

fn render_name_prompt(frame: &mut Frame, app: &App, geometry: &mut Geometry) {
    let prompt = app.name_prompt.as_ref().unwrap();
    let area = centered_fixed(58, 9, frame.area());
    frame.render_widget(Clear, area);
    let (title, confirm) = match prompt.kind {
        NamePromptKind::Save => (" Save custom layout ", "Save"),
        NamePromptKind::Rename { .. } => (" Rename custom layout ", "Rename"),
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightGreen))
            .title(title),
        area,
    );
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(3),
    ])
    .split(area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    }));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Name"),
            Line::from(format!("> {}_", prompt.value)),
        ]),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("Geometry only · Enter/Esc also work")
            .style(Style::default().fg(Color::DarkGray)),
        rows[1],
    );
    render_dialog_buttons(frame, geometry, rows[2], confirm, Color::LightGreen);
}

fn render_delete_confirm(frame: &mut Frame, app: &App, geometry: &mut Geometry) {
    let index = app.delete_confirm.unwrap();
    let name = app
        .saved_catalog
        .layouts
        .get(index)
        .map(|layout| layout.name.as_str())
        .unwrap_or("this layout");
    let area = centered_fixed(56, 7, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(" Delete saved layout "),
        area,
    );
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Length(3)]).split(area.inner(
        Margin {
            horizontal: 2,
            vertical: 1,
        },
    ));
    frame.render_widget(
        Paragraph::new(format!("Delete '{name}'?"))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
        rows[0],
    );
    render_dialog_buttons(frame, geometry, rows[1], "Delete", Color::Red);
}

fn render_dialog_buttons(
    frame: &mut Frame,
    geometry: &mut Geometry,
    area: ratatui::layout::Rect,
    confirm: &str,
    confirm_color: Color,
) {
    let buttons = centered_rect(64, 100, area);
    let cells = Layout::horizontal([
        Constraint::Ratio(1, 2),
        Constraint::Length(2),
        Constraint::Ratio(1, 2),
    ])
    .split(buttons);
    for (action, label, color, rect) in [
        (UiAction::DialogConfirm, confirm, confirm_color, cells[0]),
        (UiAction::DialogCancel, "Cancel", Color::Gray, cells[2]),
    ] {
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(Style::default().fg(color))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(color)),
                ),
            rect,
        );
        geometry.action_zones.push(ActionZone {
            action,
            rect: from_ratatui(rect),
        });
    }
}

fn preset_preview_rect(rect: ratatui::layout::Rect, preset: PresetKind) -> ratatui::layout::Rect {
    let (width_divisor, height_divisor) = preset.preview_divisors();
    let width = rect.width - rect.width % width_divisor;
    let height = rect.height - rect.height % height_divisor;
    if width == 0 || height == 0 {
        return rect;
    }
    ratatui::layout::Rect::new(
        rect.x + (rect.width - width) / 2,
        rect.y + (rect.height - height) / 2,
        width,
        height,
    )
}

fn render_pane(frame: &mut Frame, app: &App, pane: &PaneRect) {
    let draft = is_draft_pane(&pane.pane_id);
    let metadata = app.snapshot.metadata.get(&pane.pane_id);
    let raw_title = if draft {
        "New shell"
    } else {
        metadata
            .and_then(|value| value.terminal_title_stripped.as_deref())
            .unwrap_or(&pane.pane_id)
    };
    let cwd = metadata
        .and_then(|value| value.cwd.as_deref())
        .map(compact_cwd)
        .unwrap_or_default();
    let title = visible_title(raw_title, &cwd);
    let process = metadata.and_then(|value| value.process_name.as_deref());
    let agent = metadata.and_then(|value| value.agent.as_deref());
    let status = metadata.and_then(|value| value.agent_status.as_deref());
    let secondary = if draft {
        "Created on Apply".into()
    } else {
        secondary_label(agent, process, status)
    };

    let selected = pane.pane_id == app.selected;
    let moving = app.carrying.as_ref() == Some(&pane.pane_id)
        || app.dragging.as_ref() == Some(&pane.pane_id);
    let border_color = if draft {
        if selected {
            Color::LightGreen
        } else {
            Color::Green
        }
    } else if moving {
        Color::Yellow
    } else if selected {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let mut lines = Vec::new();
    if let Some(title) = title {
        lines.push(Line::from(Span::styled(title, Style::default().bold())));
    }
    if !secondary.is_empty() {
        lines.push(Line::from(secondary));
    }
    if !cwd.is_empty() {
        lines.push(Line::from(Span::styled(
            cwd,
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(if draft {
                    " new ".into()
                } else {
                    format!(" {} ", short_pane_id(&pane.pane_id))
                }),
        ),
        ratatui::layout::Rect::new(pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height),
    );
}

fn add_zones(pane: &PaneRect) -> Vec<AddZone> {
    let r = pane.rect;
    // Odd dimensions give the one-cell ASCII '+' an exact center. Since
    // terminal cells are taller than wide, 3x3 is also close to a visual
    // rotation of the 7x1 horizontal control.
    let side_width = r.width.clamp(1, 3);
    let side_height = r.height.clamp(1, 3);
    let horizontal_width = r.width.clamp(1, 7);
    let horizontal_height = 1;
    let center_x =
        r.x.saturating_add(r.width.saturating_sub(horizontal_width) / 2);
    let center_y = r.y.saturating_add(r.height.saturating_sub(side_height) / 2);
    [
        (
            Edge::Left,
            Rect {
                x: r.x,
                y: center_y,
                width: side_width,
                height: side_height,
            },
        ),
        (
            Edge::Right,
            Rect {
                x: r.x.saturating_add(r.width.saturating_sub(side_width)),
                y: center_y,
                width: side_width,
                height: side_height,
            },
        ),
        (
            Edge::Top,
            Rect {
                x: center_x,
                y: r.y,
                width: horizontal_width,
                height: horizontal_height,
            },
        ),
        (
            Edge::Bottom,
            Rect {
                x: center_x,
                y: r.y
                    .saturating_add(r.height.saturating_sub(horizontal_height)),
                width: horizontal_width,
                height: horizontal_height,
            },
        ),
    ]
    .into_iter()
    .map(|(edge, rect)| AddZone {
        pane_id: pane.pane_id.clone(),
        edge,
        rect,
    })
    .collect()
}

fn render_add_zones(frame: &mut Frame, zones: &[AddZone]) {
    for zone in zones {
        frame.render_widget(Clear, to_ratatui(zone.rect));
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::LightCyan)),
            to_ratatui(zone.rect),
        );
        let marker = add_marker_rect(zone.rect);
        frame.render_widget(
            Paragraph::new("+").alignment(Alignment::Center).style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .bold(),
            ),
            to_ratatui(marker),
        );
    }
}

fn add_marker_rect(zone: Rect) -> Rect {
    Rect {
        x: zone.x.saturating_add(zone.width / 2),
        y: zone.y.saturating_add(zone.height / 2),
        width: 1,
        height: 1,
    }
}

fn render_drop_preview(frame: &mut Frame, app: &App, geometry: &Geometry) {
    let keyboard_preview = app.carrying.as_ref().and_then(|source| {
        (source != &app.selected).then(|| DropPreview {
            pane_id: app.selected.clone(),
            edge: app.drop_edge,
        })
    });
    let Some(preview) = app.drop_preview.as_ref().or(keyboard_preview.as_ref()) else {
        return;
    };
    let Some(pane) = geometry
        .panes
        .iter()
        .find(|pane| pane.pane_id == preview.pane_id)
    else {
        return;
    };
    let (zone, label) = drop_zone(pane.rect, preview.edge);
    frame.render_widget(Clear, to_ratatui(zone));
    frame.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Black).bg(Color::Cyan).bold())
            .block(Block::default().borders(Borders::ALL)),
        to_ratatui(zone),
    );
}

fn drop_zone(rect: Rect, edge: Option<Edge>) -> (Rect, &'static str) {
    let quarter_width = (rect.width / 4).max(3).min(rect.width);
    let quarter_height = (rect.height / 4).max(3).min(rect.height);
    match edge {
        Some(Edge::Left) => (
            Rect {
                width: quarter_width,
                ..rect
            },
            "Left",
        ),
        Some(Edge::Right) => (
            Rect {
                x: rect
                    .x
                    .saturating_add(rect.width.saturating_sub(quarter_width)),
                width: quarter_width,
                ..rect
            },
            "Right",
        ),
        Some(Edge::Top) => (
            Rect {
                height: quarter_height,
                ..rect
            },
            "Top",
        ),
        Some(Edge::Bottom) => (
            Rect {
                y: rect
                    .y
                    .saturating_add(rect.height.saturating_sub(quarter_height)),
                height: quarter_height,
                ..rect
            },
            "Bottom",
        ),
        None => (rect, "Swap"),
    }
}

fn footer(app: &App) -> String {
    if let Some(progress) = &app.progress {
        return match progress {
            ApplyProgress::Validating => "Validating live layout…".into(),
            ApplyProgress::Applying { current, total } => {
                format!("Applying {current}/{total}…")
            }
            ApplyProgress::Verifying => "Verifying result…".into(),
            ApplyProgress::Recovering => "Restoring original layout…".into(),
            ApplyProgress::Done => "Layout applied".into(),
        };
    }
    let modified = if app.preview != app.snapshot.tree {
        " · Modified"
    } else {
        ""
    };
    if app.preset_picker.is_some() {
        return "Enter: Preview · /: Switch Built-in ↔ Saved · Esc: Back · ?: Help".into();
    }
    if app.dragging.is_some() {
        return format!("Drop on center to swap · Drop on edge to split{modified}");
    }
    if app.carrying.is_some() {
        return format!("Arrows choose target · Space Drop · Esc Release · ? Help{modified}");
    }
    format!("Drag to arrange · Click + to add · ? Help{modified}")
}

fn render_help(frame: &mut Frame, app: &App) {
    let area = centered_rect(64, 72, frame.area());
    frame.render_widget(Clear, area);
    let preset_mode = app.preset_picker.is_some();
    let text = if preset_mode {
        vec![
            Line::styled("Mouse", Style::default().fg(Color::Cyan).bold()),
            Line::from("  Built-in / Saved   Switch layout source"),
            Line::from("  Layout card        Select a layout"),
            Line::from("  Preview            Return it to Editor as a preview"),
            Line::from("  Rename / Delete    Manage the selected saved layout"),
            Line::from(""),
            Line::styled("Keyboard", Style::default().fg(Color::Cyan).bold()),
            Line::from("  /                  Switch Built-in / Saved"),
            Line::from("  Arrows / h j k l   Select a layout"),
            Line::from("  Enter              Preview selected layout"),
            Line::from("  r / d              Rename / delete a saved layout"),
            Line::from("  Esc                Return to Editor"),
            Line::from("  ?                  Close this help"),
        ]
    } else {
        vec![
            Line::styled("Mouse", Style::default().fg(Color::Cyan).bold()),
            Line::from("  Selected pane +    Add a draft shell on that edge"),
            Line::from("  Drag to center     Swap panes"),
            Line::from("  Drag to edge       Re-parent pane"),
            Line::from("  Drag divider       Resize split"),
            Line::from(""),
            Line::styled("Keyboard", Style::default().fg(Color::Cyan).bold()),
            Line::from("  p                  Open layout presets"),
            Line::from("  s                  Save preview as a custom layout"),
            Line::from("  Arrows / h j k l   Select spatially"),
            Line::from("  Space              Pick up / drop"),
            Line::from("  [ / ]              Resize selected split"),
            Line::from("  =                  Balance all splits 50/50"),
            Line::from("  u / r              Undo / reset preview"),
            Line::from("  d                  Remove selected draft"),
            Line::from("  Enter              Apply preview"),
            Line::from("  Esc                Cancel editor"),
            Line::from("  ?                  Close this help"),
        ]
    };
    let title = if preset_mode {
        " Presets help · ? or Esc to close "
    } else {
        " Editor help · ? or Esc to close "
    };
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(title),
        ),
        area,
    );
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn centered_fixed(width: u16, height: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    ratatui::layout::Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn secondary_label(agent: Option<&str>, process: Option<&str>, status: Option<&str>) -> String {
    match agent {
        Some(agent) => match status.filter(|value| !value.is_empty() && *value != "unknown") {
            Some(status) => format!("{agent} · {status}"),
            None => agent.into(),
        },
        None => process.unwrap_or("shell").into(),
    }
}

fn compact_cwd(cwd: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if cwd == home {
            return "~".into();
        }
        if let Some(rest) = cwd.strip_prefix(&format!("{home}/")) {
            return format!("~/{rest}");
        }
    }
    cwd.into()
}

fn compact_title<'a>(title: &'a str, cwd: &str) -> &'a str {
    if !cwd.is_empty() {
        if let Some(value) = title.strip_suffix(&format!(": {cwd}")) {
            return value;
        }
    }
    title
}

fn visible_title<'a>(title: &'a str, cwd: &str) -> Option<&'a str> {
    let title = compact_title(title, cwd);
    (!looks_like_user_host(title)).then_some(title)
}

fn looks_like_user_host(title: &str) -> bool {
    let Some((user, host)) = title.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && !host.is_empty()
        && !host.contains('@')
        && user
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn short_pane_id(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}

fn to_ratatui(rect: Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect::new(rect.x, rect.y, rect.width, rect.height)
}

fn from_ratatui(rect: ratatui::layout::Rect) -> Rect {
    Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{herdr::Snapshot, model::LayoutNode};
    use ratatui::{backend::TestBackend, Terminal};
    use std::collections::HashMap;

    #[test]
    fn shell_status_does_not_show_unknown() {
        assert_eq!(secondary_label(None, Some("bash"), Some("unknown")), "bash");
        assert_eq!(
            secondary_label(Some("grok"), None, Some("idle")),
            "grok · idle"
        );
    }

    #[test]
    fn pane_ids_are_shortened_for_display() {
        assert_eq!(short_pane_id("w8:p3"), "p3");
        assert_eq!(compact_title("thuanlee@vm: ~", "~"), "thuanlee@vm");
    }

    #[test]
    fn generic_user_host_terminal_titles_are_hidden() {
        assert_eq!(visible_title("thuanlee@vm: ~", "~"), None);
        assert_eq!(visible_title("thuanlee@vm", "~"), None);
        assert_eq!(
            visible_title("Shopify store limits - grok", "~"),
            Some("Shopify store limits - grok")
        );
    }

    #[test]
    fn add_buttons_follow_their_edge_orientation() {
        let zones = add_zones(&PaneRect {
            pane_id: "p1".into(),
            rect: Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            },
        });
        let left = zones.iter().find(|zone| zone.edge == Edge::Left).unwrap();
        let top = zones.iter().find(|zone| zone.edge == Edge::Top).unwrap();
        assert_eq!((left.rect.width, left.rect.height), (3, 3));
        assert_eq!((top.rect.width, top.rect.height), (7, 1));
        assert_eq!(
            add_marker_rect(left.rect),
            Rect {
                x: left.rect.x + 1,
                y: left.rect.y + 1,
                width: 1,
                height: 1,
            }
        );
    }

    #[test]
    fn preset_gallery_renders_every_template_and_exposes_mouse_targets() {
        let mut app = App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "p1".into(),
            tree: LayoutNode::Pane {
                pane_id: "p1".into(),
            },
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        });
        app.open_preset_picker();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut geometry = None;

        terminal
            .draw(|frame| geometry = Some(draw(frame, &app)))
            .unwrap();

        let geometry = geometry.unwrap();
        assert_eq!(geometry.preset_cards.len(), PresetKind::ALL.len());
        assert_eq!(geometry.action_zones.len(), 6);
        for action in [UiAction::PresetPreview, UiAction::PresetBuiltIn] {
            let zone = geometry
                .action_zones
                .iter()
                .find(|zone| zone.action == action)
                .unwrap();
            assert_eq!(
                terminal
                    .backend()
                    .buffer()
                    .cell((zone.rect.x, zone.rect.y))
                    .unwrap()
                    .fg,
                Color::Gray
            );
        }
    }

    #[test]
    fn mode_tabs_are_equal_sized_and_highlight_without_background_fill() {
        let app = App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "p1".into(),
            tree: LayoutNode::Pane {
                pane_id: "p1".into(),
            },
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| drop(draw(frame, &app))).unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "┏");
        assert_eq!(buffer.cell((11, 0)).unwrap().symbol(), "┓");
        assert_eq!(buffer.cell((12, 0)).unwrap().symbol(), "┌");
        assert_eq!(buffer.cell((23, 0)).unwrap().symbol(), "┐");
        assert_eq!(buffer.cell((0, 0)).unwrap().fg, Color::Cyan);
        assert_eq!(buffer.cell((12, 0)).unwrap().fg, Color::Gray);
        for x in 1..11 {
            assert_ne!(buffer.cell((x, 1)).unwrap().bg, Color::Cyan);
        }
    }

    #[test]
    fn delete_action_is_available_only_for_a_selected_draft() {
        let mut app = App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "p1".into(),
            tree: LayoutNode::Pane {
                pane_id: "p1".into(),
            },
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut geometry = None;
        terminal
            .draw(|frame| geometry = Some(draw(frame, &app)))
            .unwrap();
        assert!(!geometry
            .as_ref()
            .unwrap()
            .action_zones
            .iter()
            .any(|zone| { zone.action == UiAction::DeleteDraft }));

        app.add_draft("p1", Edge::Right);
        terminal
            .draw(|frame| geometry = Some(draw(frame, &app)))
            .unwrap();
        let geometry = geometry.unwrap();
        let delete = geometry
            .action_zones
            .iter()
            .find(|zone| zone.action == UiAction::DeleteDraft)
            .unwrap();
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((delete.rect.x, delete.rect.y))
                .unwrap()
                .fg,
            Color::Gray
        );
    }

    #[test]
    fn empty_saved_gallery_and_name_prompt_render_safely() {
        let mut app = App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "p1".into(),
            tree: LayoutNode::Pane {
                pane_id: "p1".into(),
            },
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        });
        app.open_preset_picker();
        app.toggle_preset_collection();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let geometry = draw(frame, &app);
                assert!(geometry.preset_cards.is_empty());
            })
            .unwrap();

        app.close_preset_picker();
        app.open_save_prompt();
        let mut prompt_geometry = None;
        terminal
            .draw(|frame| {
                prompt_geometry = Some(draw(frame, &app));
            })
            .unwrap();
        let prompt_geometry = prompt_geometry.unwrap();
        assert!(prompt_geometry
            .action_zones
            .iter()
            .any(|zone| zone.action == UiAction::DialogConfirm));
        assert!(prompt_geometry
            .action_zones
            .iter()
            .any(|zone| zone.action == UiAction::DialogCancel));
    }

    #[test]
    fn success_toast_does_not_resize_the_editor_canvas() {
        let mut app = App::new(Snapshot {
            workspace_id: "w".into(),
            tab_id: "t".into(),
            focused_pane_id: "p1".into(),
            tree: LayoutNode::Pane {
                pane_id: "p1".into(),
            },
            metadata: HashMap::new(),
            revisions: HashMap::new(),
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut without_message = None;
        terminal
            .draw(|frame| without_message = Some(draw(frame, &app)))
            .unwrap();
        app.set_success("Custom layout saved");
        let mut with_message = None;
        terminal
            .draw(|frame| with_message = Some(draw(frame, &app)))
            .unwrap();
        assert_eq!(without_message.unwrap().panes, with_message.unwrap().panes);
    }
}
