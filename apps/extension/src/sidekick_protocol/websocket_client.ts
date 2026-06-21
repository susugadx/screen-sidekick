import {
  SidekickProtocolClient,
  type ProtocolTransport,
} from "./core.js";
import type { DaemonSettings } from "./types.js";

const WEBSOCKET_REQUEST_TOO_LARGE_MESSAGE =
  "Daemon request is too large for the WebSocket limit.";

type MessageHandler = (data: unknown) => void;
type DisconnectHandler = (error: Error) => void;

export class WebSocketSidekickClient extends SidekickProtocolClient {
  readonly wsUrl: string;

  private constructor(
    private readonly transportAdapter: WebSocketTransport,
    readonly settings: DaemonSettings,
  ) {
    const options = {
      authToken: settings.token,
      notOpenMessage: "Daemon WebSocket is not open",
      requestTooLargeMessage: WEBSOCKET_REQUEST_TOO_LARGE_MESSAGE,
    };
    const extensionId = currentExtensionId();
    super(
      transportAdapter,
      extensionId ? { ...options, extensionId } : options,
    );
    this.wsUrl = transportAdapter.wsUrl;
  }

  static async connect(
    settings: DaemonSettings,
    clientVersion: string,
  ): Promise<WebSocketSidekickClient> {
    const wsUrl = buildDaemonWebSocketUrl(settings.url);
    const socket = await openWebSocket(wsUrl);
    const client = new WebSocketSidekickClient(
      new WebSocketTransport(socket, wsUrl.toString()),
      settings,
    );
    try {
      await client.initializeConnection(clientVersion);
      return client;
    } catch (error) {
      client.close();
      throw error;
    }
  }

  matches(settings: DaemonSettings): boolean {
    return (
      this.transportAdapter.isOpen() &&
      this.wsUrl === buildDaemonWebSocketUrl(settings.url).toString() &&
      this.settings.token === settings.token
    );
  }
}

class WebSocketTransport implements ProtocolTransport {
  private readonly messageHandlers = new Set<MessageHandler>();
  private readonly disconnectHandlers = new Set<DisconnectHandler>();

  constructor(
    private readonly socket: WebSocket,
    readonly wsUrl: string,
  ) {
    this.socket.addEventListener("message", (event) => {
      for (const handler of this.messageHandlers) {
        handler(event.data);
      }
    });
    this.socket.addEventListener("close", () => {
      this.dispatchDisconnect(new Error("Daemon WebSocket closed"));
    });
    this.socket.addEventListener("error", () => {
      this.dispatchDisconnect(new Error("Daemon WebSocket failed"));
    });
  }

  isOpen(): boolean {
    return this.socket.readyState === WebSocket.OPEN;
  }

  send(text: string): void {
    this.socket.send(text);
  }

  close(): void {
    this.socket.close();
  }

  onMessage(handler: MessageHandler): void {
    this.messageHandlers.add(handler);
  }

  onDisconnect(handler: DisconnectHandler): void {
    this.disconnectHandlers.add(handler);
  }

  private dispatchDisconnect(error: Error): void {
    for (const handler of this.disconnectHandlers) {
      handler(error);
    }
  }
}

export function buildDaemonWebSocketUrl(rawDaemonUrl: string): URL {
  const daemonUrl = parseLoopbackDaemonUrl(rawDaemonUrl);
  daemonUrl.protocol = "ws:";
  daemonUrl.pathname = "/v0/ws";
  daemonUrl.search = "";
  daemonUrl.hash = "";
  return daemonUrl;
}

export function buildDaemonCaptureUrl(rawDaemonUrl: string): URL {
  const daemonUrl = parseLoopbackDaemonUrl(rawDaemonUrl);
  daemonUrl.protocol = "http:";
  daemonUrl.pathname = "/v0/capture";
  daemonUrl.search = "";
  daemonUrl.hash = "";
  return daemonUrl;
}

function parseLoopbackDaemonUrl(rawDaemonUrl: string): URL {
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

function currentExtensionId(): string | undefined {
  return typeof chrome !== "undefined" ? chrome.runtime?.id : undefined;
}
