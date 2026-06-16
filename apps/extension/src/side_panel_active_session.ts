import type { CaptureGrant } from "./capture_permission.js";
import type { ActiveChatMarker } from "./side_panel_active_chat.js";
import type { SidePanelControlState } from "./side_panel_elements.js";
import type { DaemonSettings } from "./sidekick_protocol.js";

export interface ActiveChatRecoveryGuard {
  generation: number;
  sessionId: string;
  daemonUrl: string;
  daemonToken: string;
}

export class SidePanelActiveSessionState {
  sessionId: string | null = null;
  daemonUrl: string | null = null;
  daemonToken: string | null = null;
  generation = 0;
  activeTurnId: string | null = null;
  subscribedSessionId: string | null = null;
  requestInFlight = false;
  sessionRecoveryRequired = false;

  beginRecovery(settings: DaemonSettings, sessionId: string): ActiveChatRecoveryGuard {
    this.sessionRecoveryRequired = true;
    return this.activeChatGuard(settings, sessionId);
  }

  activeChatGuard(
    settings: DaemonSettings,
    sessionId: string,
  ): ActiveChatRecoveryGuard {
    return {
      generation: this.generation,
      sessionId,
      daemonUrl: settings.url,
      daemonToken: settings.token,
    };
  }

  isRecoveryCurrent(guard: ActiveChatRecoveryGuard): boolean {
    return (
      this.generation === guard.generation &&
      this.sessionId === guard.sessionId &&
      this.daemonUrl === guard.daemonUrl &&
      this.daemonToken === guard.daemonToken
    );
  }

  activeDaemonSettings(): DaemonSettings | null {
    if (!this.daemonUrl || !this.daemonToken) {
      return null;
    }
    return {
      url: this.daemonUrl,
      token: this.daemonToken,
    };
  }

  matchesActiveDaemonIdentity(settings: DaemonSettings): boolean {
    return this.daemonUrl === settings.url && this.daemonToken === settings.token;
  }

  shouldRecoverOnConnectionLoss(): boolean {
    return this.sessionId !== null && (this.activeTurnId !== null || this.requestInFlight);
  }

  toControlState(): SidePanelControlState {
    return {
      requestInFlight: this.requestInFlight,
      turnActive: this.activeTurnId !== null,
      sessionRecoveryRequired: this.sessionRecoveryRequired,
    };
  }

  toActiveChatMarker(captureGrant: CaptureGrant | null): ActiveChatMarker | null {
    if (!this.sessionId || !this.daemonUrl || !this.daemonToken || !captureGrant) {
      return null;
    }
    const marker: ActiveChatMarker = {
      daemonUrl: this.daemonUrl,
      daemonToken: this.daemonToken,
      tabId: captureGrant.tabId,
      origin: captureGrant.origin,
      sessionId: this.sessionId,
    };
    if (this.activeTurnId) {
      marker.activeTurnId = this.activeTurnId;
    }
    return marker;
  }

  setActiveSession(settings: DaemonSettings, sessionId: string): void {
    this.sessionId = sessionId;
    this.daemonUrl = settings.url;
    this.daemonToken = settings.token;
  }

  restoreActiveChatMarker(marker: ActiveChatMarker): void {
    this.sessionId = marker.sessionId;
    this.daemonUrl = marker.daemonUrl;
    this.daemonToken = marker.daemonToken;
    this.activeTurnId = marker.activeTurnId ?? null;
  }

  setSubscribedSessionId(sessionId: string): void {
    this.subscribedSessionId = sessionId;
  }

  clearSubscribedSession(): void {
    this.subscribedSessionId = null;
  }

  setRequestInFlight(isBusy: boolean): void {
    this.requestInFlight = isBusy;
  }

  setSessionRecoveryRequired(required: boolean): void {
    this.sessionRecoveryRequired = required;
  }

  setActiveTurnId(turnId: string | null): void {
    this.activeTurnId = turnId;
  }

  clearActiveTurn(): void {
    this.activeTurnId = null;
  }

  clearActiveChat(): void {
    this.generation += 1;
    this.sessionId = null;
    this.daemonUrl = null;
    this.daemonToken = null;
    this.subscribedSessionId = null;
    this.clearRecoveryBlockingState();
  }

  clearRecoveryBlockingState(): void {
    this.sessionRecoveryRequired = false;
    this.clearActiveTurn();
  }
}
