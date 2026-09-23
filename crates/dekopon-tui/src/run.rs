//! Terminal lifecycle and the event loop that joins keys to sessions.
//!
//! Two things here are worth more care than they look. The terminal is restored on *every* exit
//! path including a panic, because a panic inside a raw-mode alternate screen leaves the operator
//! with a shell that no longer echoes. And `tracing` never reaches stdout while this runs, because
//! this process owns the screen and one stray log line corrupts a frame.

use std::{
    io::{self, Stdout},
    panic,
    sync::Arc,
};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use dekopon_agent::prompt::{History, HistoryLimits};
use dekopon_broker_protocol::BrokerClient;
use dekopon_model::model::ChatModel;
use dekopon_process::CancelSignal;
use dekopon_shell::Interpreter;
use futures_util::StreamExt as _;
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::Instrument as _;

use crate::{
    app::{App, Mode, Notice, Pane, ShellEntry},
    record::{RecordingProgress, SessionEvent},
    session::{
        AgentSession, ConsoleOptions, LegHandle, SessionError, StopFlag, build_model, open_agent,
        session_channel,
    },
    ui,
};

/// Restores the terminal on drop, whatever the reason for the drop.
///
/// A guard rather than a call at the end of `run`: an error return, a `?`, and a panic all have to
/// leave the terminal usable, and only a destructor covers all three.
struct TerminalGuard;

impl TerminalGuard {
    /// Enters the alternate screen and installs a panic hook that leaves it.
    fn enter() -> io::Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        // No mouse capture. Every mouse event this loop received was discarded, and capturing them
        // took the terminal's own text selection and scrollback away from the operator to do it —
        // which is exactly what an operator wants after revealing a field.
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            if let Err(restore_error) = disable_raw_mode() {
                eprintln!("warning: raw mode cleanup failed: {restore_error}");
            }
            return Err(error);
        }
        let terminal = match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(terminal) => terminal,
            Err(error) => {
                restore();
                return Err(error);
            }
        };

        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            // Restore first, then let the default hook print: a backtrace drawn into the alternate
            // screen vanishes with it, which is how a panic becomes a silent exit.
            restore();
            previous(info);
        }));

        Ok((Self, terminal))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

/// Puts the terminal back the way it was found.
///
/// Both failures are reported to standard error rather than through `tracing`: this runs while the
/// screen is being given back, including from a panic hook, and a terminal left in raw mode is
/// something the operator has to fix with `reset` — so the reason has to reach them somewhere that
/// survives the subscriber.
fn restore() {
    if let Err(error) = disable_raw_mode() {
        eprintln!("warning: could not leave raw mode ({error}); run `reset` to restore your shell");
    }
    if let Err(error) = execute!(io::stdout(), LeaveAlternateScreen) {
        eprintln!("warning: could not leave the alternate screen ({error}); run `reset`");
    }
}

/// One turn in flight, and what the console needs back when it finishes.
struct RunningTurn {
    events: UnboundedReceiver<SessionEvent>,
    handle: tokio::task::JoinHandle<Result<History, SessionError>>,
    shell: bool,
}

/// Runs the console until the operator quits.
///
/// # Errors
///
/// Returns a terminal failure. Every other failure — an unreachable broker, a refused hop, a
/// session that died — is drawn rather than returned, because the console is still usable after
/// each of them.
pub async fn run(
    mut app: App,
    client: BrokerClient,
    mut options: ConsoleOptions,
) -> Result<(), ConsoleExit> {
    let (_guard, mut terminal) = TerminalGuard::enter().map_err(ConsoleExit::Terminal)?;

    let stop = StopFlag::default();
    let mut keys = EventStream::new();
    let mut running: Option<RunningTurn> = None;
    let mut history = History::new(options.history_limits);
    if options.initial_agent.is_some() {
        dispatch(
            &mut app,
            Action::Enter,
            &client,
            &mut options,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
    }

    drive_event_loop(
        &mut terminal,
        &mut app,
        &client,
        &mut options,
        &mut keys,
        &mut running,
        &mut history,
        &stop,
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "the loop owns the current terminal, inputs and cancellable session state together"
)]
async fn drive_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    client: &BrokerClient,
    options: &mut ConsoleOptions,
    keys: &mut EventStream,
    running: &mut Option<RunningTurn>,
    history: &mut History,
    stop: &StopFlag,
) -> Result<(), ConsoleExit> {
    loop {
        if let Err(error) = terminal.draw(|frame| ui::draw(frame, app)) {
            stop_running(running, stop).await;
            return Err(ConsoleExit::Terminal(error));
        }
        if app.should_quit {
            stop_running(running, stop).await;
            return Ok(());
        }

        // One select over both inputs, so a churning turn keeps redrawing while keys stay live —
        // which is what makes Esc able to stop it.
        tokio::select! {
            key = keys.next() => match key {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    if let Some(action) = on_key(app, key, stop) {
                        dispatch(app, action, client, options, running,
                                 history, stop).await;
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => { stop_running(running, stop).await; return Err(ConsoleExit::Terminal(error)); },
                None => { app.should_quit = true; },
            },
            event = recv(running) => {
                match event {
                    Some(SessionEvent::Progress(message)) => app.notice = Some(Notice::info(message)),
                    Some(SessionEvent::ShellFinished(entry)) => app.push_shell(entry),
                    Some(event) => app.on_session_event(event),
                    None => finish_turn(app, running, history, stop).await,
                }
            }
        }
    }
}

/// Join the blocking turn before releasing its broker leg and restoring the terminal.
async fn stop_running(running: &mut Option<RunningTurn>, stop: &StopFlag) {
    if let Some(turn) = running.take() {
        stop.request();
        if let Err(error) = turn.handle.await {
            tracing::warn!(%error, "stopped console task did not join cleanly");
        }
    }
}

/// Waits for the next session event, or for there to be no session.
///
/// Pending forever when nothing is running, so `select!` simply never takes this branch rather than
/// spinning on an immediately-ready `None`.
async fn recv(running: &mut Option<RunningTurn>) -> Option<SessionEvent> {
    match running {
        Some(turn) => turn.events.recv().await,
        None => std::future::pending().await,
    }
}

/// Collects a finished turn's history back and reports how it ended.
async fn finish_turn(
    app: &mut App,
    running: &mut Option<RunningTurn>,
    history: &mut History,
    stop: &StopFlag,
) {
    let Some(turn) = running.take() else {
        return;
    };
    let shell = turn.shell;
    match turn.handle.await {
        Ok(Ok(returned)) => *history = returned,
        // The session's own history is lost, so the model's replay window is now a guess. Saying so
        // is better than silently continuing against a window that no longer matches the screen.
        Ok(Err(error)) => app.notice = Some(Notice::refusal(error.to_string())),
        Err(error) => app.notice = Some(Notice::refusal(format!("the session task died: {error}"))),
    }
    stop.reset();
    if shell {
        app.busy = false;
    } else {
        app.on_session_complete(history.len());
    }
}

/// Something the loop must do that needs more than the state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Hop into the highlighted agent.
    Enter,
    /// Run one prompt as a turn.
    Turn(String),
    /// Run one line through the interpreter.
    Shell(String),
}

/// Maps one key press onto a state change, and possibly an action.
///
/// Split from the loop so the whole of the console's key handling is testable without a terminal.
pub fn on_key(app: &mut App, key: KeyEvent, stop: &StopFlag) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return None;
    }
    if app.mode == Mode::ScopeWarning {
        app.mode = Mode::Browsing;
        if key.code == KeyCode::Enter {
            app.scope_warning_confirmed = true;
            return Some(Action::Enter);
        }
        app.notice = Some(Notice::info("requested scope was not opened"));
        return None;
    }
    if app.mode == Mode::Help {
        app.mode = Mode::Browsing;
        return None;
    }
    if app.mode == Mode::Composing {
        return on_composing_key(app, key);
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Tab => app.pane = app.pane.next(),
        KeyCode::BackTab => app.pane = app.pane.previous(),
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        KeyCode::Char('i') if matches!(app.pane, Pane::Turns | Pane::Shell) => {
            app.mode = Mode::Composing;
        }
        KeyCode::Enter if app.pane == Pane::Agents => return Some(Action::Enter),
        // The turns pane's own cursor. `o` and `r` are the two keys the help overlay has always
        // advertised, and they act on one capability call at a time rather than on a selection
        // mode: `r` uncovers the next still-hidden field of that one call and says so.
        KeyCode::Char('o') if app.pane == Pane::Turns => app.toggle_cursor_call(),
        KeyCode::Char('r') if app.pane == Pane::Turns => app.reveal_next(),
        // A stop is requested through the state machine first, so the console and the session agree
        // on whether there was anything to stop.
        KeyCode::Esc if app.request_stop() => stop.request(),
        _ => {}
    }
    None
}

fn on_composing_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Browsing;
            app.composer.clear();
        }
        KeyCode::Backspace => {
            app.composer.pop();
        }
        KeyCode::Char(character) => app.composer.push(character),
        KeyCode::Enter => {
            return match app.pane {
                Pane::Shell => {
                    let line = app.composer.trim().to_owned();
                    app.composer.clear();
                    app.mode = Mode::Browsing;
                    (!line.is_empty()).then_some(Action::Shell(line))
                }
                _ => app.submit_turn().map(Action::Turn),
            };
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests;

fn clear_agent_context(app: &mut App, history: &mut History, limits: HistoryLimits) {
    app.session = None;
    app.broker_trace = None;
    app.transcript = Default::default();
    app.shell_history.clear();
    *history = History::new(limits);
}

async fn dispatch(
    app: &mut App,
    action: Action,
    client: &BrokerClient,
    options: &mut ConsoleOptions,
    running: &mut Option<RunningTurn>,
    history: &mut History,
    stop: &StopFlag,
) {
    let span = match &action {
        Action::Enter => tracing::Span::none(),
        Action::Turn(_) => console_turn_span("model"),
        Action::Shell(_) => console_turn_span("shell"),
    };
    dispatch_in_context(app, action, client, options, running, history, stop)
        .instrument(span)
        .await;
}

/// Each submitted turn owns a root, never the preceding turn's context.
pub fn console_turn_span(kind: &'static str) -> tracing::Span {
    tracing::info_span!(parent: None, "console.turn", console.origin = "dekopon-console", console.kind = kind)
}

async fn dispatch_in_context(
    app: &mut App,
    action: Action,
    client: &BrokerClient,
    options: &mut ConsoleOptions,
    running: &mut Option<RunningTurn>,
    history: &mut History,
    stop: &StopFlag,
) {
    match action {
        Action::Enter => {
            if app.busy {
                app.notice = Some(Notice::refusal(
                    "stop the current turn before switching agents",
                ));
                return;
            }
            let Some(agent) = app.highlighted_id() else {
                app.notice = Some(Notice::refusal("the catalog contains no agents"));
                return;
            };
            if options.fixed_profile.as_ref().is_some_and(|name| {
                options
                    .profiles
                    .iter()
                    .any(|p| &p.name == name && p.agent != agent)
            }) {
                app.notice = Some(Notice::refusal(
                    "--profile is bound to its agent; restart with a matching profile",
                ));
                return;
            }
            let matches: Vec<_> = options
                .profiles
                .iter()
                .filter(|p| p.agent == agent)
                .collect();
            if matches.len() > 1 && options.fixed_profile.is_none() {
                app.notice = Some(Notice::refusal(format!(
                    "multiple profiles for {agent}; restart with --profile <NAME>"
                )));
                return;
            }
            if let Some(profile) = options
                .fixed_profile
                .as_ref()
                .and_then(|name| matches.iter().copied().find(|p| &p.name == name))
                .or_else(|| matches.first().copied())
            {
                options.subject = profile.subject.clone();
                options.scope = profile.scope.clone();
                (options.model, options.model_source) = crate::session::selected_model(
                    options.model_override.as_deref(),
                    profile.model.as_deref(),
                );
                options.prompt_limits.max_steps =
                    options.steps_override.or(profile.max_steps).unwrap_or(8);
                options.prompt_limits.max_capability_calls = options
                    .calls_override
                    .or(profile.max_capability_calls)
                    .unwrap_or(16);
                app.profile = Some(profile.name.clone());
            } else if !options.profiles.is_empty() {
                app.notice = Some(Notice::refusal(format!(
                    "no authored profile for {agent}; restart without --profiles and with --subject for subject-only use"
                )));
                return;
            }
            // A hop (including a refused hop or pending scope warning) must never leave
            // the prior agent usable under the newly selected subject/model.
            clear_agent_context(app, history, options.history_limits);
            app.subject = options.subject.to_string();
            app.scope_label = options
                .scope
                .as_ref()
                .map(|s| serde_json::to_string(s).expect("typed scope serializes"))
                .unwrap_or_else(|| "subject-only".into());
            app.model = options.model.clone();
            app.model_source = options.model_source;
            if options.scope.is_some() && !app.scope_warning_confirmed {
                app.mode = Mode::ScopeWarning;
                return;
            }
            app.scope_warning_confirmed = false;
            match open_agent(
                client.clone(),
                options.subject.clone(),
                agent.clone(),
                options.scope.clone(),
            )
            .await
            {
                Ok(leg) if leg.effective_capabilities().is_empty() => {
                    app.notice = Some(Notice::refusal(
                        "broker grants no capabilities for this context; no model call will be sent",
                    ));
                }
                Ok(leg) => {
                    app.enter(AgentSession::new(agent, leg, options.history_limits));
                }
                Err(error) => app.notice = Some(Notice::refusal(error.to_string())),
            }
        }
        Action::Turn(prompt) => {
            let Some(session) = app.session.as_ref() else {
                return;
            };
            let leg = match open_agent(
                client.clone(),
                options.subject.clone(),
                session.agent.clone(),
                options.scope.clone(),
            )
            .await
            {
                Ok(leg) if !leg.effective_capabilities().is_empty() => leg,
                Ok(_) => {
                    refuse_turn(
                        app,
                        "broker grants no capabilities; no model call will be sent".into(),
                    );
                    return;
                }
                Err(error) => {
                    refuse_turn(app, error.to_string());
                    return;
                }
            };
            app.broker_trace = Some(leg.session_trace().to_string());
            let model: Arc<dyn ChatModel + Send + Sync> = match build_model(options) {
                Ok(model) => Arc::from(model),
                Err(error) => {
                    refuse_turn(app, error.to_string());
                    return;
                }
            };
            let (sender, receiver) = session_channel();
            let progress = Arc::new(RecordingProgress::new(sender.clone()));
            stop.reset();
            let (handle_cancel, signal) = CancelSignal::pair();
            stop.bind_broker(handle_cancel);
            let handle = tokio::spawn(
                crate::session::run_turn(
                    Arc::new(leg.with_cancel_signal(signal).with_progress(
                        progress.clone(),
                        options.prompt_limits.max_capability_calls,
                    )),
                    model,
                    prompt,
                    instructions(app),
                    app.agents
                        .iter()
                        .find(|agent| agent.metadata.name == session.agent.as_str())
                        .expect("session catalog agent")
                        .clone(),
                    options.clone(),
                    std::mem::replace(history, History::new(options.history_limits)),
                    stop.clone(),
                    progress,
                    sender,
                )
                .in_current_span(),
            );
            *running = Some(RunningTurn {
                events: receiver,
                handle,
                shell: false,
            });
        }
        Action::Shell(line) => {
            let Some(session) = app.session.as_ref() else {
                app.notice = Some(Notice::refusal("hop into an agent first"));
                return;
            };
            if app.busy {
                app.notice = Some(Notice::refusal(
                    "another command or turn is running; press Esc to stop it",
                ));
                return;
            }
            let leg = match open_agent(
                client.clone(),
                options.subject.clone(),
                session.agent.clone(),
                options.scope.clone(),
            )
            .await
            {
                Ok(leg) if !leg.effective_capabilities().is_empty() => leg,
                Ok(_) => {
                    app.notice = Some(Notice::refusal(
                        "broker grants no capabilities; no command will be sent",
                    ));
                    return;
                }
                Err(error) => {
                    app.notice = Some(Notice::refusal(error.to_string()));
                    return;
                }
            };
            app.broker_trace = Some(leg.session_trace().to_string());
            let limits = options.shell_limits;
            let (sender, receiver) = session_channel();
            stop.reset();
            let (handle_cancel, signal) = CancelSignal::pair();
            stop.bind_broker(handle_cancel);
            app.busy = true;
            let previous = std::mem::replace(history, History::new(options.history_limits));
            let span = tracing::Span::current();
            let handle = tokio::spawn(async move {
                tokio::task::spawn_blocking(move || {
                    let _entered = span.enter();
                    let script = tracing::info_span!("console.script");
                    let _script = script.enter();
                    tracing::info!(target: "dekopon_tui::audit", script = line.as_str(), "console manual script");
                    let outcome = Interpreter::new(limits)
                        .run(&line, &LegHandle(Arc::new(leg.with_cancel_signal(signal))));
                    tracing::info!(target: "dekopon_tui::audit", output = outcome.output.as_str(), exit_code = outcome.exit_code.get(), "console manual output");
                    if sender
                        .send(SessionEvent::ShellFinished(ShellEntry {
                            input: line,
                            output: outcome.output,
                            exit_code: outcome.exit_code.get(),
                        }))
                        .is_err()
                    {
                        tracing::debug!("shell output lost because the terminal closed");
                    }
                    previous
                })
                .await
                .map_err(SessionError::Task)
            }.in_current_span());
            *running = Some(RunningTurn {
                events: receiver,
                handle,
                shell: true,
            });
        }
    }
}

/// Closes the transcript when a fresh gate or model setup refuses before inference.
fn refuse_turn(app: &mut App, message: String) {
    app.on_session_event(SessionEvent::Finished(Box::new(Err(message.clone()))));
    app.notice = Some(Notice::refusal(message));
}

/// The agent's standing orders, handed to the model fresh on every turn.
fn instructions(app: &App) -> Option<String> {
    let session = app.session.as_ref()?;
    app.agents
        .iter()
        .find(|agent| agent.metadata.name == session.agent.as_str())
        .and_then(|agent| agent.spec.instructions.clone())
}

/// Why the console stopped.
#[derive(Debug, thiserror::Error)]
pub enum ConsoleExit {
    /// The terminal itself failed.
    #[error("the terminal could not be driven")]
    Terminal(#[source] io::Error),
    /// Setting up the session layer failed before the console could open.
    #[error(transparent)]
    Session(SessionError),
}
