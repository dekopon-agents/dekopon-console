use std::{io::IsTerminal as _, time::Duration};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dekopon_agent::prompt::{ConversationTurn, History};
use dekopon_broker_protocol::{BrokerClient, FrameLimits};
use dekopon_core::SecretUseProposal;
use dekopon_protocol::{Agent, AgentKind, AgentSpec, ApiVersion, ObjectMeta};
use dekopon_shell::{
    CapabilityCallResult, CapabilityDescription, CapabilityInvoker, CommandRun, Interpreter, Limits,
};
use serde_json::Value;
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

use super::{Action, RunningTurn, TerminalGuard, dispatch, drive_event_loop, on_key};
use crate::{
    app::{App, Mode, Pane, Payload},
    profile::OperatorProfile,
    record::{CallOutcome, CapabilityCall, RecordingInvoker, Sequence, SessionEvent},
    session::{ConsoleOptions, StopFlag},
};

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

fn console() -> App {
    let agent = Agent {
        api_version: ApiVersion::V1Alpha1,
        kind: AgentKind::Agent,
        metadata: ObjectMeta::named("ville-github"),
        spec: AgentSpec {
            description: "a fixture".to_owned(),
            enabled: true,
            instructions: None,
            skills: Vec::new(),
            model_class: None,
            instructions_file: None,
        },
        status: None,
    };
    App::new(
        vec![agent],
        "slack.t0123abc.u9xyz".to_owned(),
        "/run/dekopon/broker.sock".to_owned(),
        "/config/dekopon/chatgpt-auth.console.json".to_owned(),
    )
}

#[test]
fn terminal_guard_restores_after_normal_exit_and_panic_when_run_under_a_pty() {
    if !std::io::stdout().is_terminal() {
        return;
    }
    {
        let (_guard, _terminal) = TerminalGuard::enter().expect("enter raw alternate screen");
        assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
    }
    assert!(
        !crossterm::terminal::is_raw_mode_enabled().unwrap(),
        "normal exit restores raw mode"
    );
    let early_error = || -> std::io::Result<()> {
        let (_guard, _terminal) = TerminalGuard::enter()?;
        Err(std::io::Error::other("injected draw failure"))
    };
    assert!(early_error().is_err());
    assert!(
        !crossterm::terminal::is_raw_mode_enabled().unwrap(),
        "error return restores raw mode"
    );
    let caught = std::panic::catch_unwind(|| {
        let (_guard, _terminal) = TerminalGuard::enter().expect("enter raw alternate screen");
        panic!("injected terminal panic");
    });
    assert!(caught.is_err());
    assert!(
        !crossterm::terminal::is_raw_mode_enabled().unwrap(),
        "panic restores raw mode"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancelled_running_event_loop_restores_pty_after_join() {
    use std::{
        io::{Read as _, Write as _},
        process::{Command, Stdio},
        sync::atomic::AtomicBool,
        time::Instant,
    };
    if std::env::var_os("DEKOPON_TEST_PTY_CHILD").is_some() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (guard, mut terminal) = TerminalGuard::enter().unwrap();
            let mut app = console();
            app.busy = true;
            app.transcript.open("in flight".into());
            let mut options =
                ConsoleOptions::new("slack.t0123abc.u9xyz".parse().unwrap(), "unused".into());
            let mut history = History::new(options.history_limits);
            let stop = StopFlag::default();
            let mut keys = crossterm::event::EventStream::new();
            let (sender, receiver) = crate::session::session_channel();
            let finished = Arc::new(AtomicBool::new(false));
            let done = Arc::clone(&finished);
            let probe = stop.clone();
            let handle = tokio::spawn(async move {
                while !probe.is_requested() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                done.store(true, Ordering::SeqCst);
                eprintln!("CANCEL_HANDLED");
                drop(sender);
                Ok(History::default())
            });
            let mut running = Some(RunningTurn {
                events: receiver,
                handle,
                shell: false,
            });
            let dir = tempfile::tempdir().unwrap();
            let client = BrokerClient::new(
                dir.path().join("absent.sock"),
                rustix::process::geteuid().as_raw(),
                FrameLimits::default(),
            )
            .unwrap();
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
            .unwrap();
            assert!(
                finished.load(Ordering::SeqCst),
                "cancelled work was not joined"
            );
            assert!(running.is_none());
            assert!(app.transcript.turns()[0].stop_requested);
            drop(guard);
            assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
            eprintln!("PTY_RESTORED");
        });
        return;
    }
    let capture = tempfile::tempdir().unwrap();
    let transcript = capture.path().join("pty.log");
    let mut command = Command::new("script");
    #[cfg(target_os = "macos")]
    command
        .args([
            "-q",
            transcript.to_str().unwrap(),
            "sh",
            "-c",
            "stty cols 80 rows 24; exec \"$@\"",
            "sh",
        ])
        .arg(std::env::current_exe().unwrap())
        .args([
            "run::tests::cancelled_running_event_loop_restores_pty_after_join",
            "--exact",
            "--nocapture",
        ]);
    #[cfg(target_os = "linux")]
    {
        let executable = std::env::current_exe()
            .unwrap()
            .display()
            .to_string()
            .replace('\'', "'\\''");
        let shell = format!(
            "stty cols 80 rows 24; exec '{executable}' run::tests::cancelled_running_event_loop_restores_pty_after_join --exact --nocapture"
        );
        command.args(["-q", "-c", &shell, transcript.to_str().unwrap()]);
    }
    let mut child = command
        .env("DEKOPON_TEST_PTY_CHILD", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&output);
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(count) = stdout.read(&mut chunk) {
            if count == 0 {
                break;
            }
            received.lock().unwrap().extend_from_slice(&chunk[..count]);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    fn wait_for(
        child: &mut std::process::Child,
        output: &Arc<Mutex<Vec<u8>>>,
        deadline: Instant,
        needle: &str,
    ) {
        loop {
            let text = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
            if text.contains(needle) {
                return;
            }
            if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("PTY fixture did not emit {needle}: {text}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    wait_for(&mut child, &output, deadline, "LIVE");
    child.stdin.as_mut().unwrap().write_all(b"\x1b").unwrap();
    wait_for(&mut child, &output, deadline, "CANCEL_HANDLED");
    child.stdin.as_mut().unwrap().write_all(b"q").unwrap();
    wait_for(&mut child, &output, deadline, "PTY_RESTORED");
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(status.success(), "PTY child failed: {status}");
    let text = output.lock().unwrap();
    assert!(text.windows(8).any(|part| part == b"\x1b[?1049h"));
    assert!(text.windows(8).any(|part| part == b"\x1b[?1049l"));
}

#[test]
fn scoped_claim_requires_an_explicit_confirmation_before_enter() {
    let mut app = console();
    app.mode = Mode::ScopeWarning;
    let stop = StopFlag::default();
    assert_eq!(on_key(&mut app, press(KeyCode::Char('q')), &stop), None);
    assert!(!app.scope_warning_confirmed);
    assert!(!app.should_quit);
    app.mode = Mode::ScopeWarning;
    assert_eq!(
        on_key(&mut app, press(KeyCode::Enter), &stop),
        Some(Action::Enter)
    );
    assert!(app.scope_warning_confirmed);
}

#[test]
fn control_c_quits_from_any_mode() {
    for mode in [Mode::Browsing, Mode::Composing, Mode::Help] {
        let mut app = console();
        app.mode = mode.clone();
        let key = KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            ..press(KeyCode::Char('c'))
        };
        assert!(on_key(&mut app, key, &StopFlag::default()).is_none());
        assert!(app.should_quit, "ctrl-c must work in {mode:?}");
    }
}

#[test]
fn any_key_dismisses_the_overlay_without_acting() {
    let mut app = console();
    app.mode = Mode::Help;
    assert!(on_key(&mut app, press(KeyCode::Char('q')), &StopFlag::default()).is_none());
    assert_eq!(app.mode, Mode::Browsing);
    assert!(
        !app.should_quit,
        "dismissing the overlay must not also quit"
    );
}

#[test]
fn tab_cycles_panes_in_both_directions() {
    let mut app = console();
    let stop = StopFlag::default();
    on_key(&mut app, press(KeyCode::Tab), &stop);
    assert_eq!(app.pane, Pane::Detail);
    on_key(&mut app, press(KeyCode::BackTab), &stop);
    assert_eq!(app.pane, Pane::Agents);
}

#[test]
fn enter_on_the_agent_list_asks_to_hop() {
    let mut app = console();
    assert_eq!(
        on_key(&mut app, press(KeyCode::Enter), &StopFlag::default()),
        Some(Action::Enter)
    );
}

#[test]
fn composing_collects_text_and_enter_submits_a_shell_line() {
    let mut app = console();
    let stop = StopFlag::default();
    app.pane = Pane::Shell;
    on_key(&mut app, press(KeyCode::Char('i')), &stop);
    assert_eq!(app.mode, Mode::Composing);

    for character in "cap --list".chars() {
        on_key(&mut app, press(KeyCode::Char(character)), &stop);
    }
    assert_eq!(app.composer, "cap --list");

    assert_eq!(
        on_key(&mut app, press(KeyCode::Enter), &stop),
        Some(Action::Shell("cap --list".to_owned()))
    );
    assert!(app.composer.is_empty());
    assert_eq!(app.mode, Mode::Browsing);
}

#[test]
fn escape_while_composing_discards_rather_than_stopping_a_turn() {
    let mut app = console();
    let stop = StopFlag::default();
    app.pane = Pane::Turns;
    app.mode = Mode::Composing;
    app.composer = "half a thought".to_owned();
    app.busy = true;

    on_key(&mut app, press(KeyCode::Esc), &stop);
    assert_eq!(app.mode, Mode::Browsing);
    assert!(app.composer.is_empty());
    assert!(
        !stop.is_requested(),
        "leaving the composer must not stop the running turn"
    );
}

#[test]
fn escape_while_browsing_stops_a_running_turn() {
    let mut app = console();
    let stop = StopFlag::default();
    app.busy = true;
    app.transcript.open("ask".to_owned());

    on_key(&mut app, press(KeyCode::Esc), &stop);
    assert!(stop.is_requested());
    assert!(app.transcript.turns()[0].stop_requested);
}

/// A controlled broker-command seam: first invocation remains in flight until released by the
/// harness. A cancelled next proposal is refused before it reaches the simulated broker.
struct BlockingCommands {
    started: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    stop: StopFlag,
    accepted: Arc<AtomicUsize>,
}

impl CapabilityInvoker for BlockingCommands {
    fn granted(&self) -> Vec<String> {
        vec!["probe.first".into(), "probe.second".into()]
    }
    fn is_granted(&self, capability: &str) -> bool {
        matches!(capability, "probe.first" | "probe.second")
    }
    fn command_words(&self) -> Vec<String> {
        vec!["probe".into()]
    }
    fn has_command_word(&self, word: &str) -> bool {
        word == "probe"
    }
    fn describe(&self, _: &str) -> Option<CapabilityDescription> {
        None
    }
    fn run_command(&self, word: &str, argv: &[String], _: Option<&str>) -> Option<CommandRun> {
        if word != "probe" {
            return None;
        }
        Some(CommandRun::Proposed {
            capability: format!("probe.{}", argv.first()?),
            input: json!({}),
            secret_use: None,
        })
    }
    fn invoke(
        &self,
        capability: &str,
        _: Value,
        _: Option<SecretUseProposal>,
    ) -> CapabilityCallResult {
        if self.stop.is_requested() {
            return CapabilityCallResult::Denied {
                reason: "session-cancelled".into(),
            };
        }
        self.accepted.fetch_add(1, Ordering::SeqCst);
        if capability == "probe.first" {
            self.started.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
        CapabilityCallResult::Succeeded(json!({"effect": capability}))
    }
}

#[test]
fn esc_during_an_accepted_command_prevents_the_next_broker_effect() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (events_tx, mut events_rx) = crate::session::session_channel();
    let accepted = Arc::new(AtomicUsize::new(0));
    let stop = StopFlag::default();
    let commands = BlockingCommands {
        started: started_tx,
        release: Mutex::new(release_rx),
        stop: stop.clone(),
        accepted: Arc::clone(&accepted),
    };
    let task = std::thread::spawn(move || {
        let invoker = RecordingInvoker::new(commands, events_tx, Sequence::default());
        Interpreter::new(Limits::default()).run("probe first; probe second", &invoker)
    });
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first command reached broker seam");
    let mut app = console();
    app.busy = true;
    app.transcript.open("run two commands".into());
    on_key(&mut app, press(KeyCode::Esc), &stop);
    assert!(stop.is_requested());
    release_tx.send(()).unwrap();
    let result = task.join().unwrap();
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "second effect must not reach broker after stop: {result:?}"
    );
    app.on_session_event(SessionEvent::ScriptStarted {
        sequence: 0,
        script: "probe first; probe second".into(),
    });
    while let Ok(event) = events_rx.try_recv() {
        app.on_session_event(event);
    }
    assert!(app.transcript.turns()[0].stop_requested);
    let calls = app.calls();
    assert_eq!(
        calls.len(),
        2,
        "the refused second proposal remains observable"
    );
    assert!(matches!(
        app.call_at(calls[0]).unwrap().outcome,
        CallOutcome::Succeeded(_)
    ));
    assert!(matches!(
        app.call_at(calls[1]).unwrap().outcome,
        CallOutcome::Denied(_)
    ));
}

#[test]
fn cancellation_does_not_erase_an_effect_the_broker_already_accepted() {
    let mut app = console();
    app.busy = true;
    app.transcript.open("perform action".into());
    let stop = StopFlag::default();
    let (handle, signal) = dekopon_process::CancelSignal::pair();
    stop.bind_broker(handle);
    on_key(&mut app, press(KeyCode::Esc), &stop);
    assert!(signal.is_cancelled(), "Esc reaches the broker leg");
    app.on_session_event(SessionEvent::ScriptStarted {
        sequence: 0,
        script: "probe action".into(),
    });
    app.on_session_event(SessionEvent::Capability(Box::new(CapabilityCall {
        sequence: 1,
        capability: "probe.action".into(),
        input: json!({}),
        outcome: CallOutcome::Succeeded(json!({"effect": "complete"})),
        elapsed: Duration::from_millis(5),
    })));
    assert!(app.transcript.turns()[0].stop_requested);
    assert!(
        matches!(
            app.transcript.turns()[0].scripts[0].calls[0].outcome,
            CallOutcome::Succeeded(_)
        ),
        "accepted effects are still visible after a stop request"
    );
}

#[tokio::test]
async fn switching_profiles_clears_the_model_replay_and_visible_history() {
    let first = "slack.t0123abc.u9xyz".parse().unwrap();
    let second = "slack.t0123abc.u8xyz".parse().unwrap();
    let mut options = ConsoleOptions::new(first, "test-model".into());
    options.profiles.push(OperatorProfile {
        name: "second".into(),
        agent: "other".parse().unwrap(),
        subject: second,
        scope: None,
        model: None,
        max_steps: None,
        max_capability_calls: None,
    });
    let mut history = History::new(options.history_limits);
    history.record(ConversationTurn::completed(
        "first profile secret",
        "answer",
    ));
    let mut app = console();
    let mut other = app.agents[0].clone();
    other.metadata.name = "other".into();
    app.agents.push(other);
    app.selected_agent = 1;
    app.profile = Some("first".into());
    app.transcript.open("first profile secret".into());
    app.shell_history.push(crate::app::ShellEntry {
        input: "first profile shell".into(),
        output: "secret".into(),
        exit_code: 0,
    });
    let directory = tempfile::tempdir().unwrap();
    let client = BrokerClient::new(
        directory.path().join("absent.sock"),
        rustix::process::geteuid().as_raw(),
        FrameLimits::default(),
    )
    .unwrap();
    let mut running = None;
    dispatch(
        &mut app,
        Action::Enter,
        &client,
        &mut options,
        &mut running,
        &mut history,
        &StopFlag::default(),
    )
    .await;
    assert_eq!(app.profile.as_deref(), Some("second"));
    assert_eq!(app.subject, "slack.t0123abc.u8xyz");
    assert_eq!(app.model_source, crate::session::ModelSource::Default);
    assert_eq!(app.model, crate::session::DEFAULT_MODEL);
    assert!(
        history.is_empty(),
        "new profile must not replay the old subject's turns"
    );
    assert!(app.transcript.turns().is_empty());
    assert!(app.shell_history.is_empty());
    assert!(
        app.session.is_none(),
        "absent broker must refuse, not open a stale leg"
    );
}

#[test]
fn escape_stops_a_manual_shell_command_without_an_open_model_turn() {
    let mut app = console();
    let stop = StopFlag::default();
    app.busy = true;
    on_key(&mut app, press(KeyCode::Esc), &stop);
    assert!(stop.is_requested());
    assert!(
        app.notice
            .as_ref()
            .is_some_and(|notice| notice.text.contains("not") || notice.text.contains("complete"))
    );
}

#[test]
fn escape_with_nothing_running_requests_nothing() {
    let mut app = console();
    let stop = StopFlag::default();
    on_key(&mut app, press(KeyCode::Esc), &stop);
    assert!(!stop.is_requested());
}

#[test]
fn backspace_removes_one_character() {
    let mut app = console();
    let stop = StopFlag::default();
    app.mode = Mode::Composing;
    app.composer = "abc".to_owned();
    on_key(&mut app, press(KeyCode::Backspace), &stop);
    assert_eq!(app.composer, "ab");
}

#[test]
fn composing_is_only_offered_where_there_is_something_to_type_into() {
    let mut app = console();
    let stop = StopFlag::default();
    app.pane = Pane::Agents;
    on_key(&mut app, press(KeyCode::Char('i')), &stop);
    assert_eq!(
        app.mode,
        Mode::Browsing,
        "the agent list has no composer, so `i` must not open one"
    );
}

/// A transcript holding two capability calls, the first of which carries a secret in each half.
fn with_two_calls() -> App {
    let mut app = console();
    app.transcript.open("look".to_owned());
    app.on_session_event(SessionEvent::ScriptStarted {
        sequence: 0,
        script: "gh issue list".to_owned(),
    });
    app.on_session_event(SessionEvent::Capability(Box::new(CapabilityCall {
        sequence: 1,
        capability: "gh.issue.list".to_owned(),
        input: json!({"headers": {"authorization": "Bearer ghp_0123456789abcdefghij"}}),
        outcome: CallOutcome::Succeeded(json!({"token": "ghs_zyxwvutsrqponmlkjihg"})),
        elapsed: Duration::from_millis(5),
    })));
    app.on_session_event(SessionEvent::Capability(Box::new(CapabilityCall {
        sequence: 2,
        capability: "gh.issue.read".to_owned(),
        input: json!({"number": 7}),
        outcome: CallOutcome::Succeeded(json!({"title": "a bug"})),
        elapsed: Duration::from_millis(4),
    })));
    app.pane = Pane::Turns;
    app
}

#[test]
fn o_expands_the_call_under_the_cursor_and_collapses_it_again() {
    let mut app = with_two_calls();
    let stop = StopFlag::default();

    on_key(&mut app, press(KeyCode::Char('o')), &stop);
    assert_eq!(
        app.expanded_call,
        Some((0, 0, 0)),
        "the key the help overlay advertises has to reach the state it advertises"
    );

    on_key(&mut app, press(KeyCode::Char('o')), &stop);
    assert_eq!(app.expanded_call, None, "pressing it again collapses");

    // The cursor is what makes it act on *one* call, so moving it moves what `o` expands.
    on_key(&mut app, press(KeyCode::Char('j')), &stop);
    on_key(&mut app, press(KeyCode::Char('o')), &stop);
    assert_eq!(app.expanded_call, Some((0, 0, 1)));
}

#[test]
fn o_and_r_belong_to_the_turns_pane() {
    let mut app = with_two_calls();
    let stop = StopFlag::default();
    app.pane = Pane::Agents;

    on_key(&mut app, press(KeyCode::Char('o')), &stop);
    on_key(&mut app, press(KeyCode::Char('r')), &stop);
    assert_eq!(app.expanded_call, None);
    assert!(app.revealed.is_empty());
    // ...and `j` still moves the agent list there, rather than a cursor that pane cannot show.
    assert_eq!(app.selected_call, 0);
}

#[test]
fn r_reveals_one_field_per_keystroke_and_never_becomes_a_mode() {
    let mut app = with_two_calls();
    let stop = StopFlag::default();

    on_key(&mut app, press(KeyCode::Char('r')), &stop);
    assert_eq!(app.revealed.len(), 1, "one keystroke, one field");
    assert!(app.is_revealed((0, 0, 0), Payload::Input, "headers.authorization"));
    assert!(
        app.notice
            .as_ref()
            .is_some_and(|notice| notice.text.contains("scrollback")),
        "the scrollback warning is the whole reason revealing is deliberate"
    );

    on_key(&mut app, press(KeyCode::Char('r')), &stop);
    assert_eq!(app.revealed.len(), 2, "the next press takes the next field");
    assert!(app.is_revealed((0, 0, 0), Payload::Output, "token"));

    // Nothing left in this call, and saying so beats a keystroke that appears to do nothing.
    on_key(&mut app, press(KeyCode::Char('r')), &stop);
    assert_eq!(app.revealed.len(), 2);
    assert!(
        app.notice
            .as_ref()
            .is_some_and(|notice| notice.text.contains("nothing left")),
        "got: {:?}",
        app.notice
    );

    // The second call carries no secret, and revealing the first one did not uncover it.
    on_key(&mut app, press(KeyCode::Char('j')), &stop);
    on_key(&mut app, press(KeyCode::Char('r')), &stop);
    assert_eq!(app.revealed.len(), 2);
}

#[test]
fn the_call_cursor_saturates_rather_than_wrapping() {
    let mut app = with_two_calls();
    let stop = StopFlag::default();

    on_key(&mut app, press(KeyCode::Char('k')), &stop);
    assert_eq!(app.selected_call, 0, "already at the top");
    for _ in 0..5 {
        on_key(&mut app, press(KeyCode::Char('j')), &stop);
    }
    assert_eq!(app.selected_call, 1, "two calls, so the last index is one");
}

#[test]
fn o_and_r_say_so_when_there_is_no_call_to_act_on() {
    let mut app = console();
    let stop = StopFlag::default();
    app.pane = Pane::Turns;

    on_key(&mut app, press(KeyCode::Char('o')), &stop);
    assert!(
        app.notice.as_ref().is_some_and(|notice| notice.is_refusal),
        "a key that cannot act has to say why, not look broken"
    );

    app.notice = None;
    on_key(&mut app, press(KeyCode::Char('r')), &stop);
    assert!(app.notice.as_ref().is_some_and(|notice| notice.is_refusal));
}

#[tokio::test]
async fn profile_switches_preserve_cli_override_and_update_model_provenance() {
    use crate::session::{DEFAULT_MODEL, ModelSource};
    let mut options = ConsoleOptions::new(
        "slack.t0123abc.u9xyz".parse().unwrap(),
        DEFAULT_MODEL.into(),
    );
    options.profiles.push(OperatorProfile {
        name: "authored".into(),
        agent: "reviewer".parse().unwrap(),
        subject: options.subject.clone(),
        scope: None,
        model: Some("profile-model".into()),
        max_steps: None,
        max_capability_calls: None,
    });
    let mut app = console();
    options.profiles[0].agent = app.agents[0].metadata.name.parse().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let client = BrokerClient::new(
        directory.path().join("absent.sock"),
        rustix::process::geteuid().as_raw(),
        FrameLimits::default(),
    )
    .unwrap();
    let mut history = History::new(options.history_limits);
    let mut running = None;
    for (cli, profile, expected, source) in [
        (
            Some("cli-model"),
            Some("profile-model"),
            "cli-model",
            ModelSource::CliOverride,
        ),
        (
            Some("cli-model"),
            Some("other-profile-model"),
            "cli-model",
            ModelSource::CliOverride,
        ),
        (
            None,
            Some("other-profile-model"),
            "other-profile-model",
            ModelSource::Profile,
        ),
        (None, None, DEFAULT_MODEL, ModelSource::Default),
    ] {
        options.model_override = cli.map(str::to_owned);
        options.profiles[0].model = profile.map(str::to_owned);
        dispatch(
            &mut app,
            Action::Enter,
            &client,
            &mut options,
            &mut running,
            &mut history,
            &StopFlag::default(),
        )
        .await;
        assert_eq!((app.model.as_str(), app.model_source), (expected, source));
        assert_eq!(
            (options.model.as_str(), options.model_source),
            (expected, source)
        );
    }
}
