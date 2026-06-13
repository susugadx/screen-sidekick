use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use screen_sidekick_sidekick_protocol::ErrorCode;

use crate::{row_mapping::str_to_error_code, BeginTurn, BeginTurnOutcome, SessionStoreError};

const MESSAGE_SEND_METHOD: &str = "message/send";
const STATUS_FAILED: &str = "failed";
const STATUS_CANCELLED: &str = "cancelled";

pub(crate) fn begin_turn(
    connection: &Arc<Mutex<Connection>>,
    request: BeginTurn,
) -> Result<BeginTurnOutcome, SessionStoreError> {
    let mut connection = connection
        .lock()
        .map_err(|_| SessionStoreError::LockPoisoned)?;
    let transaction = connection.transaction()?;

    if let Some(existing) = existing_message_send(&transaction, &request)? {
        return existing.outcome_for_retry(&request.request_hash);
    }

    super::ensure_session_exists(&transaction, &request.session_id)?;
    ensure_no_active_turn(&transaction)?;
    ensure_attachments_can_link(&transaction, &request)?;

    let outcome = insert_new_turn(&transaction, &request)?;
    transaction.commit()?;
    Ok(outcome)
}

#[derive(Debug)]
struct ExistingMessageSend {
    request_hash: String,
    message_id: Option<String>,
    turn_id: Option<String>,
    idempotency_status: String,
    turn_status: Option<String>,
    error_code: Option<String>,
}

impl ExistingMessageSend {
    fn outcome_for_retry(self, request_hash: &str) -> Result<BeginTurnOutcome, SessionStoreError> {
        if self.request_hash != request_hash {
            return Err(SessionStoreError::IdempotencyConflict);
        }
        if self.idempotency_status == STATUS_FAILED
            || self.turn_status.as_deref() == Some(STATUS_FAILED)
        {
            return Err(SessionStoreError::IdempotencyFailed(stored_error_code(
                self.error_code.as_deref(),
            )));
        }
        if self.idempotency_status == STATUS_CANCELLED
            || self.turn_status.as_deref() == Some(STATUS_CANCELLED)
        {
            return Err(SessionStoreError::IdempotencyCancelled);
        }

        let (Some(message_id), Some(turn_id)) = (self.message_id, self.turn_id) else {
            return Err(SessionStoreError::TurnAlreadyRunning);
        };
        Ok(BeginTurnOutcome {
            message_id,
            turn_id,
            reused: true,
        })
    }
}

fn existing_message_send(
    transaction: &Transaction<'_>,
    request: &BeginTurn,
) -> Result<Option<ExistingMessageSend>, SessionStoreError> {
    transaction
        .query_row(
            r#"
            SELECT idempotency_keys.request_hash,
                   idempotency_keys.message_id,
                   idempotency_keys.turn_id,
                   idempotency_keys.status,
                   turns.status,
                   turns.error_code
            FROM idempotency_keys
            LEFT JOIN turns ON turns.id = idempotency_keys.turn_id
            WHERE idempotency_keys.session_id = ?
                AND idempotency_keys.method = ?
                AND idempotency_keys.key = ?
            "#,
            params![
                request.session_id.as_str(),
                MESSAGE_SEND_METHOD,
                request.idempotency_key.as_str()
            ],
            |row| {
                Ok(ExistingMessageSend {
                    request_hash: row.get(0)?,
                    message_id: row.get(1)?,
                    turn_id: row.get(2)?,
                    idempotency_status: row.get(3)?,
                    turn_status: row.get(4)?,
                    error_code: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(SessionStoreError::from)
}

fn ensure_no_active_turn(transaction: &Transaction<'_>) -> Result<(), SessionStoreError> {
    let active_turn_id: Option<String> = transaction
        .query_row(
            "SELECT id FROM turns WHERE status IN ('pending', 'running') LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if active_turn_id.is_some() {
        return Err(SessionStoreError::TurnAlreadyRunning);
    }
    Ok(())
}

fn ensure_attachments_can_link(
    transaction: &Transaction<'_>,
    request: &BeginTurn,
) -> Result<(), SessionStoreError> {
    for attachment_id in &request.attachment_ids {
        match attachment_link_state(transaction, &request.session_id, attachment_id)? {
            AttachmentLinkState::Unlinked => {}
            AttachmentLinkState::Linked => {
                return Err(SessionStoreError::AttachmentAlreadyLinked(
                    attachment_id.to_owned(),
                ));
            }
            AttachmentLinkState::Missing => {
                return Err(SessionStoreError::AttachmentNotFound(
                    attachment_id.to_owned(),
                ));
            }
        }
    }
    Ok(())
}

enum AttachmentLinkState {
    Unlinked,
    Linked,
    Missing,
}

fn attachment_link_state(
    transaction: &Transaction<'_>,
    session_id: &str,
    attachment_id: &str,
) -> Result<AttachmentLinkState, SessionStoreError> {
    let linked_message_id: Option<Option<String>> = transaction
        .query_row(
            "SELECT message_id FROM attachments WHERE id = ? AND session_id = ?",
            params![attachment_id, session_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match linked_message_id {
        Some(None) => AttachmentLinkState::Unlinked,
        Some(Some(_)) => AttachmentLinkState::Linked,
        None => AttachmentLinkState::Missing,
    })
}

fn insert_new_turn(
    transaction: &Transaction<'_>,
    request: &BeginTurn,
) -> Result<BeginTurnOutcome, SessionStoreError> {
    let now = super::now_string();
    let message_id = super::prefixed_id("msg");
    let turn_id = super::prefixed_id("turn");
    let expires_at = (super::unix_seconds() + 24 * 60 * 60).to_string();

    transaction.execute(
        "INSERT INTO idempotency_keys(session_id, method, key, request_hash, status, created_at, expires_at) VALUES (?, ?, ?, ?, 'in_progress', ?, ?)",
        params![
            request.session_id.as_str(),
            MESSAGE_SEND_METHOD,
            request.idempotency_key.as_str(),
            request.request_hash.as_str(),
            now.as_str(),
            expires_at.as_str()
        ],
    )?;
    transaction.execute(
        "INSERT INTO messages(id, session_id, role, text, status, turn_id, created_at) VALUES (?, ?, 'user', ?, 'pending', ?, ?)",
        params![
            message_id.as_str(),
            request.session_id.as_str(),
            request.user_text.as_str(),
            turn_id.as_str(),
            now.as_str()
        ],
    )?;
    transaction.execute(
        "INSERT INTO turns(id, session_id, user_message_id, status, started_at) VALUES (?, ?, ?, 'pending', ?)",
        params![
            turn_id.as_str(),
            request.session_id.as_str(),
            message_id.as_str(),
            now.as_str()
        ],
    )?;
    transaction.execute(
        "UPDATE attachments SET message_id = ? WHERE session_id = ? AND message_id IS NULL AND id IN (SELECT value FROM json_each(?))",
        params![
            message_id.as_str(),
            request.session_id.as_str(),
            super::serde_json_array(&request.attachment_ids)
        ],
    )?;
    transaction.execute(
        "UPDATE sessions SET active_turn_id = ?, updated_at = ? WHERE id = ?",
        params![turn_id.as_str(), now.as_str(), request.session_id.as_str()],
    )?;
    transaction.execute(
        "UPDATE idempotency_keys SET message_id = ?, turn_id = ? WHERE session_id = ? AND method = ? AND key = ?",
        params![
            message_id.as_str(),
            turn_id.as_str(),
            request.session_id.as_str(),
            MESSAGE_SEND_METHOD,
            request.idempotency_key.as_str()
        ],
    )?;

    Ok(BeginTurnOutcome {
        message_id,
        turn_id,
        reused: false,
    })
}

fn stored_error_code(error_code: Option<&str>) -> ErrorCode {
    error_code
        .and_then(str_to_error_code)
        .unwrap_or(ErrorCode::CodexTurnFailed)
}
