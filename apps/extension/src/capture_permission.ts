export const REOPEN_SIDEKICK_FOR_TAB_MESSAGE =
  "Reopen Screen Sidekick on this tab before capture";
export const UNSUPPORTED_CAPTURE_TAB_MESSAGE =
  "Current tab cannot be captured by Screen Sidekick";
const MISSING_MANIFEST_HOST_PERMISSION_FRAGMENT =
  "Extension manifest must request permission";
const MISSING_CAPTURE_VISIBLE_TAB_PERMISSION_FRAGMENT =
  "Either the '<all_urls>' or 'activeTab' permission is required";

export interface CapturePermissionTab {
  id?: number | undefined;
  url?: string | undefined;
}

export interface CaptureGrant {
  tabId: number;
  origin: string;
  hostPermissionPattern: string;
}

export function createCaptureGrant(tab: CapturePermissionTab): CaptureGrant {
  if (typeof tab.id !== "number") {
    throw new Error("Active tab is unavailable");
  }

  const access = captureAccessFromUrl(tab.url);
  if (!access) {
    throw new Error(UNSUPPORTED_CAPTURE_TAB_MESSAGE);
  }

  return {
    tabId: tab.id,
    origin: access.origin,
    hostPermissionPattern: access.hostPermissionPattern,
  };
}

export function assertFreshCaptureGrant(
  tab: CapturePermissionTab,
  grant: CaptureGrant,
): void {
  const currentGrant = createCaptureGrant(tab);
  if (currentGrant.tabId !== grant.tabId || currentGrant.origin !== grant.origin) {
    throw new Error(REOPEN_SIDEKICK_FOR_TAB_MESSAGE);
  }
}

export function captureOriginFromUrl(rawUrl: string | undefined): string | null {
  return captureAccessFromUrl(rawUrl)?.origin ?? null;
}

export function captureHostPermissionPatternFromUrl(
  rawUrl: string | undefined,
): string | null {
  return captureAccessFromUrl(rawUrl)?.hostPermissionPattern ?? null;
}

export function isMissingManifestHostPermissionError(error: unknown): boolean {
  return (
    error instanceof Error &&
    error.message.includes(MISSING_MANIFEST_HOST_PERMISSION_FRAGMENT)
  );
}

export function isMissingCaptureVisibleTabPermissionError(error: unknown): boolean {
  return (
    error instanceof Error &&
    error.message.includes(MISSING_CAPTURE_VISIBLE_TAB_PERMISSION_FRAGMENT)
  );
}

function captureAccessFromUrl(
  rawUrl: string | undefined,
): { origin: string; hostPermissionPattern: string } | null {
  if (!rawUrl) {
    return null;
  }

  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    return null;
  }

  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return null;
  }

  return {
    origin: url.origin,
    hostPermissionPattern: `${url.protocol}//${url.hostname}/*`,
  };
}
