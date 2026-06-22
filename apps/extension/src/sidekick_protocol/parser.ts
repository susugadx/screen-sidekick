import {
  JSONRPC_VERSION,
  SidekickProtocolError,
  type CodexReadiness,
  type ErrorCode,
  type InitializeResult,
  type MessageSendIdempotencyDisposition,
  type MessageRole,
  type MessageSendResult,
  type MessageStatus,
  type ProtocolLimits,
  type SidekickProtocolErrorOptions,
  type SafetyStatus,
  type SidekickAttachment,
  type SidekickMessage,
  type SidekickNotification,
  type SidekickSession,
  type SidekickSessionSnapshot,
  type SidekickTurn,
  type TurnStatus,
  type WireMessage,
} from "./types.js";

export function parseWireMessageText(text: string): WireMessage | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  return parseWireMessage(value);
}

export function parseSidekickNotification(
  method: string,
  params: unknown,
): SidekickNotification {
  switch (method) {
    case "turn/delta": {
      const parsed = parseTurnDeltaNotification(params);
      return parsed ?? ignoredNotification(method);
    }
    case "turn/completed": {
      const parsed = parseTurnNotification(params);
      return parsed
        ? { kind: "turn_completed", sessionId: parsed.sessionId, turn: parsed.turn }
        : ignoredNotification(method);
    }
    case "turn/failed": {
      const parsed = parseTurnFailedNotification(params);
      return parsed ?? ignoredNotification(method);
    }
    case "turn/cancelled": {
      const parsed = parseTurnNotification(params);
      return parsed
        ? { kind: "turn_cancelled", sessionId: parsed.sessionId, turn: parsed.turn }
        : ignoredNotification(method);
    }
    case "message/created": {
      const parsed = parseMessageCreatedNotification(params);
      return parsed ?? ignoredNotification(method);
    }
    case "error": {
      const error = parseProtocolError(params);
      return {
        kind: "error",
        error: error ?? new SidekickProtocolError("internal_error", "Daemon reported an error"),
      };
    }
    case "session/updated":
    case "context/attached":
    case "turn/started":
    case "status/changed":
      return ignoredNotification(method);
    default:
      return ignoredNotification(method);
  }
}

export function parseSessionCreateResult(value: unknown): SidekickSession | null {
  if (!isRecord(value)) {
    return null;
  }
  return parseSessionSummary(value.session);
}

export function parseSessionGetResult(value: unknown): SidekickSessionSnapshot | null {
  if (!isRecord(value)) {
    return null;
  }
  const session = parseSessionSummary(value.session);
  const messages = parseArray(value.messages, parseMessage);
  const attachments = parseArray(value.attachments, parseAttachment);
  const activeTurn =
    value.active_turn === undefined || value.active_turn === null
      ? undefined
      : parseTurn(value.active_turn);
  if (!session || !messages || !attachments || activeTurn === null) {
    return null;
  }
  const snapshot: SidekickSessionSnapshot = {
    session,
    messages,
    attachments,
  };
  if (activeTurn) {
    snapshot.activeTurn = activeTurn;
  }
  return snapshot;
}

export function parseAttachBrowserContextResult(value: unknown): SidekickAttachment | null {
  if (!isRecord(value)) {
    return null;
  }
  return parseAttachment(value.attachment);
}

export function parseMessageSendResult(value: unknown): MessageSendResult | null {
  if (!isRecord(value)) {
    return null;
  }
  const messageId = getString(value, "message_id");
  const turnId = getString(value, "turn_id");
  const reused = getBoolean(value, "reused");
  if (!messageId || !turnId || reused === null) {
    return null;
  }
  return {
    messageId,
    turnId,
    reused,
  };
}

export function parseInitializeResult(value: unknown): InitializeResult | null {
  if (!isRecord(value)) {
    return null;
  }
  const codexReadiness = parseCodexReadiness(value.codex_readiness);
  const limits = parseProtocolLimits(value.limits);
  if (!codexReadiness || !limits) {
    return null;
  }
  return { codexReadiness, limits };
}

export function parseIgnoredResult(_value: unknown): Record<string, never> {
  return {};
}

function parseWireMessage(value: unknown): WireMessage | null {
  if (!isRecord(value)) {
    return null;
  }

  if (getString(value, "jsonrpc") !== JSONRPC_VERSION) {
    return null;
  }

  const id = getString(value, "id");
  if (id) {
    if (Object.hasOwn(value, "result")) {
      return {
        kind: "success",
        id,
        result: value.result,
      };
    }
    const error = parseProtocolError(value.error);
    return error ? { kind: "failure", id, error } : null;
  }

  const method = getString(value, "method");
  if (!method) {
    return null;
  }
  return {
    kind: "notification",
    method,
    params: value.params,
  };
}

function parseProtocolError(value: unknown): SidekickProtocolError | null {
  if (!isRecord(value)) {
    return null;
  }
  const code = parseErrorCode(value.code);
  const message = getString(value, "message");
  if (!code || !message) {
    return null;
  }
  const data = isRecord(value.data) ? value.data : null;
  const retryable = data ? getBoolean(data, "retryable") : null;
  const messageSendIdempotencyDisposition = data
    ? parseMessageSendIdempotencyDisposition(data.message_send_idempotency_disposition)
    : undefined;
  const options: SidekickProtocolErrorOptions = {};
  if (retryable !== null) {
    options.retryable = retryable;
  }
  if (messageSendIdempotencyDisposition) {
    options.messageSendIdempotencyDisposition = messageSendIdempotencyDisposition;
  }
  return new SidekickProtocolError(code, message, options);
}

function parseMessageSendIdempotencyDisposition(
  value: unknown,
): MessageSendIdempotencyDisposition | undefined {
  return value === "discard" ? value : undefined;
}

function parseCodexReadiness(value: unknown): CodexReadiness | null {
  if (!isRecord(value)) {
    return null;
  }
  const available = getBoolean(value, "available");
  if (available === null) {
    return null;
  }

  const readiness: CodexReadiness = { available };
  const version = getString(value, "version");
  if (version) {
    readiness.version = version;
  }
  if (Object.hasOwn(value, "error_code")) {
    const errorCode = parseErrorCode(value.error_code);
    if (!errorCode) {
      return null;
    }
    readiness.errorCode = errorCode;
  }
  return readiness;
}

function parseProtocolLimits(value: unknown): ProtocolLimits | null {
  if (!isRecord(value)) {
    return null;
  }
  const maxMessageBytes = getPositiveInteger(value, "max_message_bytes");
  const maxAttachmentBytes = getPositiveInteger(value, "max_attachment_bytes");
  if (maxMessageBytes === null || maxAttachmentBytes === null) {
    return null;
  }
  return {
    maxMessageBytes,
    maxAttachmentBytes,
  };
}

function parseTurnDeltaNotification(value: unknown): SidekickNotification | null {
  if (!isRecord(value)) {
    return null;
  }
  const sessionId = getString(value, "session_id");
  const turnId = getString(value, "turn_id");
  const delta = getStringAllowEmpty(value, "delta");
  if (!sessionId || !turnId || delta === null) {
    return null;
  }
  return {
    kind: "turn_delta",
    sessionId,
    turnId,
    delta,
  };
}

function parseTurnNotification(value: unknown): { sessionId: string; turn: SidekickTurn } | null {
  if (!isRecord(value)) {
    return null;
  }
  const sessionId = getString(value, "session_id");
  const turn = parseTurn(value.turn);
  if (!sessionId || !turn) {
    return null;
  }
  return { sessionId, turn };
}

function parseTurnFailedNotification(value: unknown): SidekickNotification | null {
  const parsed = parseTurnNotification(value);
  if (parsed) {
    const message = isRecord(value) ? getString(value, "message") : null;
    const notification: SidekickNotification = {
      kind: "turn_failed",
      sessionId: parsed.sessionId,
      turn: parsed.turn,
    };
    if (message) {
      notification.message = message;
    }
    return {
      ...notification,
    };
  }
  if (!isRecord(value)) {
    return null;
  }
  const sessionId = getString(value, "session_id");
  const message = getString(value, "message");
  const notification: SidekickNotification = {
    kind: "turn_failed",
  };
  if (sessionId) {
    notification.sessionId = sessionId;
  }
  if (message) {
    notification.message = message;
  }
  return notification;
}

function parseMessageCreatedNotification(value: unknown): SidekickNotification | null {
  if (!isRecord(value)) {
    return null;
  }
  const sessionId = getString(value, "session_id");
  const message = parseMessage(value.message);
  if (!sessionId || !message) {
    return null;
  }
  return {
    kind: "message_created",
    sessionId,
    message,
  };
}

function parseSessionSummary(value: unknown): SidekickSession | null {
  if (!isRecord(value)) {
    return null;
  }
  const id = getString(value, "id");
  const title = getString(value, "title");
  if (!id || !title) {
    return null;
  }
  return { id, title };
}

function parseAttachment(value: unknown): SidekickAttachment | null {
  if (!isRecord(value)) {
    return null;
  }
  const id = getString(value, "id");
  const sessionId = getString(value, "session_id");
  const summary = getString(value, "summary");
  const safetyStatus = parseSafetyStatus(value.safety_status);
  const debugAvailable = getBoolean(value, "debug_available");
  if (!id || !sessionId || !summary || !safetyStatus || debugAvailable === null) {
    return null;
  }
  return {
    id,
    sessionId,
    summary,
    safetyStatus,
    debugAvailable,
  };
}

function parseMessage(value: unknown): SidekickMessage | null {
  if (!isRecord(value)) {
    return null;
  }
  const id = getString(value, "id");
  const sessionId = getString(value, "session_id");
  const role = parseMessageRole(value.role);
  const text = getStringAllowEmpty(value, "text");
  const status = parseMessageStatus(value.status);
  const turnId = getString(value, "turn_id");
  if (!id || !sessionId || !role || text === null || !status) {
    return null;
  }
  const message: SidekickMessage = {
    id,
    sessionId,
    role,
    text,
    status,
  };
  if (turnId) {
    message.turnId = turnId;
  }
  return message;
}

function parseTurn(value: unknown): SidekickTurn | null {
  if (!isRecord(value)) {
    return null;
  }
  const id = getString(value, "id");
  const sessionId = getString(value, "session_id");
  const status = parseTurnStatus(value.status);
  if (!id || !sessionId || !status) {
    return null;
  }
  return { id, sessionId, status };
}

function parseArray<T>(value: unknown, parseItem: (item: unknown) => T | null): T[] | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const parsed: T[] = [];
  for (const item of value) {
    const parsedItem = parseItem(item);
    if (!parsedItem) {
      return null;
    }
    parsed.push(parsedItem);
  }
  return parsed;
}

function ignoredNotification(method: string): SidekickNotification {
  return { kind: "ignored", method };
}

function parseErrorCode(value: unknown): ErrorCode | null {
  if (typeof value !== "string") {
    return null;
  }

  switch (value) {
    case "unauthorized":
    case "forbidden_origin":
    case "unsupported_protocol_version":
    case "setup_required":
    case "invalid_request":
    case "invalid_params":
    case "method_not_found":
    case "payload_too_large":
    case "rate_limited":
    case "session_not_found":
    case "message_not_found":
    case "attachment_not_found":
    case "turn_not_found":
    case "turn_already_running":
    case "turn_cancel_unsupported":
    case "context_too_large":
    case "context_rejected":
    case "browser_permission_missing":
    case "browser_capture_failed":
    case "safety_review_failed":
    case "codex_not_found":
    case "codex_not_logged_in":
    case "codex_app_server_unavailable":
    case "unsupported_codex_version":
    case "codex_turn_failed":
    case "approval_required":
    case "approval_ui_not_supported":
    case "workspace_required":
    case "workspace_not_found":
    case "internal_error":
      return value;
    default:
      return null;
  }
}

function parseSafetyStatus(value: unknown): SafetyStatus | null {
  return value === "clean" || value === "warning" ? value : null;
}

function parseMessageRole(value: unknown): MessageRole | null {
  return value === "user" || value === "assistant" || value === "system_notice" ? value : null;
}

function parseMessageStatus(value: unknown): MessageStatus | null {
  return value === "pending" ||
    value === "streaming" ||
    value === "completed" ||
    value === "failed" ||
    value === "cancelled"
    ? value
    : null;
}

function parseTurnStatus(value: unknown): TurnStatus | null {
  return value === "pending" ||
    value === "running" ||
    value === "completed" ||
    value === "failed" ||
    value === "cancelled"
    ? value
    : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function getStringAllowEmpty(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" ? value : null;
}

function getBoolean(record: Record<string, unknown>, key: string): boolean | null {
  const value = record[key];
  return typeof value === "boolean" ? value : null;
}

function getPositiveInteger(record: Record<string, unknown>, key: string): number | null {
  const value = record[key];
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0
    ? value
    : null;
}
