import type {
  SidekickMessage,
  SidekickSessionSnapshot,
  SidekickTurn,
} from "./sidekick_protocol.js";

export type SessionSnapshotResolution =
  | {
      kind: "active_turn";
      turn: SidekickTurn;
    }
  | {
      kind: "terminal_message";
      message: SidekickMessage;
      status: "failed" | "cancelled";
    }
  | {
      kind: "ready";
    };

export function resolveSessionSnapshot(
  snapshot: SidekickSessionSnapshot,
  restoredActiveTurnId: string | null,
  pendingTerminalMessage: SidekickMessage | null,
): SessionSnapshotResolution {
  if (snapshot.activeTurn && isActiveTurnStatus(snapshot.activeTurn.status)) {
    return {
      kind: "active_turn",
      turn: snapshot.activeTurn,
    };
  }

  const terminalMessage =
    findRestoredTerminalActiveTurnMessage(snapshot, restoredActiveTurnId) ??
    pendingTerminalMessage;
  if (terminalMessage?.status === "failed" || terminalMessage?.status === "cancelled") {
    return {
      kind: "terminal_message",
      message: terminalMessage,
      status: terminalMessage.status,
    };
  }

  return { kind: "ready" };
}

export function isActiveTurnStatus(status: SidekickTurn["status"]): boolean {
  return status === "pending" || status === "running";
}

function findRestoredTerminalActiveTurnMessage(
  snapshot: SidekickSessionSnapshot,
  restoredActiveTurnId: string | null,
): SidekickMessage | null {
  if (!restoredActiveTurnId) {
    return null;
  }
  return (
    snapshot.messages.find(
      (message) =>
        message.role === "user" &&
        message.turnId === restoredActiveTurnId &&
        (message.status === "failed" || message.status === "cancelled"),
    ) ?? null
  );
}
