//! Command-line tests over the real binary.
//!
//! Every one of these runs without a broker, because the flags they pin are resolved before the
//! console connects to anything. They were untested for the whole time this lived inside `dekopon`,
//! which is how `--api-key-env` could have stopped requiring `--endpoint` without anyone noticing.

use std::{path::Path, process::Command};

use tempfile::TempDir;

/// A catalog with one agent, so catalog loading is never the reason a test fails.
const CATALOG: &str = r"apiVersion: dekopon.dev/v1alpha1
kind: Provider
metadata:
  name: echo
spec:
  description: Echo provider declaration
  type: echo
  credentialRef: echo-default
---
apiVersion: dekopon.dev/v1alpha1
kind: Capability
metadata:
  name: echo.echo
spec:
  description: Echo a message back
  provider: echo
  effect: read-only
  risk: Low
  idempotency: idempotent
  permissions:
    - operation: echo:invoke
---
apiVersion: dekopon.dev/v1alpha1
kind: Agent
metadata:
  name: reviewer
spec:
  description: A fixture agent
  enabled: true
  capabilities:
    - echo.echo
  providers:
    - echo
";

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dekopon-console"));
    // The subject is the one setting with no default, and it reads the environment. A developer
    // machine that exports it would otherwise change what these tests assert.
    command.env_remove("DEKOPON_CONSOLE_SUBJECT");
    command
}

fn catalog() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("create a catalog fixture");
    let path = directory.path().join("dekopon.yaml");
    std::fs::write(&path, CATALOG).expect("write the catalog fixture");
    (directory, path)
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn help_lists_every_flag_a_deployment_might_need() {
    let output = binary()
        .arg("--help")
        .output()
        .expect("the console binary starts");

    assert_eq!(output.status.code(), Some(0));
    let help = stdout(&output);
    for flag in [
        "--model",
        "--endpoint",
        "--api-key-env",
        "--auth-file",
        "--max-steps",
        "--max-capability-calls",
        "--subject",
        "--socket",
        "--server-uid",
    ] {
        assert!(help.contains(flag), "{flag} is undocumented: {help}");
    }
}

#[test]
fn an_api_key_variable_without_an_endpoint_is_a_usage_error() {
    // On its own it names a variable for a request nothing will send. Accepting it would leave an
    // operator convinced they had configured a bearer token for the ChatGPT subscription path.
    let output = binary()
        .args(["--api-key-env", "OPENAI_API_KEY"])
        .output()
        .expect("the console binary starts");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--endpoint"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_credential_file_and_an_endpoint_are_mutually_exclusive() {
    // They select different model backends. Silently preferring one would resolve a credential the
    // operator did not intend to spend.
    let output = binary()
        .args([
            "--auth-file",
            "/nonexistent/chatgpt-auth.console.json",
            "--endpoint",
            "http://127.0.0.1:8081/v1",
        ])
        .output()
        .expect("the console binary starts");

    assert_eq!(output.status.code(), Some(2));
    let message = stderr(&output);
    assert!(
        message.contains("--auth-file") && message.contains("--endpoint"),
        "the conflict has to name both: {message}"
    );
}

#[test]
fn the_session_bounds_are_numbers_and_are_checked_at_the_command_line() {
    for flag in ["--max-steps", "--max-capability-calls"] {
        let output = binary()
            .args([flag, "lots"])
            .output()
            .expect("the console binary starts");
        assert_eq!(
            output.status.code(),
            Some(2),
            "{flag} accepted a non-number"
        );
    }
}

#[test]
fn no_subject_refuses_before_it_touches_anything() {
    let output = binary().output().expect("the console binary starts");

    assert_eq!(output.status.code(), Some(1));
    let message = stderr(&output);
    assert!(message.contains("--subject"), "{message}");
    assert!(message.contains("DEKOPON_CONSOLE_SUBJECT"), "{message}");
    assert!(
        message.contains("allowDevelopmentSubjects"),
        "the refusal must name the broker-side opt-in too, or the next failure is a mystery: \
         {message}"
    );
}

#[test]
fn a_subject_no_service_could_issue_is_refused_at_the_command_line() {
    let output = binary()
        .args(["--subject", "sms.15550100000"])
        .output()
        .expect("the console binary starts");

    // Exit 2, not a broker refusal several steps later where the operator learns nothing about
    // which part was wrong.
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn every_flag_together_reaches_the_broker_connection_rather_than_a_usage_error() {
    let (_directory, path) = catalog();
    let socket = Path::new("/nonexistent/dekopon/broker.sock");
    let output = binary()
        .args(["--config".as_ref(), path.as_os_str()])
        .args(["--socket".as_ref(), socket.as_os_str()])
        .args([
            "--subject",
            "dev.console.xavier",
            "--model",
            "gpt-5.6-luna",
            "--endpoint",
            "http://127.0.0.1:8081/v1",
            "--api-key-env",
            "DEKOPON_TEST_MODEL_KEY",
            "--max-steps",
            "3",
            "--max-capability-calls",
            "5",
        ])
        .output()
        .expect("the console binary starts");

    let message = stderr(&output);
    assert_ne!(
        output.status.code(),
        Some(2),
        "every one of these is a supported flag: {message}"
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        message.contains("broker.sock") || message.contains("broker"),
        "the failure must be the missing broker, not something earlier: {message}"
    );
}

#[test]
fn the_chatgpt_path_is_the_default_and_takes_the_credential_file_it_is_given() {
    // No `--endpoint`, so the subscription path runs and resolves a credential *before* the screen
    // opens — the point of resolving it there is that a refusal reaches a plain terminal rather
    // than a full-screen frame. An explicit `--auth-file` is accepted as written, which is the
    // documented way out of the console's own `chatgpt-auth.console.json`, so this run gets past
    // credential resolution and stops at the broker instead.
    let (_directory, path) = catalog();
    let socket = Path::new("/nonexistent/dekopon/broker.sock");
    let output = binary()
        .args(["--config".as_ref(), path.as_os_str()])
        .args(["--socket".as_ref(), socket.as_os_str()])
        .args([
            "--subject",
            "dev.console.xavier",
            "--auth-file",
            "/nonexistent/dekopon/chatgpt-auth.console.json",
        ])
        .output()
        .expect("the console binary starts");

    let message = stderr(&output);
    assert_ne!(output.status.code(), Some(2), "{message}");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        message.contains("broker"),
        "an explicit credential file is taken as written, so the broker is what is missing: \
         {message}"
    );
}

#[test]
fn chat_skips_catalog_and_credential_resolution() {
    let output = binary()
        .env(
            "DEKOPON_CHATGPT_AUTH_FILE",
            "/nonexistent/chatgpt-auth.json",
        )
        .args([
            "--subject",
            "tel.15550100000",
            "--chat-socket",
            "/nonexistent/chat.sock",
            "--conversation",
            "test-chat",
        ])
        .output()
        .expect("binary starts");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("chat socket I/O failed"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("credential"));
}

/// These negative-startup children must finish even if model setup accidentally blocks.
fn bounded_output(command: &mut Command) -> std::process::Output {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("console startup exceeded 5 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn shell_skips_the_shared_credential_guard_before_connecting() {
    let (directory, path) = catalog();
    let poison = directory.path().join("poison-auth.json");
    std::fs::write(&poison, "not a model credential").unwrap();
    for shell in [true, false] {
        let mut command = binary();
        command
            .env("DEKOPON_CHATGPT_AUTH_FILE", &poison)
            .args(["--config".as_ref(), path.as_os_str()])
            .args([
                "--subject",
                "dev.console.xavier",
                "--socket",
                "/nonexistent/broker.sock",
            ]);
        if shell {
            command.arg("--shell");
        }
        let output = bounded_output(&mut command);
        assert_eq!(output.status.code(), Some(1));
        let message = stderr(&output);
        if shell {
            assert!(message.contains("no broker found"), "{message}");
            assert!(!message.contains("poison-auth"), "{message}");
            assert!(!message.contains("credential"), "{message}");
        } else {
            assert!(message.contains("refusing to use"), "{message}");
            assert!(message.contains("poison-auth"), "{message}");
        }
    }
    assert_eq!(
        std::fs::read_to_string(poison).unwrap(),
        "not a model credential"
    );
}

#[test]
fn shell_rejects_model_settings_and_chat_instead_of_resolving_them() {
    for (flag, value) in [
        ("--auth-file", "/nonexistent/auth.json"),
        ("--endpoint", "not-a-url"),
        ("--api-key-env", "DEKOPON_TEST_DO_NOT_READ"),
        ("--model", "unused-model"),
        ("--chat-socket", "/nonexistent/chat.sock"),
    ] {
        let output = bounded_output(binary().args(["--shell", flag, value]));
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("--shell"));
        assert!(stderr(&output).contains(flag));
    }
    let output = bounded_output(binary().arg("--help"));
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("--shell"));
}
