import assert from "node:assert/strict";
import test from "node:test";

import { resolveSessionSnapshot } from "../dist/side_panel_session_snapshot.js";

test("active turn wins over restored and pending terminal messages", () => {
  const activeTurn = turn({ id: "turn_active", status: "running" });
  const restoredTerminalMessage = userMessage({
    id: "msg_restored",
    turnId: "turn_restored",
    status: "failed",
  });
  const pendingTerminalMessage = userMessage({
    id: "msg_pending",
    turnId: "turn_pending",
    status: "cancelled",
  });

  const resolution = resolveSessionSnapshot(
    snapshot({
      messages: [restoredTerminalMessage],
      activeTurn,
    }),
    "turn_restored",
    pendingTerminalMessage,
  );

  assert.deepEqual(resolution, {
    kind: "active_turn",
    turn: activeTurn,
  });
});

test("restored failed active turn resolves terminal before ready", () => {
  const failedMessage = userMessage({
    id: "msg_failed",
    turnId: "turn_1",
    status: "failed",
  });

  assert.deepEqual(
    resolveSessionSnapshot(snapshot({ messages: [failedMessage] }), "turn_1", null),
    {
      kind: "terminal_message",
      message: failedMessage,
      status: "failed",
    },
  );
});

test("restored cancelled active turn resolves terminal before ready", () => {
  const cancelledMessage = userMessage({
    id: "msg_cancelled",
    turnId: "turn_1",
    status: "cancelled",
  });

  assert.deepEqual(
    resolveSessionSnapshot(snapshot({ messages: [cancelledMessage] }), "turn_1", null),
    {
      kind: "terminal_message",
      message: cancelledMessage,
      status: "cancelled",
    },
  );
});

test("ready resolves only when no active or current terminal turn exists", () => {
  const oldFailedMessage = userMessage({
    id: "msg_old",
    turnId: "turn_old",
    status: "failed",
  });

  assert.deepEqual(
    resolveSessionSnapshot(snapshot({ messages: [oldFailedMessage] }), "turn_current", null),
    { kind: "ready" },
  );
});

function snapshot({ messages = [], activeTurn = null } = {}) {
  return {
    session: {
      id: "sess_1",
      title: "Screen Sidekick",
    },
    messages,
    attachments: [],
    activeTurn,
  };
}

function userMessage({ id, turnId, status }) {
  return {
    id,
    sessionId: "sess_1",
    role: "user",
    text: "Question",
    status,
    turnId,
  };
}

function turn({ id, status }) {
  return {
    id,
    sessionId: "sess_1",
    status,
  };
}
