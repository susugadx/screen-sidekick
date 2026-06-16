use rusqlite::{params, Connection, OptionalExtension};
use screen_sidekick_sidekick_protocol::Attachment;

use crate::{
    row_mapping::{attachment_from_row, safety_status_to_str, source_type_to_str},
    AttachmentCodexContext, CreateAttachment, SessionStore, SessionStoreError,
};

impl SessionStore {
    pub fn create_attachment(
        &self,
        attachment: CreateAttachment,
    ) -> Result<Attachment, SessionStoreError> {
        let id = super::prefixed_id("att");
        let now = super::now_string();
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        super::ensure_session_exists(&connection, &attachment.session_id)?;
        if let Some(message_id) = attachment.message_id.as_deref() {
            ensure_message_belongs_to_session(&connection, &attachment.session_id, message_id)?;
        }
        connection.execute(
            r#"
            INSERT INTO attachments(
                id, session_id, message_id, source_type, created_at, summary,
                sanitized_context_json, safety_review_json, source_metadata_json,
                safety_status, debug_available
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                id,
                attachment.session_id,
                attachment.message_id,
                source_type_to_str(&attachment.source_type),
                now,
                attachment.summary,
                attachment.sanitized_context_json,
                attachment.safety_review_json,
                attachment.source_metadata_json,
                safety_status_to_str(&attachment.safety_status),
                i64::from(attachment.debug_available)
            ],
        )?;
        drop(connection);
        self.get_attachment(&id)
    }

    pub fn list_attachments(&self, session_id: &str) -> Result<Vec<Attachment>, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, source_type, created_at, summary, safety_status, debug_available FROM attachments WHERE session_id = ? ORDER BY created_at ASC",
        )?;
        let attachments = statement
            .query_map(params![session_id], attachment_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(attachments)
    }

    pub fn sanitized_context_json(&self, attachment_id: &str) -> Result<String, SessionStoreError> {
        Ok(self
            .attachment_codex_context(attachment_id)?
            .sanitized_context_json)
    }

    pub fn attachment_codex_context(
        &self,
        attachment_id: &str,
    ) -> Result<AttachmentCodexContext, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let value = connection
            .query_row(
                "SELECT sanitized_context_json, safety_review_json FROM attachments WHERE id = ?",
                params![attachment_id],
                |row| {
                    Ok(AttachmentCodexContext {
                        sanitized_context_json: row.get(0)?,
                        safety_review_json: row.get(1)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| SessionStoreError::AttachmentNotFound(attachment_id.to_owned()))?;
        Ok(value)
    }

    fn get_attachment(&self, attachment_id: &str) -> Result<Attachment, SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, source_type, created_at, summary, safety_status, debug_available FROM attachments WHERE id = ?",
        )?;
        let attachment = statement
            .query_row(params![attachment_id], attachment_from_row)
            .optional()?
            .ok_or_else(|| SessionStoreError::AttachmentNotFound(attachment_id.to_owned()))?;
        Ok(attachment)
    }
}

fn ensure_message_belongs_to_session(
    connection: &Connection,
    session_id: &str,
    message_id: &str,
) -> Result<(), SessionStoreError> {
    let exists: Option<String> = connection
        .query_row(
            "SELECT id FROM messages WHERE id = ? AND session_id = ?",
            params![message_id, session_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_some() {
        Ok(())
    } else {
        Err(SessionStoreError::MessageNotFound)
    }
}
