use rusqlite::{params, OptionalExtension};
use screen_sidekick_sidekick_protocol::{CodexStatus, SessionGetResult, SessionSummary};

use crate::{
    now_string, prefixed_id, row_mapping::session_from_row, SessionStore, SessionStoreError,
};

impl SessionStore {
    pub fn create_session(&self, title: Option<&str>) -> Result<SessionSummary, SessionStoreError> {
        let now = now_string();
        let id = prefixed_id("sess");
        let title = title.unwrap_or("New chat");
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO sessions(id, title, created_at, updated_at, source_summary) VALUES (?, ?, ?, ?, '')",
            params![id, title, now, now],
        )?;
        drop(connection);
        self.get_session_summary(&id)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, title, created_at, updated_at, active_turn_id, source_summary FROM sessions WHERE archived_at IS NULL ORDER BY updated_at DESC",
        )?;
        let sessions = statement
            .query_map([], |row| session_from_row(row, CodexStatus::NotStarted))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn get_session(&self, session_id: &str) -> Result<SessionGetResult, SessionStoreError> {
        let session = self.get_session_summary(session_id)?;
        let messages = self.list_messages(session_id)?;
        let attachments = self.list_attachments(session_id)?;
        let active_turn = match &session.active_turn_id {
            Some(turn_id) => Some(self.get_turn(turn_id)?),
            None => None,
        };
        Ok(SessionGetResult {
            session,
            messages,
            attachments,
            active_turn,
        })
    }

    pub fn get_session_summary(
        &self,
        session_id: &str,
    ) -> Result<SessionSummary, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, title, created_at, updated_at, active_turn_id, source_summary FROM sessions WHERE id = ? AND archived_at IS NULL",
        )?;
        let session = statement
            .query_row(params![session_id], |row| {
                session_from_row(row, CodexStatus::NotStarted)
            })
            .optional()?
            .ok_or(SessionStoreError::SessionNotFound)?;
        Ok(session)
    }
}
