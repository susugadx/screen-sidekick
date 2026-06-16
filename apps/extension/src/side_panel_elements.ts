import { parseDaemonSettings } from "./side_panel_daemon_settings.js";
import type { DaemonSettings } from "./sidekick_protocol.js";

export interface SidePanelElements {
  bridgeForm: HTMLFormElement;
  bridgeUrl: HTMLInputElement;
  bridgeToken: HTMLInputElement;
  saveBridge: HTMLButtonElement;
  messageForm: HTMLFormElement;
  messageInput: HTMLTextAreaElement;
  ask: HTMLButtonElement;
  transcript: HTMLDivElement;
  debugCapture: HTMLButtonElement;
  copyJson: HTMLButtonElement;
  copyPrompt: HTMLButtonElement;
  screenContextJson: HTMLTextAreaElement;
  promptText: HTMLTextAreaElement;
  safetySummary: HTMLPreElement;
  status: HTMLSpanElement;
}

export interface SidePanelControlState {
  requestInFlight: boolean;
  turnActive: boolean;
  sessionRecoveryRequired: boolean;
}

export function loadSidePanelElements(): SidePanelElements {
  return {
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
}

export function readDaemonSettingsFromElements(
  elements: SidePanelElements,
): DaemonSettings {
  const settings = parseDaemonSettings({
    url: elements.bridgeUrl.value.trim(),
    token: elements.bridgeToken.value.trim(),
  });
  if (!settings) {
    throw new Error("Daemon URL and pairing token are required");
  }
  return settings;
}

export function setStatus(elements: SidePanelElements, text: string): void {
  elements.status.textContent = text;
  elements.status.className = "status";
}

export function setError(elements: SidePanelElements, text: string): void {
  elements.status.textContent = text;
  elements.status.className = "status error";
}

export function updateControlsDisabled(
  elements: SidePanelElements,
  state: SidePanelControlState,
): void {
  elements.ask.disabled =
    state.requestInFlight || state.turnActive || state.sessionRecoveryRequired;
  elements.messageInput.disabled =
    state.requestInFlight || state.turnActive || state.sessionRecoveryRequired;
  elements.debugCapture.disabled = state.requestInFlight;
  elements.saveBridge.disabled = state.requestInFlight;
}

export function clearSubmittedDraftIfUnchanged(
  elements: SidePanelElements,
  submittedText: string | null,
): void {
  if (!submittedText) {
    return;
  }
  if (elements.messageInput.value.trim() === submittedText) {
    elements.messageInput.value = "";
  }
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
