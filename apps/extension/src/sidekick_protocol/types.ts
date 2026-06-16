export const SIDEKICK_PROTOCOL_VERSION = "sidekick.protocol.v0";
export const JSONRPC_VERSION = "2.0";
export const MAX_PROTOCOL_MESSAGE_CHARS = 512 * 1024;
export const DEFAULT_MAX_REQUEST_MESSAGE_BYTES = 256 * 1024;

export interface DaemonSettings {
  url: string;
  token: string;
}

export interface SidekickSession {
  id: string;
  title: string;
}

export interface SidekickSessionSnapshot {
  session: SidekickSession;
  messages: SidekickMessage[];
  attachments: SidekickAttachment[];
  activeTurn?: SidekickTurn;
}

export interface SidekickAttachment {
  id: string;
  sessionId: string;
  summary: string;
  safetyStatus: SafetyStatus;
  debugAvailable: boolean;
}

export interface MessageSendResult {
  messageId: string;
  turnId: string;
  reused: boolean;
}

export interface InitializeResult {
  codexReadiness: CodexReadiness;
  limits: ProtocolLimits;
}

export interface ProtocolLimits {
  maxMessageBytes: number;
  maxAttachmentBytes: number;
}

export interface CodexReadiness {
  available: boolean;
  version?: string;
  errorCode?: ErrorCode;
}

export interface SidekickMessage {
  id: string;
  sessionId: string;
  role: MessageRole;
  text: string;
  status: MessageStatus;
  turnId?: string;
}

export interface SidekickTurn {
  id: string;
  sessionId: string;
  status: TurnStatus;
}

export type SidekickNotification =
  | {
      kind: "turn_delta";
      sessionId: string;
      turnId: string;
      delta: string;
    }
  | {
      kind: "turn_completed";
      sessionId: string;
      turn: SidekickTurn;
    }
  | {
      kind: "turn_failed";
      sessionId?: string;
      turn?: SidekickTurn;
      message?: string;
    }
  | {
      kind: "turn_cancelled";
      sessionId: string;
      turn: SidekickTurn;
    }
  | {
      kind: "message_created";
      sessionId: string;
      message: SidekickMessage;
    }
  | {
      kind: "error";
      error: SidekickProtocolError;
    }
  | {
      kind: "connection_lost";
      message: string;
    }
  | {
      kind: "ignored";
      method: string;
    };

export type CaptureReason = "message_send" | "manual_attach" | "debug";
export type MessageMode = "ask_only" | "repo_assisted";
export type MessageRole = "user" | "assistant" | "system_notice";
export type MessageStatus = "pending" | "streaming" | "completed" | "failed" | "cancelled";
export type TurnStatus = "pending" | "running" | "completed" | "failed" | "cancelled";
export type SafetyStatus = "clean" | "warning";
export type MessageSendIdempotencyDisposition = "discard";

export type ErrorCode =
  | "unauthorized"
  | "forbidden_origin"
  | "unsupported_protocol_version"
  | "invalid_request"
  | "invalid_params"
  | "method_not_found"
  | "payload_too_large"
  | "rate_limited"
  | "session_not_found"
  | "message_not_found"
  | "attachment_not_found"
  | "turn_not_found"
  | "turn_already_running"
  | "turn_cancel_unsupported"
  | "context_too_large"
  | "context_rejected"
  | "browser_permission_missing"
  | "browser_capture_failed"
  | "safety_review_failed"
  | "codex_not_found"
  | "codex_not_logged_in"
  | "codex_app_server_unavailable"
  | "unsupported_codex_version"
  | "codex_turn_failed"
  | "approval_required"
  | "approval_ui_not_supported"
  | "workspace_required"
  | "workspace_not_found"
  | "internal_error";

export class SidekickProtocolError extends Error {
  readonly code: ErrorCode;
  readonly retryable: boolean | undefined;
  readonly messageSendIdempotencyDisposition: MessageSendIdempotencyDisposition | undefined;

  constructor(
    code: ErrorCode,
    message: string,
    options: SidekickProtocolErrorOptions | boolean = {},
  ) {
    super(message);
    this.name = "SidekickProtocolError";
    this.code = code;
    if (typeof options === "boolean") {
      this.retryable = options;
      this.messageSendIdempotencyDisposition = undefined;
      return;
    }
    this.retryable = options.retryable;
    this.messageSendIdempotencyDisposition = options.messageSendIdempotencyDisposition;
  }
}

export interface SidekickProtocolErrorOptions {
  retryable?: boolean;
  messageSendIdempotencyDisposition?: MessageSendIdempotencyDisposition;
}

export interface WireSuccess {
  kind: "success";
  id: string;
  result: unknown;
}

export interface WireFailure {
  kind: "failure";
  id: string;
  error: SidekickProtocolError;
}

export interface WireNotification {
  kind: "notification";
  method: string;
  params: unknown;
}

export type WireMessage = WireSuccess | WireFailure | WireNotification;
