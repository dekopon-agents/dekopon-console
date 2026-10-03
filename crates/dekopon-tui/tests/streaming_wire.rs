//! Always-run wire witness: console wrappers over the pinned broker client's real FD transport.
#![cfg(unix)]
use dekopon_broker_protocol::{
    BrokerClient, CommandRunOutcome, DescriptorStream, FrameLimits, ResponseEnvelope,
};
use dekopon_core::SecretUseProposal;
use dekopon_shell::{
    CapabilityCallResult, CapabilityInvoker as _, CommandProposal, CommandRun, Streams,
};
use dekopon_tui::{
    record::{CallOutcome, RecordingInvoker, Sequence, SessionEvent},
    session::{LegHandle, open_agent},
};
use serde_json::{Value, json};
use std::{
    io::{Read as _, Write as _},
    os::unix::{fs::PermissionsExt as _, net::UnixStream},
    sync::Arc,
};

#[tokio::test(flavor = "multi_thread")]
async fn console_forwards_attestation_secret_intent_and_descriptors_not_inline_stdio() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.path().join("broker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let secret = SecretUseProposal::HttpBearer {
        secret: "drn:com.xrl:secret:prod:api/token".parse().unwrap(),
    };
    let expected_secret = secret.clone();
    let server = tokio::spawn(async move {
        for operation in ["capabilities", "runCommand", "invoke"] {
            let (stream, _) = listener.accept().await.unwrap();
            let mut wire = DescriptorStream::new(stream);
            let (frame, mut descriptors) = wire
                .read_frame::<Value>(FrameLimits::default())
                .await
                .unwrap();
            let request = &frame["request"];
            assert_eq!(request["operation"], operation);
            assert_eq!(request["attestation"]["subject"], "slack.t0123abc.u9xyz");
            assert_eq!(request["attestation"]["agent"], "reviewer");
            let response = match operation {
                "capabilities" => {
                    assert!(descriptors.is_empty());
                    ResponseEnvelope::capabilities(vec![serde_json::from_value(json!({
                        "provider": "probe", "capability": { "id": "probe.upper", "description": "stream probe", "effect": "read-only", "risk": "Low", "inputSchema": {"type":"object"} }
                    })).unwrap()], vec!["probe".into()], [("probe".into(), "probe upper".into())].into())
                }
                "runCommand" => {
                    assert!(descriptors.is_empty());
                    assert_eq!(request["stdinPiped"], true);
                    assert!(
                        request.get("stdin").is_none(),
                        "only pipe presence belongs on the wire"
                    );
                    ResponseEnvelope::command_run(CommandRunOutcome::Proposed {
                        capability: "probe.upper".parse().unwrap(),
                        input: json!({"mode":"upper"}),
                        secret_use: Some(expected_secret.clone()),
                    })
                }
                "invoke" => {
                    assert_eq!(
                        request["invocation"]["secretUse"],
                        serde_json::to_value(&expected_secret).unwrap()
                    );
                    assert_eq!(request["invocation"]["input"], json!({"mode":"upper"}));
                    assert_eq!(request["streams"], json!({"stdin":true}));
                    assert_eq!(descriptors.len(), 2);
                    let streams = dekopon_broker_protocol::Streams::split_from(
                        &mut descriptors,
                        serde_json::from_value(request["streams"].clone()).unwrap(),
                    )
                    .unwrap();
                    assert!(descriptors.is_empty());
                    tokio::task::spawn_blocking(move || {
                        let mut text = String::new();
                        std::fs::File::from(streams.stdin.unwrap())
                            .read_to_string(&mut text)
                            .unwrap();
                        assert_eq!(text, "hello from stdin\n");
                        std::fs::File::from(streams.stdout)
                            .write_all(text.to_uppercase().as_bytes())
                            .unwrap();
                    })
                    .await
                    .unwrap();
                    ResponseEnvelope::invocation(serde_json::from_value(json!({
                        "invocation": request["invocation"]["id"],
                        "decision": {"decisionId":"wire-test", "authorizedBy":"broker", "policyRevision":"test"},
                        "outcome":"Failed", "exitStatus":141
                    })).unwrap(), vec![], vec![], vec![])
                }
                _ => unreachable!(),
            };
            wire.write_frame(&response, &[], FrameLimits::default())
                .await
                .unwrap();
        }
    });
    let client = BrokerClient::new(
        &socket,
        rustix::process::geteuid().as_raw(),
        FrameLimits::default(),
    )
    .unwrap();
    let leg = open_agent(
        client,
        "slack.t0123abc.u9xyz".parse().unwrap(),
        "reviewer".parse().unwrap(),
        None,
    )
    .await
    .unwrap();
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_blocking(move || {
        let invoker = RecordingInvoker::new(LegHandle(Arc::new(leg)), events, Sequence::default());
        assert_eq!(invoker.command_word_help()["probe"], "probe upper");
        let Some(CommandRun::Proposed { capability, input, secret_use, report }) = invoker.run_command("probe", &["upper".into()], true) else { panic!("expected proposal") };
        assert_eq!(secret_use, Some(secret));
        assert!(report.is_some());
        let (stdin, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"hello from stdin\n").unwrap();
        drop(writer);
        let (stdout, mut reader) = UnixStream::pair().unwrap();
        let result = invoker.invoke(CommandProposal { capability, input, secret_use, report }, Streams { stdin: Some(stdin.into()), stdout: stdout.into() });
        assert!(matches!(result, CapabilityCallResult::Exited { status, ref stderr } if status.get() == 141 && stderr.is_empty()));
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        assert_eq!(output, "HELLO FROM STDIN\n", "observation must not consume provider stdout");
    }).await.unwrap();
    server.await.unwrap();
    let SessionEvent::Capability(call) = receiver.try_recv().unwrap() else {
        panic!("expected recorded terminal result")
    };
    assert_eq!(
        call.outcome,
        CallOutcome::Exited {
            status: 141,
            stderr: String::new()
        }
    );
    assert_eq!(call.payloads().len(), 1);
}
