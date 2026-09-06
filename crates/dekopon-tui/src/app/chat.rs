//! No-TTY state and safe rendering for the development chat view.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Block, Paragraph, Wrap},
};
use serde_json::Value;

use crate::{
    chat::MAX_LINE_BYTES,
    redact::{redact, sanitize},
};

/// Always visible: a declared subject is not an authenticated production identity.
pub const CHAT_WARNING: &str = "DEVELOPMENT ONLY: owner-reachable socket; caller declares subject. Not production authentication.";

/// A bounded composer and the latest reply, not a local model history or tool transcript.
#[derive(Default)]
pub struct ChatApp {
    /// Text being composed.
    pub composer: String,
    /// Latest reply or payload-free diagnostic.
    pub reply: String,
    /// One exchange at a time, since the wire has no correlation IDs.
    pub busy: bool,
    /// Exit requested by the operator.
    pub should_quit: bool,
}

impl ChatApp {
    /// Handles keys without a terminal. Ctrl-C quits even while a reply is pending.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<String> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }
        match key.code {
            KeyCode::Esc => self.composer.clear(),
            KeyCode::Backspace => {
                self.composer.pop();
            }
            KeyCode::Char(c) if self.composer.len() + c.len_utf8() < MAX_LINE_BYTES => {
                self.composer.push(c);
            }
            KeyCode::Enter if !self.busy && !self.composer.trim().is_empty() => {
                self.busy = true;
                return Some(std::mem::take(&mut self.composer));
            }
            _ => {}
        }
        None
    }

    /// Draws every dynamic string through the existing redaction and sanitization seam.
    pub fn draw(&self, frame: &mut Frame, subject: &str, channel: &str) {
        let areas = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(5),
        ])
        .split(frame.area());
        frame.render_widget(Paragraph::new(safe(&format!(
            "{CHAT_WARNING}\nsubject: {subject} | conversation: {channel}\nEnter sends; Esc clears; Ctrl-C quits. {}",
            if self.busy { "Waiting; quitting does not cancel gateway work." } else { "" }
        ))).wrap(Wrap { trim: false }), areas[0]);
        frame.render_widget(
            Paragraph::new(safe(&self.reply))
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title("latest reply")),
            areas[1],
        );
        frame.render_widget(
            Paragraph::new(safe(&self.composer))
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title("message")),
            areas[2],
        );
    }
}

fn safe(text: &str) -> String {
    let value = redact(&Value::String(text.to_owned())).value;
    sanitize(value.as_str().unwrap_or_default())
}
