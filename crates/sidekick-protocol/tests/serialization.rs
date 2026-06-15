use screen_sidekick_sidekick_protocol::{
    method, notification, ClientCapability, ClientKind, ErrorCode, ErrorData, InitializeParams,
    JsonRpcFailure, JsonRpcNotification, JsonRpcRequest, JsonRpcSuccess,
    MessageSendIdempotencyDisposition, ProtocolError, Turn, TurnFailedNotification, TurnStatus,
    SIDEKICK_PROTOCOL_VERSION,
};
use serde_json::json;

#[test]
fn request_envelope_serializes_with_jsonrpc_version() {
    let request = JsonRpcRequest::new(
        "req_1",
        method::INITIALIZE,
        serde_json::to_value(InitializeParams {
            client_kind: ClientKind::ChromeExtension,
            client_version: "0.0.0".to_owned(),
            protocol_version: SIDEKICK_PROTOCOL_VERSION.to_owned(),
            auth_token: Some("test-token".to_owned()),
            capabilities: vec![
                ClientCapability::BrowserContext,
                ClientCapability::ChatStream,
            ],
            extension_id: Some("extension-id".to_owned()),
            origin: Some("chrome-extension://extension-id".to_owned()),
        })
        .expect("params serialize"),
    );

    let value = serde_json::to_value(request).expect("request serializes");

    assert_eq!(value["jsonrpc"], json!("2.0"));
    assert_eq!(value["id"], json!("req_1"));
    assert_eq!(value["method"], json!("initialize"));
    assert_eq!(value["params"]["client_kind"], json!("chrome_extension"));
}

#[test]
fn success_response_serializes() {
    let response = JsonRpcSuccess::new("req_1", json!({ "ok": true }));

    assert_eq!(
        serde_json::to_value(response).expect("response serializes"),
        json!({
            "jsonrpc": "2.0",
            "id": "req_1",
            "result": { "ok": true }
        })
    );
}

#[test]
fn error_response_uses_stable_error_code() {
    let response = JsonRpcFailure::new(
        "req_1",
        ProtocolError::new(ErrorCode::SessionNotFound, "Session was not found.").with_data(
            ErrorData {
                retryable: Some(false),
                ..ErrorData::default()
            },
        ),
    );

    let value = serde_json::to_value(response).expect("response serializes");

    assert_eq!(value["error"]["code"], json!("session_not_found"));
    assert_eq!(value["error"]["data"]["retryable"], json!(false));
}

#[test]
fn error_response_serializes_message_send_idempotency_disposition() {
    let response = JsonRpcFailure::new(
        "req_1",
        ProtocolError::new(
            ErrorCode::CodexNotFound,
            "Previous message/send attempt failed.",
        )
        .with_data(ErrorData {
            message_send_idempotency_disposition: Some(MessageSendIdempotencyDisposition::Discard),
            ..ErrorData::default()
        }),
    );

    let value = serde_json::to_value(response).expect("response serializes");

    assert_eq!(
        value["error"]["data"]["message_send_idempotency_disposition"],
        json!("discard")
    );
}

#[test]
fn notification_has_no_id() {
    let notification = JsonRpcNotification::new(
        notification::TURN_DELTA,
        json!({
            "session_id": "sess_1",
            "turn_id": "turn_1",
            "delta": "hello"
        }),
    );

    let value = serde_json::to_value(notification).expect("notification serializes");

    assert_eq!(value["jsonrpc"], json!("2.0"));
    assert_eq!(value["method"], json!("turn/delta"));
    assert!(value.get("id").is_none());
}

#[test]
fn turn_failed_notification_can_carry_optional_message() {
    let notification = TurnFailedNotification {
        session_id: "sess_1".to_owned(),
        turn: Turn {
            id: "turn_1".to_owned(),
            session_id: "sess_1".to_owned(),
            user_message_id: "msg_1".to_owned(),
            assistant_message_id: None,
            status: TurnStatus::Failed,
            started_at: "1".to_owned(),
            completed_at: Some("2".to_owned()),
            error: Some(ProtocolError::new(
                ErrorCode::CodexTurnFailed,
                "Turn failed.",
            )),
        },
        message: Some("model failed".to_owned()),
    };

    let value = serde_json::to_value(notification).expect("notification serializes");

    assert_eq!(value["message"], json!("model failed"));
    assert_eq!(value["turn"]["status"], json!("failed"));
}

#[test]
fn error_data_does_not_require_raw_payload_fields() {
    let value = serde_json::to_value(ProtocolError::new(
        ErrorCode::CodexNotLoggedIn,
        "Codex login is required.",
    ))
    .expect("error serializes");

    let text = serde_json::to_string(&value).expect("error JSON serializes");
    assert!(!text.contains("token=SECRET"));
    assert!(value.get("data").is_none());
}
