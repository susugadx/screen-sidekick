use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION,
            CONTENT_TYPE, ORIGIN,
        },
        HeaderValue, Method, Request, StatusCode,
    },
};
use futures_util::{SinkExt, StreamExt};
use screen_sidekick_capture_pipeline::RAW_BROWSER_CONTEXT_SCHEMA_VERSION;
use screen_sidekick_codex_client::{
    CodexClientError, CodexClientErrorKind, CodexEvent, CodexEventStream, CodexReadiness,
    CodexTurnClient, StartTurnOutcome, StartTurnRequest,
};
use screen_sidekick_session::{BeginTurn, SessionStore};
use screen_sidekick_sidekick_daemon::{
    build_daemon_router, DaemonOptions, DaemonRuntime, DaemonState, MAX_ATTACHMENT_BYTES,
    MAX_CAPTURE_BODY_BYTES,
};
use screen_sidekick_sidekick_protocol::{
    method, notification, ErrorCode, JsonRpcFailure, JsonRpcRequest, JsonRpcResponse,
    JsonRpcSuccess, MessageRole, ProtocolError, SIDEKICK_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest, http::HeaderValue as WsHeaderValue, Message as WsMessage,
    },
    MaybeTlsStream, WebSocketStream,
};
use tower::ServiceExt;

const TOKEN: &str = "test-token";
const EXTENSION_ORIGIN: &str = "chrome-extension://abcdefghijklmnop";

type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn websocket_initialize_rejects_bad_pairing_token() {
    let (_runtime, status, _store, _codex) = start_test_daemon(vec![]);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let error = send_request_expect_error(
        &mut socket,
        "init",
        method::INITIALIZE,
        initialize_params("wrong-token"),
    )
    .await;

    assert_eq!(error.code, ErrorCode::Unauthorized);
}

#[tokio::test]
async fn websocket_does_not_forward_broadcasts_before_initialize() {
    let (_runtime, status, _store, _codex) = start_test_daemon(vec![]);
    let mut unauthenticated_socket = connect_to_daemon(&status.ws_url).await;
    let mut paired_socket = connect_to_daemon(&status.ws_url).await;

    let _session_id = initialized_session(&mut paired_socket, &status.token).await;

    assert!(read_notification_with_timeout(&mut unauthenticated_socket)
        .await
        .is_none());
}

#[tokio::test]
async fn websocket_initialize_omits_turn_cancel_for_unsupported_codex_client() {
    let (_runtime, status, _store, _codex) = start_test_daemon(vec![]);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let init_result = send_request(
        &mut socket,
        "init",
        method::INITIALIZE,
        initialize_params(&status.token),
    )
    .await;

    assert!(!capabilities_include(&init_result, "turn_cancel"));
}

#[tokio::test]
async fn websocket_initialize_advertises_turn_cancel_for_supported_codex_client() {
    let (_runtime, status, _store, _codex) = start_test_daemon_with_support(vec![], true);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let init_result = send_request(
        &mut socket,
        "init",
        method::INITIALIZE,
        initialize_params(&status.token),
    )
    .await;

    assert!(capabilities_include(&init_result, "turn_cancel"));
}

#[tokio::test]
async fn legacy_capture_rejects_missing_bearer_token() {
    let app = build_daemon_router(test_state(vec![]));
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v0/capture")
        .header(ORIGIN, EXTENSION_ORIGIN)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(valid_raw_context().to_string()))
        .expect("request is valid");

    let response = app.oneshot(request).await.expect("router responds");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static(EXTENSION_ORIGIN))
    );
}

#[tokio::test]
async fn legacy_capture_response_does_not_leak_raw_secret_values() {
    let app = build_daemon_router(test_state(vec![]));
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v0/capture")
        .header(ORIGIN, EXTENSION_ORIGIN)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, "Bearer test-token")
        .body(Body::from(secret_raw_context().to_string()))
        .expect("request is valid");

    let response = app.oneshot(request).await.expect("router responds");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), MAX_CAPTURE_BODY_BYTES)
        .await
        .expect("body is readable");
    let response_text = String::from_utf8(bytes.to_vec()).expect("response is UTF-8");

    assert_eq!(status, StatusCode::OK);
    assert!(response_text.contains("[masked]"));
    assert!(response_text.contains("access_token=[REDACTED]"));
    assert_no_raw_secret_values(&response_text);
}

#[tokio::test]
async fn websocket_chat_flow_streams_turn_and_sends_only_sanitized_context_to_codex() {
    let events = vec![
        CodexEvent::Delta {
            turn_id: "fake_turn".to_owned(),
            delta: "Use the sanitized browser context.".to_owned(),
        },
        CodexEvent::Completed {
            turn_id: "fake_turn".to_owned(),
        },
    ];
    let (_runtime, status, store, codex) = start_test_daemon(events);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let init_result = send_request(
        &mut socket,
        "init",
        method::INITIALIZE,
        initialize_params(&status.token),
    )
    .await;
    assert_eq!(
        init_result["protocol_version"],
        json!(SIDEKICK_PROTOCOL_VERSION)
    );

    let session_result = send_request(
        &mut socket,
        "session",
        method::SESSION_CREATE,
        json!({ "title": "Browser question" }),
    )
    .await;
    let session_id = session_result["session"]["id"]
        .as_str()
        .expect("session id is present")
        .to_owned();

    let _ = send_request(
        &mut socket,
        "subscribe",
        method::SESSION_SUBSCRIBE,
        json!({ "session_id": session_id }),
    )
    .await;

    let attach_result = send_request(
        &mut socket,
        "attach",
        method::CONTEXT_ATTACH_BROWSER,
        json!({
            "session_id": session_id,
            "capture_id": "cap_1",
            "capture_reason": "message_send",
            "raw_context": secret_raw_context()
        }),
    )
    .await;
    let attachment_id = attach_result["attachment"]["id"]
        .as_str()
        .expect("attachment id is present")
        .to_owned();
    let stored_context = store
        .sanitized_context_json(&attachment_id)
        .expect("stored context exists");
    assert!(stored_context.contains("[masked]"));
    assert_no_raw_secret_values(&stored_context);

    let _send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id,
            "text": "What should I do next?",
            "idempotency_key": "idem-1",
            "attachment_ids": [attachment_id],
            "mode": "ask_only"
        }),
    )
    .await;

    let notifications = read_notifications_until(&mut socket, notification::TURN_COMPLETED).await;
    let notification_text = serde_json::to_string(&notifications).expect("notifications serialize");
    assert!(notification_text.contains(notification::TURN_DELTA));
    assert!(notification_text.contains("Use the sanitized browser context."));
    assert_no_raw_secret_values(&notification_text);

    let request = codex
        .last_request()
        .expect("codex start_turn request was recorded");
    assert!(request.context_text.contains("Safety review:"));
    assert!(request.context_text.contains("ScreenContext JSON:"));
    assert!(request.context_text.contains("category=\"destructive\""));
    assert!(request.context_text.contains("category=\"publish\""));
    assert!(request.context_text.contains("category=\"billing\""));
    assert!(request.context_text.contains("[masked]"));
    assert!(request.context_text.contains("access_token=[REDACTED]"));
    assert_no_raw_secret_values(&request.context_text);
}

#[tokio::test]
async fn websocket_unknown_codex_event_does_not_emit_error_notification() {
    let events = vec![
        CodexEvent::Delta {
            turn_id: "fake_turn".to_owned(),
            delta: "partial answer".to_owned(),
        },
        CodexEvent::Unknown {
            method: "experimental/new_event".to_owned(),
        },
        CodexEvent::Completed {
            turn_id: "fake_turn".to_owned(),
        },
    ];
    let (_runtime, status, _store, _codex) = start_test_daemon(events);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let session_id = initialized_session(&mut socket, &status.token).await;

    let _send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id,
            "text": "Continue",
            "idempotency_key": "unknown-event",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    let notifications = read_notifications_until(&mut socket, notification::TURN_COMPLETED).await;

    assert!(
        notifications
            .iter()
            .any(|value| value.get("method").and_then(Value::as_str)
                == Some(notification::TURN_DELTA))
    );
    assert!(!notifications
        .iter()
        .any(|value| value.get("method").and_then(Value::as_str) == Some(notification::ERROR)));
}

#[tokio::test]
async fn websocket_closes_lagged_subscribers_to_force_recovery() {
    let mut events = (0..64)
        .map(|index| CodexEvent::Delta {
            turn_id: "fake_turn".to_owned(),
            delta: format!("chunk {index}\n"),
        })
        .collect::<Vec<_>>();
    events.push(CodexEvent::Completed {
        turn_id: "fake_turn".to_owned(),
    });
    let (_runtime, status, store, _codex) = start_test_daemon_with_options(
        events,
        false,
        DaemonOptions {
            event_buffer_capacity: 1,
            ..DaemonOptions::default()
        },
    );
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let _send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "stream quickly",
            "idempotency_key": "lagged-subscriber",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    expect_socket_disconnect(&mut socket).await;
    wait_for_session_without_active_turn(&store, &session_id).await;
}

#[tokio::test]
async fn websocket_attach_browser_rejects_related_message_from_another_session() {
    let (_runtime, status, store, _codex) = start_test_daemon(vec![]);
    let source_session = store
        .create_session(Some("Source"))
        .expect("source session created");
    let target_session = store
        .create_session(Some("Target"))
        .expect("target session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: source_session.id,
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let _init_result = send_request(
        &mut socket,
        "init",
        method::INITIALIZE,
        initialize_params(&status.token),
    )
    .await;

    let error = send_request_expect_error(
        &mut socket,
        "attach",
        method::CONTEXT_ATTACH_BROWSER,
        json!({
            "session_id": target_session.id,
            "capture_id": "cap_1",
            "capture_reason": "message_send",
            "related_message_id": turn.message_id,
            "raw_context": valid_raw_context()
        }),
    )
    .await;

    assert_eq!(error.code, ErrorCode::MessageNotFound);
}

#[tokio::test]
async fn websocket_attach_browser_rejects_context_over_advertised_attachment_limit() {
    let (_runtime, status, store, _codex) = start_test_daemon(vec![]);
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let error = send_request_expect_error(
        &mut socket,
        "attach",
        method::CONTEXT_ATTACH_BROWSER,
        json!({
            "session_id": session_id,
            "capture_id": "cap_large",
            "capture_reason": "message_send",
            "raw_context": oversized_raw_context()
        }),
    )
    .await;
    let attachments = store
        .list_attachments(&session_id)
        .expect("attachments list");

    assert_eq!(error.code, ErrorCode::ContextTooLarge);
    assert_eq!(
        error.data.as_ref().and_then(|data| data.max_size_bytes),
        Some(MAX_ATTACHMENT_BYTES)
    );
    assert!(attachments.is_empty());
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
}

#[tokio::test]
async fn websocket_codex_failed_event_emits_turn_failed_and_clears_active_turn() {
    let events = vec![CodexEvent::Failed {
        turn_id: Some("fake_turn".to_owned()),
        message: "model failed".to_owned(),
    }];
    let (_runtime, status, store, _codex) = start_test_daemon(events);
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let _send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "failed-event",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    let notifications = read_notifications_until(&mut socket, notification::TURN_FAILED).await;
    let failed = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_FAILED)
        })
        .expect("turn failed notification is emitted");
    assert_eq!(failed["params"]["message"], json!("model failed"));
    assert_eq!(
        failed["params"]["turn"]["error"]["code"],
        json!("codex_turn_failed")
    );

    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != MessageRole::Assistant));
}

#[tokio::test]
async fn websocket_codex_stream_error_preserves_error_code_and_clears_active_turn() {
    let (_runtime, status, store, _codex, sender) =
        start_test_daemon_controlled_turn_with_support(false);
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "stream-error",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let turn_id = send_result["turn_id"]
        .as_str()
        .expect("turn id is returned")
        .to_owned();

    sender
        .send(Err(CodexClientError::new(
            CodexClientErrorKind::AppServerUnavailable,
            "codex app-server stream ended",
        )))
        .expect("stream error sends");

    let notifications = read_notifications_until(&mut socket, notification::TURN_FAILED).await;
    let failed = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_FAILED)
        })
        .expect("turn failed notification is emitted");
    assert_eq!(failed["params"]["session_id"], json!(session_id));
    assert_eq!(failed["params"]["turn"]["session_id"], json!(session_id));
    assert_eq!(failed["params"]["turn"]["status"], json!("failed"));
    assert_eq!(
        failed["params"]["turn"]["error"]["code"],
        json!("codex_app_server_unavailable")
    );
    assert_eq!(
        failed["params"]["message"],
        json!("codex app-server stream ended")
    );

    let stored_turn = store.get_turn(&turn_id).expect("turn loads");
    assert_eq!(
        stored_turn.status,
        screen_sidekick_sidekick_protocol::TurnStatus::Failed
    );
    assert_eq!(
        stored_turn.error.as_ref().map(|error| &error.code),
        Some(&ErrorCode::CodexAppServerUnavailable)
    );

    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != MessageRole::Assistant));

    let retry_error = send_request_expect_error(
        &mut socket,
        "send-retry",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "stream-error",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    assert_eq!(retry_error.code, ErrorCode::CodexAppServerUnavailable);
    assert_eq!(retry_error.message, "Previous message/send attempt failed.");
}

#[tokio::test]
async fn websocket_codex_stream_end_before_terminal_event_reports_app_server_unavailable() {
    let (_runtime, status, _store, _codex, sender) =
        start_test_daemon_controlled_turn_with_support(false);
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let _send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "stream-ended",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    drop(sender);

    let notifications = read_notifications_until(&mut socket, notification::TURN_FAILED).await;
    let failed = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_FAILED)
        })
        .expect("turn failed notification is emitted");
    assert_eq!(
        failed["params"]["turn"]["error"]["code"],
        json!("codex_app_server_unavailable")
    );
    assert_eq!(
        failed["params"]["message"],
        json!("Codex stream ended before completion.")
    );
}

#[tokio::test]
async fn websocket_codex_start_failure_emits_turn_failed_and_clears_active_turn() {
    let (_runtime, status, store, codex) = start_test_daemon_failing_start(CodexClientError::new(
        CodexClientErrorKind::CodexNotFound,
        "Codex CLI was not found.",
    ));
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let (error, notifications) = send_request_expect_error_collecting_notifications_until(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "start-failure",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
        notification::TURN_FAILED,
    )
    .await;

    assert_eq!(error.code, ErrorCode::CodexNotFound);
    assert_eq!(error.message, "Codex CLI was not found.");
    assert!(notifications.iter().any(|value| {
        value.get("method").and_then(Value::as_str) == Some(notification::MESSAGE_CREATED)
    }));
    let failed = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_FAILED)
        })
        .expect("turn failed notification is emitted");
    assert_eq!(failed["params"]["session_id"], json!(session_id));
    assert_eq!(failed["params"]["turn"]["session_id"], json!(session_id));
    assert_eq!(failed["params"]["turn"]["status"], json!("failed"));
    assert_eq!(
        failed["params"]["message"],
        json!("Codex CLI was not found.")
    );

    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != MessageRole::Assistant));

    let retry_error = send_request_expect_error(
        &mut socket,
        "send-retry",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "start-failure",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    assert_eq!(retry_error.code, ErrorCode::CodexNotFound);
    assert_eq!(retry_error.message, "Previous message/send attempt failed.");
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
    assert_eq!(codex.start_count(), 1);
}

#[tokio::test]
async fn websocket_codex_start_timeout_fails_turn_and_clears_active_turn() {
    let (_runtime, status, store, codex) = start_test_daemon_hanging_start();
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let (error, notifications) = tokio::time::timeout(
        Duration::from_secs(1),
        send_request_expect_error_collecting_notifications_until(
            &mut socket,
            "send",
            method::MESSAGE_SEND,
            json!({
                "session_id": session_id.clone(),
                "text": "Continue",
                "idempotency_key": "start-timeout",
                "attachment_ids": [],
                "mode": "ask_only"
            }),
            notification::TURN_FAILED,
        ),
    )
    .await
    .expect("message/send returns after codex startup timeout");

    assert_eq!(error.code, ErrorCode::CodexAppServerUnavailable);
    assert_eq!(error.message, "Codex turn startup timed out.");
    assert!(notifications.iter().any(|value| {
        value.get("method").and_then(Value::as_str) == Some(notification::MESSAGE_CREATED)
    }));
    let failed = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_FAILED)
        })
        .expect("turn failed notification is emitted");
    assert_eq!(failed["params"]["session_id"], json!(session_id));
    assert_eq!(failed["params"]["turn"]["session_id"], json!(session_id));
    assert_eq!(failed["params"]["turn"]["status"], json!("failed"));
    assert_eq!(
        failed["params"]["turn"]["error"]["code"],
        json!("codex_app_server_unavailable")
    );
    assert_eq!(
        failed["params"]["message"],
        json!("Codex turn startup timed out.")
    );

    let turn_id = failed["params"]["turn"]["id"]
        .as_str()
        .expect("turn id is present");
    let stored_turn = store.get_turn(turn_id).expect("turn loads");
    assert_eq!(
        stored_turn.status,
        screen_sidekick_sidekick_protocol::TurnStatus::Failed
    );
    assert_eq!(
        stored_turn.error.as_ref().map(|error| &error.code),
        Some(&ErrorCode::CodexAppServerUnavailable)
    );
    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != MessageRole::Assistant));

    let retry_error = send_request_expect_error(
        &mut socket,
        "send-retry",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "start-timeout",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    assert_eq!(retry_error.code, ErrorCode::CodexAppServerUnavailable);
    assert_eq!(retry_error.message, "Previous message/send attempt failed.");
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
    assert_eq!(codex.start_count(), 1);
}

#[tokio::test]
async fn websocket_codex_start_unsupported_version_persists_error_for_retry() {
    let (_runtime, status, store, codex) = start_test_daemon_failing_start(CodexClientError::new(
        CodexClientErrorKind::UnsupportedVersion,
        "Codex app-server version is unsupported.",
    ));
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let (error, notifications) = send_request_expect_error_collecting_notifications_until(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "unsupported-version",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
        notification::TURN_FAILED,
    )
    .await;

    assert_eq!(error.code, ErrorCode::UnsupportedCodexVersion);
    assert_eq!(error.message, "Codex app-server version is unsupported.");
    let failed = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_FAILED)
        })
        .expect("turn failed notification is emitted");
    assert_eq!(
        failed["params"]["turn"]["error"]["code"],
        json!("unsupported_codex_version")
    );
    assert_eq!(
        failed["params"]["message"],
        json!("Codex app-server version is unsupported.")
    );

    let turn_id = failed["params"]["turn"]["id"]
        .as_str()
        .expect("turn id is present");
    let stored_turn = store.get_turn(turn_id).expect("turn loads");
    assert_eq!(
        stored_turn.error.as_ref().map(|error| error.code),
        Some(ErrorCode::UnsupportedCodexVersion)
    );

    let retry_error = send_request_expect_error(
        &mut socket,
        "send-retry",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Continue",
            "idempotency_key": "unsupported-version",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    assert_eq!(retry_error.code, ErrorCode::UnsupportedCodexVersion);
    assert_eq!(retry_error.message, "Previous message/send attempt failed.");
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
    assert_eq!(codex.start_count(), 1);
}

#[tokio::test]
async fn websocket_rejects_second_session_message_while_daemon_turn_is_running() {
    let (_runtime, status, store, codex) = start_test_daemon_holding_turn();
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let first_session_id = initialized_session(&mut socket, &status.token).await;
    let second_session_result = send_request(
        &mut socket,
        "session-2",
        method::SESSION_CREATE,
        json!({ "title": "Second" }),
    )
    .await;
    let second_session_id = second_session_result["session"]["id"]
        .as_str()
        .expect("second session id is present")
        .to_owned();

    let first_send = send_request(
        &mut socket,
        "send-1",
        method::MESSAGE_SEND,
        json!({
            "session_id": first_session_id,
            "text": "Keep running",
            "idempotency_key": "running-turn",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    assert_eq!(first_send["reused"], json!(false));

    let error = send_request_expect_error(
        &mut socket,
        "send-2",
        method::MESSAGE_SEND,
        json!({
            "session_id": second_session_id.clone(),
            "text": "Should be rejected",
            "idempotency_key": "second-running-turn",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;

    assert_eq!(error.code, ErrorCode::TurnAlreadyRunning);
    assert_eq!(error.message, "A Codex turn is already running.");
    assert_eq!(codex.start_count(), 1);
    let second_session = store
        .get_session(&second_session_id)
        .expect("second session loads");
    assert!(second_session.messages.is_empty());
    assert!(second_session.active_turn.is_none());
}

#[tokio::test]
async fn daemon_startup_recovers_persisted_active_turn_before_message_send() {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let stale_session = store
        .create_session(Some("Stale"))
        .expect("stale session created");
    let stale_turn = store
        .begin_turn(BeginTurn {
            session_id: stale_session.id.clone(),
            user_text: "stale".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "stale-key".to_owned(),
            request_hash: "stale-hash".to_owned(),
        })
        .expect("stale turn begins");
    store
        .mark_turn_running(
            &stale_turn.turn_id,
            Some("remote_thread"),
            Some("remote_turn"),
        )
        .expect("stale turn runs");

    let codex = Arc::new(RecordingCodexClient::new(
        vec![CodexEvent::Completed {
            turn_id: "fake_turn".to_owned(),
        }],
        false,
    ));
    let state = DaemonState::new(TOKEN, store.clone(), codex.clone());
    let (_runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let recovered_turn = store.get_turn(&stale_turn.turn_id).expect("turn loads");
    let stale_session_state = store
        .get_session(&stale_session.id)
        .expect("stale session loads");
    assert_eq!(
        recovered_turn.status,
        screen_sidekick_sidekick_protocol::TurnStatus::Failed
    );
    assert!(stale_session_state.active_turn.is_none());

    let session_id = initialized_session(&mut socket, &status.token).await;
    let send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "new turn after recovery",
            "idempotency_key": "after-recovery",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let _completed = read_notifications_until(&mut socket, notification::TURN_COMPLETED).await;

    assert_eq!(send_result["reused"], json!(false));
    assert_eq!(codex.start_count(), 1);
    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
}

#[tokio::test]
async fn websocket_turn_cancel_rejects_cross_session_turn_without_calling_codex() {
    let (_runtime, status, store, codex) = start_test_daemon_holding_turn_with_support(true);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let first_session_id = initialized_session(&mut socket, &status.token).await;
    let second_session_result = send_request(
        &mut socket,
        "session-2",
        method::SESSION_CREATE,
        json!({ "title": "Second" }),
    )
    .await;
    let second_session_id = second_session_result["session"]["id"]
        .as_str()
        .expect("second session id is present")
        .to_owned();
    let _ = send_request(
        &mut socket,
        "subscribe-2",
        method::SESSION_SUBSCRIBE,
        json!({ "session_id": second_session_id }),
    )
    .await;

    let send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": first_session_id.clone(),
            "text": "Keep running",
            "idempotency_key": "cancel-mismatch",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let turn_id = send_result["turn_id"]
        .as_str()
        .expect("turn id is present")
        .to_owned();
    let _started = read_notifications_until(&mut socket, notification::TURN_STARTED).await;

    let error = send_request_expect_error(
        &mut socket,
        "cancel",
        method::TURN_CANCEL,
        json!({
            "session_id": second_session_id,
            "turn_id": turn_id
        }),
    )
    .await;

    assert_eq!(error.code, ErrorCode::TurnNotFound);
    assert_eq!(codex.cancel_count(), 0);
    assert_eq!(codex.last_cancel_turn_id(), None);
    let first_session = store
        .get_session(&first_session_id)
        .expect("first session loads");
    assert!(first_session.active_turn.is_some());
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
}

#[tokio::test]
async fn websocket_turn_cancel_emits_cancelled_for_stored_turn_session() {
    let (_runtime, status, store, codex) = start_test_daemon_holding_turn_with_support(true);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let session_id = initialized_session(&mut socket, &status.token).await;
    let send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Cancel this",
            "idempotency_key": "cancel-ok",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let turn_id = send_result["turn_id"]
        .as_str()
        .expect("turn id is present")
        .to_owned();
    let _started = read_notifications_until(&mut socket, notification::TURN_STARTED).await;

    let _cancel_result = send_request(
        &mut socket,
        "cancel",
        method::TURN_CANCEL,
        json!({
            "session_id": session_id.clone(),
            "turn_id": turn_id
        }),
    )
    .await;

    let notifications = read_notifications_until(&mut socket, notification::TURN_CANCELLED).await;
    let cancelled = notifications
        .iter()
        .find(|value| {
            value.get("method").and_then(Value::as_str) == Some(notification::TURN_CANCELLED)
        })
        .expect("turn cancelled notification is emitted");

    assert_eq!(cancelled["params"]["session_id"], json!(session_id));
    assert_eq!(cancelled["params"]["turn"]["session_id"], json!(session_id));
    assert_eq!(cancelled["params"]["turn"]["status"], json!("cancelled"));
    assert_ne!(turn_id, "fake_turn");
    assert_eq!(codex.cancel_count(), 1);
    assert_eq!(codex.last_cancel_turn_id().as_deref(), Some("fake_turn"));
    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
}

#[tokio::test]
async fn websocket_turn_cancel_ignores_late_completed_stream_after_cancel() {
    let (_runtime, status, store, _codex, late_events) =
        start_test_daemon_controlled_turn_with_support(true);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let session_id = initialized_session(&mut socket, &status.token).await;
    let send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Cancel before late terminal",
            "idempotency_key": "cancel-late-completed",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let turn_id = send_result["turn_id"]
        .as_str()
        .expect("turn id is present")
        .to_owned();
    let _started = read_notifications_until(&mut socket, notification::TURN_STARTED).await;

    let _cancel_result = send_request(
        &mut socket,
        "cancel",
        method::TURN_CANCEL,
        json!({
            "session_id": session_id.clone(),
            "turn_id": turn_id.clone()
        }),
    )
    .await;
    let _cancelled = read_notifications_until(&mut socket, notification::TURN_CANCELLED).await;

    late_events
        .send(Ok(CodexEvent::Completed {
            turn_id: "fake_turn".to_owned(),
        }))
        .expect("late event is delivered");

    assert!(read_notification_with_timeout(&mut socket).await.is_none());
    let session_state = store.get_session(&session_id).expect("session loads");
    let stored_turn = store.get_turn(&turn_id).expect("turn loads");
    assert_eq!(
        stored_turn.status,
        screen_sidekick_sidekick_protocol::TurnStatus::Cancelled
    );
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != MessageRole::Assistant));
}

#[tokio::test]
async fn websocket_turn_cancel_rejects_completed_turn_without_rewriting_transcript() {
    let events = vec![
        CodexEvent::Delta {
            turn_id: "fake_turn".to_owned(),
            delta: "answer".to_owned(),
        },
        CodexEvent::Completed {
            turn_id: "fake_turn".to_owned(),
        },
    ];
    let (_runtime, status, store, codex) = start_test_daemon_with_support(events, true);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let session_id = initialized_session(&mut socket, &status.token).await;
    let send_result = send_request(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Complete this",
            "idempotency_key": "cancel-completed",
            "attachment_ids": [],
            "mode": "ask_only"
        }),
    )
    .await;
    let turn_id = send_result["turn_id"]
        .as_str()
        .expect("turn id is present")
        .to_owned();
    let _completed = read_notifications_until(&mut socket, notification::TURN_COMPLETED).await;

    let error = send_request_expect_error(
        &mut socket,
        "cancel",
        method::TURN_CANCEL,
        json!({
            "session_id": session_id.clone(),
            "turn_id": turn_id
        }),
    )
    .await;

    assert_eq!(error.code, ErrorCode::InvalidParams);
    assert_eq!(
        error.message,
        "Turn cannot be cancelled after it has finished."
    );
    assert_eq!(codex.cancel_count(), 0);
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.active_turn.is_none());
    assert!(session_state.messages.iter().any(|message| {
        message.role == MessageRole::Assistant
            && message.text == "answer"
            && message.status == screen_sidekick_sidekick_protocol::MessageStatus::Completed
    }));
}

#[tokio::test]
async fn websocket_message_send_rejects_current_context_capture_until_daemon_can_capture() {
    let (_runtime, status, store, codex) = start_test_daemon(vec![]);
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let error = send_request_expect_error(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Capture the current tab",
            "idempotency_key": "unsupported-current-context",
            "attachment_ids": [],
            "capture_current_context": true,
            "mode": "ask_only"
        }),
    )
    .await;

    assert_eq!(error.code, ErrorCode::InvalidParams);
    assert_eq!(codex.start_count(), 0);
    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.messages.is_empty());
    assert!(session_state.active_turn.is_none());
}

#[tokio::test]
async fn websocket_message_send_rejects_repo_assisted_mode_until_workspace_is_wired() {
    let (_runtime, status, store, codex) = start_test_daemon(vec![]);
    let mut socket = connect_to_daemon(&status.ws_url).await;
    let session_id = initialized_session(&mut socket, &status.token).await;

    let error = send_request_expect_error(
        &mut socket,
        "send",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Use the repo",
            "idempotency_key": "unsupported-repo-assisted",
            "attachment_ids": [],
            "workspace_binding": "/workspace/project",
            "mode": "repo_assisted"
        }),
    )
    .await;

    assert_eq!(error.code, ErrorCode::InvalidParams);
    let missing_binding_error = send_request_expect_error(
        &mut socket,
        "send-missing-binding",
        method::MESSAGE_SEND,
        json!({
            "session_id": session_id.clone(),
            "text": "Use the repo without a binding",
            "idempotency_key": "unsupported-repo-assisted-missing-binding",
            "attachment_ids": [],
            "mode": "repo_assisted"
        }),
    )
    .await;

    assert_eq!(missing_binding_error.code, ErrorCode::InvalidParams);
    assert_eq!(codex.start_count(), 0);
    let session_state = store.get_session(&session_id).expect("session loads");
    assert!(session_state.messages.is_empty());
    assert!(session_state.active_turn.is_none());
}

#[tokio::test]
async fn websocket_message_send_idempotent_retry_does_not_duplicate_work() {
    let events = vec![CodexEvent::Completed {
        turn_id: "fake_turn".to_owned(),
    }];
    let (_runtime, status, store, codex) = start_test_daemon(events);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let session_id = initialized_session(&mut socket, &status.token).await;
    let params = json!({
        "session_id": session_id,
        "text": "Run once",
        "idempotency_key": "same-message",
        "attachment_ids": [],
        "mode": "ask_only"
    });

    let first_result =
        send_request(&mut socket, "send-1", method::MESSAGE_SEND, params.clone()).await;
    assert_eq!(first_result["reused"], json!(false));
    let _first_notifications =
        read_notifications_until(&mut socket, notification::TURN_COMPLETED).await;

    let (second_result, second_notifications) = send_request_with_interleaved_notifications(
        &mut socket,
        "send-2",
        method::MESSAGE_SEND,
        params,
    )
    .await;

    assert_eq!(second_result["reused"], json!(true));
    assert!(second_notifications.is_empty());
    assert!(read_notification_with_timeout(&mut socket).await.is_none());
    assert_eq!(codex.start_count(), 1);

    let session_state = store.get_session(&session_id).expect("session loads");
    assert_eq!(
        session_state
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        1
    );
    assert_eq!(
        session_state
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::Assistant)
            .count(),
        1
    );
    assert!(session_state.active_turn.is_none());
}

#[tokio::test]
async fn websocket_message_send_idempotency_uses_defaulted_params_canonically() {
    let events = vec![CodexEvent::Completed {
        turn_id: "fake_turn".to_owned(),
    }];
    let (_runtime, status, store, codex) = start_test_daemon(events);
    let mut socket = connect_to_daemon(&status.ws_url).await;

    let session_id = initialized_session(&mut socket, &status.token).await;
    let first_params = json!({
        "session_id": session_id.clone(),
        "text": "Run once with defaults",
        "idempotency_key": "same-defaulted-message"
    });
    let second_params = json!({
        "session_id": session_id.clone(),
        "text": "Run once with defaults",
        "idempotency_key": "same-defaulted-message",
        "attachment_ids": [],
        "capture_current_context": false,
        "mode": "ask_only"
    });

    let first_result =
        send_request(&mut socket, "send-1", method::MESSAGE_SEND, first_params).await;
    assert_eq!(first_result["reused"], json!(false));
    let _first_notifications =
        read_notifications_until(&mut socket, notification::TURN_COMPLETED).await;

    let (second_result, second_notifications) = send_request_with_interleaved_notifications(
        &mut socket,
        "send-2",
        method::MESSAGE_SEND,
        second_params,
    )
    .await;

    assert_eq!(second_result["reused"], json!(true));
    assert!(second_notifications.is_empty());
    assert_eq!(codex.start_count(), 1);
    let session_state = store.get_session(&session_id).expect("session loads");
    assert_eq!(
        session_state
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .count(),
        1
    );
}

#[tokio::test]
async fn legacy_capture_accepts_extension_preflight() {
    let app = build_daemon_router(test_state(vec![]));
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/v0/capture")
        .header(ORIGIN, EXTENSION_ORIGIN)
        .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())
        .expect("request is valid");

    let response = app.oneshot(request).await.expect("router responds");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static(EXTENSION_ORIGIN))
    );
}

fn start_test_daemon(
    events: Vec<CodexEvent>,
) -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    start_test_daemon_with_support(events, false)
}

fn start_test_daemon_with_support(
    events: Vec<CodexEvent>,
    supports_turn_cancel: bool,
) -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    start_test_daemon_with_options(events, supports_turn_cancel, DaemonOptions::default())
}

fn start_test_daemon_with_options(
    events: Vec<CodexEvent>,
    supports_turn_cancel: bool,
    options: DaemonOptions,
) -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let codex = Arc::new(RecordingCodexClient::new(events, supports_turn_cancel));
    let state = DaemonState::new_with_options(TOKEN, store.clone(), codex.clone(), options);
    let (runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    (runtime, status, store, codex)
}

fn start_test_daemon_holding_turn() -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    start_test_daemon_holding_turn_with_support(false)
}

fn start_test_daemon_holding_turn_with_support(
    supports_turn_cancel: bool,
) -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let codex = Arc::new(RecordingCodexClient::new_holding_turn(supports_turn_cancel));
    let state = DaemonState::new(TOKEN, store.clone(), codex.clone());
    let (runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    (runtime, status, store, codex)
}

fn start_test_daemon_controlled_turn_with_support(
    supports_turn_cancel: bool,
) -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
    mpsc::UnboundedSender<Result<CodexEvent, CodexClientError>>,
) {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let (client, sender) = RecordingCodexClient::new_controlled_turn(supports_turn_cancel);
    let codex = Arc::new(client);
    let state = DaemonState::new(TOKEN, store.clone(), codex.clone());
    let (runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    (runtime, status, store, codex, sender)
}

fn start_test_daemon_failing_start(
    error: CodexClientError,
) -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let codex = Arc::new(RecordingCodexClient::new_failing_start(error));
    let state = DaemonState::new(TOKEN, store.clone(), codex.clone());
    let (runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    (runtime, status, store, codex)
}

fn start_test_daemon_hanging_start() -> (
    DaemonRuntime,
    screen_sidekick_sidekick_daemon::DaemonStatus,
    SessionStore,
    Arc<RecordingCodexClient>,
) {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    let codex = Arc::new(RecordingCodexClient::new_hanging_start());
    let state = DaemonState::new_with_options(
        TOKEN,
        store.clone(),
        codex.clone(),
        DaemonOptions {
            codex_start_timeout: Duration::from_millis(10),
            ..DaemonOptions::default()
        },
    );
    let (runtime, status) = DaemonRuntime::start_with_state(state).expect("daemon starts");
    (runtime, status, store, codex)
}

fn test_state(events: Vec<CodexEvent>) -> DaemonState {
    let store = SessionStore::in_memory().expect("in-memory store opens");
    DaemonState::new(
        TOKEN,
        store,
        Arc::new(RecordingCodexClient::new(events, false)),
    )
}

async fn connect_to_daemon(ws_url: &str) -> TestSocket {
    let mut request = ws_url
        .into_client_request()
        .expect("websocket request is valid");
    request
        .headers_mut()
        .insert("Origin", WsHeaderValue::from_static(EXTENSION_ORIGIN));
    let (socket, _) = connect_async(request).await.expect("websocket connects");
    socket
}

async fn send_request(socket: &mut TestSocket, id: &str, method: &str, params: Value) -> Value {
    send_request_raw(socket, id, method, params).await;
    loop {
        let value = next_json(socket).await;
        if value.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        match serde_json::from_value::<JsonRpcResponse>(value).expect("response parses") {
            JsonRpcResponse::Success(JsonRpcSuccess { result, .. }) => return result,
            JsonRpcResponse::Error(JsonRpcFailure { error, .. }) => {
                panic!("request failed unexpectedly: {error:?}");
            }
        }
    }
}

async fn send_request_expect_error(
    socket: &mut TestSocket,
    id: &str,
    method: &str,
    params: Value,
) -> ProtocolError {
    send_request_raw(socket, id, method, params).await;
    loop {
        let value = next_json(socket).await;
        if value.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        return match serde_json::from_value::<JsonRpcResponse>(value).expect("response parses") {
            JsonRpcResponse::Success(success) => {
                panic!("request succeeded unexpectedly: {success:?}");
            }
            JsonRpcResponse::Error(JsonRpcFailure { error, .. }) => error,
        };
    }
}

async fn send_request_expect_error_collecting_notifications_until(
    socket: &mut TestSocket,
    id: &str,
    method: &str,
    params: Value,
    terminal_notification_method: &str,
) -> (ProtocolError, Vec<Value>) {
    send_request_raw(socket, id, method, params).await;
    let mut notifications = Vec::new();
    let error = loop {
        let value = next_json(socket).await;
        if value.get("id").and_then(Value::as_str) != Some(id) {
            notifications.push(value);
            continue;
        }
        break match serde_json::from_value::<JsonRpcResponse>(value).expect("response parses") {
            JsonRpcResponse::Success(success) => {
                panic!("request succeeded unexpectedly: {success:?}");
            }
            JsonRpcResponse::Error(JsonRpcFailure { error, .. }) => error,
        };
    };

    if !notifications.iter().any(|value| {
        value.get("method").and_then(Value::as_str) == Some(terminal_notification_method)
    }) {
        notifications.extend(read_notifications_until(socket, terminal_notification_method).await);
    }

    (error, notifications)
}

async fn send_request_with_interleaved_notifications(
    socket: &mut TestSocket,
    id: &str,
    method: &str,
    params: Value,
) -> (Value, Vec<Value>) {
    send_request_raw(socket, id, method, params).await;
    let mut notifications = Vec::new();
    loop {
        let value = next_json(socket).await;
        if value.get("id").and_then(Value::as_str) != Some(id) {
            notifications.push(value);
            continue;
        }
        match serde_json::from_value::<JsonRpcResponse>(value).expect("response parses") {
            JsonRpcResponse::Success(JsonRpcSuccess { result, .. }) => {
                return (result, notifications);
            }
            JsonRpcResponse::Error(JsonRpcFailure { error, .. }) => {
                panic!("request failed unexpectedly: {error:?}");
            }
        }
    }
}

async fn send_request_raw(socket: &mut TestSocket, id: &str, method: &str, params: Value) {
    let request = JsonRpcRequest::new(id, method, params);
    let text = serde_json::to_string(&request).expect("request serializes");
    socket
        .send(WsMessage::Text(text.into()))
        .await
        .expect("websocket send succeeds");
}

async fn read_notifications_until(socket: &mut TestSocket, method: &str) -> Vec<Value> {
    let mut notifications = Vec::new();
    loop {
        let value = next_json(socket).await;
        if value.get("id").is_some() {
            continue;
        }
        let done = value.get("method").and_then(Value::as_str) == Some(method);
        notifications.push(value);
        if done {
            return notifications;
        }
    }
}

async fn read_notification_with_timeout(socket: &mut TestSocket) -> Option<Value> {
    match tokio::time::timeout(Duration::from_millis(100), next_json(socket)).await {
        Ok(value) if value.get("id").is_none() => Some(value),
        Ok(value) => panic!("unexpected response after request completed: {value}"),
        Err(_) => None,
    }
}

async fn expect_socket_disconnect(socket: &mut TestSocket) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match socket.next().await {
                None | Some(Err(_)) | Some(Ok(WsMessage::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .expect("websocket disconnects after subscriber lag");
}

async fn wait_for_session_without_active_turn(store: &SessionStore, session_id: &str) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let session = store.get_session(session_id).expect("session loads");
            if session.active_turn.is_none() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("session active turn clears");
}

async fn next_json(socket: &mut TestSocket) -> Value {
    loop {
        let message = socket
            .next()
            .await
            .expect("websocket remains open")
            .expect("websocket message succeeds");
        if let WsMessage::Text(text) = message {
            return serde_json::from_str(text.as_ref()).expect("message JSON parses");
        }
    }
}

async fn initialized_session(socket: &mut TestSocket, token: &str) -> String {
    let _init_result =
        send_request(socket, "init", method::INITIALIZE, initialize_params(token)).await;
    let session_result = send_request(
        socket,
        "session",
        method::SESSION_CREATE,
        json!({ "title": "Browser question" }),
    )
    .await;
    let session_id = session_result["session"]["id"]
        .as_str()
        .expect("session id is present")
        .to_owned();
    let _ = send_request(
        socket,
        "subscribe",
        method::SESSION_SUBSCRIBE,
        json!({ "session_id": session_id }),
    )
    .await;
    session_id
}

fn initialize_params(token: &str) -> Value {
    json!({
        "client_kind": "chrome_extension",
        "client_version": "test",
        "protocol_version": SIDEKICK_PROTOCOL_VERSION,
        "auth_token": token,
        "capabilities": ["browser_context", "chat_stream"],
        "extension_id": "abcdefghijklmnop",
        "origin": EXTENSION_ORIGIN
    })
}

fn capabilities_include(init_result: &Value, capability: &str) -> bool {
    init_result["capabilities"]
        .as_array()
        .expect("capabilities are present")
        .iter()
        .any(|value| value.as_str() == Some(capability))
}

fn valid_raw_context() -> Value {
    json!({
        "schema_version": RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
        "page": {
            "url": "https://example.test/admin",
            "title": "Users"
        }
    })
}

fn secret_raw_context() -> Value {
    json!({
        "schema_version": RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
        "page": {
            "url": "https://example.test/reset/sk-PATHSECRET?access_token=URLSECRET&state=keep",
            "title": "api_key=TITLESECRET"
        },
        "selected_text": "password swordfish",
        "screenshot": {
            "format": "api_key=SCREENSHOTFORMATSECRET",
            "width": 640,
            "height": 480,
            "captured_at": "password screenshotsecret"
        },
        "buttons": [{
            "text": "client secret BUTTONSECRET",
            "visible": true
        }, {
            "text": "Delete users",
            "visible": true
        }, {
            "text": "Publish changes",
            "visible": true
        }, {
            "text": "Charge card",
            "visible": true
        }],
        "inputs": [{
            "kind": "email",
            "label": "token=INPUTSECRET",
            "visible": true
        }]
    })
}

fn oversized_raw_context() -> Value {
    json!({
        "schema_version": RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
        "page": {
            "url": "https://example.test/admin",
            "title": "Users"
        },
        "selected_text": "x".repeat(MAX_ATTACHMENT_BYTES)
    })
}

fn assert_no_raw_secret_values(text: &str) {
    for raw_secret in [
        "PATHSECRET",
        "URLSECRET",
        "TITLESECRET",
        "swordfish",
        "SCREENSHOTFORMATSECRET",
        "screenshotsecret",
        "BUTTONSECRET",
        "INPUTSECRET",
    ] {
        assert!(
            !text.contains(raw_secret),
            "daemon output leaked raw secret: {raw_secret}"
        );
    }
}

struct RecordingCodexClient {
    readiness: CodexReadiness,
    events: Vec<CodexEvent>,
    start_error: Option<CodexClientError>,
    supports_turn_cancel: bool,
    last_request: Mutex<Option<StartTurnRequest>>,
    start_count: Mutex<usize>,
    cancel_count: Mutex<usize>,
    last_cancel_turn_id: Mutex<Option<String>>,
    hold_stream_open: bool,
    hang_start: bool,
    controlled_events: Mutex<Option<CodexEventStream>>,
}

impl RecordingCodexClient {
    fn new(events: Vec<CodexEvent>, supports_turn_cancel: bool) -> Self {
        Self {
            readiness: CodexReadiness {
                available: true,
                version: Some("fake-codex".to_owned()),
                error: None,
            },
            events,
            start_error: None,
            supports_turn_cancel,
            last_request: Mutex::new(None),
            start_count: Mutex::new(0),
            cancel_count: Mutex::new(0),
            last_cancel_turn_id: Mutex::new(None),
            hold_stream_open: false,
            hang_start: false,
            controlled_events: Mutex::new(None),
        }
    }

    fn new_holding_turn(supports_turn_cancel: bool) -> Self {
        Self {
            readiness: CodexReadiness {
                available: true,
                version: Some("fake-codex".to_owned()),
                error: None,
            },
            events: Vec::new(),
            start_error: None,
            supports_turn_cancel,
            last_request: Mutex::new(None),
            start_count: Mutex::new(0),
            cancel_count: Mutex::new(0),
            last_cancel_turn_id: Mutex::new(None),
            hold_stream_open: true,
            hang_start: false,
            controlled_events: Mutex::new(None),
        }
    }

    fn new_hanging_start() -> Self {
        Self {
            readiness: CodexReadiness {
                available: true,
                version: Some("fake-codex".to_owned()),
                error: None,
            },
            events: Vec::new(),
            start_error: None,
            supports_turn_cancel: false,
            last_request: Mutex::new(None),
            start_count: Mutex::new(0),
            cancel_count: Mutex::new(0),
            last_cancel_turn_id: Mutex::new(None),
            hold_stream_open: false,
            hang_start: true,
            controlled_events: Mutex::new(None),
        }
    }

    fn new_controlled_turn(
        supports_turn_cancel: bool,
    ) -> (
        Self,
        mpsc::UnboundedSender<Result<CodexEvent, CodexClientError>>,
    ) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            Self {
                readiness: CodexReadiness {
                    available: true,
                    version: Some("fake-codex".to_owned()),
                    error: None,
                },
                events: Vec::new(),
                start_error: None,
                supports_turn_cancel,
                last_request: Mutex::new(None),
                start_count: Mutex::new(0),
                cancel_count: Mutex::new(0),
                last_cancel_turn_id: Mutex::new(None),
                hold_stream_open: false,
                hang_start: false,
                controlled_events: Mutex::new(Some(controlled_event_stream(receiver))),
            },
            sender,
        )
    }

    fn new_failing_start(error: CodexClientError) -> Self {
        Self {
            readiness: CodexReadiness {
                available: false,
                version: None,
                error: Some(error.kind.clone()),
            },
            events: Vec::new(),
            start_error: Some(error),
            supports_turn_cancel: false,
            last_request: Mutex::new(None),
            start_count: Mutex::new(0),
            cancel_count: Mutex::new(0),
            last_cancel_turn_id: Mutex::new(None),
            hold_stream_open: false,
            hang_start: false,
            controlled_events: Mutex::new(None),
        }
    }

    fn last_request(&self) -> Option<StartTurnRequest> {
        self.last_request
            .lock()
            .expect("recording lock is not poisoned")
            .clone()
    }

    fn start_count(&self) -> usize {
        *self
            .start_count
            .lock()
            .expect("recording lock is not poisoned")
    }

    fn cancel_count(&self) -> usize {
        *self
            .cancel_count
            .lock()
            .expect("recording lock is not poisoned")
    }

    fn last_cancel_turn_id(&self) -> Option<String> {
        self.last_cancel_turn_id
            .lock()
            .expect("recording lock is not poisoned")
            .clone()
    }
}

#[async_trait]
impl CodexTurnClient for RecordingCodexClient {
    fn supports_turn_cancel(&self) -> bool {
        self.supports_turn_cancel
    }

    async fn readiness(&self) -> CodexReadiness {
        self.readiness.clone()
    }

    async fn start_turn(
        &self,
        request: StartTurnRequest,
    ) -> Result<StartTurnOutcome, CodexClientError> {
        *self
            .start_count
            .lock()
            .expect("recording lock is not poisoned") += 1;
        if let Some(error) = &self.start_error {
            return Err(error.clone());
        }
        if self.hang_start {
            return futures_util::future::pending::<Result<StartTurnOutcome, CodexClientError>>()
                .await;
        }
        *self
            .last_request
            .lock()
            .expect("recording lock is not poisoned") = Some(request);
        Ok(StartTurnOutcome {
            codex_thread_id: "fake_thread".to_owned(),
            codex_turn_id: Some("fake_turn".to_owned()),
            events: if let Some(events) = self
                .controlled_events
                .lock()
                .expect("recording lock is not poisoned")
                .take()
            {
                events
            } else if self.hold_stream_open {
                Box::pin(futures_util::stream::pending())
            } else {
                fake_event_stream(self.events.clone())
            },
        })
    }

    async fn cancel_turn(&self, turn_id: &str) -> Result<(), CodexClientError> {
        *self
            .cancel_count
            .lock()
            .expect("recording lock is not poisoned") += 1;
        *self
            .last_cancel_turn_id
            .lock()
            .expect("recording lock is not poisoned") = Some(turn_id.to_owned());
        if self.supports_turn_cancel {
            Ok(())
        } else {
            Err(CodexClientError::new(
                screen_sidekick_codex_client::CodexClientErrorKind::CancelUnsupported,
                "turn cancellation is not supported by this fake client",
            ))
        }
    }
}

fn fake_event_stream(events: Vec<CodexEvent>) -> CodexEventStream {
    Box::pin(futures_util::stream::iter(events.into_iter().map(Ok)))
}

fn controlled_event_stream(
    receiver: mpsc::UnboundedReceiver<Result<CodexEvent, CodexClientError>>,
) -> CodexEventStream {
    Box::pin(futures_util::stream::unfold(
        receiver,
        |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) },
    ))
}
