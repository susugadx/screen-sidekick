import {
  RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
  buildButton,
  buildInput,
  buildPage,
  type DomCapture,
  type InputKind,
  type RawBrowserButton,
  type RawBrowserContext,
  type RawBrowserInput,
  type RawBrowserScreenshot,
} from "./capture_contract.js";
import {
  REOPEN_SIDEKICK_FOR_TAB_MESSAGE,
  assertFreshCaptureGrant,
  createCaptureGrant,
  isMissingCaptureVisibleTabPermissionError,
  isMissingManifestHostPermissionError,
  type CaptureGrant,
} from "./capture_permission.js";
import { collectBrowserContext } from "./dom_capture.js";

export type CapturedActiveTabContext = {
  context: RawBrowserContext;
  captureGrant: CaptureGrant;
};

type FreshCaptureScope = {
  tab: chrome.tabs.Tab;
  captureGrant: CaptureGrant;
};

export class SidePanelCaptureState {
  private initialCaptureGrant: CaptureGrant | null = null;
  private initialCaptureGrantError: Error | null = null;

  currentGrant(): CaptureGrant | null {
    return this.initialCaptureGrant;
  }

  async initializeGrant(): Promise<void> {
    try {
      this.initialCaptureGrant = createCaptureGrant(await getActiveTab());
      this.initialCaptureGrantError = null;
    } catch (error) {
      this.initialCaptureGrant = null;
      this.initialCaptureGrantError =
        error instanceof Error ? error : new Error(REOPEN_SIDEKICK_FOR_TAB_MESSAGE);
    }
  }

  async captureActiveTabContext(): Promise<RawBrowserContext> {
    return (await this.captureActiveTabContextWithGrant()).context;
  }

  async captureActiveTabContextWithGrant(): Promise<CapturedActiveTabContext> {
    const { tab, captureGrant } = await this.getFreshCaptureScope();
    const domCapture = await captureDom(captureGrant);
    const screenshot = await captureScreenshotMetadata(tab.windowId);
    const context: RawBrowserContext = {
      schema_version: RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
      buttons: domCapture.buttons,
      inputs: domCapture.inputs,
    };
    const page = buildPage(nonEmptyString(tab.url), nonEmptyString(tab.title));
    const selectedText = nonEmptyString(domCapture.selectedText);
    if (page) {
      context.page = page;
    }
    if (selectedText) {
      context.selected_text = selectedText;
    }
    if (screenshot) {
      context.screenshot = screenshot;
    }

    return {
      captureGrant,
      context,
    };
  }

  async assertGrantFresh(captureGrant: CaptureGrant): Promise<void> {
    assertFreshCaptureGrant(await getActiveTab(), captureGrant);
  }

  private async getFreshCaptureScope(): Promise<FreshCaptureScope> {
    const tab = await getActiveTab();
    if (!this.initialCaptureGrant) {
      throw this.initialCaptureGrantError ?? new Error(REOPEN_SIDEKICK_FOR_TAB_MESSAGE);
    }
    const captureGrant = this.initialCaptureGrant;
    assertFreshCaptureGrant(tab, captureGrant);
    return {
      tab,
      captureGrant,
    };
  }
}

async function getActiveTab(): Promise<chrome.tabs.Tab> {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  const tab = tabs[0];
  if (!tab) {
    throw new Error("Active tab is unavailable");
  }
  return tab;
}

async function captureDom(grant: CaptureGrant): Promise<DomCapture> {
  try {
    return await executeCaptureScript(grant.tabId);
  } catch (error) {
    if (!isMissingManifestHostPermissionError(error)) {
      throw error;
    }
  }

  const granted = await requestCaptureHostPermission(grant.hostPermissionPattern);
  if (!granted) {
    throw new Error("Site access was not granted for this page");
  }
  return executeCaptureScript(grant.tabId);
}

async function executeCaptureScript(tabId: number): Promise<DomCapture> {
  const results = await chrome.scripting.executeScript({
    target: { tabId },
    func: collectBrowserContext,
  });
  const firstResult = results[0]?.result;
  const parsed = parseDomCapture(firstResult);
  if (!parsed) {
    throw new Error("Page context capture failed");
  }
  return parsed;
}

async function requestCaptureHostPermission(origin: string): Promise<boolean> {
  try {
    return await chrome.permissions.request({ origins: [origin] });
  } catch {
    throw new Error("Site access permission request failed");
  }
}

async function captureScreenshotMetadata(
  windowId: number | undefined,
): Promise<RawBrowserScreenshot | undefined> {
  let dataUrl: string;
  try {
    dataUrl =
      typeof windowId === "number"
        ? await chrome.tabs.captureVisibleTab(windowId, { format: "png" })
        : await chrome.tabs.captureVisibleTab({ format: "png" });
  } catch (error) {
    if (isMissingCaptureVisibleTabPermissionError(error)) {
      return undefined;
    }
    throw error;
  }

  const dimensions = await readImageDimensions(dataUrl);
  const format = imageFormatFromDataUrl(dataUrl);
  const screenshot: RawBrowserScreenshot = {
    width: dimensions.width,
    height: dimensions.height,
    captured_at: new Date().toISOString(),
  };
  if (format) {
    screenshot.format = format;
  }

  return screenshot;
}

async function readImageDimensions(
  dataUrl: string,
): Promise<{ width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      const dimensions = {
        width: image.naturalWidth,
        height: image.naturalHeight,
      };
      image.src = "";
      resolve(dimensions);
    };
    image.onerror = () => {
      image.src = "";
      reject(new Error("Screenshot metadata capture failed"));
    };
    image.src = dataUrl;
  });
}

function imageFormatFromDataUrl(dataUrl: string): string | undefined {
  const match = /^data:image\/([a-z0-9.+-]+);base64,/i.exec(dataUrl);
  return match?.[1]?.toLowerCase();
}

function parseDomCapture(value: unknown): DomCapture | null {
  if (!isRecord(value)) {
    return null;
  }

  const selectedText = optionalString(value.selectedText);
  const buttons = parseButtons(value.buttons);
  const inputs = parseInputs(value.inputs);

  if (!buttons || !inputs) {
    return null;
  }

  const capture: DomCapture = { buttons, inputs };
  if (selectedText) {
    capture.selectedText = selectedText;
  }
  return capture;
}

function parseButtons(value: unknown): RawBrowserButton[] | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const buttons: RawBrowserButton[] = [];
  for (const item of value) {
    if (!isRecord(item)) {
      return null;
    }
    buttons.push(
      buildButton(
        optionalString(item.text),
        optionalString(item.aria_label),
        optionalString(item.title),
        optionalBoolean(item.disabled),
        optionalBoolean(item.visible),
      ),
    );
  }
  return buttons;
}

function parseInputs(value: unknown): RawBrowserInput[] | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const inputs: RawBrowserInput[] = [];
  for (const item of value) {
    if (!isRecord(item)) {
      return null;
    }
    inputs.push(
      buildInput(
        parseInputKind(item.kind),
        optionalString(item.name),
        optionalString(item.label),
        optionalString(item.aria_label),
        optionalString(item.title),
        optionalString(item.placeholder),
        optionalBoolean(item.disabled),
        optionalBoolean(item.visible),
      ),
    );
  }
  return inputs;
}

function parseInputKind(value: unknown): InputKind | undefined {
  if (typeof value !== "string") {
    return undefined;
  }

  const allowed: readonly InputKind[] = [
    "text",
    "search",
    "email",
    "password",
    "number",
    "tel",
    "url",
    "checkbox",
    "radio",
    "select",
    "textarea",
    "content_editable",
  ];
  for (const kind of allowed) {
    if (value === kind) {
      return kind;
    }
  }
  return undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function optionalBoolean(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}
