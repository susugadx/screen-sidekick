use screen_sidekick_capture_pipeline::{
    process_capture, CapturePipelineError, CaptureResponse, RawBrowserContext,
};
use screen_sidekick_session::CreateAttachment;
use screen_sidekick_sidekick_protocol::{
    notification, AttachBrowserContextParams, AttachBrowserContextResult, Attachment,
    AttachmentSourceType, CaptureReason, ErrorCode, ErrorData, JsonRpcRequest, ProtocolError,
    SafetyStatus,
};
use serde_json::{json, Value};

use crate::DaemonState;

use super::super::{parse_params, protocol_error, serialize_result};
use super::support::{broadcast, session_error};

pub(in crate::protocol) async fn handle_context_attach_browser(
    state: &DaemonState,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: AttachBrowserContextParams = parse_params(request)?;
    let attachment = attach_browser_context(state, params).await?;
    serialize_result(AttachBrowserContextResult { attachment })
}

async fn attach_browser_context(
    state: &DaemonState,
    params: AttachBrowserContextParams,
) -> Result<Attachment, ProtocolError> {
    ensure_raw_context_within_attachment_limit(
        &params.raw_context,
        state.limits.max_attachment_bytes,
    )?;
    let raw_context: RawBrowserContext =
        serde_json::from_value(params.raw_context).map_err(|_| {
            protocol_error(
                ErrorCode::ContextRejected,
                "Browser context payload is invalid.",
                None,
            )
        })?;
    let capture = process_capture(raw_context).map_err(capture_error)?;
    let summary = capture_summary(&capture);
    let safety_status = if capture.safety.has_danger {
        SafetyStatus::Warning
    } else {
        SafetyStatus::Clean
    };
    let attachment = state
        .store
        .create_attachment(CreateAttachment {
            session_id: params.session_id.clone(),
            message_id: params.related_message_id,
            source_type: AttachmentSourceType::BrowserTab,
            summary,
            sanitized_context_json: capture.screen_context_json,
            safety_review_json: serde_json::to_string(&capture.safety)
                .unwrap_or_else(|_| "{}".to_owned()),
            source_metadata_json: json!({
                "capture_id": params.capture_id,
                "capture_reason": params.capture_reason
            })
            .to_string(),
            safety_status,
            debug_available: matches!(
                params.capture_reason,
                CaptureReason::Debug | CaptureReason::ManualAttach
            ),
        })
        .map_err(session_error)?;
    broadcast(
        &state.events,
        notification::CONTEXT_ATTACHED,
        json!(
            screen_sidekick_sidekick_protocol::ContextAttachedNotification {
                session_id: params.session_id,
                attachment: attachment.clone()
            }
        ),
    );
    Ok(attachment)
}

fn ensure_raw_context_within_attachment_limit(
    raw_context: &Value,
    max_attachment_bytes: usize,
) -> Result<(), ProtocolError> {
    let size = serde_json::to_vec(raw_context)
        .map_err(|_| {
            protocol_error(
                ErrorCode::ContextRejected,
                "Browser context payload is invalid.",
                None,
            )
        })?
        .len();
    if size > max_attachment_bytes {
        return Err(protocol_error(
            ErrorCode::ContextTooLarge,
            "Browser context payload exceeds the attachment size limit.",
            Some(ErrorData {
                max_size_bytes: Some(max_attachment_bytes),
                ..ErrorData::default()
            }),
        ));
    }
    Ok(())
}

fn capture_error(error: CapturePipelineError) -> ProtocolError {
    match error {
        CapturePipelineError::UnsupportedSchemaVersion => protocol_error(
            ErrorCode::ContextRejected,
            "Browser context version is unsupported.",
            None,
        ),
        CapturePipelineError::SerializeSanitizedContext(_) => protocol_error(
            ErrorCode::SafetyReviewFailed,
            "Safety review serialization failed.",
            None,
        ),
    }
}

fn capture_summary(capture: &CaptureResponse) -> String {
    serde_json::from_str::<Value>(&capture.screen_context_json)
        .ok()
        .and_then(|value| {
            value["page"]["url"]
                .as_str()
                .or_else(|| value["page"]["title"].as_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "Browser context".to_owned())
}
