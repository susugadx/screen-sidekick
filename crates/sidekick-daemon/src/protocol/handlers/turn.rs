use screen_sidekick_codex_client::{schema_hash, StartTurnRequest};
use screen_sidekick_session::BeginTurn;
use screen_sidekick_sidekick_protocol::{
    notification, ErrorCode, JsonRpcRequest, MessageCreatedNotification, MessageMode,
    MessageSendParams, MessageSendResult, ProtocolError, TurnCancelParams, TurnNotification,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::DaemonState;

use super::super::{parse_params, protocol_error, serialize_result};
use super::support::{broadcast, codex_error, session_error};

mod context_text;
mod failure;
mod start;
mod stream;

use context_text::load_context_text;
use failure::fail_starting_turn;
use start::start_codex_turn_with_stale_thread_recovery;
use stream::spawn_turn_stream;

pub(in crate::protocol) async fn handle_message_send(
    state: &DaemonState,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: MessageSendParams = parse_params(request)?;
    validate_message_send_supported(&params)?;
    let request_hash = message_send_request_hash(&params);
    let turn = state
        .store
        .begin_turn(BeginTurn {
            session_id: params.session_id.clone(),
            user_text: params.text.clone(),
            attachment_ids: params.attachment_ids.clone(),
            idempotency_key: params.idempotency_key,
            request_hash,
        })
        .map_err(session_error)?;
    if turn.reused {
        return serialize_result(MessageSendResult {
            message_id: turn.message_id,
            turn_id: turn.turn_id,
            reused: true,
        });
    }

    let session_state = state
        .store
        .get_session(&params.session_id)
        .map_err(session_error)?;
    if let Some(message) = session_state
        .messages
        .iter()
        .find(|message| message.id == turn.message_id)
        .cloned()
    {
        broadcast(
            &state.events,
            notification::MESSAGE_CREATED,
            json!(MessageCreatedNotification {
                session_id: params.session_id.clone(),
                message
            }),
        );
    }

    let context_text = match load_context_text(&state.store, &params.attachment_ids) {
        Ok(context_text) => context_text,
        Err(error) => {
            fail_starting_turn(
                state,
                &turn.turn_id,
                error.code,
                "context_load",
                error.message.clone(),
            );
            return Err(error);
        }
    };
    let existing_thread_id = state
        .store
        .codex_thread_id(&params.session_id)
        .map_err(session_error)?;
    let codex_request = StartTurnRequest {
        session_id: params.session_id.clone(),
        codex_thread_id: existing_thread_id.clone(),
        user_message_id: turn.message_id.clone(),
        user_text: params.text,
        context_text,
    };
    let outcome = start_codex_turn_with_stale_thread_recovery(
        state,
        &params.session_id,
        &turn.turn_id,
        codex_request,
        existing_thread_id,
    )
    .await?;
    state
        .store
        .link_codex_thread(
            &params.session_id,
            &outcome.codex_thread_id,
            None,
            schema_hash().as_deref(),
        )
        .map_err(session_error)?;
    let running = state
        .store
        .mark_turn_running(
            &turn.turn_id,
            Some(&outcome.codex_thread_id),
            outcome.codex_turn_id.as_deref(),
        )
        .map_err(session_error)?;
    broadcast(
        &state.events,
        notification::TURN_STARTED,
        json!(TurnNotification {
            session_id: params.session_id.clone(),
            turn: running
        }),
    );
    spawn_turn_stream(
        state.clone(),
        params.session_id.clone(),
        turn.turn_id.clone(),
        outcome.events,
    );
    serialize_result(MessageSendResult {
        message_id: turn.message_id,
        turn_id: turn.turn_id,
        reused: false,
    })
}

fn validate_message_send_supported(params: &MessageSendParams) -> Result<(), ProtocolError> {
    if params.capture_current_context {
        return Err(protocol_error(
            ErrorCode::InvalidParams,
            "capture_current_context is not supported by the daemon yet; attach browser context first.",
            None,
        ));
    }
    if matches!(params.mode, MessageMode::RepoAssisted) {
        return Err(protocol_error(
            ErrorCode::InvalidParams,
            "repo_assisted mode is not supported by the daemon yet.",
            None,
        ));
    }
    Ok(())
}

pub(in crate::protocol) async fn handle_turn_cancel(
    state: &DaemonState,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: TurnCancelParams = parse_params(request)?;
    let cancellation_target = state
        .store
        .get_cancellable_turn_for_session(&params.session_id, &params.turn_id)
        .map_err(session_error)?;
    state
        .codex
        .cancel_turn(&cancellation_target.codex_turn_id)
        .await
        .map_err(codex_error)?;
    let turn = state
        .store
        .cancel_turn_for_session(&params.session_id, &params.turn_id)
        .map_err(session_error)?;
    broadcast(
        &state.events,
        notification::TURN_CANCELLED,
        json!(TurnNotification {
            session_id: turn.session_id.clone(),
            turn
        }),
    );
    serialize_result(json!({}))
}

fn hash_value(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn message_send_request_hash(params: &MessageSendParams) -> String {
    hash_value(&json!({
        "session_id": params.session_id,
        "text": params.text,
        "attachment_ids": params.attachment_ids,
        "capture_current_context": params.capture_current_context,
        "workspace_binding": params.workspace_binding,
        "mode": params.mode,
    }))
}
