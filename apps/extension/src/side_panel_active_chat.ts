import type { DaemonSettings } from "./sidekick_protocol.js";
import type { CaptureGrant } from "./capture_permission.js";

const ACTIVE_CHAT_STORAGE_KEY = "activeChat";
const ACTIVE_CHAT_STORAGE_LOCK = "screen-sidekick.activeChat";

export interface ActiveChatMarker {
  daemonUrl: string;
  daemonToken: string;
  tabId: number;
  origin: string;
  sessionId: string;
  activeTurnId?: string;
}

type ActiveChatStorage = {
  version: 1;
  markers: Record<string, ActiveChatMarker>;
};

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
  const marker = markerForCaptureGrant(stored[ACTIVE_CHAT_STORAGE_KEY], captureGrant);
  return marker?.daemonUrl === settings.url &&
    marker.daemonToken === settings.token &&
    marker.tabId === captureGrant.tabId &&
    marker.origin === captureGrant.origin
    ? marker
    : null;
}

export async function saveActiveChatMarker(marker: ActiveChatMarker): Promise<void> {
  await mutateActiveChatStorage(async () => {
    const stored: Record<string, unknown> = await chrome.storage.session.get([
      ACTIVE_CHAT_STORAGE_KEY,
    ]);
    const activeChat = normalizeActiveChatStorage(stored[ACTIVE_CHAT_STORAGE_KEY]);
    activeChat.markers[activeChatScopeKey(marker.tabId, marker.origin)] = marker;
    await chrome.storage.session.set({ [ACTIVE_CHAT_STORAGE_KEY]: activeChat });
  });
}

export async function clearActiveChatMarker(
  captureGrant: CaptureGrant | null = null,
): Promise<void> {
  await mutateActiveChatStorage(async () => {
    const stored: Record<string, unknown> = await chrome.storage.session.get([
      ACTIVE_CHAT_STORAGE_KEY,
    ]);
    const storedActiveChat = stored[ACTIVE_CHAT_STORAGE_KEY];
    const activeChat = parseActiveChatStorage(storedActiveChat);
    if (activeChat) {
      if (!captureGrant) {
        return;
      }
      delete activeChat.markers[activeChatScopeKey(captureGrant.tabId, captureGrant.origin)];
      if (Object.keys(activeChat.markers).length === 0) {
        await chrome.storage.session.remove(ACTIVE_CHAT_STORAGE_KEY);
        return;
      }
      await chrome.storage.session.set({ [ACTIVE_CHAT_STORAGE_KEY]: activeChat });
      return;
    }

    const legacyMarker = parseActiveChatMarker(storedActiveChat);
    if (
      legacyMarker &&
      (!captureGrant ||
        (legacyMarker.tabId === captureGrant.tabId && legacyMarker.origin === captureGrant.origin))
    ) {
      await chrome.storage.session.remove(ACTIVE_CHAT_STORAGE_KEY);
    }
  });
}

async function mutateActiveChatStorage<T>(mutation: () => Promise<T>): Promise<T> {
  return navigator.locks.request(ACTIVE_CHAT_STORAGE_LOCK, mutation);
}

function markerForCaptureGrant(
  value: unknown,
  captureGrant: CaptureGrant,
): ActiveChatMarker | null {
  const activeChat = parseActiveChatStorage(value);
  if (activeChat) {
    return activeChat.markers[activeChatScopeKey(captureGrant.tabId, captureGrant.origin)] ?? null;
  }

  const legacyMarker = parseActiveChatMarker(value);
  return legacyMarker?.tabId === captureGrant.tabId && legacyMarker.origin === captureGrant.origin
    ? legacyMarker
    : null;
}

function normalizeActiveChatStorage(value: unknown): ActiveChatStorage {
  const activeChat = parseActiveChatStorage(value);
  if (activeChat) {
    return activeChat;
  }

  const legacyMarker = parseActiveChatMarker(value);
  if (legacyMarker) {
    return {
      version: 1,
      markers: {
        [activeChatScopeKey(legacyMarker.tabId, legacyMarker.origin)]: legacyMarker,
      },
    };
  }

  return {
    version: 1,
    markers: {},
  };
}

function parseActiveChatStorage(value: unknown): ActiveChatStorage | null {
  if (!isRecord(value) || value.version !== 1 || !isRecord(value.markers)) {
    return null;
  }

  const markers: Record<string, ActiveChatMarker> = {};
  for (const [key, markerValue] of Object.entries(value.markers)) {
    const marker = parseActiveChatMarker(markerValue);
    if (marker && key === activeChatScopeKey(marker.tabId, marker.origin)) {
      markers[key] = marker;
    }
  }
  return {
    version: 1,
    markers,
  };
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

function activeChatScopeKey(tabId: number, origin: string): string {
  return `tab:${tabId}|origin:${origin}`;
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
