use rusqlite::params;
use screen_sidekick_sidekick_protocol::Message;

use crate::{row_mapping::message_from_row, SessionStore, SessionStoreError};

impl SessionStore {
    pub fn list_messages(&self, session_id: &str) -> Result<Vec<Message>, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, role, text, status, turn_id, created_at FROM messages WHERE session_id = ? ORDER BY created_at ASC",
        )?;
        let messages = statement
            .query_map(params![session_id], |row| {
                message_from_row(&connection, row)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub(crate) fn get_message(&self, message_id: &str) -> Result<Message, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, role, text, status, turn_id, created_at FROM messages WHERE id = ?",
        )?;
        let message = statement.query_row(params![message_id], |row| {
            message_from_row(&connection, row)
        })?;
        Ok(message)
    }
}
