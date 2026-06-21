import {
  serializeCaptureContextForBridge,
  type RawBrowserContext,
} from "../capture_contract.js";
import {
  parseAttachBrowserContextResult,
  parseIgnoredResult,
  parseInitializeResult,
  parseMessageSendResult,
  parseSessionCreateResult,
  parseSessionGetResult,
  parseSidekickNotification,
  parseWireMessageText,
} from "./parser.js";
import {
  DEFAULT_MAX_REQUEST_MESSAGE_BYTES,
  JSONRPC_VERSION,
  MAX_PROTOCOL_MESSAGE_CHARS,
  SIDEKICK_PROTOCOL_VERSION,
  SidekickProtocolError,
  type CaptureReason,
  type ErrorCode,
  type MessageMode,
  type MessageSendResult,
  type SidekickAttachment,
  type SidekickNotification,
  type SidekickSession,
  type SidekickSessionSnapshot,
} from "./types.js";

type PendingRequest = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timeoutId: number | null;
};

type NotificationHandler = (notification: SidekickNotification) => void;
export type SidekickRequestMethod =
  | "initialize"
  | "session/create"
  | "session/subscribe"
  | "session/get"
  | "context/attach_browser"
  | "message/send";

const DAEMON_INITIALIZE_TIMEOUT_MS = 60_000;
const DAEMON_CONTROL_REQUEST_TIMEOUT_MS = 10_000;
const DAEMON_MESSAGE_SEND_TIMEOUT_MS = 45_000;
const DEFAULT_REQUEST_TOO_LARGE_MESSAGE =
  "Sidekick request is too large for the transport limit.";
const LEGACY_TERMINAL_MESSAGE_SEND_REPLAY_MESSAGES = new Set([
  "Previous message/send attempt failed.",
  "Previous message/send attempt was cancelled.",
]);
const textEncoder = new TextEncoder();

interface AttachBrowserContextRequest {
  session_id: string;
  capture_id: string;
  raw_context: unknown;
  capture_reason: CaptureReason;
  related_message_id?: string;
}

interface MessageSendRequest {
  session_id: string;
  text: string;
  idempotency_key: string;
  attachment_ids: string[];
  capture_current_context: boolean;
  mode: MessageMode;
}

export interface SidekickClient {
  onNotification(handler: NotificationHandler): () => void;
  close(): void;
  createSession(title: string): Promise<SidekickSession>;
  subscribeSession(sessionId: string): Promise<void>;
  getSession(sessionId: string): Promise<SidekickSessionSnapshot>;
  attachBrowserContext(
    sessionId: string,
    context: RawBrowserContext,
    captureReason: CaptureReason,
    relatedMessageId?: string,
  ): Promise<SidekickAttachment>;
  sendMessage(
    sessionId: string,
    text: string,
    idempotencyKey: string,
    attachmentIds: string[],
    mode: MessageMode,
  ): Promise<MessageSendResult>;
}

export interface ProtocolTransport {
  isOpen(): boolean;
  send(text: string): void;
  close(): void;
  onMessage(handler: (data: unknown) => void): void;
  onDisconnect(handler: (error: Error) => void): void;
}

export interface SidekickProtocolClientOptions {
  authToken?: string;
  extensionId?: string;
  requestTooLargeMessage?: string;
  notOpenMessage?: string;
}

interface WireRequest {
  jsonrpc: typeof JSONRPC_VERSION;
  id: string;
  method: SidekickRequestMethod;
  params: unknown;
}

export class SidekickProtocolClient implements SidekickClient {
  private readonly pending = new Map<string, PendingRequest>();
  private readonly notificationHandlers = new Set<NotificationHandler>();
  private closedIntentionally = false;
  private connectionLostReported = false;
  private requestCounter = 0;
  private maxRequestMessageBytes = DEFAULT_MAX_REQUEST_MESSAGE_BYTES;
  private readonly authToken: string | undefined;
  private readonly extensionId: string | undefined;
  private readonly requestTooLargeMessage: string;
  private readonly notOpenMessage: string;

  protected constructor(
    private readonly transport: ProtocolTransport,
    options: SidekickProtocolClientOptions = {},
  ) {
    this.authToken = options.authToken;
    this.extensionId = options.extensionId;
    this.requestTooLargeMessage =
      options.requestTooLargeMessage ?? DEFAULT_REQUEST_TOO_LARGE_MESSAGE;
    this.notOpenMessage = options.notOpenMessage ?? "Sidekick transport is not open";

    this.transport.onMessage((data) => {
      this.handleMessage(data);
    });
    this.transport.onDisconnect((error) => {
      this.failPending(error);
      this.reportConnectionLost(error);
    });
  }

  onNotification(handler: NotificationHandler): () => void {
    this.notificationHandlers.add(handler);
    return () => {
      this.notificationHandlers.delete(handler);
    };
  }

  close(): void {
    this.closedIntentionally = true;
    this.transport.close();
  }

  async createSession(title: string): Promise<SidekickSession> {
    return this.request("session/create", { title }, parseSessionCreateResult);
  }

  async subscribeSession(sessionId: string): Promise<void> {
    await this.request(
      "session/subscribe",
      { session_id: sessionId },
      parseIgnoredResult,
    );
  }

  async getSession(sessionId: string): Promise<SidekickSessionSnapshot> {
    return this.request(
      "session/get",
      { session_id: sessionId },
      parseSessionGetResult,
    );
  }

  async attachBrowserContext(
    sessionId: string,
    context: RawBrowserContext,
    captureReason: CaptureReason,
    relatedMessageId?: string,
  ): Promise<SidekickAttachment> {
    const rawContext: unknown = JSON.parse(serializeCaptureContextForBridge(context));
    const params: AttachBrowserContextRequest = {
      session_id: sessionId,
      capture_id: createClientId("cap"),
      raw_context: rawContext,
      capture_reason: captureReason,
    };
    if (relatedMessageId) {
      params.related_message_id = relatedMessageId;
    }
    return this.request("context/attach_browser", params, parseAttachBrowserContextResult);
  }

  async sendMessage(
    sessionId: string,
    text: string,
    idempotencyKey: string,
    attachmentIds: string[],
    mode: MessageMode,
  ): Promise<MessageSendResult> {
    const params: MessageSendRequest = {
      session_id: sessionId,
      text,
      idempotency_key: idempotencyKey,
      attachment_ids: attachmentIds,
      capture_current_context: false,
      mode,
    };
    return this.request("message/send", params, parseMessageSendResult);
  }

  protected async initializeConnection(clientVersion: string): Promise<void> {
    const params: Record<string, unknown> = {
      client_kind: "chrome_extension",
      client_version: clientVersion,
      protocol_version: SIDEKICK_PROTOCOL_VERSION,
      capabilities: ["browser_context", "chat_stream", "debug_export"],
    };
    if (this.authToken) {
      params.auth_token = this.authToken;
    }
    if (this.extensionId) {
      params.extension_id = this.extensionId;
      params.origin = `chrome-extension://${this.extensionId}/`;
    }

    const result = await this.request(
      "initialize",
      params,
      parseInitializeResult,
    );
    this.maxRequestMessageBytes = result.limits.maxMessageBytes;
    if (!result.codexReadiness.available) {
      const code = result.codexReadiness.errorCode ?? "codex_app_server_unavailable";
      throw new SidekickProtocolError(code, codexReadinessErrorMessage(code));
    }
  }

  private request<T>(
    method: SidekickRequestMethod,
    params: unknown,
    parseResult: (value: unknown) => T | null,
  ): Promise<T> {
    if (!this.isOpen()) {
      return Promise.reject(new Error(this.notOpenMessage));
    }

    const id = `sidekick_extension_${++this.requestCounter}`;
    const request: WireRequest = {
      jsonrpc: JSONRPC_VERSION,
      id,
      method,
      params,
    };
    const serializedRequest = JSON.stringify(request);
    if (requestByteLength(serializedRequest) > this.maxRequestMessageBytes) {
      return Promise.reject(
        new SidekickProtocolError("payload_too_large", this.requestTooLargeMessage),
      );
    }

    return new Promise<T>((resolve, reject) => {
      const timeoutId = globalThis.setTimeout(() => {
        this.handleRequestTimeout(id, method);
      }, daemonRequestTimeoutMs(method));
      this.pending.set(id, {
        resolve: (value) => {
          const parsed = parseResult(value);
          if (!parsed) {
            reject(new Error(`Daemon response for ${method} is invalid`));
            return;
          }
          resolve(parsed);
        },
        reject,
        timeoutId,
      });
      try {
        this.transport.send(serializedRequest);
      } catch (error) {
        const pending = this.takePending(id);
        if (!pending) {
          return;
        }
        pending.reject(error instanceof Error ? error : new Error("Daemon request failed"));
      }
    });
  }

  private isOpen(): boolean {
    return this.transport.isOpen();
  }

  private handleMessage(data: unknown): void {
    const text = wireMessageText(data);
    if (text === null) {
      this.failPending(new Error("Sidekick transport sent an unsupported message"));
      return;
    }
    if (text.length > MAX_PROTOCOL_MESSAGE_CHARS) {
      this.failPending(new Error("Sidekick transport message is too large"));
      return;
    }

    const parsed = parseWireMessageText(text);
    if (!parsed) {
      const error = new SidekickProtocolError(
        "invalid_request",
        "Daemon message shape is invalid",
      );
      this.failPending(error);
      this.dispatchNotification({
        kind: "error",
        error,
      });
      return;
    }

    switch (parsed.kind) {
      case "success":
        this.resolvePending(parsed.id, parsed.result);
        return;
      case "failure":
        this.rejectPending(parsed.id, parsed.error);
        return;
      case "notification":
        this.dispatchNotification(parseSidekickNotification(parsed.method, parsed.params));
        return;
    }
  }

  private resolvePending(id: string, result: unknown): void {
    const pending = this.takePending(id);
    if (!pending) {
      return;
    }
    pending.resolve(result);
  }

  private rejectPending(id: string, error: Error): void {
    const pending = this.takePending(id);
    if (!pending) {
      return;
    }
    pending.reject(error);
  }

  private handleRequestTimeout(id: string, method: SidekickRequestMethod): void {
    const pending = this.takePending(id);
    if (!pending) {
      return;
    }
    const error = new SidekickRequestTimeoutError(method);
    pending.reject(error);
    if (method !== "message/send") {
      return;
    }
    this.reportConnectionLost(error);
    this.transport.close();
  }

  private dispatchNotification(notification: SidekickNotification): void {
    for (const handler of this.notificationHandlers) {
      handler(notification);
    }
  }

  private failPending(error: Error): void {
    const pendingRequests = [...this.pending.values()];
    this.pending.clear();
    for (const pending of pendingRequests) {
      clearPendingTimeout(pending);
      pending.reject(error);
    }
  }

  private takePending(id: string): PendingRequest | null {
    const pending = this.pending.get(id);
    if (!pending) {
      return null;
    }
    this.pending.delete(id);
    clearPendingTimeout(pending);
    return pending;
  }

  private reportConnectionLost(error: Error): void {
    if (this.closedIntentionally || this.connectionLostReported) {
      return;
    }
    this.connectionLostReported = true;
    this.dispatchNotification({
      kind: "connection_lost",
      message: error.message,
    });
  }
}

class SidekickRequestTimeoutError extends Error {
  readonly method: string;

  constructor(method: string) {
    super("Daemon request timed out");
    this.name = "SidekickRequestTimeoutError";
    this.method = method;
  }
}

function daemonRequestTimeoutMs(method: SidekickRequestMethod): number {
  switch (method) {
    case "initialize":
      return DAEMON_INITIALIZE_TIMEOUT_MS;
    case "message/send":
      return DAEMON_MESSAGE_SEND_TIMEOUT_MS;
    case "session/create":
    case "session/subscribe":
    case "session/get":
    case "context/attach_browser":
      return DAEMON_CONTROL_REQUEST_TIMEOUT_MS;
  }
}

function wireMessageText(data: unknown): string | null {
  if (typeof data === "string") {
    return data;
  }
  if (typeof data !== "object" || data === null) {
    return null;
  }
  try {
    return JSON.stringify(data);
  } catch {
    return null;
  }
}

function codexReadinessErrorMessage(code: ErrorCode): string {
  switch (code) {
    case "codex_not_found":
      return "Codex CLI was not found.";
    case "codex_not_logged_in":
      return "Codex is not logged in.";
    case "unsupported_codex_version":
      return "Codex app-server version is unsupported.";
    case "codex_app_server_unavailable":
      return "Codex app-server is unavailable.";
    default:
      return "Codex app-server is unavailable.";
  }
}

export function createMessageSendIdempotencyKey(): string {
  return createClientId("idem");
}

export function isTerminalMessageSendReplayError(error: unknown): boolean {
  return (
    error instanceof SidekickProtocolError &&
    (error.messageSendIdempotencyDisposition === "discard" ||
      LEGACY_TERMINAL_MESSAGE_SEND_REPLAY_MESSAGES.has(error.message))
  );
}

export function isMessageSendRequestTimeoutError(error: unknown): boolean {
  if (error instanceof SidekickRequestTimeoutError) {
    return error.method === "message/send";
  }
  if (!(error instanceof Error)) {
    return false;
  }
  const maybeTimeout = error as Error & { method?: unknown };
  return (
    maybeTimeout.name === "SidekickRequestTimeoutError" &&
    maybeTimeout.method === "message/send"
  );
}

function requestByteLength(value: string): number {
  return textEncoder.encode(value).byteLength;
}

function clearPendingTimeout(pending: PendingRequest): void {
  if (pending.timeoutId !== null) {
    globalThis.clearTimeout(pending.timeoutId);
  }
}

function createClientId(prefix: string): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  if (randomId) {
    return `${prefix}_${randomId}`;
  }
  return `${prefix}_${Date.now()}_${Math.floor(Math.random() * 1_000_000_000)}`;
}
