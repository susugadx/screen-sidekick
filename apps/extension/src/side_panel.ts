import {
  RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
  buildButton,
  buildInput,
  buildPage,
  serializeCaptureContextForBridge,
  type BridgeSettings,
  type CaptureBridgeResponse,
  type DomCapture,
  type InputKind,
  type RawBrowserButton,
  type RawBrowserContext,
  type RawBrowserInput,
  type RawBrowserScreenshot,
  type SafetySummary,
  type SafetyWarning,
} from "./capture_contract.js";
import {
  REOPEN_SIDEKICK_FOR_TAB_MESSAGE,
  assertFreshCaptureGrant,
  createCaptureGrant,
  type CaptureGrant,
} from "./capture_permission.js";
import { collectBrowserContext } from "./dom_capture.js";
import { clearPreviewState, setPreviewState } from "./preview_state.js";

const STORAGE_KEY = "bridgeSettings";
const FETCH_TIMEOUT_MS = 10_000;
const MAX_BRIDGE_RESPONSE_CHARS = 512 * 1024;
const STATIC_BRIDGE_REJECTION_MESSAGES = new Set([
  "extension Origin header is required",
  "only chrome-extension origins are allowed",
  "invalid CORS preflight",
  "bearer token is required",
  "bearer token is invalid",
  "capture request JSON is invalid",
  "capture request is invalid",
  "failed to serialize capture response",
]);

let initialCaptureGrant: CaptureGrant | null = null;
let initialCaptureGrantError: Error | null = null;

const elements = {
  bridgeForm: requireElement("bridge-form", HTMLFormElement),
  bridgeUrl: requireElement("bridge-url", HTMLInputElement),
  bridgeToken: requireElement("bridge-token", HTMLInputElement),
  saveBridge: requireElement("save-bridge", HTMLButtonElement),
  capture: requireElement("capture", HTMLButtonElement),
  copyJson: requireElement("copy-json", HTMLButtonElement),
  copyPrompt: requireElement("copy-prompt", HTMLButtonElement),
  screenContextJson: requireElement("screen-context-json", HTMLTextAreaElement),
  promptText: requireElement("prompt-text", HTMLTextAreaElement),
  safetySummary: requireElement("safety-summary", HTMLPreElement),
  status: requireElement("status", HTMLSpanElement),
};

void initialize();

async function initialize(): Promise<void> {
  const settings = await loadBridgeSettings();
  if (settings) {
    elements.bridgeUrl.value = settings.url;
    elements.bridgeToken.value = settings.token;
  }
  await initializeCaptureGrant();

  elements.bridgeForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void saveBridgeSettings();
  });
  elements.capture.addEventListener("click", () => {
    void captureToBridge();
  });
  elements.copyJson.addEventListener("click", () => {
    void navigator.clipboard.writeText(elements.screenContextJson.value);
  });
  elements.copyPrompt.addEventListener("click", () => {
    void navigator.clipboard.writeText(elements.promptText.value);
  });

  clearPreviewState(elements);
  setStatus("Idle");
}

async function saveBridgeSettings(): Promise<void> {
  const settings = readBridgeSettingsFromInputs();
  await chrome.storage.session.set({ [STORAGE_KEY]: settings });
  setStatus("Saved");
}

async function captureToBridge(): Promise<void> {
  setBusy(true);
  clearPreviewState(elements);
  setStatus("Capturing");

  try {
    const settings = readBridgeSettingsFromInputs();
    const endpoint = buildCaptureEndpoint(settings.url);
    const context = await captureActiveTabContext();
    const response = await postCapture(endpoint, settings.token, context);
    setPreviewState(elements, {
      screenContextJson: response.screen_context_json,
      promptText: response.prompt_text,
      safetySummaryText: formatSafetySummary(response.safety),
    });
    setStatus(response.safety.has_danger ? "Review" : "Ready");
  } catch (error) {
    setError(error instanceof Error ? error.message : "Capture failed");
  } finally {
    setBusy(false);
  }
}

async function captureActiveTabContext(): Promise<RawBrowserContext> {
  const tab = await getActiveTab();
  if (!initialCaptureGrant) {
    throw initialCaptureGrantError ?? new Error(REOPEN_SIDEKICK_FOR_TAB_MESSAGE);
  }
  const captureGrant = initialCaptureGrant;
  assertFreshCaptureGrant(tab, captureGrant);

  const domCapture = await captureDom(captureGrant.tabId);
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

  return context;
}

async function initializeCaptureGrant(): Promise<void> {
  try {
    initialCaptureGrant = createCaptureGrant(await getActiveTab());
    initialCaptureGrantError = null;
  } catch (error) {
    initialCaptureGrant = null;
    initialCaptureGrantError =
      error instanceof Error ? error : new Error(REOPEN_SIDEKICK_FOR_TAB_MESSAGE);
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

async function captureDom(tabId: number): Promise<DomCapture> {
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

async function captureScreenshotMetadata(
  windowId: number | undefined,
): Promise<RawBrowserScreenshot | undefined> {
  const dataUrl =
    typeof windowId === "number"
      ? await chrome.tabs.captureVisibleTab(windowId, { format: "png" })
      : await chrome.tabs.captureVisibleTab({ format: "png" });
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

async function postCapture(
  endpoint: URL,
  token: string,
  context: RawBrowserContext,
): Promise<CaptureBridgeResponse> {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);

  try {
    const body = serializeCaptureContextForBridge(context);
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body,
      signal: controller.signal,
    });
    const text = await response.text();

    if (text.length > MAX_BRIDGE_RESPONSE_CHARS) {
      throw new Error("Bridge response is too large");
    }

    if (!response.ok) {
      throw new Error(formatBridgeRejectionStatus(response.status, text));
    }

    const payload: unknown = JSON.parse(text);
    const parsed = parseCaptureBridgeResponse(payload);
    if (!parsed) {
      throw new Error("Bridge response shape is invalid");
    }
    return parsed;
  } finally {
    window.clearTimeout(timeout);
  }
}

function formatBridgeRejectionStatus(status: number, body: string): string {
  const trimmedBody = body.trim();
  if (STATIC_BRIDGE_REJECTION_MESSAGES.has(trimmedBody)) {
    return `Bridge rejected capture (${status}): ${trimmedBody}`;
  }
  return `Bridge rejected capture (${status})`;
}

function buildCaptureEndpoint(rawBridgeUrl: string): URL {
  let bridgeUrl: URL;
  try {
    bridgeUrl = new URL(rawBridgeUrl);
  } catch {
    throw new Error("Bridge URL is invalid");
  }

  if (
    bridgeUrl.protocol !== "http:" ||
    bridgeUrl.hostname !== "127.0.0.1" ||
    bridgeUrl.port.length === 0
  ) {
    throw new Error("Bridge URL must use http://127.0.0.1:<port>");
  }

  bridgeUrl.pathname = "/v0/capture";
  bridgeUrl.search = "";
  bridgeUrl.hash = "";
  return bridgeUrl;
}

async function loadBridgeSettings(): Promise<BridgeSettings | null> {
  const stored: Record<string, unknown> = await chrome.storage.session.get(STORAGE_KEY);
  return parseBridgeSettings(stored[STORAGE_KEY]);
}

function readBridgeSettingsFromInputs(): BridgeSettings {
  const settings = parseBridgeSettings({
    url: elements.bridgeUrl.value.trim(),
    token: elements.bridgeToken.value.trim(),
  });
  if (!settings) {
    throw new Error("Bridge URL and token are required");
  }
  return settings;
}

function parseBridgeSettings(value: unknown): BridgeSettings | null {
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

function parseCaptureBridgeResponse(value: unknown): CaptureBridgeResponse | null {
  if (!isRecord(value)) {
    return null;
  }

  const schemaVersion = getString(value, "schema_version");
  const screenContextJson = getString(value, "screen_context_json");
  const promptText = getString(value, "prompt_text");
  const safety = parseSafetySummary(value.safety);

  if (!schemaVersion || !screenContextJson || !promptText || !safety) {
    return null;
  }

  return {
    schema_version: schemaVersion,
    screen_context_json: screenContextJson,
    prompt_text: promptText,
    safety,
  };
}

function parseSafetySummary(value: unknown): SafetySummary | null {
  if (!isRecord(value)) {
    return null;
  }

  const hasDanger = getBoolean(value, "has_danger");
  const warningCount = getNumber(value, "warning_count");
  const maskedInputValues = getNumber(value, "masked_input_values");
  const maskedSecretTexts = getNumber(value, "masked_secret_texts");
  const warnings = parseSafetyWarnings(value.warnings);

  if (
    hasDanger === null ||
    warningCount === null ||
    maskedInputValues === null ||
    maskedSecretTexts === null ||
    !warnings
  ) {
    return null;
  }

  return {
    has_danger: hasDanger,
    warning_count: warningCount,
    warnings,
    masked_input_values: maskedInputValues,
    masked_secret_texts: maskedSecretTexts,
  };
}

function parseSafetyWarnings(value: unknown): SafetyWarning[] | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const warnings: SafetyWarning[] = [];
  for (const item of value) {
    if (!isRecord(item)) {
      return null;
    }
    const category = getString(item, "category");
    const categoryLabel = getString(item, "category_label");
    const source = getString(item, "source");
    const sourceLabel = getString(item, "source_label");
    if (!category || !categoryLabel || !source || !sourceLabel) {
      return null;
    }
    warnings.push({
      category,
      category_label: categoryLabel,
      source,
      source_label: sourceLabel,
    });
  }
  return warnings;
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

function formatSafetySummary(safety: SafetySummary): string {
  const warningLines =
    safety.warnings.length === 0
      ? ["warnings: none"]
      : safety.warnings.map(
          (warning) => `warning: ${warning.category_label} (${warning.source_label})`,
        );

  return [
    `danger: ${safety.has_danger ? "yes" : "no"}`,
    `warning_count: ${safety.warning_count}`,
    `masked_input_values: ${safety.masked_input_values}`,
    `masked_secret_texts: ${safety.masked_secret_texts}`,
    ...warningLines,
  ].join("\n");
}

function setBusy(isBusy: boolean): void {
  elements.capture.disabled = isBusy;
  elements.saveBridge.disabled = isBusy;
}

function setStatus(text: string): void {
  elements.status.textContent = text;
  elements.status.className = "status";
}

function setError(text: string): void {
  elements.status.textContent = text;
  elements.status.className = "status error";
}

function requireElement<T extends HTMLElement>(
  id: string,
  constructor: { new (): T },
): T {
  const element = document.getElementById(id);
  if (!(element instanceof constructor)) {
    throw new Error(`Missing element: ${id}`);
  }
  return element;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function getBoolean(record: Record<string, unknown>, key: string): boolean | null {
  const value = record[key];
  return typeof value === "boolean" ? value : null;
}

function getNumber(record: Record<string, unknown>, key: string): number | null {
  const value = record[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
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

export {};
