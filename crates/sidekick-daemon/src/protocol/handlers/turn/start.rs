use screen_sidekick_codex_client::{
    CodexClientError, CodexClientErrorKind, StartTurnOutcome, StartTurnRequest,
};
use screen_sidekick_sidekick_protocol::{ErrorCode, ProtocolError};
use tokio::time::timeout;

use crate::DaemonState;

use super::super::super::protocol_error;
use super::super::support::{codex_error, session_error};
use super::failure::fail_starting_turn;

pub(super) async fn start_codex_turn_with_stale_thread_recovery(
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
