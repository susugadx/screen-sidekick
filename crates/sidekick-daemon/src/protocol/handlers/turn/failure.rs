use screen_sidekick_sidekick_protocol::{notification, ErrorCode, TurnFailedNotification};
use serde_json::json;

use crate::DaemonState;

use super::super::support::{broadcast, broadcast_error, session_error};

pub(super) fn fail_starting_turn(
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
