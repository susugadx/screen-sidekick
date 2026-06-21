use std::collections::HashSet;

use screen_sidekick_sidekick_protocol::{
    notification, JsonRpcRequest, ProtocolError, SessionCreateParams, SessionCreateResult,
    SessionIdParams, SessionListResult,
};
use serde_json::{json, Value};

use crate::DaemonState;

use super::super::{parse_params, serialize_result};
use super::support::{broadcast, session_error};

pub(in crate::protocol) async fn handle_session_create(
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

pub(in crate::protocol) async fn handle_session_list(
    state: &DaemonState,
    _request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let sessions = state.store.list_sessions().map_err(session_error)?;
    serialize_result(SessionListResult { sessions })
}

pub(in crate::protocol) async fn handle_session_get(
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

pub(in crate::protocol) async fn handle_session_subscribe(
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

pub(in crate::protocol) async fn handle_session_unsubscribe(
    subscribed_sessions: &mut HashSet<String>,
    request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let params: SessionIdParams = parse_params(request)?;
    subscribed_sessions.remove(&params.session_id);
    serialize_result(json!({}))
}
