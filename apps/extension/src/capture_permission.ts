export const REOPEN_SIDEKICK_FOR_TAB_MESSAGE =
  "Reopen Screen Sidekick on this tab before capture";
export const UNSUPPORTED_CAPTURE_TAB_MESSAGE =
  "Current tab cannot be captured by Screen Sidekick";

export interface CapturePermissionTab {
  id?: number | undefined;
  url?: string | undefined;
}

export interface CaptureGrant {
  tabId: number;
  origin: string;
}

export function createCaptureGrant(tab: CapturePermissionTab): CaptureGrant {
  if (typeof tab.id !== "number") {
    throw new Error("Active tab is unavailable");
  }

  const origin = captureOriginFromUrl(tab.url);
  if (!origin) {
    throw new Error(UNSUPPORTED_CAPTURE_TAB_MESSAGE);
  }

  return {
    tabId: tab.id,
    origin,
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
  if (!rawUrl) {
    return null;
  }

  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    return null;
  }

  return url.protocol === "http:" || url.protocol === "https:" ? url.origin : null;
}
