import {
  NATIVE_CONNECTION_SETTINGS,
  NativeMessagingSidekickClient,
  SidekickProtocolError,
  WebSocketSidekickClient,
  isNativeConnectionSettings,
  type DaemonSettings,
  type SidekickClient,
  type SidekickNotification,
} from "./sidekick_protocol.js";

type NotificationHandler = (notification: SidekickNotification) => void;
type ManagedSidekickClient = SidekickClient & {
  matches(settings: DaemonSettings): boolean;
};

export interface ConnectedSidekickClient {
  client: SidekickClient;
  settings: DaemonSettings;
}

export class SidePanelProtocolConnection {
  private client: ManagedSidekickClient | null = null;
  private settings: DaemonSettings | null = null;
  private unsubscribeNotifications: (() => void) | null = null;

  async ensurePreferred(
    fallbackSettings: DaemonSettings | null,
    onNotification: NotificationHandler,
  ): Promise<ConnectedSidekickClient> {
    if (this.client && this.matches(NATIVE_CONNECTION_SETTINGS)) {
      return { client: this.client, settings: NATIVE_CONNECTION_SETTINGS };
    }
    if (fallbackSettings && this.client && this.matches(fallbackSettings)) {
      return { client: this.client, settings: fallbackSettings };
    }
    try {
      return await this.ensure(NATIVE_CONNECTION_SETTINGS, onNotification);
    } catch (nativeError) {
      if (!fallbackSettings || isSetupRequiredProtocolError(nativeError)) {
        throw nativeError;
      }
      return this.ensure(fallbackSettings, onNotification);
    }
  }

  async ensure(
    settings: DaemonSettings,
    onNotification: NotificationHandler,
  ): Promise<ConnectedSidekickClient> {
    if (this.client && this.matches(settings)) {
      return { client: this.client, settings };
    }

    this.disconnect();
    const version = chrome.runtime.getManifest().version;
    const client: ManagedSidekickClient = isNativeConnectionSettings(settings)
      ? await NativeMessagingSidekickClient.connect(version)
      : await WebSocketSidekickClient.connect(settings, version);
    this.unsubscribeNotifications = client.onNotification(onNotification);
    this.client = client;
    this.settings = settings;
    return { client, settings };
  }

  matches(settings: DaemonSettings): boolean {
    if (
      this.client === null ||
      this.settings?.url !== settings.url ||
      this.settings.token !== settings.token
    ) {
      return false;
    }
    try {
      return this.client.matches(settings);
    } catch {
      return false;
    }
  }

  disconnect(): void {
    const client = this.client;
    this.unsubscribeNotifications?.();
    this.unsubscribeNotifications = null;
    this.client = null;
    this.settings = null;
    client?.close();
  }
}

function isSetupRequiredProtocolError(error: unknown): boolean {
  return error instanceof SidekickProtocolError && error.code === "setup_required";
}
