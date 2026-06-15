import type { CaptureGrant } from "./capture_permission.js";
import type {
  DaemonSettings,
  SafetyStatus,
  SidekickMessage,
  SidekickTurn,
} from "./sidekick_protocol.js";

export type PendingSubmittedQuestion = {
  sessionId: string;
  text: string;
  idempotencyKey: string;
  attachmentIds: string[];
  captureGrant: CaptureGrant;
  safetyStatus: SafetyStatus;
  daemonUrl: string;
  daemonToken: string;
  retainForIdempotentRetry: boolean;
  persistedMessageId?: string;
  persistedTurnId?: string;
};

export class PendingSubmittedQuestionState {
  private pendingQuestion: PendingSubmittedQuestion | null = null;

  current(): PendingSubmittedQuestion | null {
    return this.pendingQuestion;
  }

  set(pendingQuestion: PendingSubmittedQuestion): PendingSubmittedQuestion {
    this.pendingQuestion = pendingQuestion;
    return pendingQuestion;
  }

  findRetryable(
    settings: DaemonSettings,
    text: string,
    activeSessionId: string | null,
  ): PendingSubmittedQuestion | null {
    const pendingQuestion = this.pendingQuestion;
    if (!pendingQuestion) {
      return null;
    }
    if (
      activeSessionId !== pendingQuestion.sessionId ||
      pendingQuestion.text !== text ||
      pendingQuestion.daemonUrl !== settings.url ||
      pendingQuestion.daemonToken !== settings.token
    ) {
      return null;
    }
    return pendingQuestion;
  }

  recordPersistedIds(
    pendingQuestion: PendingSubmittedQuestion,
    messageId: string,
    turnId: string,
  ): void {
    if (this.pendingQuestion !== pendingQuestion) {
      return;
    }
    pendingQuestion.persistedMessageId = messageId;
    pendingQuestion.persistedTurnId = turnId;
  }

  retainForIdempotentRetry(pendingQuestion: PendingSubmittedQuestion): void {
    if (this.pendingQuestion !== pendingQuestion) {
      return;
    }
    pendingQuestion.retainForIdempotentRetry = true;
  }

  shouldDiscardAfterFailure(
    pendingQuestion: PendingSubmittedQuestion | null,
    failure: {
      recoveryRequired: boolean;
      terminalReplayFailure: boolean;
    },
  ): pendingQuestion is PendingSubmittedQuestion {
    if (!pendingQuestion || this.pendingQuestion !== pendingQuestion) {
      return false;
    }
    if (failure.terminalReplayFailure) {
      return true;
    }
    if (failure.recoveryRequired) {
      return false;
    }
    if (pendingQuestion.retainForIdempotentRetry) {
      return false;
    }
    return !pendingSubmittedQuestionHasPersistedIds(pendingQuestion);
  }

  clearIfPersisted(message: SidekickMessage, activeTurn?: SidekickTurn): string | null {
    const pendingQuestion = this.pendingQuestion;
    if (!pendingQuestion) {
      return null;
    }
    if (!messageMatchesPendingSubmittedQuestion(message, pendingQuestion, activeTurn)) {
      return null;
    }
    return this.clear(pendingQuestion);
  }

  clear(pendingQuestion: PendingSubmittedQuestion): string | null {
    if (this.pendingQuestion !== pendingQuestion) {
      return null;
    }
    const submittedText = pendingQuestion.text;
    this.pendingQuestion = null;
    return submittedText;
  }

  discard(pendingQuestion: PendingSubmittedQuestion): void {
    if (this.pendingQuestion !== pendingQuestion) {
      return;
    }
    this.pendingQuestion = null;
  }

  discardCurrent(): void {
    this.pendingQuestion = null;
  }
}

export function messageMatchesPendingSubmittedQuestion(
  message: SidekickMessage,
  pendingQuestion: PendingSubmittedQuestion,
  activeTurn?: SidekickTurn,
): boolean {
  if (message.sessionId !== pendingQuestion.sessionId || message.text !== pendingQuestion.text) {
    return false;
  }
  if (
    pendingQuestion.persistedMessageId &&
    message.id === pendingQuestion.persistedMessageId
  ) {
    return true;
  }
  if (pendingQuestion.persistedTurnId && message.turnId === pendingQuestion.persistedTurnId) {
    return true;
  }
  return Boolean(
    activeTurn &&
      activeTurn.sessionId === pendingQuestion.sessionId &&
      message.turnId === activeTurn.id,
  );
}

function pendingSubmittedQuestionHasPersistedIds(
  pendingQuestion: PendingSubmittedQuestion,
): boolean {
  return Boolean(pendingQuestion.persistedMessageId || pendingQuestion.persistedTurnId);
}
