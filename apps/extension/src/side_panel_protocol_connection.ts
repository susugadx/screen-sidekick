import {
  SidekickProtocolClient,
  type DaemonSettings,
  type SidekickNotification,
} from "./sidekick_protocol.js";

type NotificationHandler = (notification: SidekickNotification) => void;

export class SidePanelProtocolConnection {
  private client: SidekickProtocolClient | null = null;
  private unsubscribeNotifications: (() => void) | null = null;

  async ensure(
    settings: DaemonSettings,
    onNotification: NotificationHandler,
  ): Promise<SidekickProtocolClient> {
    if (this.client && this.matches(settings)) {
      return this.client;
    }

    this.disconnect();
    const version = chrome.runtime.getManifest().version;
    const client = await SidekickProtocolClient.connect(settings, version);
    this.unsubscribeNotifications = client.onNotification(onNotification);
    this.client = client;
    return client;
  }

  matches(settings: DaemonSettings): boolean {
    try {
      return this.client?.matches(settings) ?? false;
    } catch {
      return false;
    }
  }

  disconnect(): void {
    const client = this.client;
    this.unsubscribeNotifications?.();
    this.unsubscribeNotifications = null;
    this.client = null;
    client?.close();
  }
}
