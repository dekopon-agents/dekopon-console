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
