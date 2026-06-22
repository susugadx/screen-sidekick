use std::collections::HashSet;

use screen_sidekick_session::SessionStoreError;
use screen_sidekick_sidekick_protocol::{
    method, notification, AuthStatus, ClientCapability, ErrorCode, ErrorData, InitializeParams,
    InitializeResult, JsonRpcNotification, JsonRpcRequest, MessageSendResult, ProtocolError,
    TurnFailedNotification, SIDEKICK_PROTOCOL_VERSION,
};
use serde_json::json;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::DaemonState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolConnectionAuth {
    PairingToken,
    NativeHost { origin: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisconnectCleanup {
    NativeHost,
    SidecarWebSocket,
}

impl DisconnectCleanup {
    fn failure_reason(self) -> &'static str {
        match self {
            Self::NativeHost => "native_host_port_closed",
            Self::SidecarWebSocket => "sidecar_websocket_connection_closed",
        }
    }

    fn failure_message(self) -> &'static str {
        match self {
            Self::NativeHost => "Native host connection closed before the turn completed.",
            Self::SidecarWebSocket => {
                "Sidecar WebSocket connection closed before the turn completed."
            }
        }
    }
}

pub struct ProtocolConnection {
    state: DaemonState,
    subscribed_sessions: HashSet<String>,
    owned_active_turns: HashSet<String>,
    initialized: bool,
    auth: ProtocolConnectionAuth,
    disconnect_cleanup: Option<DisconnectCleanup>,
}

impl ProtocolConnection {
    #[must_use]
    pub fn websocket(state: DaemonState) -> Self {
        Self::new(state, ProtocolConnectionAuth::PairingToken)
    }

    #[must_use]
    pub fn sidecar_websocket(state: DaemonState) -> Self {
        Self::new_with_disconnect_cleanup(
            state,
            ProtocolConnectionAuth::PairingToken,
            Some(DisconnectCleanup::SidecarWebSocket),
        )
    }

    #[must_use]
    pub fn native_host(state: DaemonState, origin: Option<String>) -> Self {
        Self::new(state, ProtocolConnectionAuth::NativeHost { origin })
    }

    #[must_use]
    pub fn new(state: DaemonState, auth: ProtocolConnectionAuth) -> Self {
        let disconnect_cleanup = match &auth {
            ProtocolConnectionAuth::PairingToken => None,
            ProtocolConnectionAuth::NativeHost { .. } => Some(DisconnectCleanup::NativeHost),
        };
        Self::new_with_disconnect_cleanup(state, auth, disconnect_cleanup)
    }

    fn new_with_disconnect_cleanup(
        state: DaemonState,
        auth: ProtocolConnectionAuth,
        disconnect_cleanup: Option<DisconnectCleanup>,
    ) -> Self {
        Self {
            state,
            subscribed_sessions: HashSet::new(),
            owned_active_turns: HashSet::new(),
            initialized: false,
            auth,
            disconnect_cleanup,
        }
    }

    #[must_use]
    pub fn event_receiver(&self) -> broadcast::Receiver<JsonRpcNotification> {
        self.state.events.subscribe()
    }

    pub async fn handle_text(&mut self, text: &str) -> Option<String> {
        let request = match serde_json::from_str::<JsonRpcRequest>(text) {
            Ok(request) => request,
            Err(_) => {
                return Some(super::error_response_text(
                    "unknown",
                    ErrorCode::InvalidRequest,
                    "Request JSON is invalid.",
                    None,
                ));
            }
        };

        Some(self.handle_request(request).await)
    }

    pub fn notification_text(&mut self, notification: &JsonRpcNotification) -> Option<String> {
        self.observe_notification(notification);
        if !self.initialized || !notification_is_visible(notification, &self.subscribed_sessions) {
            return None;
        }
        serde_json::to_string(notification).ok()
    }

    pub fn fail_owned_active_turns_on_disconnect(&mut self) -> Result<usize, SessionStoreError> {
        let Some(disconnect) = self.disconnect_cleanup else {
            return Ok(0);
        };
        let turn_ids = self.owned_active_turns.drain().collect::<Vec<_>>();
        let mut failed_count = 0_usize;
        for turn_id in turn_ids {
            match self.state.store.fail_turn(
                &turn_id,
                ErrorCode::CodexAppServerUnavailable,
                Some(disconnect.failure_reason()),
            ) {
                Ok(turn) => {
                    failed_count += 1;
                    let _ = self.state.events.send(JsonRpcNotification::new(
                        notification::TURN_FAILED,
                        json!(TurnFailedNotification {
                            session_id: turn.session_id.clone(),
                            turn,
                            message: Some(disconnect.failure_message().to_owned()),
                        }),
                    ));
                }
                Err(SessionStoreError::TurnNotCancellable | SessionStoreError::TurnNotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(failed_count)
    }

    async fn handle_request(&mut self, request: JsonRpcRequest) -> String {
        if !self.initialized && request.method != method::INITIALIZE {
            return super::error_response_text(
                &request.id,
                ErrorCode::Unauthorized,
                "Initialize is required.",
                None,
            );
        }

        let response = match request.method.as_str() {
            method::INITIALIZE => self.handle_initialize(&request).await,
            method::SESSION_CREATE => {
                super::handlers::session::handle_session_create(&self.state, &request).await
            }
            method::SESSION_LIST => {
                super::handlers::session::handle_session_list(&self.state, &request).await
            }
            method::SESSION_GET => {
                super::handlers::session::handle_session_get(&self.state, &request).await
            }
            method::SESSION_SUBSCRIBE => {
                super::handlers::session::handle_session_subscribe(
                    &self.state,
                    &mut self.subscribed_sessions,
                    &request,
                )
                .await
            }
            method::SESSION_UNSUBSCRIBE => {
                super::handlers::session::handle_session_unsubscribe(
                    &mut self.subscribed_sessions,
                    &request,
                )
                .await
            }
            method::CONTEXT_ATTACH_BROWSER => {
                super::handlers::context::handle_context_attach_browser(&self.state, &request).await
            }
            method::MESSAGE_SEND => {
                super::handlers::turn::handle_message_send(&self.state, &request).await
            }
            method::TURN_CANCEL => {
                super::handlers::turn::handle_turn_cancel(&self.state, &request).await
            }
            method::STATUS_GET => {
                super::handlers::status::handle_status_get(&self.state, &request).await
            }
            _ => Err(super::protocol_error(
                ErrorCode::MethodNotFound,
                "Method was not found.",
                None,
            )),
        };

        match response {
            Ok(result) => {
                self.observe_successful_result(&request.method, &result);
                super::success_response_text(&request.id, result)
            }
            Err(error) => {
                super::error_response_text(&request.id, error.code, error.message, error.data)
            }
        }
    }

    async fn handle_initialize(
        &mut self,
        request: &JsonRpcRequest,
    ) -> Result<Value, ProtocolError> {
        let params: InitializeParams = super::parse_params(request)?;
        if params.protocol_version != SIDEKICK_PROTOCOL_VERSION {
            return Err(super::protocol_error(
                ErrorCode::UnsupportedProtocolVersion,
                "Sidekick protocol version is not supported.",
                Some(ErrorData {
                    supported_versions: Some(vec![SIDEKICK_PROTOCOL_VERSION.to_owned()]),
                    ..ErrorData::default()
                }),
            ));
        }

        match &self.auth {
            ProtocolConnectionAuth::PairingToken => {
                if params.auth_token.as_deref() != Some(self.state.token()) {
                    return Err(super::protocol_error(
                        ErrorCode::Unauthorized,
                        "Pairing token is invalid.",
                        Some(ErrorData {
                            retryable: Some(false),
                            ..ErrorData::default()
                        }),
                    ));
                }
            }
            ProtocolConnectionAuth::NativeHost { origin } => {
                validate_native_host_origin(origin.as_deref())?;
            }
        }

        self.initialized = true;
        let readiness = self.state.codex.readiness().await;
        let mut capabilities = vec![
            ClientCapability::BrowserContext,
            ClientCapability::ChatStream,
            ClientCapability::DebugExport,
        ];
        if self.state.codex.supports_turn_cancel() {
            capabilities.push(ClientCapability::TurnCancel);
        }
        super::serialize_result(InitializeResult {
            protocol_version: SIDEKICK_PROTOCOL_VERSION.to_owned(),
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            capabilities,
            auth_status: AuthStatus::Ready,
            codex_readiness: super::codex_readiness_to_protocol(readiness),
            limits: self.state.limits,
            warnings: Vec::new(),
        })
    }

    fn observe_successful_result(&mut self, request_method: &str, result: &Value) {
        if request_method != method::MESSAGE_SEND {
            return;
        }
        let Ok(result) = serde_json::from_value::<MessageSendResult>(result.clone()) else {
            return;
        };
        if !result.reused {
            self.owned_active_turns.insert(result.turn_id);
        }
    }

    fn observe_notification(&mut self, notification: &JsonRpcNotification) {
        match notification.method.as_str() {
            notification::TURN_COMPLETED
            | notification::TURN_FAILED
            | notification::TURN_CANCELLED => {
                if let Some(turn_id) = notification
                    .params
                    .get("turn")
                    .and_then(|turn| turn.get("id"))
                    .and_then(Value::as_str)
                {
                    self.owned_active_turns.remove(turn_id);
                }
            }
            _ => {}
        }
    }
}

fn validate_native_host_origin(origin: Option<&str>) -> Result<(), ProtocolError> {
    let Some(origin) = origin else {
        return Ok(());
    };
    let Some(extension_id) = origin
        .strip_prefix("chrome-extension://")
        .and_then(|value| value.strip_suffix('/'))
    else {
        return Err(forbidden_origin_error());
    };
    if extension_id.is_empty()
        || !extension_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(forbidden_origin_error());
    }
    Ok(())
}

fn forbidden_origin_error() -> ProtocolError {
    super::protocol_error(
        ErrorCode::ForbiddenOrigin,
        "Native host caller origin is not allowed.",
        Some(ErrorData {
            retryable: Some(false),
            ..ErrorData::default()
        }),
    )
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
