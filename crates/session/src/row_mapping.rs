use rusqlite::{params, Connection};
use screen_sidekick_sidekick_protocol::{
    Attachment, AttachmentSourceType, CodexStatus, ErrorCode, Message, MessageRole, MessageStatus,
    ProtocolError, SafetyStatus, SessionSummary, Turn, TurnStatus,
};

pub(crate) fn session_from_row(
    row: &rusqlite::Row<'_>,
    codex_status: CodexStatus,
) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        active_turn_id: row.get(4)?,
        source_summary: row.get(5)?,
        codex_status,
    })
}

pub(crate) fn message_from_row(
    connection: &Connection,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Message> {
    let id: String = row.get(0)?;
    let attachment_ids = attachment_ids_for_message(connection, &id)?;
    Ok(Message {
        id,
        session_id: row.get(1)?,
        role: str_to_message_role(&row.get::<_, String>(2)?),
        text: row.get(3)?,
        status: str_to_message_status(&row.get::<_, String>(4)?),
        turn_id: row.get(5)?,
        created_at: row.get(6)?,
        attachment_ids,
    })
}

pub(crate) fn attachment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source_type: str_to_source_type(&row.get::<_, String>(2)?),
        created_at: row.get(3)?,
        summary: row.get(4)?,
        safety_status: str_to_safety_status(&row.get::<_, String>(5)?),
        debug_available: row.get::<_, i64>(6)? != 0,
    })
}

pub(crate) fn turn_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Turn> {
    let status_text: String = row.get(4)?;
    let error_code_text: Option<String> = row.get(7)?;
    let error = error_code_text
        .as_deref()
        .and_then(str_to_error_code)
        .map(|code| ProtocolError::new(code, "Turn failed."));
    Ok(Turn {
        id: row.get(0)?,
        session_id: row.get(1)?,
        user_message_id: row.get(2)?,
        assistant_message_id: row.get(3)?,
        status: str_to_turn_status(&status_text),
        started_at: row.get(5)?,
        completed_at: row.get(6)?,
        error,
    })
}

pub(crate) fn source_type_to_str(source_type: &AttachmentSourceType) -> &'static str {
    match source_type {
        AttachmentSourceType::BrowserTab => "browser_tab",
        AttachmentSourceType::DesktopScreen => "desktop_screen",
        AttachmentSourceType::ManualText => "manual_text",
    }
}

pub(crate) fn safety_status_to_str(status: &SafetyStatus) -> &'static str {
    match status {
        SafetyStatus::Clean => "clean",
        SafetyStatus::Warning => "warning",
    }
}

pub(crate) fn error_code_to_str(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::CodexTurnFailed => "codex_turn_failed",
        ErrorCode::CodexAppServerUnavailable => "codex_app_server_unavailable",
        ErrorCode::CodexNotLoggedIn => "codex_not_logged_in",
        ErrorCode::CodexNotFound => "codex_not_found",
        ErrorCode::UnsupportedCodexVersion => "unsupported_codex_version",
        ErrorCode::TurnCancelUnsupported => "turn_cancel_unsupported",
        _ => "internal_error",
    }
}

fn attachment_ids_for_message(
    connection: &Connection,
    message_id: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection
        .prepare("SELECT id FROM attachments WHERE message_id = ? ORDER BY created_at ASC")?;
    let attachment_ids = statement
        .query_map(params![message_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(attachment_ids)
}

fn str_to_source_type(value: &str) -> AttachmentSourceType {
    match value {
        "desktop_screen" => AttachmentSourceType::DesktopScreen,
        "manual_text" => AttachmentSourceType::ManualText,
        _ => AttachmentSourceType::BrowserTab,
    }
}

fn str_to_safety_status(value: &str) -> SafetyStatus {
    match value {
        "warning" => SafetyStatus::Warning,
        _ => SafetyStatus::Clean,
    }
}

fn str_to_message_role(value: &str) -> MessageRole {
    match value {
        "assistant" => MessageRole::Assistant,
        "system_notice" => MessageRole::SystemNotice,
        _ => MessageRole::User,
    }
}

fn str_to_message_status(value: &str) -> MessageStatus {
    match value {
        "streaming" => MessageStatus::Streaming,
        "completed" => MessageStatus::Completed,
        "failed" => MessageStatus::Failed,
        "cancelled" => MessageStatus::Cancelled,
        _ => MessageStatus::Pending,
    }
}

fn str_to_turn_status(value: &str) -> TurnStatus {
    match value {
        "running" => TurnStatus::Running,
        "completed" => TurnStatus::Completed,
        "failed" => TurnStatus::Failed,
        "cancelled" => TurnStatus::Cancelled,
        _ => TurnStatus::Pending,
    }
}

pub(crate) fn str_to_error_code(value: &str) -> Option<ErrorCode> {
    match value {
        "codex_turn_failed" => Some(ErrorCode::CodexTurnFailed),
        "codex_app_server_unavailable" => Some(ErrorCode::CodexAppServerUnavailable),
        "codex_not_logged_in" => Some(ErrorCode::CodexNotLoggedIn),
        "codex_not_found" => Some(ErrorCode::CodexNotFound),
        "unsupported_codex_version" => Some(ErrorCode::UnsupportedCodexVersion),
        "turn_cancel_unsupported" => Some(ErrorCode::TurnCancelUnsupported),
        "internal_error" => Some(ErrorCode::InternalError),
        _ => None,
    }
}
