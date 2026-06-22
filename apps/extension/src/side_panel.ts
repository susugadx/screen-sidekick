import { clearPreviewState, setPreviewState } from "./preview_state.js";
import {
  clearActiveChatMarker,
  loadActiveChatMarker,
  saveActiveChatMarker,
} from "./side_panel_active_chat.js";
import {
  SidePanelActiveSessionState,
  type ActiveChatRecoveryGuard,
} from "./side_panel_active_session.js";
import { formatSafetySummary, postCaptureToBridge } from "./side_panel_bridge.js";
import { SidePanelCaptureState } from "./side_panel_capture.js";
import {
  loadStoredDaemonSettings,
  storeDaemonSettings,
} from "./side_panel_daemon_settings.js";
import {
  clearSubmittedDraftIfUnchanged as clearSubmittedDraftIfElementsUnchanged,
  loadSidePanelElements,
  readDaemonSettingsFromElements,
  setError as setElementError,
  setStatus as setElementStatus,
  updateControlsDisabled as updateElementsControlsDisabled,
} from "./side_panel_elements.js";
import {
  PendingSubmittedQuestionState,
  type PendingSubmittedQuestion,
} from "./side_panel_pending_submission.js";
import { SidePanelProtocolConnection } from "./side_panel_protocol_connection.js";
import {
  isActiveTurnStatus,
  resolveSessionSnapshot,
} from "./side_panel_session_snapshot.js";
import { SidePanelTranscriptView } from "./side_panel_transcript.js";
import {
  SidekickProtocolError,
  NATIVE_CONNECTION_SETTINGS,
  createMessageSendIdempotencyKey,
  isNativeConnectionSettings,
  isMessageSendRequestTimeoutError,
  isTerminalMessageSendReplayError,
  type DaemonSettings,
  type SafetyStatus,
  type SidekickClient,
  type SidekickMessage,
  type SidekickNotification,
  type SidekickSessionSnapshot,
  type SidekickTurn,
} from "./sidekick_protocol.js";

const captureState = new SidePanelCaptureState();
const pendingSubmittedQuestions = new PendingSubmittedQuestionState();
const activeSession = new SidePanelActiveSessionState();
const protocolConnection = new SidePanelProtocolConnection();

type PreparedSubmittedQuestion = {
  client: SidekickClient;
  settings: DaemonSettings;
  sessionId: string;
  pendingQuestion: PendingSubmittedQuestion;
  safetyStatus?: SafetyStatus;
};

type EnsureActiveSessionOptions = {
  resetStaleSession?: boolean;
};

const elements = loadSidePanelElements();
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
  for (const quickQuestion of elements.quickQuestions) {
    quickQuestion.addEventListener("click", () => {
      const question = quickQuestion.textContent?.trim();
      if (!question) {
        return;
      }
      elements.messageInput.value = question;
      elements.messageInput.focus();
    });
  }
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
  await recoverActiveChatFromStorage(NATIVE_CONNECTION_SETTINGS);
  if (settings && !activeSession.sessionId) {
    await recoverActiveChatFromStorage(settings);
  }
}

async function saveDaemonSettings(): Promise<void> {
  const settings = readDaemonSettingsFromInputs();
  await storeDaemonSettings(settings);

  const activeSettings = activeSession.activeDaemonSettings();
  if (!activeSettings || !isNativeConnectionSettings(activeSettings)) {
    if (!protocolConnection.matches(settings)) {
      disconnectProtocolClient();
    }
  }
  if (
    activeSettings &&
    !isNativeConnectionSettings(activeSettings) &&
    !activeSession.matchesActiveDaemonIdentity(settings)
  ) {
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
    const fallbackSettings = readOptionalDaemonSettingsFromInputs();
    const submittedQuestion = await prepareSubmittedQuestion(fallbackSettings, question);
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
        activeChatGuard(submittedQuestion.settings, submittedQuestion.sessionId),
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
    const recoveryRequired = activeSession.sessionRecoveryRequired;
    const messageSendTimeout = isMessageSendRequestTimeoutError(error);
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
    if (activeSession.sessionRecoveryRequired) {
      setStatus("Reconnecting to daemon");
    } else if (!messageSendTimeout && !recoveryRequired) {
      transcriptView.clearActiveAssistantReference();
      setError(error instanceof Error ? error.message : "Ask failed");
    }
  } finally {
    setRequestInFlight(false);
  }
}

async function prepareSubmittedQuestion(
  fallbackSettings: DaemonSettings | null,
  question: string,
): Promise<PreparedSubmittedQuestion> {
  const activeSettings = activeSession.activeDaemonSettings();
  const retryQuestion = activeSettings
    ? pendingSubmittedQuestions.findRetryable(
        activeSettings,
        question,
        activeSession.sessionId,
      )
    : null;
  if (retryQuestion && activeSettings) {
    await assertPendingSubmittedQuestionCaptureScopeFresh(retryQuestion);
    pendingSubmittedQuestions.retainForIdempotentRetry(retryQuestion);
    setStatus("Asking");
    const client = await ensureProtocolClient(activeSettings);
    let sessionId: string;
    try {
      sessionId = await ensureActiveSession(client, activeSettings);
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
      settings: activeSettings,
      sessionId,
      pendingQuestion: retryQuestion,
      safetyStatus: retryQuestion.safetyStatus,
    };
  }

  setStatus("Capturing");
  const capturedContext = await captureState.captureActiveTabContextWithGrant();
  const connected = await ensurePreferredProtocolClient(fallbackSettings);
  const client = connected.client;
  const settings = connected.settings;
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
    settings,
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

async function ensurePreferredProtocolClient(
  fallbackSettings: DaemonSettings | null,
): Promise<{ client: SidekickClient; settings: DaemonSettings }> {
  const connected = await protocolConnection.ensurePreferred(
    fallbackSettings,
    handleSidekickNotification,
  );
  if (
    activeSession.activeDaemonSettings() !== null &&
    !activeSession.matchesActiveDaemonIdentity(connected.settings)
  ) {
    clearActiveChatState();
    void clearActiveChatMarker(captureState.currentGrant());
  }
  return connected;
}

async function ensureProtocolClient(settings: DaemonSettings): Promise<SidekickClient> {
  const settingsChanged =
    activeSession.activeDaemonSettings() !== null &&
    !activeSession.matchesActiveDaemonIdentity(settings);
  if (!protocolConnection.matches(settings)) {
    disconnectProtocolClient();
  }
  if (settingsChanged) {
    clearActiveChatState();
    void clearActiveChatMarker(captureState.currentGrant());
  }

  return (await protocolConnection.ensure(settings, handleSidekickNotification)).client;
}

async function ensureActiveSession(
  client: SidekickClient,
  settings: DaemonSettings,
  options: EnsureActiveSessionOptions = {},
): Promise<string> {
  if (activeSession.sessionId) {
    const sessionId = activeSession.sessionId;
    if (activeSession.subscribedSessionId !== sessionId) {
      try {
        await client.subscribeSession(sessionId);
        activeSession.setSubscribedSessionId(sessionId);
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
  activeSession.setActiveSession(settings, session.id);
  activeSession.setSubscribedSessionId(session.id);
  void persistActiveChatMarker();
  return session.id;
}

function handleSidekickNotification(notification: SidekickNotification): void {
  switch (notification.kind) {
    case "turn_delta":
      transcriptView.appendTurnDelta(
        notification.turnId,
        notification.delta,
        activeSession.activeTurnId,
      );
      return;
    case "turn_completed":
      if (notification.turn.id === activeSession.activeTurnId) {
        clearActiveTurn();
        setStatus("Ready");
      }
      return;
    case "turn_failed":
      if (!notification.turn || notification.turn.id === activeSession.activeTurnId) {
        clearActiveTurn(true);
        setError(notification.message ?? "Codex turn failed");
      }
      return;
    case "turn_cancelled":
      if (notification.turn.id === activeSession.activeTurnId) {
        clearActiveTurn(true);
        setStatus("Cancelled");
      }
      return;
    case "message_created":
      if (notification.sessionId === activeSession.sessionId) {
        appendObservedSessionMessage(notification.message);
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
  if (activeSession.shouldRecoverOnConnectionLoss()) {
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

  activeSession.restoreActiveChatMarker(marker);
  const guard = beginSessionRecovery(settings, marker.sessionId);
  if (marker.activeTurnId) {
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
  const sessionId = activeSession.sessionId;
  if (!sessionId) {
    return;
  }

  let guard: ActiveChatRecoveryGuard | null = null;
  try {
    const settings = activeSession.activeDaemonSettings();
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
  client: SidekickClient,
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
  client: SidekickClient,
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
  activeSession.setSubscribedSessionId(guard.sessionId);
  activeSession.setActiveSession(
    { url: guard.daemonUrl, token: guard.daemonToken },
    snapshot.session.id,
  );
  renderSessionSnapshot(snapshot, activeTurnSafetyStatus);
  void persistActiveChatMarker();
  return true;
}

function beginSessionRecovery(
  settings: DaemonSettings,
  sessionId: string,
): ActiveChatRecoveryGuard {
  setSessionRecoveryRequired(true);
  return activeSession.beginRecovery(settings, sessionId);
}

function activeChatGuard(
  settings: DaemonSettings,
  sessionId: string,
): ActiveChatRecoveryGuard {
  return activeSession.activeChatGuard(settings, sessionId);
}

function isActiveChatRecoveryCurrent(guard: ActiveChatRecoveryGuard): boolean {
  return activeSession.isRecoveryCurrent(guard);
}

function renderSessionSnapshot(
  snapshot: SidekickSessionSnapshot,
  activeTurnSafetyStatus?: SafetyStatus,
): void {
  const restoredActiveTurnId = activeSession.activeTurnId;
  const resolution = resolveSessionSnapshot(
    snapshot,
    restoredActiveTurnId,
    pendingSubmittedQuestions.findTerminalMessage(snapshot.messages),
  );
  transcriptView.renderSnapshotMessages(
    snapshot,
    restoredActiveTurnId,
    isActiveTurnStatus,
    clearPendingSubmittedQuestionIfPersisted,
  );

  if (resolution.kind === "active_turn") {
    setActiveTurnId(resolution.turn.id);
    transcriptView.ensureAssistantPlaceholder();
    setActiveTurnProgressStatus(activeTurnSafetyStatus);
    return;
  }

  if (resolution.kind === "terminal_message" && resolution.status === "failed") {
    clearSubmittedDraftIfUnchanged(
      pendingSubmittedQuestions.clearIfTerminalMessage(resolution.message),
    );
    clearActiveTurn(true);
    setError("Codex turn failed");
    return;
  }
  if (resolution.kind === "terminal_message" && resolution.status === "cancelled") {
    clearSubmittedDraftIfUnchanged(
      pendingSubmittedQuestions.clearIfTerminalMessage(resolution.message),
    );
    clearActiveTurn(true);
    setStatus("Cancelled");
    return;
  }

  clearActiveTurn();
  setStatus("Ready");
}

function appendObservedSessionMessage(message: SidekickMessage): void {
  pendingSubmittedQuestions.recordObservedMessage(message);
  transcriptView.appendSessionMessage(
    message,
    undefined,
    activeSession.activeTurnId,
    isActiveTurnStatus,
    () => undefined,
  );
}

function appendSessionMessage(message: SidekickMessage, activeTurn?: SidekickTurn): void {
  transcriptView.appendSessionMessage(
    message,
    activeTurn,
    activeSession.activeTurnId,
    isActiveTurnStatus,
    clearPendingSubmittedQuestionIfPersisted,
  );
}

function setActiveTurnProgressStatus(safetyStatus?: SafetyStatus): void {
  setStatus(safetyStatus === "warning" ? "Review" : "Asking");
}

function disconnectProtocolClient(): void {
  protocolConnection.disconnect();
  activeSession.clearSubscribedSession();
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
  clearSubmittedDraftIfElementsUnchanged(elements, submittedText);
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
  activeSession.clearActiveChat();
  pendingSubmittedQuestions.discardCurrent();
  clearTranscript();
  void persistActiveChatMarker();
  updateControlsDisabled();
}

function clearRecoveryBlockingState(): void {
  activeSession.clearRecoveryBlockingState();
  transcriptView.clearActiveAssistant(true);
  void persistActiveChatMarker();
  updateControlsDisabled();
}

function clearTranscript(): void {
  transcriptView.reset();
}

async function persistActiveChatMarker(): Promise<void> {
  const captureGrant = captureState.currentGrant();
  const marker = activeSession.toActiveChatMarker(captureGrant);
  if (!marker) {
    await clearActiveChatMarker(captureGrant);
    return;
  }
  await saveActiveChatMarker(marker);
}

function readDaemonSettingsFromInputs(): DaemonSettings {
  return readDaemonSettingsFromElements(elements);
}

function readOptionalDaemonSettingsFromInputs(): DaemonSettings | null {
  const hasUrl = elements.bridgeUrl.value.trim().length > 0;
  const hasToken = elements.bridgeToken.value.trim().length > 0;
  return hasUrl && hasToken ? readDaemonSettingsFromInputs() : null;
}

function setRequestInFlight(isBusy: boolean): void {
  activeSession.setRequestInFlight(isBusy);
  updateControlsDisabled();
}

function setSessionRecoveryRequired(required: boolean): void {
  activeSession.setSessionRecoveryRequired(required);
  updateControlsDisabled();
}

function setActiveTurnId(turnId: string | null): void {
  activeSession.setActiveTurnId(turnId);
  void persistActiveChatMarker();
  updateControlsDisabled();
}

function updateControlsDisabled(): void {
  updateElementsControlsDisabled(elements, activeSession.toControlState());
}

function setStatus(text: string): void {
  setElementStatus(elements, text);
}

function setError(text: string): void {
  setElementError(elements, text);
}

export {};
