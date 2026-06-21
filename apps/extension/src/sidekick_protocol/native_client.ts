import {
  SidekickProtocolClient,
  type ProtocolTransport,
} from "./core.js";
import type { DaemonSettings } from "./types.js";

export const NATIVE_HOST_NAME = "com.screen_sidekick.host";
export const NATIVE_CONNECTION_SETTINGS: DaemonSettings = {
  url: `native://${NATIVE_HOST_NAME}`,
  token: "native-messaging",
};

const NATIVE_REQUEST_TOO_LARGE_MESSAGE =
  "Native Messaging request is too large for the Sidekick protocol limit.";

type MessageHandler = (data: unknown) => void;
type DisconnectHandler = (error: Error) => void;

export class NativeMessagingSidekickClient extends SidekickProtocolClient {
  private constructor(private readonly transportAdapter: NativePortTransport) {
    const options = {
      notOpenMessage: "Screen Sidekick native host is not connected",
      requestTooLargeMessage: NATIVE_REQUEST_TOO_LARGE_MESSAGE,
    };
    const extensionId = currentExtensionId();
    super(
      transportAdapter,
      extensionId ? { ...options, extensionId } : options,
    );
  }

  static async connect(clientVersion: string): Promise<NativeMessagingSidekickClient> {
    const port = connectNativeHost();
    const client = new NativeMessagingSidekickClient(new NativePortTransport(port));
    try {
      await client.initializeConnection(clientVersion);
      return client;
    } catch (error) {
      client.close();
      throw error;
    }
  }

  matches(settings: DaemonSettings): boolean {
    return isNativeConnectionSettings(settings) && this.transportAdapter.isOpen();
  }
}

class NativePortTransport implements ProtocolTransport {
  private readonly messageHandlers = new Set<MessageHandler>();
  private readonly disconnectHandlers = new Set<DisconnectHandler>();
  private disconnected = false;

  constructor(private readonly port: chrome.runtime.Port) {
    this.port.onMessage.addListener((message: unknown) => {
      for (const handler of this.messageHandlers) {
        handler(message);
      }
    });
    this.port.onDisconnect.addListener(() => {
      this.disconnected = true;
      this.dispatchDisconnect(new Error(nativeDisconnectMessage()));
    });
  }

  isOpen(): boolean {
    return !this.disconnected;
  }

  send(text: string): void {
    const message: unknown = JSON.parse(text);
    this.port.postMessage(message);
  }

  close(): void {
    this.disconnected = true;
    this.port.disconnect();
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

export function isNativeConnectionSettings(settings: DaemonSettings): boolean {
  return (
    settings.url === NATIVE_CONNECTION_SETTINGS.url &&
    settings.token === NATIVE_CONNECTION_SETTINGS.token
  );
}

function connectNativeHost(): chrome.runtime.Port {
  if (typeof chrome === "undefined" || typeof chrome.runtime?.connectNative !== "function") {
    throw new Error("Native Messaging is unavailable in this extension context.");
  }
  try {
    return chrome.runtime.connectNative(NATIVE_HOST_NAME);
  } catch {
    throw new Error("Screen Sidekick native host is not installed.");
  }
}

function currentExtensionId(): string | undefined {
  return typeof chrome !== "undefined" ? chrome.runtime?.id : undefined;
}

function nativeDisconnectMessage(): string {
  const rawMessage = chrome.runtime.lastError?.message ?? "";
  if (rawMessage.includes("Specified native messaging host not found")) {
    return "Screen Sidekick native host is not installed.";
  }
  if (rawMessage.includes("Access to the specified native messaging host is forbidden")) {
    return "Screen Sidekick native host is not allowed for this extension ID.";
  }
  if (rawMessage.includes("Error when communicating with the native messaging host")) {
    return "Screen Sidekick native host protocol failed.";
  }
  if (rawMessage.includes("Native host has exited")) {
    return "Screen Sidekick native host exited.";
  }
  return "Screen Sidekick native host disconnected.";
}
