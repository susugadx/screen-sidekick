import assert from "node:assert/strict";
import test from "node:test";

import {
  SidekickProtocolClient,
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
  });

  assert.deepEqual(result, {
    codexReadiness: {
      available: false,
      version: "codex-fake 1.0.0",
      errorCode: "unsupported_codex_version",
    },
  });
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
      });
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

function readyInitializeResult() {
  return {
    codex_readiness: {
      available: true,
      version: "codex-fake",
    },
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
