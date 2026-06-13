use rusqlite::{params, Connection, OptionalExtension};
use screen_sidekick_sidekick_protocol::{ErrorCode, Message, Turn, TurnStatus};

use crate::{
    row_mapping::{error_code_to_str, turn_from_row},
    SessionStore, SessionStoreError, TurnCancellationTarget,
};

impl SessionStore {
    pub fn recover_interrupted_active_turns(&self) -> Result<usize, SessionStoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let mut statement =
            transaction.prepare("SELECT id FROM turns WHERE status IN ('pending', 'running')")?;
        let turn_ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        if turn_ids.is_empty() {
            transaction.commit()?;
            return Ok(0);
        }

        let now = super::now_string();
        let turn_ids_json = super::serde_json_array(&turn_ids);
        transaction.execute(
            r#"
            UPDATE messages
            SET status = 'failed', completed_at = ?
            WHERE id IN (
                SELECT user_message_id
                FROM turns
                WHERE id IN (SELECT value FROM json_each(?))
            )
            "#,
            params![now, turn_ids_json],
        )?;
        transaction.execute(
            r#"
            UPDATE turns
            SET status = 'failed',
                error_code = ?,
                error_debug_id = 'daemon_startup_recovery',
                completed_at = ?
            WHERE id IN (SELECT value FROM json_each(?))
                AND status IN ('pending', 'running')
            "#,
            params![
                error_code_to_str(ErrorCode::CodexAppServerUnavailable),
                now,
                turn_ids_json
            ],
        )?;
        transaction.execute(
            r#"
            UPDATE sessions
            SET active_turn_id = NULL, updated_at = ?
            WHERE active_turn_id IN (SELECT value FROM json_each(?))
            "#,
            params![now, turn_ids_json],
        )?;
        transaction.execute(
            r#"
            UPDATE idempotency_keys
            SET status = 'failed'
            WHERE turn_id IN (SELECT value FROM json_each(?))
            "#,
            params![turn_ids_json],
        )?;
        transaction.commit()?;
        Ok(turn_ids.len())
    }

    pub fn link_codex_thread(
        &self,
        session_id: &str,
        codex_thread_id: &str,
        codex_cli_version: Option<&str>,
        codex_schema_hash: Option<&str>,
    ) -> Result<(), SessionStoreError> {
        self.ensure_session_exists(session_id)?;
        let now = super::now_string();
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        connection.execute(
            r#"
            INSERT INTO codex_thread_links(session_id, codex_thread_id, codex_cli_version, codex_schema_hash, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(session_id) DO UPDATE SET
                codex_thread_id = excluded.codex_thread_id,
                codex_cli_version = excluded.codex_cli_version,
                codex_schema_hash = excluded.codex_schema_hash,
                updated_at = excluded.updated_at
            "#,
            params![
                session_id,
                codex_thread_id,
                codex_cli_version,
                codex_schema_hash,
                now,
                now
            ],
        )?;
        Ok(())
    }

    pub fn codex_thread_id(&self, session_id: &str) -> Result<Option<String>, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let thread_id = connection
            .query_row(
                "SELECT codex_thread_id FROM codex_thread_links WHERE session_id = ?",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(thread_id)
    }

    pub fn mark_turn_running(
        &self,
        turn_id: &str,
        codex_thread_id: Option<&str>,
        codex_turn_id: Option<&str>,
    ) -> Result<Turn, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let changed = connection.execute(
            "UPDATE turns SET status = 'running', codex_thread_id = COALESCE(?, codex_thread_id), codex_turn_id = COALESCE(?, codex_turn_id) WHERE id = ?",
            params![codex_thread_id, codex_turn_id, turn_id],
        )?;
        if changed == 0 {
            return Err(SessionStoreError::TurnNotFound);
        }
        drop(connection);
        self.get_turn(turn_id)
    }

    pub fn complete_turn(
        &self,
        turn_id: &str,
        assistant_text: &str,
    ) -> Result<(Turn, Message), SessionStoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let (session_id, user_message_id, status) =
            turn_session_user_message_and_status(&transaction, turn_id)?;
        if !is_active_status_text(&status) {
            return Err(SessionStoreError::TurnNotCancellable);
        }

        let now = super::now_string();
        let assistant_message_id = super::prefixed_id("msg");
        transaction.execute(
            "INSERT INTO messages(id, session_id, role, text, status, turn_id, created_at, completed_at) VALUES (?, ?, 'assistant', ?, 'completed', ?, ?, ?)",
            params![assistant_message_id, session_id, assistant_text, turn_id, now, now],
        )?;
        transaction.execute(
            "UPDATE messages SET status = 'completed', completed_at = ? WHERE id = ?",
            params![now, user_message_id],
        )?;
        transaction.execute(
            "UPDATE turns SET status = 'completed', assistant_message_id = ?, completed_at = ? WHERE id = ?",
            params![assistant_message_id, now, turn_id],
        )?;
        transaction.execute(
            "UPDATE sessions SET active_turn_id = NULL, updated_at = ? WHERE id = ?",
            params![now, session_id],
        )?;
        transaction.execute(
            "UPDATE idempotency_keys SET status = 'completed' WHERE turn_id = ?",
            params![turn_id],
        )?;
        transaction.commit()?;
        drop(connection);
        let turn = self.get_turn(turn_id)?;
        let message = self.get_message(&assistant_message_id)?;
        Ok((turn, message))
    }

    pub fn fail_turn(
        &self,
        turn_id: &str,
        error_code: ErrorCode,
        debug_id: Option<&str>,
    ) -> Result<Turn, SessionStoreError> {
        self.finish_failed_or_cancelled(turn_id, "failed", Some(error_code), debug_id)
    }

    pub fn cancel_turn(&self, turn_id: &str) -> Result<Turn, SessionStoreError> {
        self.finish_cancelled(turn_id, None)
    }

    pub fn get_turn(&self, turn_id: &str) -> Result<Turn, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, user_message_id, assistant_message_id, status, started_at, completed_at, error_code, error_debug_id FROM turns WHERE id = ?",
        )?;
        let turn = statement
            .query_row(params![turn_id], turn_from_row)
            .optional()?
            .ok_or(SessionStoreError::TurnNotFound)?;
        Ok(turn)
    }

    pub fn get_turn_for_session(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<Turn, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, user_message_id, assistant_message_id, status, started_at, completed_at, error_code, error_debug_id FROM turns WHERE id = ? AND session_id = ?",
        )?;
        let turn = statement
            .query_row(params![turn_id, session_id], turn_from_row)
            .optional()?
            .ok_or(SessionStoreError::TurnNotFound)?;
        Ok(turn)
    }

    pub fn get_cancellable_turn_for_session(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<TurnCancellationTarget, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, user_message_id, assistant_message_id, status, started_at, completed_at, error_code, error_debug_id, codex_turn_id FROM turns WHERE id = ? AND session_id = ?",
        )?;
        let (turn, codex_turn_id) = statement
            .query_row(params![turn_id, session_id], |row| {
                Ok((turn_from_row(row)?, row.get::<_, Option<String>>(9)?))
            })
            .optional()?
            .ok_or(SessionStoreError::TurnNotFound)?;
        if is_active_turn_status(&turn.status) {
            let codex_turn_id = codex_turn_id
                .filter(|id| !id.is_empty())
                .ok_or(SessionStoreError::TurnCancelTargetMissing)?;
            Ok(TurnCancellationTarget {
                turn,
                codex_turn_id,
            })
        } else {
            Err(SessionStoreError::TurnNotCancellable)
        }
    }

    pub fn turn_is_active(&self, turn_id: &str) -> Result<bool, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let status = connection
            .query_row(
                "SELECT status FROM turns WHERE id = ?",
                params![turn_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(SessionStoreError::TurnNotFound)?;
        Ok(is_active_status_text(&status))
    }

    pub fn cancel_turn_for_session(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<Turn, SessionStoreError> {
        self.finish_cancelled(turn_id, Some(session_id))
    }

    fn finish_failed_or_cancelled(
        &self,
        turn_id: &str,
        status: &str,
        error_code: Option<ErrorCode>,
        debug_id: Option<&str>,
    ) -> Result<Turn, SessionStoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let (session_id, user_message_id, current_status) =
            turn_session_user_message_and_status(&transaction, turn_id)?;
        if !is_active_status_text(&current_status) {
            return Err(SessionStoreError::TurnNotCancellable);
        }

        let now = super::now_string();
        transaction.execute(
            "UPDATE messages SET status = ?, completed_at = ? WHERE id = ?",
            params![status, now, user_message_id],
        )?;
        transaction.execute(
            "UPDATE turns SET status = ?, error_code = ?, error_debug_id = ?, completed_at = ? WHERE id = ?",
            params![
                status,
                error_code.map(error_code_to_str),
                debug_id,
                now,
                turn_id
            ],
        )?;
        transaction.execute(
            "UPDATE sessions SET active_turn_id = NULL, updated_at = ? WHERE id = ?",
            params![now, session_id],
        )?;
        transaction.execute(
            "UPDATE idempotency_keys SET status = ? WHERE turn_id = ?",
            params![status, turn_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_turn(turn_id)
    }

    fn finish_cancelled(
        &self,
        turn_id: &str,
        expected_session_id: Option<&str>,
    ) -> Result<Turn, SessionStoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let transaction = connection.transaction()?;
        let (session_id, user_message_id, status) = match expected_session_id {
            Some(session_id) => {
                turn_session_user_message_and_status_for_session(&transaction, session_id, turn_id)?
            }
            None => turn_session_user_message_and_status(&transaction, turn_id)?,
        };
        if !is_active_status_text(&status) {
            return Err(SessionStoreError::TurnNotCancellable);
        }

        let now = super::now_string();
        transaction.execute(
            "UPDATE messages SET status = 'cancelled', completed_at = ? WHERE id = ?",
            params![now, user_message_id],
        )?;
        transaction.execute(
            "UPDATE turns SET status = 'cancelled', error_code = NULL, error_debug_id = NULL, completed_at = ? WHERE id = ?",
            params![now, turn_id],
        )?;
        transaction.execute(
            "UPDATE sessions SET active_turn_id = NULL, updated_at = ? WHERE id = ?",
            params![now, session_id],
        )?;
        transaction.execute(
            "UPDATE idempotency_keys SET status = 'cancelled' WHERE turn_id = ?",
            params![turn_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_turn(turn_id)
    }
}

fn turn_session_user_message_and_status(
    connection: &Connection,
    turn_id: &str,
) -> Result<(String, String, String), SessionStoreError> {
    connection
        .query_row(
            "SELECT session_id, user_message_id, status FROM turns WHERE id = ?",
            params![turn_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(SessionStoreError::TurnNotFound)
}

fn turn_session_user_message_and_status_for_session(
    connection: &Connection,
    session_id: &str,
    turn_id: &str,
) -> Result<(String, String, String), SessionStoreError> {
    connection
        .query_row(
            "SELECT session_id, user_message_id, status FROM turns WHERE id = ? AND session_id = ?",
            params![turn_id, session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(SessionStoreError::TurnNotFound)
}

fn is_active_turn_status(status: &TurnStatus) -> bool {
    matches!(status, TurnStatus::Pending | TurnStatus::Running)
}

fn is_active_status_text(status: &str) -> bool {
    matches!(status, "pending" | "running")
}
