import type { DaemonSettings } from "./sidekick_protocol.js";

const STORAGE_KEY = "daemonSettings";
const LEGACY_STORAGE_KEY = "bridgeSettings";

export async function loadStoredDaemonSettings(): Promise<DaemonSettings | null> {
  const stored: Record<string, unknown> = await chrome.storage.session.get([
    STORAGE_KEY,
    LEGACY_STORAGE_KEY,
  ]);
  return parseDaemonSettings(stored[STORAGE_KEY]) ?? parseDaemonSettings(stored[LEGACY_STORAGE_KEY]);
}

export async function storeDaemonSettings(settings: DaemonSettings): Promise<void> {
  await chrome.storage.session.set({ [STORAGE_KEY]: settings });
  await chrome.storage.session.remove(LEGACY_STORAGE_KEY);
}

export function parseDaemonSettings(value: unknown): DaemonSettings | null {
  if (!isRecord(value)) {
    return null;
  }

  const url = getString(value, "url");
  const token = getString(value, "token");
  if (!url || !token) {
    return null;
  }

  return { url, token };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}
