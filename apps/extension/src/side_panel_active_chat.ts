import type { DaemonSettings } from "./sidekick_protocol.js";
import type { CaptureGrant } from "./capture_permission.js";

const ACTIVE_CHAT_STORAGE_KEY = "activeChat";

export interface ActiveChatMarker {
  daemonUrl: string;
  daemonToken: string;
  tabId: number;
  origin: string;
  sessionId: string;
  activeTurnId?: string;
}

export async function loadActiveChatMarker(
  settings: DaemonSettings,
  captureGrant: CaptureGrant | null,
): Promise<ActiveChatMarker | null> {
  if (!captureGrant) {
    return null;
  }
  const stored: Record<string, unknown> = await chrome.storage.session.get([
    ACTIVE_CHAT_STORAGE_KEY,
  ]);
  const marker = parseActiveChatMarker(stored[ACTIVE_CHAT_STORAGE_KEY]);
  return marker?.daemonUrl === settings.url &&
    marker.daemonToken === settings.token &&
    marker.tabId === captureGrant.tabId &&
    marker.origin === captureGrant.origin
    ? marker
    : null;
}

export async function saveActiveChatMarker(marker: ActiveChatMarker): Promise<void> {
  await chrome.storage.session.set({ [ACTIVE_CHAT_STORAGE_KEY]: marker });
}

export async function clearActiveChatMarker(): Promise<void> {
  await chrome.storage.session.remove(ACTIVE_CHAT_STORAGE_KEY);
}

function parseActiveChatMarker(value: unknown): ActiveChatMarker | null {
  if (!isRecord(value)) {
    return null;
  }

  const daemonUrl = getString(value, "daemonUrl");
  const daemonToken = getString(value, "daemonToken");
  const tabId = getNumber(value, "tabId");
  const origin = getString(value, "origin");
  const sessionId = getString(value, "sessionId");
  const activeTurnId = getString(value, "activeTurnId");
  if (!daemonUrl || !daemonToken || tabId === null || !origin || !sessionId) {
    return null;
  }
  const marker: ActiveChatMarker = { daemonUrl, daemonToken, tabId, origin, sessionId };
  if (activeTurnId) {
    marker.activeTurnId = activeTurnId;
  }
  return marker;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function getNumber(record: Record<string, unknown>, key: string): number | null {
  const value = record[key];
  return typeof value === "number" && Number.isInteger(value) ? value : null;
}
