import type {
  SidekickMessage,
  SidekickSessionSnapshot,
  SidekickTurn,
} from "./sidekick_protocol.js";

type ActiveTurnStatusPredicate = (status: SidekickTurn["status"]) => boolean;
type UserMessageHandler = (message: SidekickMessage, activeTurn?: SidekickTurn) => void;

export class SidePanelTranscriptView {
  private activeAssistantText = "";
  private activeAssistantTextElement: HTMLDivElement | null = null;
  private renderedMessageIds = new Set<string>();

  constructor(private readonly transcript: HTMLDivElement) {}

  appendTurnDelta(turnId: string, delta: string, activeTurnId: string | null): boolean {
    if (turnId !== activeTurnId || !this.activeAssistantTextElement) {
      return false;
    }
    this.activeAssistantText += delta;
    this.activeAssistantTextElement.textContent = this.activeAssistantText;
    this.scrollToEnd();
    return true;
  }

  appendSessionMessage(
    message: SidekickMessage,
    activeTurn: SidekickTurn | undefined,
    activeTurnId: string | null,
    isActiveTurnStatus: ActiveTurnStatusPredicate,
    onUserMessage: UserMessageHandler,
  ): void {
    if (this.renderedMessageIds.has(message.id)) {
      return;
    }
    this.renderedMessageIds.add(message.id);

    if (message.role === "user") {
      onUserMessage(message, activeTurn);
      this.appendTranscriptMessage("user", message.text);
      return;
    }
    if (message.role !== "assistant") {
      return;
    }

    const belongsToActiveTurn =
      (activeTurn && message.turnId === activeTurn.id && isActiveTurnStatus(activeTurn.status)) ||
      (!activeTurn && message.turnId === activeTurnId);
    if (belongsToActiveTurn && this.activeAssistantTextElement) {
      this.activeAssistantText = message.text;
      this.activeAssistantTextElement.textContent = this.activeAssistantText;
      this.scrollToEnd();
      return;
    }

    const body = this.appendTranscriptMessage("assistant", message.text);
    if (belongsToActiveTurn) {
      this.activeAssistantText = message.text;
      this.activeAssistantTextElement = body;
    }
  }

  renderSnapshotMessages(
    snapshot: SidekickSessionSnapshot,
    activeTurnId: string | null,
    isActiveTurnStatus: ActiveTurnStatusPredicate,
    onUserMessage: UserMessageHandler,
  ): void {
    this.reset();
    for (const message of snapshot.messages) {
      this.appendSessionMessage(
        message,
        snapshot.activeTurn,
        activeTurnId,
        isActiveTurnStatus,
        onUserMessage,
      );
    }
  }

  startAssistantPlaceholder(): void {
    this.activeAssistantText = "";
    this.activeAssistantTextElement = this.appendTranscriptMessage("assistant", "");
  }

  ensureAssistantPlaceholder(): void {
    if (!this.activeAssistantTextElement) {
      this.startAssistantPlaceholder();
    }
  }

  clearActiveAssistant(removeEmptyAssistantPlaceholder = false): void {
    if (
      removeEmptyAssistantPlaceholder &&
      this.activeAssistantTextElement &&
      this.activeAssistantText.length === 0
    ) {
      this.activeAssistantTextElement.closest(".message")?.remove();
    }
    this.clearActiveAssistantReference();
  }

  clearActiveAssistantReference(): void {
    this.activeAssistantTextElement = null;
    this.activeAssistantText = "";
  }

  reset(): void {
    this.transcript.replaceChildren();
    this.renderedMessageIds = new Set();
    this.clearActiveAssistantReference();
  }

  private appendTranscriptMessage(role: "user" | "assistant", text: string): HTMLDivElement {
    const item = document.createElement("div");
    item.className = `message ${role}`;

    const label = document.createElement("div");
    label.className = "message-label";
    label.textContent = role === "user" ? "You" : "Codex";

    const body = document.createElement("div");
    body.className = "message-text";
    body.textContent = text;

    item.append(label, body);
    this.transcript.append(item);
    this.scrollToEnd();
    return body;
  }

  private scrollToEnd(): void {
    this.transcript.scrollTop = this.transcript.scrollHeight;
  }
}
