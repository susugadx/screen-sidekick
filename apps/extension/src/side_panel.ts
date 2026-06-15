import { clearPreviewState, setPreviewState } from "./preview_state.js";
import {
  clearActiveChatMarker,
  loadActiveChatMarker,
  saveActiveChatMarker,
  type ActiveChatMarker,
} from "./side_panel_active_chat.js";
import { formatSafetySummary, postCaptureToBridge } from "./side_panel_bridge.js";
import { SidePanelCaptureState } from "./side_panel_capture.js";
import {
  loadStoredDaemonSettings,
  parseDaemonSettings,
  storeDaemonSettings,
} from "./side_panel_daemon_settings.js";
import {
  PendingSubmittedQuestionState,
  type PendingSubmittedQuestion,
} from "./side_panel_pending_submission.js";
import { SidePanelTranscriptView } from "./side_panel_transcript.js";
import {
  SidekickProtocolError,
  SidekickProtocolClient,
  createMessageSendIdempotencyKey,
  isTerminalMessageSendReplayError,
  type DaemonSettings,
  type SafetyStatus,
  type SidekickMessage,
  type SidekickNotification,
  type SidekickSessionSnapshot,
  type SidekickTurn,
} from "./sidekick_protocol.js";

const captureState = new SidePanelCaptureState();
const pendingSubmittedQuestions = new PendingSubmittedQuestionState();

let protocolClient: SidekickProtocolClient | null = null;
let unsubscribeProtocolNotifications: (() => void) | null = null;
let activeSessionId: string | null = null;
let activeSessionDaemonUrl: string | null = null;
let activeSessionDaemonToken: string | null = null;
let activeChatGeneration = 0;
let activeTurnId: string | null = null;
let subscribedSessionId: string | null = null;
let requestInFlight = false;
let sessionRecoveryRequired = false;

type PreparedSubmittedQuestion = {
  client: SidekickProtocolClient;
  sessionId: string;
  pendingQuestion: PendingSubmittedQuestion;
  safetyStatus?: SafetyStatus;
};

type ActiveChatRecoveryGuard = {
  generation: number;
  sessionId: string;
  daemonUrl: string;
  daemonToken: string;
};

type EnsureActiveSessionOptions = {
  resetStaleSession?: boolean;
};

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
const transcriptView = new SidePanelTranscriptView(elements.transcript);

void initialize();

async function initialize(): Promise<void> {
  const settings = await loadStoredDaemonSettings();
  if (settings) {
    elements.bridgeUrl.value = settings.url;
    elements.bridgeToken.value = settings.token;
  }
  await captureState.initializeGrant();

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
  await storeDaemonSettings(settings);

  if (protocolClient && !protocolClientMatchesSavedSettings(protocolClient, settings)) {
    disconnectProtocolClient();
  }
  if (hasActiveDaemonIdentity() && !activeDaemonIdentityMatches(settings)) {
    clearActiveChatState();
    await clearActiveChatMarker(captureState.currentGrant());
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

  let pendingQuestion: PendingSubmittedQuestion | null = null;
  try {
    const settings = readDaemonSettingsFromInputs();
    const submittedQuestion = await prepareSubmittedQuestion(settings, question);
    pendingQuestion = submittedQuestion.pendingQuestion;
    const sendResult = await submittedQuestion.client.sendMessage(
      submittedQuestion.sessionId,
      question,
      pendingQuestion.idempotencyKey,
      pendingQuestion.attachmentIds,
      "ask_only",
    );
    pendingSubmittedQuestions.recordPersistedIds(
      pendingQuestion,
      sendResult.messageId,
      sendResult.turnId,
    );
    if (sendResult.reused) {
      const applied = await loadAndRenderSessionSnapshot(
        submittedQuestion.client,
        activeChatGuard(settings, submittedQuestion.sessionId),
        submittedQuestion.safetyStatus,
      );
      if (applied) {
        clearPendingSubmittedQuestion(pendingQuestion);
      }
      return;
    }
    renderAcceptedSubmittedQuestion(
      submittedQuestion.sessionId,
      question,
      sendResult.messageId,
      sendResult.turnId,
      submittedQuestion.safetyStatus,
    );
    clearPendingSubmittedQuestion(pendingQuestion);
  } catch (error) {
    const recoveryRequired = sessionRecoveryRequired;
    if (pendingQuestion && isTerminalMessageSendReplayError(error)) {
      pendingSubmittedQuestions.discard(pendingQuestion);
    } else {
      if (recoveryRequired && pendingQuestion) {
        pendingSubmittedQuestions.retainForIdempotentRetry(pendingQuestion);
      }
      if (
        pendingSubmittedQuestions.shouldDiscardAfterFailure(pendingQuestion, {
          recoveryRequired,
        })
      ) {
        pendingSubmittedQuestions.discard(pendingQuestion);
      }
    }
    if (recoveryRequired) {
      setStatus("Reconnecting to daemon");
    } else {
      transcriptView.clearActiveAssistantReference();
      setError(error instanceof Error ? error.message : "Ask failed");
    }
  } finally {
    setRequestInFlight(false);
  }
}

async function prepareSubmittedQuestion(
  settings: DaemonSettings,
  question: string,
): Promise<PreparedSubmittedQuestion> {
  const retryQuestion = pendingSubmittedQuestions.findRetryable(
    settings,
    question,
    activeSessionId,
  );
  if (retryQuestion) {
    await assertPendingSubmittedQuestionCaptureScopeFresh(retryQuestion);
    pendingSubmittedQuestions.retainForIdempotentRetry(retryQuestion);
    setStatus("Asking");
    const client = await ensureProtocolClient(settings);
    let sessionId: string;
    try {
      sessionId = await ensureActiveSession(client, settings);
    } catch (error) {
      if (isSessionNotFoundError(error)) {
        pendingSubmittedQuestions.discard(retryQuestion);
        clearActiveChatState();
        void clearActiveChatMarker(captureState.currentGrant());
      }
      throw error;
    }
    return {
      client,
      sessionId,
      pendingQuestion: retryQuestion,
      safetyStatus: retryQuestion.safetyStatus,
    };
  }

  setStatus("Capturing");
  const capturedContext = await captureState.captureActiveTabContextWithGrant();
  const client = await ensureProtocolClient(settings);
  const sessionId = await ensureActiveSession(client, settings, {
    resetStaleSession: true,
  });
  const attachment = await client.attachBrowserContext(
    sessionId,
    capturedContext.context,
    "message_send",
  );
  const pendingQuestion: PendingSubmittedQuestion = {
    sessionId,
    text: question,
    idempotencyKey: createMessageSendIdempotencyKey(),
    attachmentIds: [attachment.id],
    captureGrant: capturedContext.captureGrant,
    safetyStatus: attachment.safetyStatus,
    daemonUrl: settings.url,
    daemonToken: settings.token,
    retainForIdempotentRetry: false,
  };
  pendingSubmittedQuestions.set(pendingQuestion);
  return {
    client,
    sessionId,
    pendingQuestion,
    safetyStatus: attachment.safetyStatus,
  };
}

function renderAcceptedSubmittedQuestion(
  sessionId: string,
  question: string,
  messageId: string,
  turnId: string,
  safetyStatus: SafetyStatus | undefined,
): void {
  setActiveTurnId(turnId);
  appendSessionMessage({
    id: messageId,
    sessionId,
    role: "user",
    text: question,
    status: "pending",
    turnId,
  });
  transcriptView.startAssistantPlaceholder();
  setActiveTurnProgressStatus(safetyStatus);
}

async function captureDebugToBridge(): Promise<void> {
  setRequestInFlight(true);
  clearPreviewState(elements);
  setStatus("Capturing");

  try {
    const settings = readDaemonSettingsFromInputs();
    const context = await captureState.captureActiveTabContext();
    const response = await postCaptureToBridge(settings, context);
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
  disconnectProtocolClient();
  if (settingsChanged) {
    clearActiveChatState();
    void clearActiveChatMarker(captureState.currentGrant());
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
  options: EnsureActiveSessionOptions = {},
): Promise<string> {
  if (activeSessionId) {
    const sessionId = activeSessionId;
    if (subscribedSessionId !== sessionId) {
      try {
        await client.subscribeSession(sessionId);
        subscribedSessionId = sessionId;
        return sessionId;
      } catch (error) {
        if (!options.resetStaleSession || !isSessionNotFoundError(error)) {
          throw error;
        }
        clearActiveChatState();
        await clearActiveChatMarker(captureState.currentGrant());
      }
    } else {
      return sessionId;
    }
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
      transcriptView.appendTurnDelta(notification.turnId, notification.delta, activeTurnId);
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
  transcriptView.clearActiveAssistant(removeEmptyAssistantPlaceholder);
  setActiveTurnId(null);
}

function handleProtocolConnectionLost(message: string): void {
  disconnectProtocolClient();
  if (shouldRecoverActiveSessionOnConnectionLoss()) {
    const pendingQuestion = pendingSubmittedQuestions.current();
    if (pendingQuestion) {
      pendingSubmittedQuestions.retainForIdempotentRetry(pendingQuestion);
    }
    setSessionRecoveryRequired(true);
    setStatus("Reconnecting to daemon");
    void recoverActiveSessionAfterConnectionLoss(message);
    return;
  }
  setError(message);
}

async function recoverActiveChatFromStorage(settings: DaemonSettings): Promise<void> {
  const marker = await loadActiveChatMarker(settings, captureState.currentGrant());
  if (!marker) {
    return;
  }

  activeSessionId = marker.sessionId;
  activeSessionDaemonUrl = marker.daemonUrl;
  activeSessionDaemonToken = marker.daemonToken;
  const guard = beginSessionRecovery(settings, marker.sessionId);
  if (marker.activeTurnId) {
    setActiveTurnId(marker.activeTurnId);
    setStatus("Reconnecting to daemon");
  } else {
    setStatus("Restoring session");
  }

  try {
    const client = await ensureProtocolClient(settings);
    await recoverSessionSnapshot(client, guard);
  } catch (error) {
    if (!isActiveChatRecoveryCurrent(guard)) {
      return;
    }
    handleSessionRecoveryError(error, "Session recovery failed");
  }
}

async function recoverActiveSessionAfterConnectionLoss(fallbackMessage: string): Promise<void> {
  const sessionId = activeSessionId;
  if (!sessionId) {
    return;
  }

  let guard: ActiveChatRecoveryGuard | null = null;
  try {
    const settings = activeDaemonSettings();
    if (!settings) {
      throw new Error(fallbackMessage);
    }
    guard = beginSessionRecovery(settings, sessionId);
    const client = await ensureProtocolClient(settings);
    await recoverSessionSnapshot(client, guard);
  } catch (error) {
    if (guard && !isActiveChatRecoveryCurrent(guard)) {
      return;
    }
    handleSessionRecoveryError(error, fallbackMessage);
  }
}

async function recoverSessionSnapshot(
  client: SidekickProtocolClient,
  guard: ActiveChatRecoveryGuard,
): Promise<void> {
  await client.subscribeSession(guard.sessionId);
  const applied = await loadAndRenderSessionSnapshot(client, guard);
  if (!applied) {
    return;
  }
  setSessionRecoveryRequired(false);
}

async function loadAndRenderSessionSnapshot(
  client: SidekickProtocolClient,
  guard: ActiveChatRecoveryGuard,
  activeTurnSafetyStatus?: SafetyStatus,
): Promise<boolean> {
  const snapshot = await client.getSession(guard.sessionId);
  if (!isActiveChatRecoveryCurrent(guard)) {
    return false;
  }
  if (snapshot.session.id !== guard.sessionId) {
    throw new Error("Daemon session response did not match requested session");
  }
  subscribedSessionId = guard.sessionId;
  activeSessionId = snapshot.session.id;
  activeSessionDaemonUrl = guard.daemonUrl;
  activeSessionDaemonToken = guard.daemonToken;
  renderSessionSnapshot(snapshot, activeTurnSafetyStatus);
  void persistActiveChatMarker();
  return true;
}

function beginSessionRecovery(
  settings: DaemonSettings,
  sessionId: string,
): ActiveChatRecoveryGuard {
  setSessionRecoveryRequired(true);
  return activeChatGuard(settings, sessionId);
}

function activeChatGuard(
  settings: DaemonSettings,
  sessionId: string,
): ActiveChatRecoveryGuard {
  return {
    generation: activeChatGeneration,
    sessionId,
    daemonUrl: settings.url,
    daemonToken: settings.token,
  };
}

function isActiveChatRecoveryCurrent(guard: ActiveChatRecoveryGuard): boolean {
  return (
    activeChatGeneration === guard.generation &&
    activeSessionId === guard.sessionId &&
    activeSessionDaemonUrl === guard.daemonUrl &&
    activeSessionDaemonToken === guard.daemonToken
  );
}

function renderSessionSnapshot(
  snapshot: SidekickSessionSnapshot,
  activeTurnSafetyStatus?: SafetyStatus,
): void {
  transcriptView.renderSnapshotMessages(
    snapshot,
    activeTurnId,
    isActiveTurnStatus,
    clearPendingSubmittedQuestionIfPersisted,
  );

  if (snapshot.activeTurn && isActiveTurnStatus(snapshot.activeTurn.status)) {
    setActiveTurnId(snapshot.activeTurn.id);
    transcriptView.ensureAssistantPlaceholder();
    setActiveTurnProgressStatus(activeTurnSafetyStatus);
    return;
  }

  clearActiveTurn();
  setStatus("Ready");
}

function appendSessionMessage(message: SidekickMessage, activeTurn?: SidekickTurn): void {
  transcriptView.appendSessionMessage(
    message,
    activeTurn,
    activeTurnId,
    isActiveTurnStatus,
    clearPendingSubmittedQuestionIfPersisted,
  );
}

function setActiveTurnProgressStatus(safetyStatus?: SafetyStatus): void {
  setStatus(safetyStatus === "warning" ? "Review" : "Asking");
}

function isActiveTurnStatus(status: SidekickTurn["status"]): boolean {
  return status === "pending" || status === "running";
}

function disconnectProtocolClient(): void {
  const client = protocolClient;
  unsubscribeProtocolNotifications?.();
  unsubscribeProtocolNotifications = null;
  protocolClient = null;
  subscribedSessionId = null;
  client?.close();
}

function protocolClientMatchesSavedSettings(
  client: SidekickProtocolClient,
  settings: DaemonSettings,
): boolean {
  try {
    return client.matches(settings);
  } catch {
    return false;
  }
}

async function assertPendingSubmittedQuestionCaptureScopeFresh(
  pendingQuestion: PendingSubmittedQuestion,
): Promise<void> {
  try {
    await captureState.assertGrantFresh(pendingQuestion.captureGrant);
  } catch (error) {
    pendingSubmittedQuestions.discard(pendingQuestion);
    throw error;
  }
}

function clearPendingSubmittedQuestionIfPersisted(
  message: SidekickMessage,
  activeTurn?: SidekickTurn,
): void {
  const submittedText = pendingSubmittedQuestions.clearIfPersisted(message, activeTurn);
  clearSubmittedDraftIfUnchanged(submittedText);
}

function clearPendingSubmittedQuestion(pendingQuestion: PendingSubmittedQuestion): void {
  const submittedText = pendingSubmittedQuestions.clear(pendingQuestion);
  clearSubmittedDraftIfUnchanged(submittedText);
}

function clearSubmittedDraftIfUnchanged(submittedText: string | null): void {
  if (!submittedText) {
    return;
  }
  if (elements.messageInput.value.trim() === submittedText) {
    elements.messageInput.value = "";
  }
}

function handleSessionRecoveryError(error: unknown, fallbackMessage: string): void {
  if (isSessionNotFoundError(error)) {
    clearActiveChatState();
    void clearActiveChatMarker(captureState.currentGrant());
    setError("Daemon session was not found");
    return;
  }
  clearRecoveryBlockingState();
  setError(error instanceof Error ? error.message : fallbackMessage);
}

function isSessionNotFoundError(error: unknown): boolean {
  return error instanceof SidekickProtocolError && error.code === "session_not_found";
}

function clearActiveChatState(): void {
  activeChatGeneration += 1;
  activeSessionId = null;
  activeSessionDaemonUrl = null;
  activeSessionDaemonToken = null;
  subscribedSessionId = null;
  pendingSubmittedQuestions.discardCurrent();
  clearRecoveryBlockingState();
  clearTranscript();
}

function hasActiveDaemonIdentity(): boolean {
  return activeSessionDaemonUrl !== null || activeSessionDaemonToken !== null;
}

function activeDaemonIdentityMatches(settings: DaemonSettings): boolean {
  return activeSessionDaemonUrl === settings.url && activeSessionDaemonToken === settings.token;
}

function activeDaemonSettings(): DaemonSettings | null {
  if (!activeSessionDaemonUrl || !activeSessionDaemonToken) {
    return null;
  }
  return {
    url: activeSessionDaemonUrl,
    token: activeSessionDaemonToken,
  };
}

function shouldRecoverActiveSessionOnConnectionLoss(): boolean {
  return activeSessionId !== null && (activeTurnId !== null || requestInFlight);
}

function clearRecoveryBlockingState(): void {
  setSessionRecoveryRequired(false);
  clearActiveTurn(true);
}

function clearTranscript(): void {
  transcriptView.reset();
}

async function persistActiveChatMarker(): Promise<void> {
  const captureGrant = captureState.currentGrant();
  if (
    !activeSessionId ||
    !activeSessionDaemonUrl ||
    !activeSessionDaemonToken ||
    !captureGrant
  ) {
    await clearActiveChatMarker(captureGrant);
    return;
  }
  const marker: ActiveChatMarker = {
    daemonUrl: activeSessionDaemonUrl,
    daemonToken: activeSessionDaemonToken,
    tabId: captureGrant.tabId,
    origin: captureGrant.origin,
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

export {};
