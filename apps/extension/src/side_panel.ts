import {
  RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
  buildButton,
  buildInput,
  buildPage,
  serializeCaptureContextForBridge,
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
  isMissingCaptureVisibleTabPermissionError,
  isMissingManifestHostPermissionError,
  type CaptureGrant,
} from "./capture_permission.js";
import { collectBrowserContext } from "./dom_capture.js";
import { clearPreviewState, setPreviewState } from "./preview_state.js";
import {
  clearActiveChatMarker,
  loadActiveChatMarker,
  saveActiveChatMarker,
  type ActiveChatMarker,
} from "./side_panel_active_chat.js";
import {
  SidekickProtocolError,
  SidekickProtocolClient,
  type DaemonSettings,
  type SidekickMessage,
  type SidekickNotification,
  type SidekickSessionSnapshot,
  type SidekickTurn,
} from "./sidekick_protocol.js";

const STORAGE_KEY = "daemonSettings";
const LEGACY_STORAGE_KEY = "bridgeSettings";
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
let protocolClient: SidekickProtocolClient | null = null;
let unsubscribeProtocolNotifications: (() => void) | null = null;
let activeSessionId: string | null = null;
let activeSessionDaemonUrl: string | null = null;
let activeSessionDaemonToken: string | null = null;
let activeTurnId: string | null = null;
let subscribedSessionId: string | null = null;
let requestInFlight = false;
let sessionRecoveryRequired = false;
let activeAssistantText = "";
let activeAssistantTextElement: HTMLDivElement | null = null;
let renderedMessageIds = new Set<string>();
let pendingSubmittedQuestion: { sessionId: string; text: string } | null = null;

const elements = {
  bridgeForm: requireElement("bridge-form", HTMLFormElement),
  bridgeUrl: requireElement("bridge-url", HTMLInputElement),
  bridgeToken: requireElement("bridge-token", HTMLInputElement),
  saveBridge: requireElement("save-bridge", HTMLButtonElement),
  messageForm: requireElement("message-form", HTMLFormElement),
  messageInput: requireElement("message-input", HTMLTextAreaElement),
  ask: requireElement("ask", HTMLButtonElement),
  transcript: requireElement("transcript", HTMLDivElement),
  debugCapture: requireElement("debug-capture", HTMLButtonElement),
  copyJson: requireElement("copy-json", HTMLButtonElement),
  copyPrompt: requireElement("copy-prompt", HTMLButtonElement),
  screenContextJson: requireElement("screen-context-json", HTMLTextAreaElement),
  promptText: requireElement("prompt-text", HTMLTextAreaElement),
  safetySummary: requireElement("safety-summary", HTMLPreElement),
  status: requireElement("status", HTMLSpanElement),
};

void initialize();

async function initialize(): Promise<void> {
  const settings = await loadDaemonSettings();
  if (settings) {
    elements.bridgeUrl.value = settings.url;
    elements.bridgeToken.value = settings.token;
  }
  await initializeCaptureGrant();

  elements.bridgeForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void saveDaemonSettings();
  });
  elements.messageForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void askCodex();
  });
  elements.debugCapture.addEventListener("click", () => {
    void captureDebugToBridge();
  });
  elements.copyJson.addEventListener("click", () => {
    void navigator.clipboard.writeText(elements.screenContextJson.value);
  });
  elements.copyPrompt.addEventListener("click", () => {
    void navigator.clipboard.writeText(elements.promptText.value);
  });

  clearPreviewState(elements);
  updateControlsDisabled();
  setStatus("Idle");
  if (settings) {
    await recoverActiveChatFromStorage(settings);
  }
}

async function saveDaemonSettings(): Promise<void> {
  const settings = readDaemonSettingsFromInputs();
  await chrome.storage.session.set({ [STORAGE_KEY]: settings });
  await chrome.storage.session.remove(LEGACY_STORAGE_KEY);
  if (hasActiveDaemonIdentity() && !activeDaemonIdentityMatches(settings)) {
    clearActiveChatState();
    await clearActiveChatMarker();
  }
  setStatus("Saved");
}

async function askCodex(): Promise<void> {
  const question = elements.messageInput.value.trim();
  if (!question) {
    setError("Question is required");
    return;
  }

  setRequestInFlight(true);
  setStatus("Capturing");

  try {
    const settings = readDaemonSettingsFromInputs();
    const client = await ensureProtocolClient(settings);
    const sessionId = await ensureActiveSession(client, settings);
    const context = await captureActiveTabContext();
    const attachment = await client.attachBrowserContext(sessionId, context, "message_send");
    pendingSubmittedQuestion = { sessionId, text: question };
    const sendResult = await client.sendMessage(sessionId, question, [attachment.id], "ask_only");
    setActiveTurnId(sendResult.turnId);
    appendSessionMessage({
      id: sendResult.messageId,
      sessionId,
      role: "user",
      text: question,
      status: "pending",
      turnId: sendResult.turnId,
    });
    activeAssistantText = "";
    activeAssistantTextElement = appendTranscriptMessage("assistant", "");
    setStatus(attachment.safetyStatus === "warning" ? "Review" : "Asking");
  } catch (error) {
    if (sessionRecoveryRequired) {
      setStatus("Reconnecting to daemon");
    } else {
      activeAssistantTextElement = null;
      activeAssistantText = "";
      setError(error instanceof Error ? error.message : "Ask failed");
    }
  } finally {
    setRequestInFlight(false);
  }
}

async function captureDebugToBridge(): Promise<void> {
  setRequestInFlight(true);
  clearPreviewState(elements);
  setStatus("Capturing");

  try {
    const settings = readDaemonSettingsFromInputs();
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
    setRequestInFlight(false);
  }
}

async function ensureProtocolClient(settings: DaemonSettings): Promise<SidekickProtocolClient> {
  if (protocolClient?.matches(settings)) {
    return protocolClient;
  }

  const settingsChanged =
    hasActiveDaemonIdentity() && !activeDaemonIdentityMatches(settings);
  unsubscribeProtocolNotifications?.();
  unsubscribeProtocolNotifications = null;
  protocolClient?.close();
  protocolClient = null;
  subscribedSessionId = null;
  if (settingsChanged) {
    clearActiveChatState();
    void clearActiveChatMarker();
  }

  const version = chrome.runtime.getManifest().version;
  const client = await SidekickProtocolClient.connect(settings, version);
  unsubscribeProtocolNotifications = client.onNotification(handleSidekickNotification);
  protocolClient = client;
  return client;
}

async function ensureActiveSession(
  client: SidekickProtocolClient,
  settings: DaemonSettings,
): Promise<string> {
  if (activeSessionId) {
    if (subscribedSessionId !== activeSessionId) {
      await client.subscribeSession(activeSessionId);
      subscribedSessionId = activeSessionId;
    }
    return activeSessionId;
  }

  const session = await client.createSession("Screen Sidekick");
  await client.subscribeSession(session.id);
  activeSessionId = session.id;
  activeSessionDaemonUrl = settings.url;
  activeSessionDaemonToken = settings.token;
  subscribedSessionId = session.id;
  void persistActiveChatMarker();
  return session.id;
}

function handleSidekickNotification(notification: SidekickNotification): void {
  switch (notification.kind) {
    case "turn_delta":
      if (notification.turnId === activeTurnId && activeAssistantTextElement) {
        activeAssistantText += notification.delta;
        activeAssistantTextElement.textContent = activeAssistantText;
        scrollTranscriptToEnd();
      }
      return;
    case "turn_completed":
      if (notification.turn.id === activeTurnId) {
        clearActiveTurn();
        setStatus("Ready");
      }
      return;
    case "turn_failed":
      if (!notification.turn || notification.turn.id === activeTurnId) {
        clearActiveTurn(true);
        setError(notification.message ?? "Codex turn failed");
      }
      return;
    case "turn_cancelled":
      if (notification.turn.id === activeTurnId) {
        clearActiveTurn(true);
        setStatus("Cancelled");
      }
      return;
    case "message_created":
      if (notification.sessionId === activeSessionId) {
        appendSessionMessage(notification.message);
      }
      return;
    case "error":
      setError(notification.error.message);
      return;
    case "connection_lost":
      handleProtocolConnectionLost(notification.message);
      return;
    case "ignored":
      return;
  }
}

function clearActiveTurn(removeEmptyAssistantPlaceholder = false): void {
  if (
    removeEmptyAssistantPlaceholder &&
    activeAssistantTextElement &&
    activeAssistantText.length === 0
  ) {
    activeAssistantTextElement.closest(".message")?.remove();
  }
  setActiveTurnId(null);
  activeAssistantTextElement = null;
  activeAssistantText = "";
}

function handleProtocolConnectionLost(message: string): void {
  const lostClient = protocolClient;
  unsubscribeProtocolNotifications?.();
  unsubscribeProtocolNotifications = null;
  protocolClient = null;
  subscribedSessionId = null;
  lostClient?.close();
  if (shouldRecoverActiveSessionOnConnectionLoss()) {
    setSessionRecoveryRequired(true);
    setStatus("Reconnecting to daemon");
    void recoverActiveSessionAfterConnectionLoss(message);
    return;
  }
  setError(message);
}

async function recoverActiveChatFromStorage(settings: DaemonSettings): Promise<void> {
  const marker = await loadActiveChatMarker(settings);
  if (!marker) {
    return;
  }

  activeSessionId = marker.sessionId;
  activeSessionDaemonUrl = marker.daemonUrl;
  activeSessionDaemonToken = marker.daemonToken;
  if (marker.activeTurnId) {
    setActiveTurnId(marker.activeTurnId);
    setStatus("Reconnecting to daemon");
  } else {
    setStatus("Restoring session");
  }

  try {
    const client = await ensureProtocolClient(settings);
    await recoverSessionSnapshot(client, settings, marker.sessionId);
  } catch (error) {
    handleSessionRecoveryError(error, "Session recovery failed");
  }
}

async function recoverActiveSessionAfterConnectionLoss(fallbackMessage: string): Promise<void> {
  const sessionId = activeSessionId;
  if (!sessionId) {
    return;
  }

  try {
    const settings = readDaemonSettingsFromInputs();
    const client = await ensureProtocolClient(settings);
    await recoverSessionSnapshot(client, settings, sessionId);
  } catch (error) {
    handleSessionRecoveryError(error, fallbackMessage);
  }
}

async function recoverSessionSnapshot(
  client: SidekickProtocolClient,
  settings: DaemonSettings,
  sessionId: string,
): Promise<void> {
  await client.subscribeSession(sessionId);
  subscribedSessionId = sessionId;
  const snapshot = await client.getSession(sessionId);
  activeSessionId = snapshot.session.id;
  activeSessionDaemonUrl = settings.url;
  activeSessionDaemonToken = settings.token;
  renderSessionSnapshot(snapshot);
  setSessionRecoveryRequired(false);
  void persistActiveChatMarker();
}

function renderSessionSnapshot(snapshot: SidekickSessionSnapshot): void {
  elements.transcript.replaceChildren();
  renderedMessageIds = new Set();
  activeAssistantText = "";
  activeAssistantTextElement = null;

  for (const message of snapshot.messages) {
    appendSessionMessage(message, snapshot.activeTurn);
  }

  if (snapshot.activeTurn && isActiveTurnStatus(snapshot.activeTurn.status)) {
    setActiveTurnId(snapshot.activeTurn.id);
    if (!activeAssistantTextElement) {
      activeAssistantText = "";
      activeAssistantTextElement = appendTranscriptMessage("assistant", "");
    }
    setStatus("Asking");
    return;
  }

  clearActiveTurn();
  setStatus("Ready");
}

function appendSessionMessage(message: SidekickMessage, activeTurn?: SidekickTurn): void {
  if (renderedMessageIds.has(message.id)) {
    return;
  }
  renderedMessageIds.add(message.id);

  if (message.role === "user") {
    clearPendingSubmittedQuestionIfPersisted(message);
    appendTranscriptMessage("user", message.text);
    return;
  }
  if (message.role !== "assistant") {
    return;
  }

  const belongsToActiveTurn =
    (activeTurn && message.turnId === activeTurn.id && isActiveTurnStatus(activeTurn.status)) ||
    (!activeTurn && message.turnId === activeTurnId);
  if (belongsToActiveTurn && activeAssistantTextElement) {
    activeAssistantText = message.text;
    activeAssistantTextElement.textContent = activeAssistantText;
    scrollTranscriptToEnd();
    return;
  }

  const body = appendTranscriptMessage("assistant", message.text);
  if (belongsToActiveTurn) {
    activeAssistantText = message.text;
    activeAssistantTextElement = body;
  }
}

function isActiveTurnStatus(status: SidekickTurn["status"]): boolean {
  return status === "pending" || status === "running";
}

function clearPendingSubmittedQuestionIfPersisted(message: SidekickMessage): void {
  if (
    pendingSubmittedQuestion &&
    message.sessionId === pendingSubmittedQuestion.sessionId &&
    message.text === pendingSubmittedQuestion.text
  ) {
    const submittedText = pendingSubmittedQuestion.text;
    pendingSubmittedQuestion = null;
    if (elements.messageInput.value.trim() === submittedText) {
      elements.messageInput.value = "";
    }
  }
}

function handleSessionRecoveryError(error: unknown, fallbackMessage: string): void {
  if (error instanceof SidekickProtocolError && error.code === "session_not_found") {
    clearActiveChatState();
    void clearActiveChatMarker();
    setError("Daemon session was not found");
    return;
  }
  clearRecoveryBlockingState();
  setError(error instanceof Error ? error.message : fallbackMessage);
}

function clearActiveChatState(): void {
  activeSessionId = null;
  activeSessionDaemonUrl = null;
  activeSessionDaemonToken = null;
  subscribedSessionId = null;
  pendingSubmittedQuestion = null;
  clearRecoveryBlockingState();
  clearTranscript();
}

function hasActiveDaemonIdentity(): boolean {
  return activeSessionDaemonUrl !== null || activeSessionDaemonToken !== null;
}

function activeDaemonIdentityMatches(settings: DaemonSettings): boolean {
  return activeSessionDaemonUrl === settings.url && activeSessionDaemonToken === settings.token;
}

function shouldRecoverActiveSessionOnConnectionLoss(): boolean {
  return activeSessionId !== null && (activeTurnId !== null || requestInFlight);
}

function clearRecoveryBlockingState(): void {
  setSessionRecoveryRequired(false);
  clearActiveTurn(true);
}

function clearTranscript(): void {
  elements.transcript.replaceChildren();
  renderedMessageIds = new Set();
  activeAssistantTextElement = null;
  activeAssistantText = "";
}

function appendTranscriptMessage(role: "user" | "assistant", text: string): HTMLDivElement {
  const item = document.createElement("div");
  item.className = `message ${role}`;

  const label = document.createElement("div");
  label.className = "message-label";
  label.textContent = role === "user" ? "You" : "Codex";

  const body = document.createElement("div");
  body.className = "message-text";
  body.textContent = text;

  item.append(label, body);
  elements.transcript.append(item);
  scrollTranscriptToEnd();
  return body;
}

function scrollTranscriptToEnd(): void {
  elements.transcript.scrollTop = elements.transcript.scrollHeight;
}

async function captureActiveTabContext(): Promise<RawBrowserContext> {
  const tab = await getActiveTab();
  if (!initialCaptureGrant) {
    throw initialCaptureGrantError ?? new Error(REOPEN_SIDEKICK_FOR_TAB_MESSAGE);
  }
  const captureGrant = initialCaptureGrant;
  assertFreshCaptureGrant(tab, captureGrant);

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
      throw new Error("Daemon response is too large");
    }

    if (!response.ok) {
      throw new Error(formatDaemonRejectionStatus(response.status, text));
    }

    const payload: unknown = JSON.parse(text);
    const parsed = parseCaptureBridgeResponse(payload);
    if (!parsed) {
      throw new Error("Daemon response shape is invalid");
    }
    return parsed;
  } finally {
    window.clearTimeout(timeout);
  }
}

function formatDaemonRejectionStatus(status: number, body: string): string {
  const trimmedBody = body.trim();
  if (STATIC_BRIDGE_REJECTION_MESSAGES.has(trimmedBody)) {
    return `Daemon rejected capture (${status}): ${trimmedBody}`;
  }
  return `Daemon rejected capture (${status})`;
}

function buildCaptureEndpoint(rawDaemonUrl: string): URL {
  let daemonUrl: URL;
  try {
    daemonUrl = new URL(rawDaemonUrl);
  } catch {
    throw new Error("Daemon URL is invalid");
  }

  if (
    daemonUrl.protocol !== "http:" ||
    daemonUrl.hostname !== "127.0.0.1" ||
    daemonUrl.port.length === 0
  ) {
    throw new Error("Daemon URL must use http://127.0.0.1:<port>");
  }

  daemonUrl.pathname = "/v0/capture";
  daemonUrl.search = "";
  daemonUrl.hash = "";
  return daemonUrl;
}

async function loadDaemonSettings(): Promise<DaemonSettings | null> {
  const stored: Record<string, unknown> = await chrome.storage.session.get([
    STORAGE_KEY,
    LEGACY_STORAGE_KEY,
  ]);
  return parseDaemonSettings(stored[STORAGE_KEY]) ?? parseDaemonSettings(stored[LEGACY_STORAGE_KEY]);
}

async function persistActiveChatMarker(): Promise<void> {
  if (!activeSessionId || !activeSessionDaemonUrl || !activeSessionDaemonToken) {
    await clearActiveChatMarker();
    return;
  }
  const marker: ActiveChatMarker = {
    daemonUrl: activeSessionDaemonUrl,
    daemonToken: activeSessionDaemonToken,
    sessionId: activeSessionId,
  };
  if (activeTurnId) {
    marker.activeTurnId = activeTurnId;
  }
  await saveActiveChatMarker(marker);
}

function readDaemonSettingsFromInputs(): DaemonSettings {
  const settings = parseDaemonSettings({
    url: elements.bridgeUrl.value.trim(),
    token: elements.bridgeToken.value.trim(),
  });
  if (!settings) {
    throw new Error("Daemon URL and pairing token are required");
  }
  return settings;
}

function parseDaemonSettings(value: unknown): DaemonSettings | null {
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

function setRequestInFlight(isBusy: boolean): void {
  requestInFlight = isBusy;
  updateControlsDisabled();
}

function setSessionRecoveryRequired(required: boolean): void {
  sessionRecoveryRequired = required;
  updateControlsDisabled();
}

function setActiveTurnId(turnId: string | null): void {
  activeTurnId = turnId;
  void persistActiveChatMarker();
  updateControlsDisabled();
}

function updateControlsDisabled(): void {
  const turnActive = activeTurnId !== null;
  elements.ask.disabled = requestInFlight || turnActive || sessionRecoveryRequired;
  elements.messageInput.disabled = requestInFlight || turnActive || sessionRecoveryRequired;
  elements.debugCapture.disabled = requestInFlight;
  elements.saveBridge.disabled = requestInFlight;
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
