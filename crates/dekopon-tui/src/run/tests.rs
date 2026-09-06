use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dekopon_protocol::{Agent, AgentKind, AgentSpec, ApiVersion, ObjectMeta};
use serde_json::json;

use super::{Action, on_key};
use crate::{
    app::{App, Mode, Pane, Payload},
    record::{CallOutcome, CapabilityCall, SessionEvent},
    session::StopFlag,
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
            capabilities: Vec::new(),
            providers: Vec::new(),
            model_class: None,
            policy_profile: None,
        },
        status: None,
    };
    App::new(
        vec![agent],
        "dev.console.xavier".to_owned(),
        "/run/dekopon/broker.sock".to_owned(),
        "/config/dekopon/chatgpt-auth.console.json".to_owned(),
    )
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

#[test]
fn shell_only_cannot_compose_or_submit_turns_after_pane_switching() {
    let mut app = console();
    app.restrict_to_shell();
    let stop = StopFlag::default();
    for _ in 0..8 {
        on_key(&mut app, press(KeyCode::Tab), &stop);
        if app.pane == Pane::Turns {
            on_key(&mut app, press(KeyCode::Char('i')), &stop);
            assert_eq!(app.mode, Mode::Browsing);
            assert!(app.notice.as_ref().unwrap().text.contains("turns disabled"));
        }
    }
    // Even a caller directly placing the app in composing mode cannot submit a turn.
    app.pane = Pane::Turns;
    app.mode = Mode::Composing;
    app.composer = "do not send".to_owned();
    assert!(on_key(&mut app, press(KeyCode::Enter), &stop).is_none());
    assert!(!app.busy);
    assert!(app.transcript.turns().is_empty());
    assert_eq!(app.composer, "do not send");
    app.pane = Pane::Shell;
    assert_eq!(
        on_key(&mut app, press(KeyCode::Enter), &stop),
        Some(Action::Shell("do not send".to_owned()))
    );
}

#[test]
fn shell_model_setup_ignores_poisoned_credential_discovery_but_turn_setup_refuses() {
    use crate::session::{ConsoleOptions, ModelChoice, SessionError};
    const CHILD: &str = "DEKOPON_TEST_SHELL_SETUP_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut app = console();
        let mut options = ConsoleOptions::new(
            "dev.console.xavier".parse().unwrap(),
            "fixture-model".to_owned(),
        );
        options.model_choice = ModelChoice::ShellOnly;
        assert!(super::prepare_model(&mut app, &options).unwrap().is_none());
        assert!(!app.turns_enabled());
        // Direct model construction cannot be used to bypass the startup selection either.
        assert!(matches!(
            crate::session::build_model(&options),
            Err(SessionError::TurnsDisabled)
        ));
        options.model_choice = ModelChoice::ChatGptSubscription { auth_file: None };
        assert!(matches!(
            super::prepare_model(&mut console(), &options),
            Err(SessionError::SharedCredential { .. })
        ));
        options.model_choice = ModelChoice::ChatGptSubscription {
            auth_file: Some(std::path::PathBuf::from(std::env::var_os(CHILD).unwrap())),
        };
        assert!(matches!(
            super::prepare_model(&mut console(), &options),
            Err(SessionError::ChatGpt(_))
        ));
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let poisoned = directory.path().join("not-a-credential.json");
    std::fs::write(&poisoned, "invalid credential fixture").unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "run::tests::shell_model_setup_ignores_poisoned_credential_discovery_but_turn_setup_refuses", "--nocapture"])
        .env(CHILD, &poisoned)
        .env("DEKOPON_CHATGPT_AUTH_FILE", &poisoned)
        .env("HOME", directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("APPDATA", directory.path())
        .spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("model setup child exceeded 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(poisoned).unwrap(),
        "invalid credential fixture"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_only_uses_the_existing_attested_leg_and_preserves_policy_and_pipeline_results() {
    use crate::session::{ConsoleOptions, ModelChoice};
    use dekopon_agent::prompt::History;
    use dekopon_broker_protocol::{
        BrokerClient, BrokerRequest, ERROR_UNAUTHENTICATED, FrameLimits, RequestEnvelope,
        ResponseEnvelope, read_frame, write_frame,
    };
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("broker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let capability = serde_json::from_value(json!({
        "provider": "echo",
        "capability": {
            "id": "echo.echo", "description": "fixture", "effect": "read-only",
            "risk": "Low", "idempotency": "idempotent", "inputSchema": {"type": "object"}
        }
    }))
    .unwrap();
    let server = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), async move {
            let mut requests = Vec::new();
            for response in [
                ResponseEnvelope::capabilities(vec![capability], vec![]),
                ResponseEnvelope::error(ERROR_UNAUTHENTICATED, "policy denied fixture"),
                ResponseEnvelope::capabilities(vec![], vec![]),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                requests.push(
                    read_frame::<_, RequestEnvelope>(&mut stream, FrameLimits::default())
                        .await
                        .unwrap(),
                );
                write_frame(&mut stream, &response, FrameLimits::default())
                    .await
                    .unwrap();
            }
            requests
        })
        .await
        .expect("bounded broker fixture")
    });
    let client = BrokerClient::new(
        &socket,
        rustix::process::geteuid().as_raw(),
        FrameLimits::default(),
    )
    .unwrap();
    let mut app = console();
    let mut options =
        ConsoleOptions::new("dev.console.xavier".parse().unwrap(), "unused".to_owned());
    options.model_choice = ModelChoice::ShellOnly;
    let model = super::prepare_model(&mut app, &options).unwrap();
    let mut running = None;
    let mut history = History::new(options.history_limits);
    let stop = StopFlag::default();
    tokio::time::timeout(Duration::from_secs(5), async {
        super::dispatch(
            &mut app,
            Action::Enter,
            &client,
            &options,
            &model,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
        assert!(app.session.is_some());
        assert_eq!(app.pane, Pane::Shell);
        assert!(!app.turns_enabled());
        super::dispatch(
            &mut app,
            Action::Shell("echo pipeline | cat".to_owned()),
            &client,
            &options,
            &model,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
        assert_eq!(app.shell_history[0].output, "pipeline");
        assert_eq!(app.shell_history[0].exit_code, 0);
        super::dispatch(
            &mut app,
            Action::Shell("cap echo.echo '{}'".to_owned()),
            &client,
            &options,
            &model,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
        assert_eq!(app.shell_history[1].exit_code, 126);
        assert!(
            app.shell_history[1]
                .output
                .contains("policy denied fixture")
        );
        // Neither a direct action nor another agent hop can start a model in this run.
        super::dispatch(
            &mut app,
            Action::Turn("must not run".to_owned()),
            &client,
            &options,
            &model,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
        assert!(running.is_none());
        assert!(history.is_empty());
        assert!(app.notice.as_ref().unwrap().text.contains("turns disabled"));
        super::dispatch(
            &mut app,
            Action::Enter,
            &client,
            &options,
            &model,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
        assert!(!app.turns_enabled());
        assert_eq!(app.pane, Pane::Shell);
        assert!(app.session.as_ref().unwrap().is_empty());
        assert!(app.notice.as_ref().unwrap().text.contains("policy grants"));
    })
    .await
    .expect("bounded shell dispatch");
    // A fresh normal run still constructs and uses its model, submits a turn, observes its
    // completion, and returns history. No usable ChatGPT credential is involved in this test.
    let endpoint = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    options.model_choice = ModelChoice::OpenAiCompatible {
        endpoint: format!("http://{}/v1", endpoint.local_addr().unwrap()),
        api_key_env: None,
    };
    options.model_timeout = Duration::from_secs(3);
    let model_server = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        tokio::time::timeout(Duration::from_secs(5), async move {
            let (mut stream, _) = endpoint.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                assert!(request.len() < 64 * 1024);
                request.push(stream.read_u8().await.unwrap());
                if request.ends_with(b"\r\n\r\n") { break; }
            }
            let headers = String::from_utf8(request).unwrap();
            assert!(headers.starts_with("POST /v1/chat/completions "));
            let length: usize = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
            }).unwrap();
            assert!(length < 64 * 1024);
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], "unused");
            assert!(body["messages"].as_array().unwrap().iter().any(|message| message["content"] == "normal turn"));
            let answer = r#"{"choices":[{"message":{"role":"assistant","content":"turn answer"}}]}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).as_bytes()).await.unwrap();
        }).await.expect("bounded model fixture");
    });
    let mut normal = console();
    normal.enter(app.session.take().unwrap());
    let model = super::prepare_model(&mut normal, &options).unwrap();
    assert!(model.is_some());
    assert!(normal.turns_enabled());
    assert_eq!(normal.pane, Pane::Detail);
    normal.composer = "normal turn".to_owned();
    let prompt = normal.submit_turn().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        super::dispatch(
            &mut normal,
            Action::Turn(prompt),
            &client,
            &options,
            &model,
            &mut running,
            &mut history,
            &stop,
        )
        .await;
        assert!(running.is_some());
        while let Some(event) = super::recv(&mut running).await {
            normal.on_session_event(event);
        }
        super::finish_turn(&mut normal, &mut running, &mut history, &stop).await;
    })
    .await
    .expect("bounded normal turn");
    model_server.await.unwrap();
    assert!(!normal.busy);
    assert_eq!(history.len(), 1);
    assert_eq!(normal.transcript.turns().len(), 1);
    assert_eq!(
        normal.transcript.turns()[0].status,
        crate::transcript::TurnStatus::Answered("turn answer".to_owned())
    );
    let requests = server.await.unwrap();
    for request in [&requests[0], &requests[2]] {
        let BrokerRequest::CapabilitiesFor { subject, agent } = &request.request else {
            panic!("hop must ask for a fresh attested surface");
        };
        assert_eq!(subject.to_string(), "dev.console.xavier");
        assert_eq!(agent.as_str(), "ville-github");
    }
    let BrokerRequest::InvokeFor {
        invocation,
        attestation,
    } = &requests[1].request
    else {
        panic!("shell must propose through the attested broker path");
    };
    assert_eq!(invocation.capability.as_str(), "echo.echo");
    assert_eq!(invocation.input, json!({}));
    assert_eq!(attestation.subject.to_string(), "dev.console.xavier");
    assert_eq!(attestation.agent.as_str(), "ville-github");
}
