use super::{
    assert_idempotency_failed_with_key, read_response, read_value, send_request,
    wait_for_turn_status, PendingCodexClient,
};
use crate::{
    run_sidecar_host, NativeHostError, SETUP_REQUIRED_MESSAGE, SETUP_REQUIRED_USER_ACTION,
};
use futures_util::StreamExt;
use screen_sidekick_session::SessionStore;
use screen_sidekick_sidekick_daemon::{DaemonRuntime, DaemonState};
use screen_sidekick_sidekick_protocol::{method, ErrorCode, TurnStatus, SIDEKICK_PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::{io::duplex, net::TcpListener};
use tokio_tungstenite::accept_async;

#[tokio::test]
async fn sidecar_host_disconnect_fails_daemon_owned_active_turn() {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let state = DaemonState::new("sidecar-token", store.clone(), Arc::new(PendingCodexClient));
    let (_runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    let (mut input_writer, input_reader) = duplex(8192);
    let (output_writer, mut output_reader) = duplex(8192);
    let ws_url = status.ws_url.clone();
    let token = status.token.clone();
    let run = tokio::spawn(async move {
        run_sidecar_host(
            input_reader,
            output_writer,
            &ws_url,
            &token,
            Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        )
        .await
    });

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
        json!({ "title": "Pending sidecar chat" }),
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
            "session_id": session_id.clone(),
            "text": "Stay pending",
            "idempotency_key": "sidecar-pending",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let send_response = read_response(&mut output_reader, "send").await;
    let turn_id = send_response["result"]["turn_id"]
        .as_str()
        .expect("send response includes turn id")
        .to_owned();

    drop(input_writer);
    run.await
        .expect("sidecar host task joins")
        .expect("sidecar host exits after stdin closes");
    let failed_turn = wait_for_turn_status(&store, &turn_id, TurnStatus::Failed).await;
    let stored_session = store
        .get_session(&session_id)
        .expect("session still exists");

    assert_eq!(
        failed_turn.error.as_ref().map(|error| &error.code),
        Some(&ErrorCode::CodexAppServerUnavailable)
    );
    assert_eq!(stored_session.session.active_turn_id.as_deref(), None);
    assert_idempotency_failed_with_key(&store, &session_id, "sidecar-pending").await;
}

#[tokio::test]
async fn wsl_sidecar_connect_failure_reports_setup_required_protocol_error() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake sidecar listener binds");
    let ws_url = format!(
        "ws://127.0.0.1:{}/v0/ws",
        listener
            .local_addr()
            .expect("fake sidecar listener has an address")
            .port()
    );
    let server = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("fake sidecar accepts one connection");
        drop(stream);
    });
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let run = tokio::spawn(async move {
        crate::sidecar::run_wsl_auto_sidecar_host(
            input_reader,
            output_writer,
            &ws_url,
            "pairing-token",
            Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        )
        .await
    });

    send_request(
        &mut input_writer,
        "init-wsl-connect-failure",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    let response = read_value(&mut output_reader).await;

    assert_setup_required_response(&response, "init-wsl-connect-failure");
    run.await
        .expect("wsl sidecar host task joins")
        .expect("wsl sidecar host exits after setup-required response");
    server.await.expect("fake sidecar server task joins");
}

#[tokio::test]
async fn wsl_sidecar_protocol_failure_after_initialize_reports_setup_required_protocol_error() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake sidecar listener binds");
    let ws_url = format!(
        "ws://127.0.0.1:{}/v0/ws",
        listener
            .local_addr()
            .expect("fake sidecar listener has an address")
            .port()
    );
    let server = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("fake sidecar accepts one connection");
        let mut socket = accept_async(stream)
            .await
            .expect("fake sidecar websocket handshakes");
        let message = socket
            .next()
            .await
            .expect("fake sidecar receives initialize")
            .expect("initialize websocket message succeeds");
        let text = message.into_text().expect("initialize is text");
        let value: Value = serde_json::from_str(text.as_ref()).expect("initialize is JSON");

        assert_eq!(value["method"], json!(method::INITIALIZE));
        assert_eq!(value["params"]["auth_token"], json!("pairing-token"));
        drop(socket);
    });
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let run = tokio::spawn(async move {
        crate::sidecar::run_wsl_auto_sidecar_host(
            input_reader,
            output_writer,
            &ws_url,
            "pairing-token",
            Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        )
        .await
    });

    send_request(
        &mut input_writer,
        "init-wsl-protocol-failure",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    let response = read_value(&mut output_reader).await;

    assert_setup_required_response(&response, "init-wsl-protocol-failure");
    run.await
        .expect("wsl sidecar host task joins")
        .expect("wsl sidecar host exits after setup-required response");
    server.await.expect("fake sidecar server task joins");
}

#[tokio::test]
async fn explicit_sidecar_connect_failure_stays_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake sidecar listener binds");
    let ws_url = format!(
        "ws://127.0.0.1:{}/v0/ws",
        listener
            .local_addr()
            .expect("fake sidecar listener has an address")
            .port()
    );
    let server = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("fake sidecar accepts one connection");
        drop(stream);
    });
    let (input_writer, input_reader) = duplex(64);
    let (output_writer, _output_reader) = duplex(64);
    drop(input_writer);

    let error = run_sidecar_host(
        input_reader,
        output_writer,
        &ws_url,
        "pairing-token",
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
    )
    .await
    .expect_err("explicit sidecar connect failure remains a sidecar error");

    assert_eq!(error, NativeHostError::SidecarConnect);
    server.await.expect("fake sidecar server task joins");
}

fn assert_setup_required_response(response: &Value, id: &str) {
    assert_eq!(response["id"], json!(id));
    assert_eq!(response["error"]["code"], json!("setup_required"));
    assert_eq!(response["error"]["message"], json!(SETUP_REQUIRED_MESSAGE));
    assert_eq!(response["error"]["data"]["retryable"], json!(false));
    assert_eq!(
        response["error"]["data"]["user_action"],
        json!(SETUP_REQUIRED_USER_ACTION)
    );
}
