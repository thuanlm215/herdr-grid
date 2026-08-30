use crate::{
    app::{App, DropPreview},
    herdr::ApplyProgress,
    model::{Edge, Geometry, PaneRect, Rect},
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
    let geo = Geometry::calculate(
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

    frame.render_widget(
        Paragraph::new(footer(app)).block(Block::default().borders(Borders::TOP)),
        rows[2],
    );

    if app.show_help {
        render_help(frame);
    }
    geo
}

fn render_pane(frame: &mut Frame, app: &App, pane: &PaneRect) {
    let metadata = app.snapshot.metadata.get(&pane.pane_id);
    let raw_title = metadata
        .and_then(|value| value.terminal_title_stripped.as_deref())
        .unwrap_or(&pane.pane_id);
    let cwd = metadata
        .and_then(|value| value.cwd.as_deref())
        .map(compact_cwd)
        .unwrap_or_default();
    let title = visible_title(raw_title, &cwd);
    let process = metadata.and_then(|value| value.process_name.as_deref());
    let agent = metadata.and_then(|value| value.agent.as_deref());
    let status = metadata.and_then(|value| value.agent_status.as_deref());
    let secondary = secondary_label(agent, process, status);

    let selected = pane.pane_id == app.selected;
    let moving = app.carrying.as_ref() == Some(&pane.pane_id)
        || app.dragging.as_ref() == Some(&pane.pane_id);
    let border_color = if moving {
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
                .title(format!(" {} ", short_pane_id(&pane.pane_id))),
        ),
        ratatui::layout::Rect::new(pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height),
    );
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
    if app.carrying.is_some() {
        return format!("Arrows choose target · Space Drop · Esc Release · ? Help{modified}");
    }
    format!("Drag to arrange · Enter Apply · Esc Cancel · ? Help{modified}")
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
        Line::from("  Arrows / h j k l   Select spatially"),
        Line::from("  Space              Pick up / drop"),
        Line::from("  [ / ]              Resize selected split"),
        Line::from("  u / r              Undo / reset preview"),
        Line::from("  Enter              Apply preview"),
        Line::from("  Esc                Cancel or dismiss"),
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
}
