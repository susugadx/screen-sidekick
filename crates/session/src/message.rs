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
            "SELECT id, session_id, role, text, status, turn_id, created_at FROM messages WHERE session_id = ? ORDER BY created_at ASC, rowid ASC",
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

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use screen_sidekick_sidekick_protocol::MessageRole;

    use crate::SessionStore;

    #[test]
    fn list_messages_orders_same_timestamp_rows_by_insertion_order() {
        let store = SessionStore::in_memory().expect("store opens");
        let session = store.create_session(Some("Test")).expect("session created");
        {
            let connection = store.connection.lock().expect("store lock is not poisoned");
            connection
                .execute(
                    "INSERT INTO messages(id, session_id, role, text, status, created_at) VALUES (?, ?, 'user', 'question', 'completed', '123')",
                    params!["msg_user", session.id.as_str()],
                )
                .expect("user message inserts");
            connection
                .execute(
                    "INSERT INTO messages(id, session_id, role, text, status, created_at) VALUES (?, ?, 'assistant', 'answer', 'completed', '123')",
                    params!["msg_assistant", session.id.as_str()],
                )
                .expect("assistant message inserts");
        }

        let messages = store
            .list_messages(&session.id)
            .expect("messages list in transcript order");

        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[0].text, "question");
        assert_eq!(messages[1].role, MessageRole::Assistant);
        assert_eq!(messages[1].text, "answer");
    }
}
