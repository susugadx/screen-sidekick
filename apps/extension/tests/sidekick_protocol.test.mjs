import assert from "node:assert/strict";
import test from "node:test";

import { flushMicrotasks, installManualTimers } from "./manual_timers.mjs";
import {
  SidekickProtocolClient,
  buildDaemonCaptureUrl,
  buildDaemonWebSocketUrl,
  parseInitializeResult,
  parseSidekickNotification,
  parseWireMessageText,
} from "../dist/sidekick_protocol.js";

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
  const PreviousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = class extends ProtocolFakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = ProtocolFakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = ProtocolFakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = ProtocolFakeWebSocket.CLOSED;

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
    globalThis.WebSocket = PreviousWebSocket;
  }
});

test("connect rejects and closes websocket when codex readiness is unavailable", async () => {
  const server = new UnavailableInitializeServer();
  const PreviousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = class extends ProtocolFakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = ProtocolFakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = ProtocolFakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = ProtocolFakeWebSocket.CLOSED;

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
    globalThis.WebSocket = PreviousWebSocket;
  }
});

test("connect rejects and closes websocket when initialize response shape is invalid", async () => {
  const server = new MalformedInitializeServer();
  const PreviousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = class extends ProtocolFakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = ProtocolFakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = ProtocolFakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = ProtocolFakeWebSocket.CLOSED;

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
    globalThis.WebSocket = PreviousWebSocket;
  }
});

test("connect times out unanswered initialize and closes websocket", async () => {
  const server = new UnansweredInitializeServer();
  const PreviousWebSocket = globalThis.WebSocket;
  const timers = installManualTimers();
  globalThis.WebSocket = class extends ProtocolFakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = ProtocolFakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = ProtocolFakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = ProtocolFakeWebSocket.CLOSED;

  try {
    const connectPromise = SidekickProtocolClient.connect(
      { url: "http://127.0.0.1:43001", token: "pairing-token" },
      "test",
    );
    await flushMicrotasks();
    assert.equal(server.socket.sent[0]?.method, "initialize");

    const rejected = assert.rejects(connectPromise, /Daemon request timed out/);
    timers.fireNext();
    await rejected;

    assert.equal(server.socket.closeCount, 1);
    assert.equal(server.socket.readyState, ProtocolFakeWebSocket.CLOSED);
    assert.equal(timers.size, 0);
  } finally {
    timers.restore();
    globalThis.WebSocket = PreviousWebSocket;
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

test("message/send waits for delayed response without local request timeout", async () => {
  const { client, server } = await connectProtocolClient(new DelayedMessageSendServer());
  const timers = installManualTimers();

  try {
    let settled = false;
    const sendPromise = client
      .sendMessage("sess_1", "Slow question", [], "ask_only")
      .then((result) => {
        settled = true;
        return result;
      });
    await flushMicrotasks();

    assert.equal(server.delayedMessageSendId, "sidekick_extension_2");
    assert.equal(timers.size, 0);
    assert.equal(settled, false);

    server.releaseMessageSend();
    assert.deepEqual(await sendPromise, {
      messageId: "msg_delayed",
      turnId: "turn_delayed",
      reused: false,
    });
    assert.equal(settled, true);
  } finally {
    timers.restore();
  }
});

test("oversized outgoing request is rejected locally before websocket send", async () => {
  const { client, server } = await connectProtocolClient(new SmallMessageLimitServer());

  await assert.rejects(
    () => client.sendMessage("sess_1", "x".repeat(512), [], "ask_only"),
    (error) => {
      assert.ok(error instanceof Error);
      assert.equal(error.name, "SidekickProtocolError");
      assert.equal(error.code, "payload_too_large");
      assert.equal(error.message, "Daemon request is too large for the WebSocket limit.");
      return true;
    },
  );

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

async function connectProtocolClient(server) {
  const PreviousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = class extends ProtocolFakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = ProtocolFakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = ProtocolFakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = ProtocolFakeWebSocket.CLOSED;

  try {
    const client = await SidekickProtocolClient.connect(
      { url: "http://127.0.0.1:43001", token: "pairing-token" },
      "test",
    );
    return { client, server };
  } finally {
    globalThis.WebSocket = PreviousWebSocket;
  }
}

class AcceptingServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receiveSuccess(request.id, readyInitializeResult());
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

class RejectingInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receiveFailure(request.id, "unauthorized", "Pairing token is invalid.");
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

class UnavailableInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receiveSuccess(request.id, {
        codex_readiness: {
          available: false,
          error_code: "unsupported_codex_version",
        },
        limits: readyProtocolLimits(),
      });
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

class UnansweredInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

class MalformedInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receive({
        jsonrpc: "2.0",
        id: request.id,
        error: {
          code: "invalid_request",
        },
      });
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

class HangingSessionGetServer {
  socket = null;
  hangingSessionGetId = null;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, readyInitializeResult());
        return;
      case "session/get":
        this.hangingSessionGetId = request.id;
        return;
      case "session/create":
        socket.receiveSuccess(request.id, {
          session: {
            id: "sess_after_timeout",
            title: request.params.title,
          },
        });
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }
}

class DelayedMessageSendServer {
  socket = null;
  delayedMessageSendId = null;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, readyInitializeResult());
        return;
      case "message/send":
        this.delayedMessageSendId = request.id;
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }

  releaseMessageSend() {
    this.socket.receiveSuccess(this.delayedMessageSendId, {
      message_id: "msg_delayed",
      turn_id: "turn_delayed",
      reused: false,
    });
  }
}

class SmallMessageLimitServer {
  socket = null;
  messageSendCount = 0;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, {
          ...readyInitializeResult(),
          limits: {
            max_message_bytes: 160,
            max_attachment_bytes: 131072,
          },
        });
        return;
      case "message/send":
        this.messageSendCount += 1;
        socket.receiveFailure(request.id, "internal_error", "Should not be sent.");
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }
}

class MalformedSessionGetServer {
  socket = null;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, readyInitializeResult());
        return;
      case "session/get":
        socket.receive({
          jsonrpc: "2.0",
          id: request.id,
          error: {
            code: "invalid_request",
          },
        });
        return;
      case "session/create":
        socket.receiveSuccess(request.id, {
          session: {
            id: "sess_after_malformed",
            title: request.params.title,
          },
        });
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }
}

function readyInitializeResult() {
  return {
    codex_readiness: {
      available: true,
      version: "codex-fake",
    },
    limits: readyProtocolLimits(),
  };
}

function readyProtocolLimits() {
  return {
    max_message_bytes: 262144,
    max_attachment_bytes: 131072,
  };
}

class ProtocolFakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 3;

  constructor(url, server) {
    this.url = String(url);
    this.server = server;
    this.readyState = ProtocolFakeWebSocket.CONNECTING;
    this.listeners = new Map();
    this.sent = [];
    this.closeCount = 0;
    server.socket = this;
    queueMicrotask(() => {
      this.readyState = ProtocolFakeWebSocket.OPEN;
      this.emit("open", {});
    });
  }

  addEventListener(type, listener, options = {}) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push({ listener, once: options.once === true });
    this.listeners.set(type, listeners);
  }

  close() {
    this.closeCount += 1;
    this.readyState = ProtocolFakeWebSocket.CLOSED;
    this.emit("close", {});
  }

  send(text) {
    const request = JSON.parse(text);
    this.sent.push(request);
    this.server.handle(this, request);
  }

  receiveFailure(id, code, message) {
    this.receive({
      jsonrpc: "2.0",
      id,
      error: {
        code,
        message,
      },
    });
  }

  receiveSuccess(id, result) {
    this.receive({
      jsonrpc: "2.0",
      id,
      result,
    });
  }

  receive(value) {
    this.emit("message", {
      data: JSON.stringify(value),
    });
  }

  emit(type, event) {
    const listeners = this.listeners.get(type) ?? [];
    this.listeners.set(
      type,
      listeners.filter(({ listener, once }) => {
        listener(event);
        return !once;
      }),
    );
  }
}
