use crate::{
    app::{App, DropPreview},
    herdr::ApplyProgress,
    model::{is_draft_pane, AddZone, Edge, Geometry, PaneRect, Rect},
};
use ratatui::{
    layout::{Alignment, Constraint, Layout},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn draw(frame: &mut Frame, app: &App) -> Geometry {
    let outer = frame.area();
    let message_height = u16::from(app.message.is_some()) * 3;
    let rows = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(message_height),
        Constraint::Length(2),
    ])
    .split(outer);

    let canvas = rows[0];
    let mut geo = Geometry::calculate(
        &app.preview,
        Rect {
            x: canvas.x,
            y: canvas.y,
            width: canvas.width,
            height: canvas.height,
        },
    );
    for pane in &geo.panes {
        render_pane(frame, app, pane);
    }
    if app.add_mode {
        if let Some(target) = &app.add_target {
            if let Some(pane) = geo.panes.iter().find(|pane| &pane.pane_id == target) {
                geo.add_zones = add_zones(pane);
                render_add_zones(frame, &geo.add_zones);
            }
        }
    }
    render_drop_preview(frame, app, &geo);

    if let Some(message) = &app.message {
        frame.render_widget(
            Paragraph::new(message.as_str())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red))
                        .title(" Error · Esc dismiss "),
                ),
            rows[1],
        );
    }

    let footer_style = if app.add_mode {
        Style::default().fg(Color::LightCyan).bold()
    } else {
        Style::default()
    };
    frame.render_widget(
        Paragraph::new(footer(app)).style(footer_style).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(footer_style),
        ),
        rows[2],
    );

    if app.show_help {
        render_help(frame);
    }
    geo
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
        if app.add_mode && app.add_target.as_ref() == Some(&pane.pane_id) {
            Color::LightGreen
        } else {
            Color::Green
        }
    } else if app.add_mode {
        if app.add_target.as_ref() == Some(&pane.pane_id) {
            Color::LightCyan
        } else {
            Color::Cyan
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
    if app.dragging.is_some() {
        return format!("Drop on center to swap · Drop on edge to split{modified}");
    }
    if app.add_mode {
        return format!(
            "ADD PANE · Click pane → + · d Delete draft · Enter Done · Esc Cancel preview{modified}"
        );
    }
    if app.carrying.is_some() {
        return format!("Arrows choose target · Space Drop · Esc Release · ? Help{modified}");
    }
    format!("Drag to arrange · n Add pane · Enter Apply · Esc Cancel · ? Help{modified}")
}

fn render_help(frame: &mut Frame) {
    let area = centered_rect(64, 72, frame.area());
    frame.render_widget(Clear, area);
    let text = vec![
        Line::styled("Mouse", Style::default().fg(Color::Cyan).bold()),
        Line::from("  Drag to center     Swap panes"),
        Line::from("  Drag to edge       Re-parent pane"),
        Line::from("  Drag divider       Resize split"),
        Line::from(""),
        Line::styled("Keyboard", Style::default().fg(Color::Cyan).bold()),
        Line::from("  n                  Add pane mode"),
        Line::from("  Arrows / h j k l   Select spatially"),
        Line::from("  Space              Pick up / drop"),
        Line::from("  [ / ]              Resize selected split"),
        Line::from("  u / r              Undo / reset preview"),
        Line::from("  Enter              Apply preview"),
        Line::from("  Esc                Cancel or dismiss"),
        Line::from(""),
        Line::styled(
            "Add pane mode",
            Style::default().fg(Color::LightGreen).bold(),
        ),
        Line::from("  Click pane, then + Add a draft shell"),
        Line::from("  d                  Remove selected draft"),
        Line::from("  Enter              Keep drafts and exit mode"),
        Line::from("  Esc                Discard preview and exit mode"),
    ];
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Help · ? or Esc to close "),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
