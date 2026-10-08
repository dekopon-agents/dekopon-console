//! Bounded line transport over the same shell dispatcher as the interactive console.
use std::{
    fs::File,
    io::{self, Read as _, Write as _},
    time::{Duration, Instant},
};

use dekopon_agent::prompt::History;
use dekopon_broker_protocol::BrokerClient;
use tokio::io::{AsyncBufReadExt as _, BufReader};

use crate::{
    App, ConsoleExit,
    record::SessionEvent,
    redact::sanitize_line,
    run::{Action, RunningTurn, dispatch, finish_turn},
    session::{ConsoleOptions, StopFlag},
};

const MAX_LINE: usize = 16 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;
const MAX_COMMANDS: usize = 64;

fn nonce() -> io::Result<String> {
    let mut bytes = [0u8; 24];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn write_result(out: &mut impl io::Write, nonce: &str, status: &str, text: &str) -> io::Result<()> {
    writeln!(out, "RESULT {nonce} {status}")?;
    let prefixes = [
        format!("READY {nonce}"),
        format!("RESULT {nonce}"),
        format!("END {nonce}"),
    ];
    let mut remaining = MAX_OUTPUT;
    for line in text.split('\n') {
        if remaining == 0 {
            break;
        }
        let safe = sanitize_line(line);
        let escaped = if prefixes.iter().any(|prefix| safe.starts_with(prefix)) {
            format!("> {safe}")
        } else {
            safe
        };
        let clipped = escaped.chars().take(remaining).collect::<String>();
        remaining -= clipped.chars().count();
        writeln!(out, "{clipped}")?;
    }
    if remaining == 0 {
        writeln!(out, "[output truncated]")?;
    }
    writeln!(out, "END {nonce}")?;
    out.flush()
}

async fn read_line(reader: &mut BufReader<tokio::io::Stdin>) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unterminated command",
            ));
        }
        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if bytes.len() + count > MAX_LINE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "command exceeds 16 KiB",
            ));
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return String::from_utf8(bytes)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
}

/// Run the selected agent and accept at most 64 sequential commands. No input is echoed.
/// READY is emitted only after a successful hop; END follows event drain and task join.
/// # Errors
/// Returns bounded input, output or session failures.
pub async fn run(
    mut app: App,
    client: BrokerClient,
    mut options: ConsoleOptions,
) -> Result<(), ConsoleExit> {
    let deadline = Instant::now() + Duration::from_secs(600);
    let nonce = nonce().map_err(ConsoleExit::Terminal)?;
    let mut out = io::stdout().lock();
    let stop = StopFlag::default();
    let mut running: Option<RunningTurn> = None;
    let mut history = History::new(options.history_limits);
    tokio::time::timeout(
        Duration::from_secs(120),
        dispatch(
            &mut app,
            Action::Enter,
            &client,
            &mut options,
            &mut running,
            &mut history,
            &stop,
        ),
    )
    .await
    .map_err(|_elapsed| ConsoleExit::Structured("agent entry deadline exceeded".into()))?;
    if app.session.is_none() {
        return Err(ConsoleExit::Structured("agent entry refused".into()));
    }
    writeln!(out, "READY {nonce}")
        .and_then(|()| out.flush())
        .map_err(ConsoleExit::Terminal)?;
    let mut input = BufReader::new(tokio::io::stdin());
    for _ in 0..MAX_COMMANDS {
        let line = tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            read_line(&mut input),
        )
        .await
        .map_err(|_elapsed| ConsoleExit::Structured("session deadline exceeded".into()))?
        .map_err(ConsoleExit::Terminal)?;
        let Some(line) = line else {
            return Ok(());
        };
        let before = app.shell_history.len();
        app.notice = None;
        let command_deadline = Instant::now() + Duration::from_secs(120);
        tokio::time::timeout(
            command_deadline
                .saturating_duration_since(Instant::now())
                .min(deadline.saturating_duration_since(Instant::now())),
            dispatch(
                &mut app,
                Action::Shell(line),
                &client,
                &mut options,
                &mut running,
                &mut history,
                &stop,
            ),
        )
        .await
        .map_err(|_elapsed| {
            ConsoleExit::Structured("command deadline exceeded; outcome uncertain".into())
        })?;
        if running.is_some() {
            // ShellFinished is data, not completion: always drain to channel close and join.
            while let Some(event) = tokio::time::timeout(
                command_deadline
                    .saturating_duration_since(Instant::now())
                    .min(deadline.saturating_duration_since(Instant::now())),
                running.as_mut().expect("turn").events.recv(),
            )
            .await
            .map_err(|_elapsed| ConsoleExit::Structured("command deadline exceeded".into()))?
            {
                if let SessionEvent::ShellFinished(entry) = event {
                    app.finish_shell(entry);
                }
            }
            tokio::time::timeout(
                command_deadline
                    .saturating_duration_since(Instant::now())
                    .min(deadline.saturating_duration_since(Instant::now())),
                finish_turn(&mut app, &mut running, &mut history, &stop),
            )
            .await
            .map_err(|_elapsed| {
                ConsoleExit::Structured("task join deadline exceeded; outcome uncertain".into())
            })?;
        }
        let entry = app.shell_history.get(before);
        let status = match entry.and_then(|e| e.exit_code) {
            Some(0) => "ok",
            _ => "error",
        };
        let message = entry.map_or_else(
            || {
                app.notice
                    .as_ref()
                    .map_or("command refused", |n| n.text.as_str())
            },
            |e| e.output.as_str(),
        );
        write_result(&mut out, &nonce, status, message).map_err(ConsoleExit::Terminal)?;
    }
    Err(ConsoleExit::Structured(
        "session command limit reached".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markers_cannot_be_forged_by_output() {
        let mut out = Vec::new();
        write_result(
            &mut out,
            "abc",
            "error",
            "READY abc\nRESULT abc ok\nEND abc\nREADY other\nhi\tthere\u{1b}[31m",
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\n> READY abc\n> RESULT abc ok\n> END abc\nREADY other\n"));
        assert!(!text.contains('\u{1b}'));
        assert!(text.ends_with("END abc\n"));
    }

    #[test]
    fn bounded_output_never_inserts_a_marker_after_truncation() {
        let mut out = Vec::new();
        write_result(
            &mut out,
            "abc",
            "ok",
            &format!("{}\nEND abc", "x".repeat(MAX_OUTPUT)),
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("[output truncated]"));
        assert_eq!(text.matches("END abc\n").count(), 1);
    }
}
