//! The tab bar, the status line, and the keybinding overlay.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::Theme;
use crate::app::{App, Pane};

/// Shows the represented identity separately from the authenticated console peer.
pub fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let agent = app
        .session
        .as_ref()
        .map_or("picker", |session| session.agent.as_str());
    // A single overflowing line used to clip off the live-effects warning at 80 columns.
    // Keep that warning on its own first row; identity and scope have bounded separate rows.
    let [warning, model, context, identity, scope] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .areas(area);
    for (row, label) in [
        (warning, "LIVE PROVIDERS — effects are real".to_owned()),
        (
            model,
            format!("model ({}): {}", app.model_source.label(), app.model),
        ),
        (
            context,
            format!(
                "agent: {agent} · profile: {}",
                app.profile.as_deref().unwrap_or("subject-only")
            ),
        ),
        (
            identity,
            format!(
                "subject: {} · console: dekopon-console (uid {})",
                app.subject,
                rustix::process::geteuid().as_raw()
            ),
        ),
        (scope, format!("REQUESTED scope: {}", app.scope_label)),
    ] {
        frame.render_widget(
            Paragraph::new(crate::redact::sanitize_line(&label)).wrap(Wrap { trim: true }),
            row,
        );
    }
}

/// Draws the pane tabs.
pub fn draw_tabs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut spans = Vec::with_capacity(Pane::ORDER.len() * 2);
    for pane in Pane::ORDER {
        let style = if pane == app.pane {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().fg(Theme::FORGOTTEN)
        };
        spans.push(Span::styled(format!(" {} ", pane.title()), style));
        spans.push(Span::raw(" "));
    }
    if app.busy {
        spans.push(Span::styled(
            "· running",
            Style::default().fg(Theme::LOCAL_WRITE),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draws the status line: the notice, then the facts that must never be a guess.
pub fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(56)]).areas(area);

    let notice = app.notice.as_ref().map_or_else(
        || Span::styled("? for keys", Style::default().fg(Theme::FORGOTTEN)),
        |notice| {
            let style = if notice.is_refusal {
                Style::default().fg(Theme::DENIED)
            } else {
                Style::default()
            };
            Span::styled(crate::redact::sanitize_line(&notice.text), style)
        },
    );
    frame.render_widget(Paragraph::new(Line::from(notice)), left);

    // The subject and the credential file decide what a session may do and whose token it spends.
    // Both are resolved rather than typed, so both are on screen rather than in an operator's head.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "{} · {}",
                app.subject,
                credential_label(&app.credential_path)
            ),
            Style::default().fg(Theme::FORGOTTEN),
        )))
        .alignment(Alignment::Right),
        right,
    );
}

/// The credential file's own name, which is the part that says whose credential this is.
fn credential_label(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Confirmation for a scope claim which this broker wire cannot echo as trusted context.
pub fn draw_scope_warning(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 86, 12);
    frame.render_widget(Clear, area);
    let text = format!(
        "REQUESTED scope: {}\n\nThe broker protocol does not echo authenticated chat scope. An attestor with empty chatScopes silently downgrades this claim to subject-only. Console-via Cedar must refuse missing trusted scope; verify exact grants before any model turn.\n\nLIVE PROVIDERS — effects are real. Enter to continue; any other key to cancel.",
        app.scope_label
    );
    frame.render_widget(
        Paragraph::new(
            text.lines()
                .map(crate::redact::sanitize_line)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Unverified route-context claim"),
        ),
        area,
    );
}

/// Draws the keybinding overlay.
pub fn draw_help(frame: &mut Frame<'_>) {
    let area = centered(frame.area(), 62, 17);
    frame.render_widget(Clear, area);

    let keys = [
        ("tab / shift-tab", "next / previous pane"),
        ("j k  ↑ ↓", "move"),
        ("enter", "hop into the highlighted agent"),
        ("i", "compose a turn or a shell line"),
        ("enter (composing)", "send"),
        ("esc", "stop a running turn, or leave the composer"),
        ("o", "expand the highlighted capability call"),
        ("r", "reveal one redacted field"),
        ("?", "this"),
        ("q", "quit"),
    ];
    let mut lines: Vec<Line<'_>> = keys
        .iter()
        .map(|(key, meaning)| {
            Line::from(vec![
                Span::styled(
                    format!("{key:>18}  "),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(*meaning),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  a stop is cooperative: calls already sent still complete",
        Style::default().fg(Theme::FORGOTTEN),
    ));

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" keys ")),
        area,
    );
}

/// Centres a fixed-size box, clamped to what the terminal actually has.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}
