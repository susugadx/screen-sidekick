use super::*;
use async_trait::async_trait;
use futures_util::stream;
use screen_sidekick_codex_client::{
    CodexClientError, CodexEvent, CodexEventStream, CodexReadiness, CodexTurnClient,
    StartTurnOutcome, StartTurnRequest,
};
use screen_sidekick_session::SessionStore;
use screen_sidekick_sidekick_protocol::{
    method, JsonRpcRequest, JsonRpcResponse, TurnStatus, SIDEKICK_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{duplex, AsyncReadExt};

mod environment;
mod sidecar_relay;

#[tokio::test]
async fn native_frame_roundtrips_utf8_json() {
    let (mut writer, mut reader) = duplex(1024);
    let payload = json!({ "hello": "世界" }).to_string();

    write_native_message(&mut writer, &payload, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .expect("frame writes");

    let decoded = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES)
        .await
        .expect("frame reads")
        .expect("frame exists");

    assert_eq!(decoded, payload);
}

#[tokio::test]
async fn native_frame_writer_uses_exact_length_prefixed_bytes() {
    let (mut writer, mut reader) = duplex(1024);
    let payload = "{\"jsonrpc\":\"2.0\"}";

    write_native_message(&mut writer, payload, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .expect("frame writes");

    let mut bytes = vec![0_u8; 4 + payload.len()];
    reader
        .read_exact(&mut bytes)
        .await
        .expect("written frame reads");

    assert_eq!(&bytes[..4], &(payload.len() as u32).to_ne_bytes());
    assert_eq!(&bytes[4..], payload.as_bytes());
}

#[tokio::test]
async fn native_frame_rejects_invalid_utf8() {
    let (mut writer, mut reader) = duplex(1024);
    writer
        .write_all(&2_u32.to_ne_bytes())
        .await
        .expect("length writes");
    writer
        .write_all(&[0xff, 0xff])
        .await
        .expect("payload writes");

    let error = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES)
        .await
        .expect_err("invalid UTF-8 is rejected");

    assert_eq!(error, NativeFrameError::InvalidUtf8);
}

#[tokio::test]
async fn native_frame_rejects_oversized_length_without_reading_payload() {
    let (mut writer, mut reader) = duplex(1024);
    writer
        .write_all(&17_u32.to_ne_bytes())
        .await
        .expect("length writes");

    let error = read_native_message(&mut reader, 16)
        .await
        .expect_err("oversized frame is rejected");

    assert_eq!(
        error,
        NativeFrameError::PayloadTooLarge {
            size: 17,
            max_size: 16
        }
    );
}

#[tokio::test]
async fn native_frame_reports_partial_length() {
    let (mut writer, mut reader) = duplex(1024);
    writer
        .write_all(&[1, 2])
        .await
        .expect("partial length writes");
    drop(writer);

    let error = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES)
        .await
        .expect_err("partial length is rejected");

    assert_eq!(error, NativeFrameError::PartialLength);
}

#[tokio::test]
async fn in_process_host_initializes_without_pairing_token() {
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let state = test_state();
    let run = tokio::spawn(run_in_process_host(
        input_reader,
        output_writer,
        state,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
    ));

    let request = JsonRpcRequest::new(
        "init",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    );
    write_native_message(
        &mut input_writer,
        &serde_json::to_string(&request).expect("request serializes"),
        MAX_NATIVE_INCOMING_MESSAGE_BYTES,
    )
    .await
    .expect("request writes");
    drop(input_writer);

    let response = read_native_message(&mut output_reader, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .expect("response reads")
        .expect("response exists");
    let response: Value = serde_json::from_str(&response).expect("response is JSON");

    assert_eq!(response["id"], json!("init"));
    assert_eq!(response["result"]["auth_status"], json!("ready"));
    run.await
        .expect("host task joins")
        .expect("host exits after stdin closes");
}

#[tokio::test]
async fn in_process_host_streams_message_send_notifications() {
    let (mut input_writer, input_reader) = duplex(8192);
    let (output_writer, mut output_reader) = duplex(8192);
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let state = DaemonState::new(
        "unused-native-token",
        store.clone(),
        Arc::new(StreamingCodexClient),
    );
    let run = tokio::spawn(run_in_process_host(
        input_reader,
        output_writer,
        state,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
    ));

    send_request(
        &mut input_writer,
        "init",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    assert_eq!(
        read_value(&mut output_reader).await["result"]["auth_status"],
        json!("ready")
    );

    send_request(
        &mut input_writer,
        "session",
        method::SESSION_CREATE,
        json!({ "title": "Native chat" }),
    )
    .await;
    let session_response = read_value(&mut output_reader).await;
    let session_id = session_response["result"]["session"]["id"]
        .as_str()
        .expect("session id is present")
        .to_owned();

    send_request(
        &mut input_writer,
        "subscribe",
        method::SESSION_SUBSCRIBE,
        json!({ "session_id": session_id }),
    )
    .await;
    assert_eq!(
        read_response(&mut output_reader, "subscribe").await["id"],
        json!("subscribe")
    );

    send_request(
        &mut input_writer,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id,
            "text": "What changed?",
            "idempotency_key": "native-stream",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    let mut saw_send_response = false;
    let mut saw_delta = false;
    let mut saw_completed = false;
    let mut local_turn_id = None;
    for _ in 0..8 {
        let value = read_value(&mut output_reader).await;
        if value.get("id").and_then(Value::as_str) == Some("send") {
            saw_send_response = true;
            local_turn_id = value["result"]["turn_id"].as_str().map(str::to_owned);
        }
        if value.get("method").and_then(Value::as_str) == Some("turn/delta") {
            saw_delta = true;
            assert_eq!(value["params"]["delta"], json!("Native answer"));
        }
        if value.get("method").and_then(Value::as_str) == Some("turn/completed") {
            saw_completed = true;
            break;
        }
    }

    assert!(saw_send_response);
    assert!(saw_delta);
    assert!(saw_completed);
    let local_turn_id = local_turn_id.expect("send response includes turn id");
    drop(input_writer);
    run.await
        .expect("host task joins")
        .expect("host exits after stdin closes");
    assert_eq!(
        store
            .get_turn(&local_turn_id)
            .expect("completed turn remains stored")
            .status,
        TurnStatus::Completed
    );
}

#[tokio::test]
async fn in_process_host_fails_owned_active_turns_when_port_closes() {
    let (mut input_writer, input_reader) = duplex(8192);
    let (output_writer, mut output_reader) = duplex(8192);
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let state = DaemonState::new(
        "unused-native-token",
        store.clone(),
        Arc::new(PendingCodexClient),
    );
    let run = tokio::spawn(run_in_process_host(
        input_reader,
        output_writer,
        state,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
    ));

    send_request(
        &mut input_writer,
        "init",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    assert_eq!(
        read_value(&mut output_reader).await["result"]["auth_status"],
        json!("ready")
    );

    send_request(
        &mut input_writer,
        "session",
        method::SESSION_CREATE,
        json!({ "title": "Pending native chat" }),
    )
    .await;
    let session_response = read_value(&mut output_reader).await;
    let session_id = session_response["result"]["session"]["id"]
        .as_str()
        .expect("session id is present")
        .to_owned();

    send_request(
        &mut input_writer,
        "subscribe",
        method::SESSION_SUBSCRIBE,
        json!({ "session_id": session_id }),
    )
    .await;
    assert_eq!(
        read_response(&mut output_reader, "subscribe").await["id"],
        json!("subscribe")
    );

    send_request(
        &mut input_writer,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id,
            "text": "Stay pending",
            "idempotency_key": "native-pending",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let send_response = read_response(&mut output_reader, "send").await;
    let local_turn_id = send_response["result"]["turn_id"]
        .as_str()
        .expect("send response includes turn id")
        .to_owned();

    drop(input_writer);
    run.await
        .expect("host task joins")
        .expect("host exits after stdin closes");

    let stored_turn = store.get_turn(&local_turn_id).expect("turn still exists");
    let stored_session = store
        .get_session(&session_id)
        .expect("session still exists");

    assert_eq!(stored_turn.status, TurnStatus::Failed);
    assert_eq!(stored_session.session.active_turn_id.as_deref(), None);
    assert_idempotency_failed(&store, &session_id).await;
}

#[tokio::test]
async fn malformed_frame_error_does_not_include_payload_or_token() {
    let error = NativeFrameError::InvalidUtf8;
    let response = frame_error_response(&error);

    assert!(!response.contains("pairing-token"));
    assert!(!response.contains("password swordfish"));
    assert!(response.contains("Native Messaging frame is not valid UTF-8."));
}

fn test_state() -> DaemonState {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    DaemonState::new("unused-native-token", store, Arc::new(ReadyCodexClient))
}

async fn send_request<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    id: &str,
    method: &str,
    params: Value,
) {
    let request = JsonRpcRequest::new(id, method, params);
    write_native_message(
        writer,
        &serde_json::to_string(&request).expect("request serializes"),
        MAX_NATIVE_INCOMING_MESSAGE_BYTES,
    )
    .await
    .expect("request writes");
}

async fn read_value<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> Value {
    let text = read_native_message(reader, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .expect("response frame reads")
        .expect("response frame exists");
    serde_json::from_str(&text).expect("response is JSON")
}

async fn read_response<R: tokio::io::AsyncRead + Unpin>(reader: &mut R, id: &str) -> Value {
    loop {
        let value = read_value(reader).await;
        if value.get("id").and_then(Value::as_str) == Some(id) {
            return value;
        }
    }
}

struct ReadyCodexClient;

#[async_trait]
impl CodexTurnClient for ReadyCodexClient {
    async fn readiness(&self) -> CodexReadiness {
        CodexReadiness {
            available: true,
            version: Some("codex-fake".to_owned()),
            error: None,
        }
    }

    async fn start_turn(
        &self,
        _request: StartTurnRequest,
    ) -> Result<StartTurnOutcome, CodexClientError> {
        Ok(StartTurnOutcome {
            codex_thread_id: "thread_1".to_owned(),
            codex_turn_id: Some("codex_turn_1".to_owned()),
            events: Box::pin(stream::empty::<Result<_, _>>()) as CodexEventStream,
        })
    }

    async fn cancel_turn(&self, _turn_id: &str) -> Result<(), CodexClientError> {
        Ok(())
    }
}

struct StreamingCodexClient;

#[async_trait]
impl CodexTurnClient for StreamingCodexClient {
    async fn readiness(&self) -> CodexReadiness {
        CodexReadiness {
            available: true,
            version: Some("codex-fake".to_owned()),
            error: None,
        }
    }

    async fn start_turn(
        &self,
        _request: StartTurnRequest,
    ) -> Result<StartTurnOutcome, CodexClientError> {
        let events = vec![
            Ok(CodexEvent::Delta {
                turn_id: "codex_turn_1".to_owned(),
                delta: "Native answer".to_owned(),
            }),
            Ok(CodexEvent::Completed {
                turn_id: "codex_turn_1".to_owned(),
                final_assistant_text: None,
            }),
        ];
        Ok(StartTurnOutcome {
            codex_thread_id: "thread_1".to_owned(),
            codex_turn_id: Some("codex_turn_1".to_owned()),
            events: Box::pin(stream::iter(events)) as CodexEventStream,
        })
    }

    async fn cancel_turn(&self, _turn_id: &str) -> Result<(), CodexClientError> {
        Ok(())
    }
}

struct PendingCodexClient;

#[async_trait]
impl CodexTurnClient for PendingCodexClient {
    async fn readiness(&self) -> CodexReadiness {
        CodexReadiness {
            available: true,
            version: Some("codex-fake".to_owned()),
            error: None,
        }
    }

    async fn start_turn(
        &self,
        _request: StartTurnRequest,
    ) -> Result<StartTurnOutcome, CodexClientError> {
        Ok(StartTurnOutcome {
            codex_thread_id: "thread_pending".to_owned(),
            codex_turn_id: Some("codex_turn_pending".to_owned()),
            events: Box::pin(stream::pending::<Result<CodexEvent, CodexClientError>>())
                as CodexEventStream,
        })
    }

    async fn cancel_turn(&self, _turn_id: &str) -> Result<(), CodexClientError> {
        Ok(())
    }
}

async fn assert_idempotency_failed(store: &SessionStore, session_id: &str) {
    assert_idempotency_failed_with_key(store, session_id, "native-pending").await;
}

async fn assert_idempotency_failed_with_key(
    store: &SessionStore,
    session_id: &str,
    idempotency_key: &str,
) {
    let state = DaemonState::new(
        "unused-native-token",
        store.clone(),
        Arc::new(ReadyCodexClient),
    );
    let mut connection = ProtocolConnection::native_host(
        state,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
    );
    let init = JsonRpcRequest::new(
        "init-retry",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    );
    let init_response = connection
        .handle_text(&serde_json::to_string(&init).expect("request serializes"))
        .await
        .expect("initialize returns a response");
    assert!(matches!(
        serde_json::from_str::<JsonRpcResponse>(&init_response)
            .expect("initialize response is JSON"),
        JsonRpcResponse::Success(_)
    ));

    let retry = JsonRpcRequest::new(
        "send-retry",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id,
            "text": "Stay pending",
            "idempotency_key": idempotency_key,
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    );
    let retry_response = connection
        .handle_text(&serde_json::to_string(&retry).expect("request serializes"))
        .await
        .expect("retry returns a response");
    let retry_response =
        serde_json::from_str::<JsonRpcResponse>(&retry_response).expect("retry response is JSON");

    let JsonRpcResponse::Error(error) = retry_response else {
        panic!("retry should fail after native port cleanup");
    };
    assert_eq!(error.error.code, ErrorCode::CodexAppServerUnavailable);
}

async fn wait_for_turn_status(
    store: &SessionStore,
    turn_id: &str,
    status: TurnStatus,
) -> screen_sidekick_sidekick_protocol::Turn {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let turn = store.get_turn(turn_id).expect("turn loads");
            if turn.status == status {
                return turn;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("turn reaches expected status")
}
