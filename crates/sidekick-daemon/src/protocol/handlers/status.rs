use screen_sidekick_sidekick_protocol::{JsonRpcRequest, ProtocolError, StatusGetResult};
use serde_json::Value;

use crate::DaemonState;

use super::super::{codex_readiness_to_protocol, serialize_result};

pub(in crate::protocol) async fn handle_status_get(
    state: &DaemonState,
    _request: &JsonRpcRequest,
) -> Result<Value, ProtocolError> {
    let readiness = state.codex.readiness().await;
    serialize_result(StatusGetResult {
        codex_readiness: codex_readiness_to_protocol(readiness),
    })
}
