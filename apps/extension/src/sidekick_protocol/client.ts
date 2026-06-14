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
  JSONRPC_VERSION,
  MAX_PROTOCOL_MESSAGE_CHARS,
  SIDEKICK_PROTOCOL_VERSION,
  SidekickProtocolError,
  type CaptureReason,
  type DaemonSettings,
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
};

type NotificationHandler = (notification: SidekickNotification) => void;

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

interface WireRequest {
  jsonrpc: typeof JSONRPC_VERSION;
  id: string;
  method: string;
  params: unknown;
}

export class SidekickProtocolClient {
  private readonly pending = new Map<string, PendingRequest>();
  private readonly notificationHandlers = new Set<NotificationHandler>();
  private closedIntentionally = false;
  private connectionLostReported = false;
  private requestCounter = 0;

  private constructor(
    private readonly socket: WebSocket,
    readonly wsUrl: string,
    private readonly token: string,
  ) {
    this.socket.addEventListener("message", (event) => {
      this.handleMessage(event.data);
    });
    this.socket.addEventListener("close", () => {
      const error = new Error("Daemon WebSocket closed");
      this.failPending(error);
      this.reportConnectionLost(error);
    });
    this.socket.addEventListener("error", () => {
      const error = new Error("Daemon WebSocket failed");
      this.failPending(error);
      this.reportConnectionLost(error);
    });
  }

  static async connect(
    settings: DaemonSettings,
    clientVersion: string,
  ): Promise<SidekickProtocolClient> {
    const wsUrl = buildDaemonWebSocketUrl(settings.url);
    const socket = await openWebSocket(wsUrl);
    const client = new SidekickProtocolClient(socket, wsUrl.toString(), settings.token);
    try {
      await client.initialize(clientVersion);
      return client;
    } catch (error) {
      client.close();
      throw error;
    }
  }

  matches(settings: DaemonSettings): boolean {
    return (
      this.isOpen() &&
      this.wsUrl === buildDaemonWebSocketUrl(settings.url).toString() &&
      this.token === settings.token
    );
  }

  onNotification(handler: NotificationHandler): () => void {
    this.notificationHandlers.add(handler);
    return () => {
      this.notificationHandlers.delete(handler);
    };
  }

  close(): void {
    this.closedIntentionally = true;
    this.socket.close();
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
    return this.request("session/get", { session_id: sessionId }, parseSessionGetResult);
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
    attachmentIds: string[],
    mode: MessageMode,
  ): Promise<MessageSendResult> {
    const params: MessageSendRequest = {
      session_id: sessionId,
      text,
      idempotency_key: createClientId("idem"),
      attachment_ids: attachmentIds,
      capture_current_context: false,
      mode,
    };
    return this.request("message/send", params, parseMessageSendResult);
  }

  private async initialize(clientVersion: string): Promise<void> {
    const result = await this.request(
      "initialize",
      {
        client_kind: "chrome_extension",
        client_version: clientVersion,
        protocol_version: SIDEKICK_PROTOCOL_VERSION,
        auth_token: this.token,
        capabilities: ["browser_context", "chat_stream", "debug_export"],
      },
      parseInitializeResult,
    );
    if (!result.codexReadiness.available) {
      const code = result.codexReadiness.errorCode ?? "codex_app_server_unavailable";
      throw new SidekickProtocolError(code, codexReadinessErrorMessage(code));
    }
  }

  private request<T>(
    method: string,
    params: unknown,
    parseResult: (value: unknown) => T | null,
  ): Promise<T> {
    if (!this.isOpen()) {
      return Promise.reject(new Error("Daemon WebSocket is not open"));
    }

    const id = `sidekick_extension_${++this.requestCounter}`;
    const request: WireRequest = {
      jsonrpc: JSONRPC_VERSION,
      id,
      method,
      params,
    };

    return new Promise<T>((resolve, reject) => {
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
      });
      this.socket.send(JSON.stringify(request));
    });
  }

  private isOpen(): boolean {
    return this.socket.readyState === WebSocket.OPEN;
  }

  private handleMessage(data: unknown): void {
    if (typeof data !== "string") {
      this.failPending(new Error("Daemon sent an unsupported WebSocket message"));
      return;
    }
    if (data.length > MAX_PROTOCOL_MESSAGE_CHARS) {
      this.failPending(new Error("Daemon WebSocket message is too large"));
      return;
    }

    const parsed = parseWireMessageText(data);
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
    const pending = this.pending.get(id);
    if (!pending) {
      return;
    }
    this.pending.delete(id);
    pending.resolve(result);
  }

  private rejectPending(id: string, error: SidekickProtocolError): void {
    const pending = this.pending.get(id);
    if (!pending) {
      return;
    }
    this.pending.delete(id);
    pending.reject(error);
  }

  private dispatchNotification(notification: SidekickNotification): void {
    for (const handler of this.notificationHandlers) {
      handler(notification);
    }
  }

  private failPending(error: Error): void {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
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

export function buildDaemonWebSocketUrl(rawDaemonUrl: string): URL {
  let daemonUrl: URL;
  try {
    daemonUrl = new URL(rawDaemonUrl);
  } catch {
    throw new Error("Daemon URL is invalid");
  }

  if (
    daemonUrl.hostname !== "127.0.0.1" ||
    daemonUrl.port.length === 0 ||
    (daemonUrl.protocol !== "http:" && daemonUrl.protocol !== "ws:")
  ) {
    throw new Error("Daemon URL must use http://127.0.0.1:<port> or ws://127.0.0.1:<port>");
  }

  daemonUrl.protocol = "ws:";
  daemonUrl.pathname = "/v0/ws";
  daemonUrl.search = "";
  daemonUrl.hash = "";
  return daemonUrl;
}

function openWebSocket(url: URL): Promise<WebSocket> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.addEventListener(
      "open",
      () => {
        resolve(socket);
      },
      { once: true },
    );
    socket.addEventListener(
      "error",
      () => {
        reject(new Error("Daemon WebSocket failed to open"));
      },
      { once: true },
    );
  });
}

function createClientId(prefix: string): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  if (randomId) {
    return `${prefix}_${randomId}`;
  }
  return `${prefix}_${Date.now()}_${Math.floor(Math.random() * 1_000_000_000)}`;
}
