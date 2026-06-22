use futures_util::StreamExt;
use screen_sidekick_codex_client::{CodexClientErrorKind, CodexEvent};
use screen_sidekick_session::SessionStoreError;
use screen_sidekick_sidekick_protocol::{
    notification, ErrorCode, MessageCreatedNotification, TurnDeltaNotification,
    TurnFailedNotification, TurnNotification,
};
use serde_json::json;

use crate::DaemonState;

use super::super::support::{broadcast, broadcast_error, session_error};

pub(super) fn spawn_turn_stream(
    state: DaemonState,
    session_id: String,
    local_turn_id: String,
    mut events: screen_sidekick_codex_client::CodexEventStream,
) {
    tokio::spawn(async move {
        let mut assistant_delta_text = String::new();
        let mut final_assistant_text: Option<String> = None;
        while let Some(event) = events.next().await {
            match event {
                Ok(CodexEvent::TurnStarted { .. }) => {}
                Ok(CodexEvent::Delta { delta, .. }) => {
                    match state.store.turn_is_active(&local_turn_id) {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(error) => {
                            broadcast_error(&state.events, session_error(error));
                            return;
                        }
                    }
                    assistant_delta_text.push_str(&delta);
                    broadcast(
                        &state.events,
                        notification::TURN_DELTA,
                        json!(TurnDeltaNotification {
                            session_id: session_id.clone(),
                            turn_id: local_turn_id.clone(),
                            delta
                        }),
                    );
                }
                Ok(CodexEvent::FinalAssistantMessage { text, .. }) => {
                    match state.store.turn_is_active(&local_turn_id) {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(error) => {
                            broadcast_error(&state.events, session_error(error));
                            return;
                        }
                    }
                    final_assistant_text = Some(text);
                }
                Ok(CodexEvent::Completed {
                    final_assistant_text: completed_assistant_text,
                    ..
                }) => {
                    if completed_assistant_text.is_some() {
                        final_assistant_text = completed_assistant_text;
                    }
                    let assistant_text = final_assistant_text
                        .as_deref()
                        .unwrap_or(&assistant_delta_text);
                    match state.store.complete_turn(&local_turn_id, assistant_text) {
                        Ok((turn, message)) => {
                            broadcast(
                                &state.events,
                                notification::MESSAGE_CREATED,
                                json!(MessageCreatedNotification {
                                    session_id: session_id.clone(),
                                    message
                                }),
                            );
                            broadcast(
                                &state.events,
                                notification::TURN_COMPLETED,
                                json!(TurnNotification {
                                    session_id: session_id.clone(),
                                    turn
                                }),
                            );
                        }
                        Err(SessionStoreError::TurnNotCancellable) => {}
                        Err(error) => broadcast_error(&state.events, session_error(error)),
                    }
                    return;
                }
                Ok(CodexEvent::Failed {
                    message,
                    error_kind,
                    ..
                }) => {
                    fail_streaming_turn(
                        &state,
                        &session_id,
                        &local_turn_id,
                        streaming_failure_error_code(error_kind),
                        message,
                    );
                    return;
                }
                Ok(CodexEvent::Unknown { method }) => {
                    #[cfg(debug_assertions)]
                    eprintln!("Screen Sidekick ignored unknown Codex app-server event: {method}");
                    #[cfg(not(debug_assertions))]
                    drop(method);
                }
                Err(error) => {
                    let error_code = error.to_sidekick_error_code();
                    fail_streaming_turn(
                        &state,
                        &session_id,
                        &local_turn_id,
                        error_code,
                        error.message,
                    );
                    return;
                }
            }
        }
        fail_streaming_turn(
            &state,
            &session_id,
            &local_turn_id,
            ErrorCode::CodexAppServerUnavailable,
            "Codex stream ended before completion.".to_owned(),
        );
    });
}

fn fail_streaming_turn(
    state: &DaemonState,
    session_id: &str,
    turn_id: &str,
    error_code: ErrorCode,
    message: String,
) {
    match state
        .store
        .fail_turn(turn_id, error_code, Some("codex_stream"))
    {
        Ok(turn) => {
            broadcast(
                &state.events,
                notification::TURN_FAILED,
                json!(TurnFailedNotification {
                    session_id: turn.session_id.clone(),
                    turn,
                    message: Some(message),
                }),
            );
        }
        Err(SessionStoreError::TurnNotCancellable) => {}
        Err(SessionStoreError::TurnNotFound) => {
            broadcast(
                &state.events,
                notification::TURN_FAILED,
                json!({ "session_id": session_id, "message": message }),
            );
        }
        Err(error) => broadcast_error(&state.events, session_error(error)),
    }
}

fn streaming_failure_error_code(error_kind: Option<CodexClientErrorKind>) -> ErrorCode {
    match error_kind {
        Some(CodexClientErrorKind::NotLoggedIn) => ErrorCode::CodexNotLoggedIn,
        _ => ErrorCode::CodexTurnFailed,
    }
}
