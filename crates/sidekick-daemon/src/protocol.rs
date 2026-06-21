use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use screen_sidekick_codex_client::{CodexClientErrorKind, CodexReadiness as CodexClientReadiness};
use screen_sidekick_sidekick_protocol::{
    CodexReadiness, ErrorCode, ErrorData, JsonRpcFailure, JsonRpcRequest, JsonRpcSuccess,
    ProtocolError,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::DaemonState;

mod connection;
mod handlers;

pub use connection::{ProtocolConnection, ProtocolConnectionAuth};

pub(crate) async fn websocket_loop(socket: WebSocket, state: DaemonState) {
    let (mut sender, mut receiver) = socket.split();
    let websocket_shutdown = state.websocket_shutdown.clone();
    let mut connection = ProtocolConnection::websocket(state);
    let mut events = connection.event_receiver();
    let mut shutdown = websocket_shutdown.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(incoming) = incoming else { break; };
                let Ok(message) = incoming else { break; };
                match message {
                    WsMessage::Text(text) => {
                        let Some(response) = connection.handle_text(&text).await else {
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
                        if let Some(text) = connection.notification_text(&notification) {
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

fn parse_params<T: DeserializeOwned>(request: &JsonRpcRequest) -> Result<T, ProtocolError> {
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

fn codex_readiness_to_protocol(readiness: CodexClientReadiness) -> CodexReadiness {
    CodexReadiness {
        available: readiness.available,
        version: readiness.version,
        error_code: readiness.error.map(|kind| match kind {
            CodexClientErrorKind::CodexNotFound => ErrorCode::CodexNotFound,
            CodexClientErrorKind::NotLoggedIn => ErrorCode::CodexNotLoggedIn,
            CodexClientErrorKind::UnsupportedVersion => ErrorCode::UnsupportedCodexVersion,
            CodexClientErrorKind::CancelUnsupported => ErrorCode::TurnCancelUnsupported,
            CodexClientErrorKind::ThreadNotFound => ErrorCode::CodexTurnFailed,
            CodexClientErrorKind::AppServerUnavailable
            | CodexClientErrorKind::RequestFailed
            | CodexClientErrorKind::Protocol
            | CodexClientErrorKind::TurnFailed => ErrorCode::CodexAppServerUnavailable,
        }),
    }
}
