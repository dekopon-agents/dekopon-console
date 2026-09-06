use super::*;
use std::os::unix::fs::PermissionsExt as _;
use tokio::net::UnixListener;

fn socket() -> (tempfile::TempDir, std::path::PathBuf, UnixListener) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    (dir, path, listener)
}

async fn client(path: &Path) -> ChatClient {
    ChatClient::connect(path, "tel.15550100000".parse().unwrap(), "test-chat".into())
        .await
        .unwrap()
}

#[tokio::test]
async fn real_socket_preserves_envelope_and_orders_multiple_turns() {
    let (_dir, path, listener) = socket();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        for text in ["hello\nworld", "again"] {
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap(),
                json!({
                    "subject": "tel.15550100000", "channel": "test-chat", "text": text
                })
            );
            stream
                .get_mut()
                .write_all(b"{\"reply\":\"answer\",\"images\":[]}\n")
                .await
                .unwrap();
        }
    });
    let mut client = client(&path).await;
    for text in ["hello\nworld", "again"] {
        assert_eq!(client.send(text).await.unwrap(), "answer");
    }
    server.await.unwrap();
}

#[tokio::test]
async fn rejects_unsafe_mode_symlink_and_non_socket() {
    let (dir, path, _listener) = socket();
    for mode in [0o660, 0o666, 0o400] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(matches!(
            ChatClient::connect(&path, "tel.15550100000".parse().unwrap(), "dev".into()).await,
            Err(ChatError::UnsafeSocket)
        ));
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let file = dir.path().join("file");
    std::fs::write(&file, "").unwrap();
    for path in [link, file] {
        assert!(matches!(
            ChatClient::connect(&path, "tel.15550100000".parse().unwrap(), "dev".into()).await,
            Err(ChatError::UnsafeSocket)
        ));
    }
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
    assert!(matches!(
        ChatClient::connect(&path, "tel.15550100000".parse().unwrap(), "dev".into()).await,
        Err(ChatError::UnsafeSocket)
    ));
}

#[tokio::test]
async fn real_socket_bounds_malformed_utf8_and_disconnect_poison_connection() {
    for (bytes, expected) in [
        (vec![b'x'; MAX_LINE_BYTES + 1], "large"),
        (b"not json\n".to_vec(), "malformed"),
        (b"{\"reply\":42}\n".to_vec(), "malformed"),
        (vec![0xff, b'\n'], "malformed"),
        (b"{\"reply\":\"partial\"}".to_vec(), "closed"),
        (vec![], "closed"),
    ] {
        let (_dir, path, listener) = socket();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut request = String::new();
            stream.read_line(&mut request).await.unwrap();
            let result = stream.get_mut().write_all(&bytes).await;
            if let Err(error) = result {
                assert_eq!(expected, "large");
                assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            }
        });
        let mut client = client(&path).await;
        let error = client.send("hello").await.unwrap_err();
        assert!(matches!(
            (expected, error),
            ("large", ChatError::ReplyTooLarge)
                | ("malformed", ChatError::MalformedReply)
                | ("closed", ChatError::Closed)
        ));
        assert!(matches!(client.send("next").await, Err(ChatError::Closed)));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn request_envelope_limit_is_checked_before_any_write_and_exact_reply_bound_passes() {
    let (_dir, path, listener) = socket();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&request).unwrap()["text"],
            "ok"
        );
        let reply = format!("{{\"reply\":\"{}\"}}\n", "x".repeat(MAX_LINE_BYTES - 13));
        assert_eq!(reply.len(), MAX_LINE_BYTES);
        stream.get_mut().write_all(reply.as_bytes()).await.unwrap();
    });
    let mut client = client(&path).await;
    for text in ["x".repeat(MAX_LINE_BYTES), "\n".repeat(MAX_LINE_BYTES / 2)] {
        assert!(matches!(
            client.send(&text).await,
            Err(ChatError::RequestTooLarge)
        ));
    }
    assert_eq!(client.send("ok").await.unwrap().len(), MAX_LINE_BYTES - 13);
    server.await.unwrap();
}

#[tokio::test]
async fn stalled_real_peer_times_out_without_reusing_late_response() {
    let (_dir, path, listener) = socket();
    let mut client = client(&path).await;
    let (_peer, _) = listener.accept().await.unwrap();
    assert!(matches!(
        client.exchange("hello", Duration::from_millis(20)).await,
        Err(ChatError::Timeout(_))
    ));
    assert!(matches!(client.send("again").await, Err(ChatError::Closed)));
}

#[test]
fn chat_keys_bound_composer_and_never_pipeline_and_render_safely_without_tty() {
    use crate::app::chat::ChatApp;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    let mut app = ChatApp::default();
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    app.composer = "hello".into();
    assert_eq!(app.on_key(key(KeyCode::Enter)), Some("hello".into()));
    app.composer = "second".into();
    assert_eq!(app.on_key(key(KeyCode::Enter)), None);
    assert_eq!(app.composer, "second");
    app.composer = "x".repeat(MAX_LINE_BYTES - 1);
    app.on_key(key(KeyCode::Char('x')));
    assert_eq!(app.composer.len(), MAX_LINE_BYTES - 1);
    let hostile = "\x1b[2J\u{202e} ghp_abcdefghijklmnopqrstuvwxyz";
    app.composer = hostile.into();
    app.reply = hostile.into();
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal
        .draw(|frame| app.draw(frame, hostile, hostile))
        .unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(rendered.contains("DEVELOPMENT ONLY"));
    assert!(rendered.contains("caller declares subject"));
    assert!(!rendered.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
    assert!(!rendered.contains('\x1b'));
    assert!(!rendered.contains('\u{202e}'));
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[tokio::test]
async fn bare_relative_socket_connects_in_private_working_directory() {
    const CHILD: &str = "DEKOPON_CHAT_RELATIVE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Change only the child's cwd, never the parallel test process's global cwd.
        let dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "chat::tests::bare_relative_socket_connects_in_private_working_directory",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    let path = Path::new("chat.sock");
    let listener = UnixListener::bind(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut client = client(path).await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["text"],
            "hello"
        );
        stream
            .get_mut()
            .write_all(b"{\"reply\":\"relative works\"}\n")
            .await
            .unwrap();
    });
    assert_eq!(client.send("hello").await.unwrap(), "relative works");
    server.await.unwrap();
}

#[test]
fn io_diagnostics_render_kind_and_errno_without_error_payload() {
    use crate::app::chat::ChatApp;
    use ratatui::{Terminal, backend::TestBackend};

    let hostile = "hostile-payload \x1b[2J\u{202e} ghp_abcdefghijklmnopqrstuvwxyz";
    let os_error = io::Error::from_raw_os_error(rustix::io::Errno::PIPE.raw_os_error());
    let errno = os_error.raw_os_error().unwrap();
    for (error, expected_errno) in [
        (os_error, format!("Some({errno})")),
        (
            io::Error::new(io::ErrorKind::BrokenPipe, hostile),
            "None".into(),
        ),
    ] {
        // The same Display boundary consumed by run_chat, then its actual renderer.
        let app = ChatApp {
            reply: ChatError::Io(error).to_string(),
            ..ChatApp::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
        terminal
            .draw(|frame| app.draw(frame, "subject", "channel"))
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("kind: BrokenPipe"));
        assert!(rendered.contains(&format!("errno: {expected_errno}")));
        assert!(!rendered.contains("hostile-payload"));
        assert!(!rendered.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(!rendered.contains('\x1b'));
        assert!(!rendered.contains('\u{202e}'));
    }
}
