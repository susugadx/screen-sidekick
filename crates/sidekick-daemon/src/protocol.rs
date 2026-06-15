use std::collections::HashSet;

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use screen_sidekick_capture_pipeline::{
    process_capture, CapturePipelineError, CaptureResponse, RawBrowserContext,
};
use screen_sidekick_codex_client::{
    schema_hash, CodexClientError, CodexClientErrorKind, CodexEvent, StartTurnOutcome,
    StartTurnRequest,
};
use screen_sidekick_session::{BeginTurn, CreateAttachment, SessionStore, SessionStoreError};
use screen_sidekick_sidekick_protocol::{
    method, notification, AttachBrowserContextParams, AttachBrowserContextResult, Attachment,
    AttachmentSourceType, AuthStatus, CaptureReason, ClientCapability, CodexReadiness, ErrorCode,
    ErrorData, InitializeParams, InitializeResult, JsonRpcFailure, JsonRpcNotification,
    JsonRpcRequest, JsonRpcSuccess, MessageCreatedNotification, MessageMode, MessageSendParams,
    MessageSendResult, ProtocolError, SafetyStatus, SessionCreateParams, SessionCreateResult,
    SessionIdParams, SessionListResult, StatusGetResult, TurnCancelParams, TurnDeltaNotification,
    TurnFailedNotification, TurnNotification, SIDEKICK_PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{sync::broadcast, time::timeout};

use crate::DaemonState;

pub(crate) async fn websocket_loop(socket: WebSocket, state: DaemonState) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = state.events.subscribe();
    let mut shutdown = state.websocket_shutdown.subscribe();
    let mut subscribed_sessions = HashSet::<String>::new();
    let mut initialized = false;

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(incoming) = incoming else { break; };
                let Ok(message) = incoming else { break; };
                match message {
                    WsMessage::Text(text) => {
                        let Some(response) = handle_ws_text(&state, &mut initialized, &mut subscribed_sessions, &text).await else {
                            continue;
                        };
                        if sender.send(WsMessage::Text(response.into())).await.is_err() {
                            break;
                        }
                    }
                    WsMessage::Close(_) => break,
                    WsMessage::Ping(bytes) => {
                        if sender.send(WsMessage::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    WsMessage::Binary(_) | WsMessage::Pong(_) => {}
                }
            }
            notification = events.recv() => {
                match notification {
                    Ok(notification) => {
                        if initialized && notification_is_visible(&notification, &subscribed_sessions) {
                            let Ok(text) = serde_json::to_string(&notification) else { continue; };
                            if sender.send(WsMessage::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_))
                    | Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = shutdown.recv() => break,
        }
    }
}

async fn handle_ws_text(
    state: &DaemonState,
    initialized: &mut bool,
    subscribed_sessions: &mut HashSet<String>,
    text: &str,
) -> Option<String> {
    let request = match serde_json::from_str::<JsonRpcRequest>(text) {
        Ok(request) => request,
        Err(_) => {
            return Some(error_response_text(
                "unknown",
                ErrorCode::InvalidRequest,
                "Request JSON is invalid.",
                None,
            ));
        }
    };

    if !*initialized && request.method != method::INITIALIZE {
        return Some(error_response_text(
            &request.id,
            ErrorCode::Unauthorized,
            "Initialize is required.",
            None,
        ));
    }

    let response = match request.method.as_str() {
        method::INITIALIZE => handle_initialize(state, initialized, &request).await,
        method::SESSION_CREATE => handle_session_create(state, &request).await,
        method::SESSION_LIST => handle_session_list(state, &request).await,
        method::SESSION_GET => handle_session_get(state, &request).await,
        method::SESSION_SUBSCRIBE => {
            handle_session_subscribe(state, subscribed_sessions, &request).await
        }
        method::SESSION_UNSUBSCRIBE => {
            handle_session_unsubscribe(subscribed_sessions, &request).await
        }
        method::CONTEXT_ATTACH_BROWSER => handle_context_attach_browser(state, &request).await,
        method::MESSAGE_SEND => handle_message_send(state, &request).await,
        method::TURN_CANCEL => handle_turn_cancel(state, &request).await,
        method::STATUS_GET => handle_status_get(state, &request).await,
        _ => Err(protocol_error(
            ErrorCode::MethodNotFound,
            "Method was not found.",
            None,
        )),
    };

    Some(match response {
        Ok(result) => success_response_text(&request.id, result),
        Err(error) => error_response_text(&request.id, error.code, error.message, error.data),
    })
}

async fn handle_initialize(
    state: &DaemonState,
    initialized: &mut bool,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: InitializeParams = parse_params(request)?;
    if params.protocol_version != SIDEKICK_PROTOCOL_VERSION {
        return Err(protocol_error(
            ErrorCode::UnsupportedProtocolVersion,
            "Sidekick protocol version is not supported.",
            Some(ErrorData {
                supported_versions: Some(vec![SIDEKICK_PROTOCOL_VERSION.to_owned()]),
                ..ErrorData::default()
            }),
        ));
    }
    if params.auth_token.as_deref() != Some(state.token()) {
        return Err(protocol_error(
            ErrorCode::Unauthorized,
            "Pairing token is invalid.",
            Some(ErrorData {
                retryable: Some(false),
                ..ErrorData::default()
            }),
        ));
    }
    *initialized = true;
    let readiness = state.codex.readiness().await;
    let mut capabilities = vec![
        ClientCapability::BrowserContext,
        ClientCapability::ChatStream,
        ClientCapability::DebugExport,
    ];
    if state.codex.supports_turn_cancel() {
        capabilities.push(ClientCapability::TurnCancel);
    }
    serialize_result(InitializeResult {
        protocol_version: SIDEKICK_PROTOCOL_VERSION.to_owned(),
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        capabilities,
        auth_status: AuthStatus::Ready,
        codex_readiness: codex_readiness_to_protocol(readiness),
        limits: state.limits,
        warnings: Vec::new(),
    })
}

async fn handle_session_create(
    state: &DaemonState,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: SessionCreateParams = parse_params(request)?;
    let session = state
        .store
        .create_session(params.title.as_deref())
        .map_err(session_error)?;
    broadcast(
        &state.events,
        notification::SESSION_UPDATED,
        json!({ "session": session }),
    );
    serialize_result(SessionCreateResult { session })
}

async fn handle_session_list(
    state: &DaemonState,
    _request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let sessions = state.store.list_sessions().map_err(session_error)?;
    serialize_result(SessionListResult { sessions })
}

async fn handle_session_get(
    state: &DaemonState,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: SessionIdParams = parse_params(request)?;
    let session = state
        .store
        .get_session(&params.session_id)
        .map_err(session_error)?;
    serialize_result(session)
}

async fn handle_session_subscribe(
    state: &DaemonState,
    subscribed_sessions: &mut HashSet<String>,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: SessionIdParams = parse_params(request)?;
    let session = state
        .store
        .get_session(&params.session_id)
        .map_err(session_error)?;
    subscribed_sessions.insert(params.session_id);
    serialize_result(session)
}

async fn handle_session_unsubscribe(
    subscribed_sessions: &mut HashSet<String>,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: SessionIdParams = parse_params(request)?;
    subscribed_sessions.remove(&params.session_id);
    serialize_result(json!({}))
}

async fn handle_context_attach_browser(
    state: &DaemonState,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: AttachBrowserContextParams = parse_params(request)?;
    let attachment = attach_browser_context(state, params).await?;
    serialize_result(AttachBrowserContextResult { attachment })
}

async fn handle_message_send(
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

async fn start_codex_turn_with_stale_thread_recovery(
    state: &DaemonState,
    session_id: &str,
    turn_id: &str,
    request: StartTurnRequest,
    existing_thread_id: Option<String>,
) -> Result<StartTurnOutcome, ProtocolError> {
    match start_codex_turn_once(state, request.clone()).await {
        Ok(outcome) => Ok(outcome),
        Err(StartCodexTurnError::Client(error))
            if error.kind == CodexClientErrorKind::ThreadNotFound =>
        {
            let Some(stale_thread_id) = existing_thread_id else {
                return Err(fail_codex_start_attempt(
                    state,
                    turn_id,
                    StartCodexTurnError::Client(error),
                    "codex_start_turn",
                    "codex_start_timeout",
                ));
            };
            if let Err(error) = state
                .store
                .clear_codex_thread_link(session_id, &stale_thread_id)
            {
                let message =
                    "Session storage failed while clearing stale Codex thread link.".to_owned();
                fail_starting_turn(
                    state,
                    turn_id,
                    ErrorCode::InternalError,
                    "codex_thread_link_clear",
                    message,
                );
                return Err(session_error(error));
            }

            let mut retry_request = request;
            retry_request.codex_thread_id = None;
            match start_codex_turn_once(state, retry_request).await {
                Ok(outcome) => Ok(outcome),
                Err(error) => Err(fail_codex_start_attempt(
                    state,
                    turn_id,
                    error,
                    "codex_start_turn_retry",
                    "codex_start_retry_timeout",
                )),
            }
        }
        Err(error) => Err(fail_codex_start_attempt(
            state,
            turn_id,
            error,
            "codex_start_turn",
            "codex_start_timeout",
        )),
    }
}

async fn start_codex_turn_once(
    state: &DaemonState,
    request: StartTurnRequest,
) -> Result<StartTurnOutcome, StartCodexTurnError> {
    match timeout(state.codex_start_timeout, state.codex.start_turn(request)).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(error)) => Err(StartCodexTurnError::Client(error)),
        Err(_) => Err(StartCodexTurnError::Timeout),
    }
}

enum StartCodexTurnError {
    Client(CodexClientError),
    Timeout,
}

fn fail_codex_start_attempt(
    state: &DaemonState,
    turn_id: &str,
    error: StartCodexTurnError,
    error_debug_id: &str,
    timeout_debug_id: &str,
) -> ProtocolError {
    match error {
        StartCodexTurnError::Client(error) => {
            let error_code = error.to_sidekick_error_code();
            let message = error.message.clone();
            let response_error = codex_error(error);
            fail_starting_turn(state, turn_id, error_code, error_debug_id, message);
            response_error
        }
        StartCodexTurnError::Timeout => {
            let message = "Codex turn startup timed out.".to_owned();
            fail_starting_turn(
                state,
                turn_id,
                ErrorCode::CodexAppServerUnavailable,
                timeout_debug_id,
                message.clone(),
            );
            protocol_error(ErrorCode::CodexAppServerUnavailable, &message, None)
        }
    }
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

async fn handle_turn_cancel(
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

async fn handle_status_get(
    state: &DaemonState,
    _request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let readiness = state.codex.readiness().await;
    serialize_result(StatusGetResult {
        codex_readiness: codex_readiness_to_protocol(readiness),
    })
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

fn spawn_turn_stream(
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

fn fail_starting_turn(
    state: &DaemonState,
    turn_id: &str,
    error_code: ErrorCode,
    debug_id: &str,
    message: String,
) {
    match state.store.fail_turn(turn_id, error_code, Some(debug_id)) {
        Ok(failed_turn) => {
            broadcast(
                &state.events,
                notification::TURN_FAILED,
                json!(TurnFailedNotification {
                    session_id: failed_turn.session_id.clone(),
                    turn: failed_turn,
                    message: Some(message),
                }),
            );
        }
        Err(error) => broadcast_error(&state.events, session_error(error)),
    }
}

fn load_context_text(
    store: &SessionStore,
    attachment_ids: &[String],
) -> Result<String, ProtocolError> {
    let mut parts = Vec::new();
    for attachment_id in attachment_ids {
        let context = store
            .attachment_codex_context(attachment_id)
            .map_err(session_error)?;
        parts.push(format_attachment_context(context)?);
    }
    Ok(parts.join("\n\n"))
}

fn format_attachment_context(
    context: screen_sidekick_session::AttachmentCodexContext,
) -> Result<String, ProtocolError> {
    Ok(format!(
        "Safety review:\n{}\n\nScreenContext JSON:\n{}",
        format_safety_review(&context.safety_review_json)?,
        context.sanitized_context_json
    ))
}

fn format_safety_review(safety_review_json: &str) -> Result<String, ProtocolError> {
    let review: StoredSafetyReview = serde_json::from_str(safety_review_json).map_err(|_| {
        protocol_error(
            ErrorCode::SafetyReviewFailed,
            "Stored safety review is invalid.",
            None,
        )
    })?;
    let mut lines = vec![
        format!("has_danger: {}", review.has_danger),
        format!("warning_count: {}", review.warning_count),
        format!("masked_input_values: {}", review.masked_input_values),
        format!("masked_secret_texts: {}", review.masked_secret_texts),
    ];
    if review.warnings.is_empty() {
        lines.push("warnings: none".to_owned());
    } else {
        for warning in review.warnings {
            lines.push(format!(
                "warning: category={} category_label={} source={} source_label={}",
                quote_for_prompt(&warning.category),
                quote_for_prompt(&warning.category_label),
                quote_for_prompt(&warning.source),
                quote_for_prompt(&warning.source_label)
            ));
        }
    }
    Ok(lines.join("\n"))
}

fn quote_for_prompt(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn success_response_text(id: &str, result: Value) -> String {
    serde_json::to_string(&JsonRpcSuccess::new(id, result))
        .unwrap_or_else(|_| "{\"jsonrpc\":\"2.0\",\"id\":\"unknown\",\"result\":{}}".to_owned())
}

fn error_response_text(
    id: &str,
    code: ErrorCode,
    message: impl Into<String>,
    data: Option<Box<ErrorData>>,
) -> String {
    let error = ProtocolError {
        code,
        message: message.into(),
        data,
    };
    serde_json::to_string(&JsonRpcFailure::new(id, error)).unwrap_or_else(|_| {
        "{\"jsonrpc\":\"2.0\",\"id\":\"unknown\",\"error\":{\"code\":\"internal_error\",\"message\":\"internal error\"}}".to_owned()
    })
}

fn parse_params<T: serde::de::DeserializeOwned>(
    request: &JsonRpcRequest,
) -> Result<T, ProtocolError> {
    serde_json::from_value(request.params.clone()).map_err(|_| {
        protocol_error(
            ErrorCode::InvalidParams,
            "Request params are invalid.",
            None,
        )
    })
}

fn serialize_result<T: Serialize>(value: T) -> Result<Value, ProtocolError> {
    serde_json::to_value(value).map_err(|_| {
        protocol_error(
            ErrorCode::InternalError,
            "Response serialization failed.",
            None,
        )
    })
}

fn protocol_error(code: ErrorCode, message: &str, data: Option<ErrorData>) -> ProtocolError {
    ProtocolError {
        code,
        message: message.to_owned(),
        data: data.map(Box::new),
    }
}

fn session_error(error: SessionStoreError) -> ProtocolError {
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
            protocol_error(code, "Previous message/send attempt failed.", None)
        }
        SessionStoreError::IdempotencyCancelled => protocol_error(
            ErrorCode::InvalidParams,
            "Previous message/send attempt was cancelled.",
            None,
        ),
        SessionStoreError::Sqlite(_) | SessionStoreError::LockPoisoned => {
            protocol_error(ErrorCode::InternalError, "Session storage failed.", None)
        }
    }
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

fn codex_error(error: CodexClientError) -> ProtocolError {
    protocol_error(error.to_sidekick_error_code(), &error.message, None)
}

fn codex_readiness_to_protocol(
    readiness: screen_sidekick_codex_client::CodexReadiness,
) -> CodexReadiness {
    CodexReadiness {
        available: readiness.available,
        version: readiness.version,
        error_code: readiness.error.map(|kind| match kind {
            screen_sidekick_codex_client::CodexClientErrorKind::CodexNotFound => {
                ErrorCode::CodexNotFound
            }
            screen_sidekick_codex_client::CodexClientErrorKind::NotLoggedIn => {
                ErrorCode::CodexNotLoggedIn
            }
            screen_sidekick_codex_client::CodexClientErrorKind::UnsupportedVersion => {
                ErrorCode::UnsupportedCodexVersion
            }
            screen_sidekick_codex_client::CodexClientErrorKind::CancelUnsupported => {
                ErrorCode::TurnCancelUnsupported
            }
            screen_sidekick_codex_client::CodexClientErrorKind::ThreadNotFound => {
                ErrorCode::CodexTurnFailed
            }
            screen_sidekick_codex_client::CodexClientErrorKind::AppServerUnavailable
            | screen_sidekick_codex_client::CodexClientErrorKind::RequestFailed
            | screen_sidekick_codex_client::CodexClientErrorKind::Protocol
            | screen_sidekick_codex_client::CodexClientErrorKind::TurnFailed => {
                ErrorCode::CodexAppServerUnavailable
            }
        }),
    }
}

fn notification_is_visible(
    notification: &JsonRpcNotification,
    subscribed_sessions: &HashSet<String>,
) -> bool {
    match notification
        .params
        .get("session_id")
        .and_then(Value::as_str)
    {
        Some(session_id) => subscribed_sessions.contains(session_id),
        None => true,
    }
}

fn broadcast(sender: &broadcast::Sender<JsonRpcNotification>, method: &str, params: Value) {
    let _ = sender.send(JsonRpcNotification::new(method, params));
}

fn broadcast_error(sender: &broadcast::Sender<JsonRpcNotification>, error: ProtocolError) {
    broadcast(sender, notification::ERROR, json!(error));
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

#[derive(Debug, Deserialize)]
struct StoredSafetyReview {
    #[serde(default)]
    has_danger: bool,
    #[serde(default)]
    warning_count: usize,
    #[serde(default)]
    warnings: Vec<StoredSafetyWarning>,
    #[serde(default)]
    masked_input_values: usize,
    #[serde(default)]
    masked_secret_texts: usize,
}

#[derive(Debug, Deserialize)]
struct StoredSafetyWarning {
    category: String,
    category_label: String,
    source: String,
    source_label: String,
}
