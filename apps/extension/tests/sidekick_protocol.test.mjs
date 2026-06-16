import assert from "node:assert/strict";
import test from "node:test";

import { flushMicrotasks, installManualTimers } from "./manual_timers.mjs";
import {
  SidekickProtocolClient,
  SidekickProtocolError,
  buildDaemonCaptureUrl,
  buildDaemonWebSocketUrl,
  isTerminalMessageSendReplayError,
  parseInitializeResult,
  parseSidekickNotification,
  parseWireMessageText,
} from "../dist/sidekick_protocol.js";
import {
  AcceptingServer,
  DelayedMessageSendServer,
  HangingAttachBrowserContextServer,
  HangingSessionCreateServer,
  HangingSessionGetServer,
  MalformedInitializeServer,
  MalformedSessionGetServer,
  ProtocolFakeWebSocket,
  RejectingInitializeServer,
  SmallMessageLimitServer,
  UnansweredInitializeServer,
  UnavailableInitializeServer,
  connectProtocolClient,
  installProtocolWebSocket,
} from "./support/protocol_harness.mjs";

test("builds daemon websocket URL without carrying query tokens", () => {
  const url = buildDaemonWebSocketUrl("http://127.0.0.1:43001?token=SECRET#debug");

  assert.equal(url.toString(), "ws://127.0.0.1:43001/v0/ws");
  assert.equal(url.toString().includes("SECRET"), false);
});

test("builds daemon capture URL from websocket settings without carrying query tokens", () => {
  const url = buildDaemonCaptureUrl("ws://127.0.0.1:43001?token=SECRET#debug");

  assert.equal(url.toString(), "http://127.0.0.1:43001/v0/capture");
  assert.equal(url.toString().includes("SECRET"), false);
});

test("rejects non-loopback daemon websocket URL", () => {
  assert.throws(
    () => buildDaemonWebSocketUrl("http://localhost:43001"),
    /Daemon URL must use/,
  );
  assert.throws(
    () => buildDaemonWebSocketUrl("https://127.0.0.1:43001"),
    /Daemon URL must use/,
  );
});

test("parses JSON-RPC protocol errors with stable codes", () => {
  const message = parseWireMessageText(
    JSON.stringify({
      jsonrpc: "2.0",
      id: "request-1",
      error: {
        code: "unauthorized",
        message: "Pairing token is invalid.",
        data: { retryable: false },
      },
    }),
  );

  assert.equal(message.kind, "failure");
  assert.equal(message.id, "request-1");
  assert.equal(message.error.code, "unauthorized");
  assert.equal(message.error.retryable, false);
  assert.equal(message.error.messageSendIdempotencyDisposition, undefined);
});

test("parses structured message send idempotency disposition", () => {
  const message = parseWireMessageText(
    JSON.stringify({
      jsonrpc: "2.0",
      id: "request-1",
      error: {
        code: "codex_not_found",
        message: "Previous message/send attempt failed.",
        data: {
          message_send_idempotency_disposition: "discard",
        },
      },
    }),
  );

  assert.equal(message.kind, "failure");
  assert.equal(message.error.messageSendIdempotencyDisposition, "discard");
  assert.equal(
    isTerminalMessageSendReplayError(message.error),
    true,
  );
});

test("classifies terminal message send replay errors from structured data and v0 legacy text", () => {
  assert.equal(
    isTerminalMessageSendReplayError(
      new SidekickProtocolError("codex_not_found", "Different message.", {
        messageSendIdempotencyDisposition: "discard",
      }),
    ),
    true,
  );
  assert.equal(
    isTerminalMessageSendReplayError(
      new SidekickProtocolError("codex_not_found", "Previous message/send attempt failed."),
    ),
    true,
  );
  assert.equal(
    isTerminalMessageSendReplayError(
      new SidekickProtocolError("invalid_params", "Previous message/send attempt was cancelled."),
    ),
    true,
  );
  assert.equal(
    isTerminalMessageSendReplayError(
      new SidekickProtocolError("internal_error", "Retry setup failed."),
    ),
    false,
  );
  assert.equal(
    isTerminalMessageSendReplayError(
      new SidekickProtocolError("codex_not_found", "Previous message/send attempt failed"),
    ),
    false,
  );
  assert.equal(
    isTerminalMessageSendReplayError(new Error("Previous message/send attempt failed.")),
    false,
  );
});

test("parses initialize result codex readiness", () => {
  const result = parseInitializeResult({
    codex_readiness: {
      available: false,
      version: "codex-fake 1.0.0",
      error_code: "unsupported_codex_version",
    },
    limits: {
      max_message_bytes: 262144,
      max_attachment_bytes: 131072,
    },
  });

  assert.deepEqual(result, {
    codexReadiness: {
      available: false,
      version: "codex-fake 1.0.0",
      errorCode: "unsupported_codex_version",
    },
    limits: {
      maxMessageBytes: 262144,
      maxAttachmentBytes: 131072,
    },
  });
});

test("rejects initialize result when protocol limits are missing or invalid", () => {
  const base = {
    codex_readiness: {
      available: true,
    },
  };

  assert.equal(parseInitializeResult(base), null);
  assert.equal(
    parseInitializeResult({
      ...base,
      limits: {
        max_message_bytes: 0,
        max_attachment_bytes: 131072,
      },
    }),
    null,
  );
  assert.equal(
    parseInitializeResult({
      ...base,
      limits: {
        max_message_bytes: 262144.5,
        max_attachment_bytes: 131072,
      },
    }),
    null,
  );
  assert.equal(
    parseInitializeResult({
      ...base,
      limits: {
        max_message_bytes: 262144,
      },
    }),
    null,
  );
});

test("parses turn delta notification and ignores invalid params", () => {
  const delta = parseSidekickNotification("turn/delta", {
    session_id: "sess_1",
    turn_id: "turn_1",
    delta: "",
  });
  const invalid = parseSidekickNotification("turn/delta", {
    session_id: "sess_1",
  });

  assert.deepEqual(delta, {
    kind: "turn_delta",
    sessionId: "sess_1",
    turnId: "turn_1",
    delta: "",
  });
  assert.deepEqual(invalid, {
    kind: "ignored",
    method: "turn/delta",
  });
});

test("parses cancelled turn notification as terminal state", () => {
  const cancelled = parseSidekickNotification("turn/cancelled", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "cancelled",
    },
  });

  assert.deepEqual(cancelled, {
    kind: "turn_cancelled",
    sessionId: "sess_1",
    turn: {
      id: "turn_1",
      sessionId: "sess_1",
      status: "cancelled",
    },
  });
});

test("parses turn failed notification with turn and concrete message", () => {
  const failed = parseSidekickNotification("turn/failed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "failed",
    },
    message: "model failed",
  });

  assert.deepEqual(failed, {
    kind: "turn_failed",
    sessionId: "sess_1",
    turn: {
      id: "turn_1",
      sessionId: "sess_1",
      status: "failed",
    },
    message: "model failed",
  });
});

test("connect closes websocket when initialize is rejected", async () => {
  const server = new RejectingInitializeServer();
  const restoreWebSocket = installProtocolWebSocket(server);

  try {
    await assert.rejects(
      () =>
        SidekickProtocolClient.connect(
          { url: "http://127.0.0.1:43001", token: "bad-token" },
          "test",
        ),
      /Pairing token is invalid/,
    );
    assert.equal(server.socket.closeCount, 1);
    assert.equal(server.socket.readyState, ProtocolFakeWebSocket.CLOSED);
  } finally {
    restoreWebSocket();
  }
});

test("connect rejects and closes websocket when codex readiness is unavailable", async () => {
  const server = new UnavailableInitializeServer();
  const restoreWebSocket = installProtocolWebSocket(server);

  try {
    await assert.rejects(
      () =>
        SidekickProtocolClient.connect(
          { url: "http://127.0.0.1:43001", token: "pairing-token" },
          "test",
        ),
      /Codex app-server version is unsupported/,
    );
    assert.equal(server.socket.closeCount, 1);
    assert.equal(server.socket.readyState, ProtocolFakeWebSocket.CLOSED);
  } finally {
    restoreWebSocket();
  }
});

test("connect rejects and closes websocket when initialize response shape is invalid", async () => {
  const server = new MalformedInitializeServer();
  const restoreWebSocket = installProtocolWebSocket(server);

  try {
    await assert.rejects(
      () =>
        SidekickProtocolClient.connect(
          { url: "http://127.0.0.1:43001", token: "pairing-token" },
          "test",
        ),
      /Daemon message shape is invalid/,
    );
    assert.equal(server.socket.closeCount, 1);
    assert.equal(server.socket.readyState, ProtocolFakeWebSocket.CLOSED);
  } finally {
    restoreWebSocket();
  }
});

test("connect times out unanswered initialize and closes websocket", async () => {
  const server = new UnansweredInitializeServer();
  const timers = installManualTimers();
  const restoreWebSocket = installProtocolWebSocket(server);

  try {
    const connectPromise = SidekickProtocolClient.connect(
      { url: "http://127.0.0.1:43001", token: "pairing-token" },
      "test",
    );
    await flushMicrotasks();
    assert.equal(server.socket.sent[0]?.method, "initialize");
    assert.equal(timers.nextDelay(), 60_000);

    const rejected = assert.rejects(connectPromise, /Daemon request timed out/);
    timers.fireNext();
    await rejected;

    assert.equal(server.socket.closeCount, 1);
    assert.equal(server.socket.readyState, ProtocolFakeWebSocket.CLOSED);
    assert.equal(timers.size, 0);
  } finally {
    timers.restore();
    restoreWebSocket();
  }
});

test("malformed daemon message rejects pending request without closing socket", async () => {
  const { client, server } = await connectProtocolClient(new MalformedSessionGetServer());
  const notifications = [];
  client.onNotification((notification) => notifications.push(notification));

  await assert.rejects(
    () => client.getSession("sess_1"),
    /Daemon message shape is invalid/,
  );

  assert.equal(notifications.length, 1);
  assert.equal(notifications[0].kind, "error");
  assert.equal(notifications[0].error.code, "invalid_request");
  assert.equal(server.socket.readyState, ProtocolFakeWebSocket.OPEN);

  const session = await client.createSession("After malformed response");
  assert.deepEqual(session, {
    id: "sess_after_malformed",
    title: "After malformed response",
  });
});

test("session/get timeout cleans pending state and ignores late response", async () => {
  const { client, server } = await connectProtocolClient(new HangingSessionGetServer());
  const timers = installManualTimers();

  try {
    const sessionGetPromise = client.getSession("sess_1");
    await flushMicrotasks();
    assert.equal(server.hangingSessionGetId, "sidekick_extension_2");
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 10_000);

    const rejected = assert.rejects(sessionGetPromise, /Daemon request timed out/);
    timers.fireNext();
    await rejected;

    server.socket.receiveSuccess(server.hangingSessionGetId, {
      session: {
        id: "sess_1",
        title: "Late session",
      },
      messages: [],
      attachments: [],
      active_turn: null,
    });
  } finally {
    timers.restore();
  }

  const session = await client.createSession("After timeout");
  assert.deepEqual(session, {
    id: "sess_after_timeout",
    title: "After timeout",
  });
});

test("session/create timeout cleans pending state and ignores late response", async () => {
  const { client, server } = await connectProtocolClient(new HangingSessionCreateServer());
  const timers = installManualTimers();

  try {
    const createPromise = client.createSession("Hung create");
    await flushMicrotasks();
    assert.equal(server.hangingSessionCreateId, "sidekick_extension_2");
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 10_000);

    const rejected = assert.rejects(createPromise, /Daemon request timed out/);
    timers.fireNext();
    await rejected;

    server.socket.receiveSuccess(server.hangingSessionCreateId, {
      session: {
        id: "sess_late_create",
        title: "Late create",
      },
    });
  } finally {
    timers.restore();
  }

  const snapshot = await client.getSession("sess_1");
  assert.equal(snapshot.session.id, "sess_1");
  assert.deepEqual(snapshot.messages, []);
});

test("context/attach_browser timeout cleans pending state and ignores late response", async () => {
  const { client, server } = await connectProtocolClient(
    new HangingAttachBrowserContextServer(),
  );
  const timers = installManualTimers();

  try {
    const attachPromise = client.attachBrowserContext(
      "sess_1",
      { schema_version: "raw_browser_context.v0.1" },
      "manual_attach",
    );
    await flushMicrotasks();
    assert.equal(server.hangingAttachBrowserContextId, "sidekick_extension_2");
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 10_000);

    const rejected = assert.rejects(attachPromise, /Daemon request timed out/);
    timers.fireNext();
    await rejected;

    server.socket.receiveSuccess(server.hangingAttachBrowserContextId, {
      attachment: {
        id: "att_late",
        session_id: "sess_1",
        summary: "Late attach",
        safety_status: "clean",
        debug_available: false,
      },
    });
  } finally {
    timers.restore();
  }

  const session = await client.createSession("After attach timeout");
  assert.deepEqual(session, {
    id: "sess_after_attach_timeout",
    title: "After attach timeout",
  });
});

test("message/send clears side-effect timeout when delayed response succeeds", async () => {
  const { client, server } = await connectProtocolClient(new DelayedMessageSendServer());
  const timers = installManualTimers();

  try {
    let settled = false;
    const sendPromise = client
      .sendMessage("sess_1", "Slow question", "idem_from_caller", ["att_1"], "ask_only")
      .then((result) => {
        settled = true;
        return result;
      });
    await flushMicrotasks();

    assert.equal(server.delayedMessageSendId, "sidekick_extension_2");
    assert.equal(server.delayedMessageSendParams.idempotency_key, "idem_from_caller");
    assert.deepEqual(server.delayedMessageSendParams.attachment_ids, ["att_1"]);
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 45_000);
    assert.equal(settled, false);

    server.releaseMessageSend();
    assert.deepEqual(await sendPromise, {
      messageId: "msg_delayed",
      turnId: "turn_delayed",
      reused: false,
    });
    assert.equal(settled, true);
    assert.equal(timers.size, 0);
  } finally {
    timers.restore();
  }
});

test("message/send timeout reports connection lost once and closes socket", async () => {
  const { client, server } = await connectProtocolClient(new DelayedMessageSendServer());
  const timers = installManualTimers();
  const notifications = [];
  client.onNotification((notification) => notifications.push(notification));

  try {
    const sendPromise = client.sendMessage(
      "sess_1",
      "Slow question",
      "idem_from_caller",
      ["att_1"],
      "ask_only",
    );
    await flushMicrotasks();

    assert.equal(server.delayedMessageSendId, "sidekick_extension_2");
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 45_000);

    const rejected = assert.rejects(sendPromise, /Daemon request timed out/);
    timers.fireNext();
    await rejected;

    assert.deepEqual(notifications, [
      {
        kind: "connection_lost",
        message: "Daemon request timed out",
      },
    ]);
    assert.equal(server.socket.closeCount, 1);
    assert.equal(server.socket.readyState, ProtocolFakeWebSocket.CLOSED);
    assert.equal(timers.size, 0);

    server.releaseMessageSend();
    assert.equal(notifications.length, 1);
  } finally {
    timers.restore();
  }
});

test("oversized outgoing request is rejected locally before websocket send", async () => {
  const { client, server } = await connectProtocolClient(new SmallMessageLimitServer());
  const timers = installManualTimers();

  try {
    await assert.rejects(
      () => client.sendMessage("sess_1", "x".repeat(512), "idem_large", [], "ask_only"),
      (error) => {
        assert.ok(error instanceof Error);
        assert.equal(error.name, "SidekickProtocolError");
        assert.equal(error.code, "payload_too_large");
        assert.equal(error.message, "Daemon request is too large for the WebSocket limit.");
        return true;
      },
    );
    assert.equal(timers.size, 0);
  } finally {
    timers.restore();
  }

  assert.equal(server.messageSendCount, 0);
  assert.equal(server.socket.sent.some((request) => request.method === "message/send"), false);
});

test("dispatches connection lost once for unexpected socket close or error", async () => {
  const first = await connectProtocolClient(new AcceptingServer());
  const firstNotifications = [];
  first.client.onNotification((notification) => firstNotifications.push(notification));

  first.server.socket.close();
  first.server.socket.emit("error", {});

  assert.deepEqual(firstNotifications, [
    {
      kind: "connection_lost",
      message: "Daemon WebSocket closed",
    },
  ]);

  const second = await connectProtocolClient(new AcceptingServer());
  const secondNotifications = [];
  second.client.onNotification((notification) => secondNotifications.push(notification));

  second.server.socket.emit("error", {});
  second.server.socket.close();

  assert.deepEqual(secondNotifications, [
    {
      kind: "connection_lost",
      message: "Daemon WebSocket failed",
    },
  ]);
});

test("does not dispatch connection lost for intentional client close", async () => {
  const { client, server } = await connectProtocolClient(new AcceptingServer());
  const notifications = [];
  client.onNotification((notification) => notifications.push(notification));

  client.close();

  assert.equal(server.socket.closeCount, 1);
  assert.deepEqual(notifications, []);
});
