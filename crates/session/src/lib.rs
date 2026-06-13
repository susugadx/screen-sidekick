#![forbid(unsafe_code)]

mod attachment;
mod begin_turn;
mod message;
mod row_mapping;
mod session;
mod turn;

use rusqlite::{params, Connection, OptionalExtension};
use screen_sidekick_sidekick_protocol::{AttachmentSourceType, ErrorCode, SafetyStatus, Turn};
use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct SessionStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug)]
pub enum SessionStoreError {
    Sqlite(rusqlite::Error),
    LockPoisoned,
    SessionNotFound,
    MessageNotFound,
    AttachmentNotFound(String),
    AttachmentAlreadyLinked(String),
    TurnNotFound,
    TurnNotCancellable,
    TurnCancelTargetMissing,
    TurnAlreadyRunning,
    IdempotencyConflict,
    IdempotencyFailed(ErrorCode),
    IdempotencyCancelled,
}

impl fmt::Display for SessionStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(_) => formatter.write_str("sqlite session store failed"),
            Self::LockPoisoned => formatter.write_str("sqlite session store lock was poisoned"),
            Self::SessionNotFound => formatter.write_str("session was not found"),
            Self::MessageNotFound => formatter.write_str("message was not found"),
            Self::AttachmentNotFound(id) => write!(formatter, "attachment was not found: {id}"),
            Self::AttachmentAlreadyLinked(id) => {
                write!(formatter, "attachment is already linked to a message: {id}")
            }
            Self::TurnNotFound => formatter.write_str("turn was not found"),
            Self::TurnNotCancellable => formatter.write_str("turn cannot be cancelled"),
            Self::TurnCancelTargetMissing => {
                formatter.write_str("turn is missing a Codex cancellation target")
            }
            Self::TurnAlreadyRunning => formatter.write_str("a turn is already running"),
            Self::IdempotencyConflict => formatter.write_str("idempotency key request mismatch"),
            Self::IdempotencyFailed(_) => formatter.write_str("idempotent request already failed"),
            Self::IdempotencyCancelled => {
                formatter.write_str("idempotent request was already cancelled")
            }
        }
    }
}

impl Error for SessionStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::LockPoisoned
            | Self::SessionNotFound
            | Self::MessageNotFound
            | Self::AttachmentNotFound(_)
            | Self::AttachmentAlreadyLinked(_)
            | Self::TurnNotFound
            | Self::TurnNotCancellable
            | Self::TurnCancelTargetMissing
            | Self::TurnAlreadyRunning
            | Self::IdempotencyConflict
            | Self::IdempotencyFailed(_)
            | Self::IdempotencyCancelled => None,
        }
    }
}

impl From<rusqlite::Error> for SessionStoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAttachment {
    pub session_id: String,
    pub message_id: Option<String>,
    pub source_type: AttachmentSourceType,
    pub summary: String,
    pub sanitized_context_json: String,
    pub safety_review_json: String,
    pub source_metadata_json: String,
    pub safety_status: SafetyStatus,
    pub debug_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginTurn {
    pub session_id: String,
    pub user_text: String,
    pub attachment_ids: Vec<String>,
    pub idempotency_key: String,
    pub request_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginTurnOutcome {
    pub message_id: String,
    pub turn_id: String,
    pub reused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentCodexContext {
    pub sanitized_context_json: String,
    pub safety_review_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCancellationTarget {
    pub turn: Turn,
    pub codex_turn_id: String,
}

impl SessionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionStoreError> {
        let connection = Connection::open(path)?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, SessionStoreError> {
        let connection = Connection::open_in_memory()?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&self) -> Result<(), SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                archived_at TEXT,
                active_turn_id TEXT,
                source_summary TEXT NOT NULL DEFAULT '',
                default_workspace_id TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                text TEXT NOT NULL,
                status TEXT NOT NULL,
                turn_id TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
                source_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                summary TEXT NOT NULL,
                sanitized_context_json TEXT NOT NULL,
                safety_review_json TEXT NOT NULL,
                source_metadata_json TEXT NOT NULL,
                safety_status TEXT NOT NULL,
                debug_available INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS turns (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                user_message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                assistant_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
                codex_thread_id TEXT,
                codex_turn_id TEXT,
                status TEXT NOT NULL,
                error_code TEXT,
                error_debug_id TEXT,
                started_at TEXT NOT NULL,
                completed_at TEXT
            );

            CREATE UNIQUE INDEX IF NOT EXISTS turns_one_active_per_session
                ON turns(session_id)
                WHERE status IN ('pending', 'running');

            CREATE TABLE IF NOT EXISTS codex_thread_links (
                session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                codex_thread_id TEXT NOT NULL,
                codex_cli_version TEXT,
                codex_schema_hash TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS idempotency_keys (
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                method TEXT NOT NULL,
                key TEXT NOT NULL,
                request_hash TEXT NOT NULL,
                message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
                turn_id TEXT REFERENCES turns(id) ON DELETE SET NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                PRIMARY KEY(session_id, method, key)
            );

            INSERT OR IGNORE INTO schema_migrations(version, applied_at)
            VALUES ('0001_initial', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
            "#,
        )?;
        Ok(())
    }

    pub fn begin_turn(&self, request: BeginTurn) -> Result<BeginTurnOutcome, SessionStoreError> {
        begin_turn::begin_turn(&self.connection, request)
    }

    pub(crate) fn ensure_session_exists(&self, session_id: &str) -> Result<(), SessionStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| SessionStoreError::LockPoisoned)?;
        ensure_session_exists(&connection, session_id)
    }
}

pub(crate) fn ensure_session_exists(
    connection: &Connection,
    session_id: &str,
) -> Result<(), SessionStoreError> {
    let exists: Option<String> = connection
        .query_row(
            "SELECT id FROM sessions WHERE id = ? AND archived_at IS NULL",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_some() {
        Ok(())
    } else {
        Err(SessionStoreError::SessionNotFound)
    }
}

pub(crate) fn prefixed_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

pub(crate) fn now_string() -> String {
    unix_seconds().to_string()
}

pub(crate) fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub(crate) fn serde_json_array(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}
