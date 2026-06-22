use screen_sidekick_codex_client::CodexClientError;
use screen_sidekick_session::SessionStoreError;
use screen_sidekick_sidekick_protocol::{
    notification, ErrorCode, ErrorData, JsonRpcNotification, MessageSendIdempotencyDisposition,
    ProtocolError,
};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::super::protocol_error;

pub(in crate::protocol::handlers) fn session_error(error: SessionStoreError) -> ProtocolError {
    match error {
        SessionStoreError::SessionNotFound => {
            protocol_error(ErrorCode::SessionNotFound, "Session was not found.", None)
        }
        SessionStoreError::MessageNotFound => {
            protocol_error(ErrorCode::MessageNotFound, "Message was not found.", None)
        }
        SessionStoreError::AttachmentNotFound(_) => protocol_error(
            ErrorCode::AttachmentNotFound,
            "Attachment was not found.",
            None,
        ),
        SessionStoreError::AttachmentAlreadyLinked(_) => protocol_error(
            ErrorCode::InvalidParams,
            "Attachment is already linked to another message.",
            None,
        ),
        SessionStoreError::TurnNotFound => {
            protocol_error(ErrorCode::TurnNotFound, "Turn was not found.", None)
        }
        SessionStoreError::TurnNotCancellable => protocol_error(
            ErrorCode::InvalidParams,
            "Turn cannot be cancelled after it has finished.",
            None,
        ),
        SessionStoreError::TurnCancelTargetMissing => protocol_error(
            ErrorCode::TurnCancelUnsupported,
            "Codex turn id is unavailable for cancellation.",
            None,
        ),
        SessionStoreError::TurnAlreadyRunning => protocol_error(
            ErrorCode::TurnAlreadyRunning,
            "A Codex turn is already running.",
            Some(ErrorData {
                retryable: Some(true),
                ..ErrorData::default()
            }),
        ),
        SessionStoreError::IdempotencyConflict => protocol_error(
            ErrorCode::InvalidParams,
            "Idempotency key was reused with a different request.",
            None,
        ),
        SessionStoreError::IdempotencyFailed(code) => {
            message_send_idempotency_discard_error(code, "Previous message/send attempt failed.")
        }
        SessionStoreError::IdempotencyCancelled => message_send_idempotency_discard_error(
            ErrorCode::InvalidParams,
            "Previous message/send attempt was cancelled.",
        ),
        SessionStoreError::Sqlite(_) | SessionStoreError::LockPoisoned => {
            protocol_error(ErrorCode::InternalError, "Session storage failed.", None)
        }
    }
}

fn message_send_idempotency_discard_error(code: ErrorCode, message: &'static str) -> ProtocolError {
    protocol_error(
        code,
        message,
        Some(ErrorData {
            message_send_idempotency_disposition: Some(MessageSendIdempotencyDisposition::Discard),
            ..ErrorData::default()
        }),
    )
}

pub(in crate::protocol::handlers) fn codex_error(error: CodexClientError) -> ProtocolError {
    protocol_error(error.to_sidekick_error_code(), &error.message, None)
}

pub(in crate::protocol::handlers) fn broadcast(
    sender: &broadcast::Sender<JsonRpcNotification>,
    method: &str,
    params: Value,
) {
    let _ = sender.send(JsonRpcNotification::new(method, params));
}

pub(in crate::protocol::handlers) fn broadcast_error(
    sender: &broadcast::Sender<JsonRpcNotification>,
    error: ProtocolError,
) {
    broadcast(sender, notification::ERROR, json!(error));
}
