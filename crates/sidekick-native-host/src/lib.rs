#![forbid(unsafe_code)]

use std::{env, fmt};

use futures_util::{SinkExt, StreamExt};
use screen_sidekick_sidekick_daemon::{DaemonState, ProtocolConnection};
use screen_sidekick_sidekick_protocol::{
    ErrorCode, ErrorData, JsonRpcFailure, ProtocolError, JSONRPC_VERSION,
};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest, http::HeaderValue as WsHeaderValue, Message as WsMessage,
    },
};
use url::Url;

pub const NATIVE_HOST_NAME: &str = "com.screen_sidekick.host";
pub const SCREEN_SIDEKICK_DAEMON_WS_URL_ENV: &str = "SCREEN_SIDEKICK_DAEMON_WS_URL";
pub const SCREEN_SIDEKICK_DAEMON_TOKEN_ENV: &str = "SCREEN_SIDEKICK_DAEMON_TOKEN";
pub const MAX_NATIVE_INCOMING_MESSAGE_BYTES: usize =
    screen_sidekick_sidekick_daemon::MAX_WS_MESSAGE_BYTES;
pub const MAX_NATIVE_OUTGOING_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeFrameError {
    PartialLength,
    PayloadTooLarge { size: usize, max_size: usize },
    PartialPayload,
    InvalidUtf8,
    Io,
}

impl NativeFrameError {
    fn protocol_error_code(&self) -> ErrorCode {
        match self {
            Self::PayloadTooLarge { .. } => ErrorCode::PayloadTooLarge,
            Self::PartialLength | Self::PartialPayload | Self::InvalidUtf8 | Self::Io => {
                ErrorCode::InvalidRequest
            }
        }
    }

    fn protocol_error_message(&self) -> &'static str {
        match self {
            Self::PayloadTooLarge { .. } => "Native Messaging frame exceeds the Sidekick limit.",
            Self::InvalidUtf8 => "Native Messaging frame is not valid UTF-8.",
            Self::PartialLength | Self::PartialPayload | Self::Io => {
                "Native Messaging frame is invalid."
            }
        }
    }

    fn closes_connection(&self) -> bool {
        matches!(
            self,
            Self::PartialLength | Self::PartialPayload | Self::PayloadTooLarge { .. } | Self::Io
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeWriteError {
    PayloadTooLarge { size: usize, max_size: usize },
    LengthOverflow,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeHostError {
    FrameRead(NativeFrameError),
    FrameWrite(NativeWriteError),
    SidecarUrl,
    SidecarConnect,
    SidecarProtocol,
    RuntimeStart,
    TurnCleanup,
}

impl fmt::Display for NativeHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::FrameRead(_) => "native host received an invalid frame",
            Self::FrameWrite(_) => "native host failed to write a protocol frame",
            Self::SidecarUrl => "native host sidecar URL is invalid",
            Self::SidecarConnect => {
                "native host failed to connect to the configured daemon sidecar"
            }
            Self::SidecarProtocol => "native host sidecar protocol failed",
            Self::RuntimeStart => "native host failed to start Sidekick runtime",
            Self::TurnCleanup => "native host failed to clean up owned active turns",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NativeHostError {}

pub async fn read_native_message<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_size: usize,
) -> Result<Option<String>, NativeFrameError> {
    let mut length_bytes = [0_u8; 4];
    let mut read = 0_usize;
    while read < length_bytes.len() {
        match reader.read(&mut length_bytes[read..]).await {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => return Err(NativeFrameError::PartialLength),
            Ok(count) => read += count,
            Err(_) => return Err(NativeFrameError::Io),
        }
    }

    let size = u32::from_ne_bytes(length_bytes) as usize;
    if size > max_size {
        return Err(NativeFrameError::PayloadTooLarge { size, max_size });
    }

    let mut payload = vec![0_u8; size];
    if reader.read_exact(&mut payload).await.is_err() {
        return Err(NativeFrameError::PartialPayload);
    }
    String::from_utf8(payload)
        .map(Some)
        .map_err(|_| NativeFrameError::InvalidUtf8)
}

pub async fn write_native_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    text: &str,
    max_size: usize,
) -> Result<(), NativeWriteError> {
    let payload = text.as_bytes();
    if payload.len() >= max_size {
        return Err(NativeWriteError::PayloadTooLarge {
            size: payload.len(),
            max_size,
        });
    }
    let length = u32::try_from(payload.len()).map_err(|_| NativeWriteError::LengthOverflow)?;
    writer
        .write_all(&length.to_ne_bytes())
        .await
        .map_err(|_| NativeWriteError::Io)?;
    writer
        .write_all(payload)
        .await
        .map_err(|_| NativeWriteError::Io)?;
    writer.flush().await.map_err(|_| NativeWriteError::Io)
}

pub async fn run_in_process_host<R, W>(
    mut reader: R,
    mut writer: W,
    state: DaemonState,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut connection = ProtocolConnection::native_host(state, caller_origin);
    let mut events = connection.event_receiver();

    let run_result = loop {
        tokio::select! {
            frame = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES) => {
                match frame {
                    Ok(Some(text)) => {
                        if let Some(response) = connection.handle_text(&text).await {
                            if let Err(error) = write_native_message(&mut writer, &response, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
                                .await
                                .map_err(NativeHostError::FrameWrite)
                            {
                                break Err(error);
                            }
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(error) => {
                        if let Err(write_error) = write_frame_error(&mut writer, &error).await {
                            break Err(write_error);
                        }
                        if error.closes_connection() {
                            break Err(NativeHostError::FrameRead(error));
                        }
                    }
                }
            }
            notification = events.recv() => {
                match notification {
                    Ok(notification) => {
                        if let Some(text) = connection.notification_text(&notification) {
                            if let Err(error) = write_native_message(&mut writer, &text, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
                                .await
                                .map_err(NativeHostError::FrameWrite)
                            {
                                break Err(error);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
                    | Err(tokio::sync::broadcast::error::RecvError::Closed) => break Ok(()),
                }
            }
        }
    };

    if connection.fail_owned_active_turns_on_disconnect().is_err() {
        return Err(NativeHostError::TurnCleanup);
    }
    run_result
}

pub async fn run_sidecar_host<R, W>(
    mut reader: R,
    mut writer: W,
    ws_url: &str,
    token: &str,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let url = validate_sidecar_ws_url(ws_url)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| NativeHostError::SidecarUrl)?;
    if let Some(origin) = caller_origin.as_deref() {
        let header = WsHeaderValue::from_str(origin).map_err(|_| NativeHostError::SidecarUrl)?;
        request.headers_mut().insert("Origin", header);
    }
    let (socket, _) = connect_async(request)
        .await
        .map_err(|_| NativeHostError::SidecarConnect)?;
    let (mut ws_sender, mut ws_receiver) = socket.split();

    loop {
        tokio::select! {
            frame = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES) => {
                match frame {
                    Ok(Some(text)) => {
                        let text = inject_sidecar_initialize_token(&text, token);
                        ws_sender
                            .send(WsMessage::Text(text.into()))
                            .await
                            .map_err(|_| NativeHostError::SidecarProtocol)?;
                    }
                    Ok(None) => return Ok(()),
                    Err(error) => {
                        write_frame_error(&mut writer, &error).await?;
                        return Err(NativeHostError::FrameRead(error));
                    }
                }
            }
            message = ws_receiver.next() => {
                match message {
                    Some(Ok(WsMessage::Text(text))) => {
                        write_native_message(&mut writer, text.as_ref(), MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
                            .await
                            .map_err(NativeHostError::FrameWrite)?;
                    }
                    Some(Ok(WsMessage::Ping(bytes))) => {
                        ws_sender
                            .send(WsMessage::Pong(bytes))
                            .await
                            .map_err(|_| NativeHostError::SidecarProtocol)?;
                    }
                    Some(Ok(WsMessage::Close(_))) | None => return Ok(()),
                    Some(Ok(WsMessage::Binary(_))) | Some(Ok(WsMessage::Pong(_))) => {}
                    Some(Ok(WsMessage::Frame(_))) => {}
                    Some(Err(_)) => return Err(NativeHostError::SidecarProtocol),
                }
            }
        }
    }
}

pub async fn run_from_environment<R, W>(
    reader: R,
    writer: W,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let sidecar_url = env::var(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV).ok();
    let sidecar_token = env::var(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV).ok();
    match (sidecar_url, sidecar_token) {
        (Some(url), Some(token)) => {
            run_sidecar_host(reader, writer, &url, &token, caller_origin).await
        }
        _ => {
            let state =
                DaemonState::default_runtime_state().map_err(|_| NativeHostError::RuntimeStart)?;
            run_in_process_host(reader, writer, state, caller_origin).await
        }
    }
}

pub fn caller_origin_from_args<I>(args: I) -> Option<String>
where
    I: IntoIterator<Item = String>,
{
    args.into_iter()
        .skip(1)
        .find(|arg| arg.starts_with("chrome-extension://"))
}

async fn write_frame_error<W: AsyncWrite + Unpin>(
    writer: &mut W,
    error: &NativeFrameError,
) -> Result<(), NativeHostError> {
    let response = frame_error_response(error);
    write_native_message(writer, &response, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .map_err(NativeHostError::FrameWrite)
}

fn frame_error_response(error: &NativeFrameError) -> String {
    let data = match error {
        NativeFrameError::PayloadTooLarge { max_size, .. } => Some(ErrorData {
            max_size_bytes: Some(*max_size),
            retryable: Some(false),
            ..ErrorData::default()
        }),
        _ => Some(ErrorData {
            retryable: Some(false),
            ..ErrorData::default()
        }),
    };
    serde_json::to_string(&JsonRpcFailure::new(
        "unknown",
        ProtocolError {
            code: error.protocol_error_code(),
            message: error.protocol_error_message().to_owned(),
            data: data.map(Box::new),
        },
    ))
    .unwrap_or_else(|_| {
        format!(
            "{{\"jsonrpc\":\"{}\",\"id\":\"unknown\",\"error\":{{\"code\":\"internal_error\",\"message\":\"internal error\"}}}}",
            JSONRPC_VERSION
        )
    })
}

fn validate_sidecar_ws_url(raw_url: &str) -> Result<Url, NativeHostError> {
    let url = Url::parse(raw_url).map_err(|_| NativeHostError::SidecarUrl)?;
    if url.scheme() != "ws"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/v0/ws"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(NativeHostError::SidecarUrl);
    }
    Ok(url)
}

fn inject_sidecar_initialize_token(text: &str, token: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return text.to_owned();
    };
    if value.get("method").and_then(Value::as_str) != Some("initialize") {
        return text.to_owned();
    }
    match value.get_mut("params") {
        Some(Value::Object(params)) => {
            params.insert("auth_token".to_owned(), json!(token));
        }
        Some(_) | None => {
            value["params"] = json!({ "auth_token": token });
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures_util::stream;
    use screen_sidekick_codex_client::{
        CodexClientError, CodexEvent, CodexEventStream, CodexReadiness, CodexTurnClient,
        StartTurnOutcome, StartTurnRequest,
    };
    use screen_sidekick_session::{BeginTurn, SessionStore};
    use screen_sidekick_sidekick_protocol::{
        method, JsonRpcRequest, JsonRpcResponse, TurnStatus, SIDEKICK_PROTOCOL_VERSION,
    };
    use std::sync::Arc;
    use tokio::io::{duplex, AsyncReadExt};

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    async fn default_in_process_host_does_not_recover_shared_active_turns() {
        let _guard = ENV_LOCK.lock().await;
        let temp = tempfile::tempdir().expect("temp dir is created");
        let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", temp.path());
        let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
        let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
        let data_dir = temp.path().join("screen-sidekick");
        std::fs::create_dir_all(&data_dir).expect("data dir is created");
        let database_path = data_dir.join("screen-sidekick.sqlite3");
        let store = SessionStore::open(&database_path).expect("store opens");
        let session = store
            .create_session(Some("Live native turn"))
            .expect("session created");
        let turn = store
            .begin_turn(BeginTurn {
                session_id: session.id.clone(),
                user_text: "still streaming".to_owned(),
                attachment_ids: Vec::new(),
                idempotency_key: "live-native-turn".to_owned(),
                request_hash: "live-native-hash".to_owned(),
            })
            .expect("turn begins");
        store
            .mark_turn_running(
                &turn.turn_id,
                Some("live_codex_thread"),
                Some("live_codex_turn"),
            )
            .expect("turn is running");
        drop(store);

        let (input_writer, input_reader) = duplex(64);
        let (output_writer, _output_reader) = duplex(64);
        drop(input_writer);

        run_from_environment(
            input_reader,
            output_writer,
            Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        )
        .await
        .expect("native host exits after stdin closes");

        let store = SessionStore::open(&database_path).expect("store reopens");
        let stored_turn = store.get_turn(&turn.turn_id).expect("turn still exists");
        let stored_session = store
            .get_session(&session.id)
            .expect("session still exists");

        assert_eq!(stored_turn.status, TurnStatus::Running);
        assert_eq!(
            stored_session.session.active_turn_id.as_deref(),
            Some(turn.turn_id.as_str())
        );
        assert!(stored_session.active_turn.is_some());
    }

    #[tokio::test]
    async fn malformed_frame_error_does_not_include_payload_or_token() {
        let error = NativeFrameError::InvalidUtf8;
        let response = frame_error_response(&error);

        assert!(!response.contains("pairing-token"));
        assert!(!response.contains("password swordfish"));
        assert!(response.contains("Native Messaging frame is not valid UTF-8."));
    }

    #[test]
    fn sidecar_url_accepts_only_explicit_loopback_websocket_endpoint() {
        assert!(validate_sidecar_ws_url("ws://127.0.0.1:43001/v0/ws").is_ok());
        assert!(validate_sidecar_ws_url("ws://localhost:43001/v0/ws").is_err());
        assert!(validate_sidecar_ws_url("http://127.0.0.1:43001/v0/ws").is_err());
        assert!(validate_sidecar_ws_url("ws://127.0.0.1:43001/v0/ws?token=SECRET").is_err());
        assert!(validate_sidecar_ws_url("ws://127.0.0.1:43001/other").is_err());
    }

    #[test]
    fn sidecar_initialize_injection_keeps_token_out_of_non_initialize_messages() {
        let request = JsonRpcRequest::new("status", method::STATUS_GET, json!({}));
        let text = serde_json::to_string(&request).expect("request serializes");

        assert_eq!(inject_sidecar_initialize_token(&text, "secret-token"), text);

        let init = JsonRpcRequest::new(
            "init",
            method::INITIALIZE,
            json!({ "protocol_version": SIDEKICK_PROTOCOL_VERSION }),
        );
        let injected = inject_sidecar_initialize_token(
            &serde_json::to_string(&init).expect("request serializes"),
            "secret-token",
        );
        let injected: Value = serde_json::from_str(&injected).expect("injected request is JSON");

        assert_eq!(injected["params"]["auth_token"], json!("secret-token"));
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

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
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
                "idempotency_key": "native-pending",
                "attachment_ids": [],
                "mode": "ask_only"
            }),
        );
        let retry_response = connection
            .handle_text(&serde_json::to_string(&retry).expect("request serializes"))
            .await
            .expect("retry returns a response");
        let retry_response = serde_json::from_str::<JsonRpcResponse>(&retry_response)
            .expect("retry response is JSON");

        let JsonRpcResponse::Error(error) = retry_response else {
            panic!("retry should fail after native port cleanup");
        };
        assert_eq!(error.error.code, ErrorCode::CodexAppServerUnavailable);
    }
}
