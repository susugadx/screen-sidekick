#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";
pub const SIDEKICK_PROTOCOL_VERSION: &str = "sidekick.protocol.v0";

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const SESSION_CREATE: &str = "session/create";
    pub const SESSION_LIST: &str = "session/list";
    pub const SESSION_GET: &str = "session/get";
    pub const SESSION_SUBSCRIBE: &str = "session/subscribe";
    pub const SESSION_UNSUBSCRIBE: &str = "session/unsubscribe";
    pub const CONTEXT_ATTACH_BROWSER: &str = "context/attach_browser";
    pub const MESSAGE_SEND: &str = "message/send";
    pub const TURN_CANCEL: &str = "turn/cancel";
    pub const STATUS_GET: &str = "status/get";
}

pub mod notification {
    pub const SESSION_UPDATED: &str = "session/updated";
    pub const CONTEXT_ATTACHED: &str = "context/attached";
    pub const MESSAGE_CREATED: &str = "message/created";
    pub const TURN_STARTED: &str = "turn/started";
    pub const TURN_DELTA: &str = "turn/delta";
    pub const TURN_COMPLETED: &str = "turn/completed";
    pub const TURN_FAILED: &str = "turn/failed";
    pub const TURN_CANCELLED: &str = "turn/cancelled";
    pub const STATUS_CHANGED: &str = "status/changed";
    pub const ERROR: &str = "error";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcRequest {
    #[must_use]
    pub fn new(id: impl Into<String>, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponse {
    Success(JsonRpcSuccess),
    Error(JsonRpcFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcSuccess {
    pub jsonrpc: String,
    pub id: String,
    pub result: Value,
}

impl JsonRpcSuccess {
    #[must_use]
    pub fn new(id: impl Into<String>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            result,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcFailure {
    pub jsonrpc: String,
    pub id: String,
    pub error: ProtocolError,
}

impl JsonRpcFailure {
    #[must_use]
    pub fn new(id: impl Into<String>, error: ProtocolError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcNotification {
    #[must_use]
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Box<ErrorData>>,
}

impl ProtocolError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: ErrorData) -> Self {
        self.data = Some(Box::new(data));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    ForbiddenOrigin,
    UnsupportedProtocolVersion,
    InvalidRequest,
    InvalidParams,
    MethodNotFound,
    PayloadTooLarge,
    RateLimited,
    SessionNotFound,
    MessageNotFound,
    AttachmentNotFound,
    TurnNotFound,
    TurnAlreadyRunning,
    TurnCancelUnsupported,
    ContextTooLarge,
    ContextRejected,
    BrowserPermissionMissing,
    BrowserCaptureFailed,
    SafetyReviewFailed,
    CodexNotFound,
    CodexNotLoggedIn,
    CodexAppServerUnavailable,
    UnsupportedCodexVersion,
    CodexTurnFailed,
    ApprovalRequired,
    ApprovalUiNotSupported,
    WorkspaceRequired,
    WorkspaceNotFound,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ErrorData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_debug_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_permission: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_versions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_send_idempotency_disposition: Option<MessageSendIdempotencyDisposition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSendIdempotencyDisposition {
    Discard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    ChromeExtension,
    TauriDesktop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientCapability {
    BrowserContext,
    DesktopContext,
    ChatStream,
    TurnCancel,
    ApprovalUi,
    BrowserActions,
    DebugExport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams {
    pub client_kind: ClientKind,
    pub client_version: String,
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<ClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub daemon_version: String,
    pub capabilities: Vec<ClientCapability>,
    pub auth_status: AuthStatus,
    pub codex_readiness: CodexReadiness,
    pub limits: ProtocolLimits,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexReadiness {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolLimits {
    pub max_message_bytes: usize,
    pub max_attachment_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIdParams {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCreateResult {
    pub session: SessionSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListResult {
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGetResult {
    pub session: SessionSummary,
    pub messages: Vec<Message>,
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn: Option<Turn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachBrowserContextParams {
    pub session_id: String,
    pub capture_id: String,
    pub raw_context: Value,
    pub capture_reason: CaptureReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachBrowserContextResult {
    pub attachment: Attachment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureReason {
    MessageSend,
    ManualAttach,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSendParams {
    pub session_id: String,
    pub text: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    #[serde(default)]
    pub capture_current_context: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_binding: Option<String>,
    #[serde(default)]
    pub mode: MessageMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSendResult {
    pub message_id: String,
    pub turn_id: String,
    pub reused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    #[default]
    AskOnly,
    RepoAssisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCancelParams {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusGetResult {
    pub codex_readiness: CodexReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    pub source_summary: String,
    pub codex_status: CodexStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexStatus {
    NotStarted,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub created_at: String,
    pub text: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub status: MessageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    SystemNotice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Pending,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub session_id: String,
    pub source_type: AttachmentSourceType,
    pub created_at: String,
    pub summary: String,
    pub safety_status: SafetyStatus,
    pub debug_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentSourceType {
    BrowserTab,
    DesktopScreen,
    ManualText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyStatus {
    Clean,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub session_id: String,
    pub user_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message_id: Option<String>,
    pub status: TurnStatus,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnDeltaNotification {
    pub session_id: String,
    pub turn_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnNotification {
    pub session_id: String,
    pub turn: Turn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnFailedNotification {
    pub session_id: String,
    pub turn: Turn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCreatedNotification {
    pub session_id: String,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAttachedNotification {
    pub session_id: String,
    pub attachment: Attachment,
}
