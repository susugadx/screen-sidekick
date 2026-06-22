import { SidekickProtocolClient } from "../../dist/sidekick_protocol.js";

export async function connectProtocolClient(server) {
  const restoreWebSocket = installProtocolWebSocket(server);

  try {
    const client = await SidekickProtocolClient.connect(
      { url: "http://127.0.0.1:43001", token: "pairing-token" },
      "test",
    );
    return { client, server };
  } finally {
    restoreWebSocket();
  }
}

export function installProtocolWebSocket(server) {
  const PreviousWebSocket = globalThis.WebSocket;
  globalThis.WebSocket = class extends ProtocolFakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = ProtocolFakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = ProtocolFakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = ProtocolFakeWebSocket.CLOSED;

  return () => {
    globalThis.WebSocket = PreviousWebSocket;
  };
}

export function installProtocolNativeMessaging(server, options = {}) {
  const previousChrome = globalThis.chrome;
  const runtime = {
    id: "abcdefghijklmnopabcdefghijklmnop",
    lastError: undefined,
    connectNative(name) {
      if (options.connectThrows) {
        throw new Error("native host missing");
      }
      if (name !== "com.screen_sidekick.host") {
        throw new Error(`unexpected native host name: ${name}`);
      }
      return new ProtocolFakeNativePort(server, runtime, options);
    },
  };
  globalThis.chrome = { runtime };

  return () => {
    globalThis.chrome = previousChrome;
  };
}

export class AcceptingServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receiveSuccess(request.id, readyInitializeResult());
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

export class RejectingInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receiveFailure(request.id, "unauthorized", "Pairing token is invalid.");
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

export class SetupRequiredInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      socket.receiveFailure(
        request.id,
        "setup_required",
        "Screen Sidekick Windows native host setup is required.",
        { retryable: false },
      );
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

export class UnavailableInitializeServer {
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

export class UnansweredInitializeServer {
  socket = null;

  handle(socket, request) {
    if (request.method === "initialize") {
      return;
    }
    socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
  }
}

export class MalformedInitializeServer {
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

export class HangingSessionGetServer {
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

export class HangingSessionCreateServer {
  socket = null;
  hangingSessionCreateId = null;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, readyInitializeResult());
        return;
      case "session/create":
        this.hangingSessionCreateId = request.id;
        return;
      case "session/get":
        socket.receiveSuccess(request.id, {
          session: {
            id: request.params.session_id,
            title: "Recovered session",
          },
          messages: [],
          attachments: [],
          active_turn: null,
        });
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }
}

export class HangingAttachBrowserContextServer {
  socket = null;
  hangingAttachBrowserContextId = null;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, readyInitializeResult());
        return;
      case "context/attach_browser":
        this.hangingAttachBrowserContextId = request.id;
        return;
      case "session/create":
        socket.receiveSuccess(request.id, {
          session: {
            id: "sess_after_attach_timeout",
            title: request.params.title,
          },
        });
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }
}

export class DelayedMessageSendServer {
  socket = null;
  delayedMessageSendId = null;
  delayedMessageSendParams = null;

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, readyInitializeResult());
        return;
      case "message/send":
        this.delayedMessageSendId = request.id;
        this.delayedMessageSendParams = request.params;
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

export class SmallMessageLimitServer {
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

export class MalformedSessionGetServer {
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

export function readyInitializeResult() {
  return {
    codex_readiness: {
      available: true,
      version: "codex-fake",
    },
    limits: readyProtocolLimits(),
  };
}

export function readyProtocolLimits() {
  return {
    max_message_bytes: 262144,
    max_attachment_bytes: 131072,
  };
}

export class ProtocolFakeWebSocket {
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

  receiveFailure(id, code, message, data = undefined) {
    this.receive({
      jsonrpc: "2.0",
      id,
      error: {
        code,
        message,
        ...(data === undefined ? {} : { data }),
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

export class ProtocolFakeNativePort {
  constructor(server, runtime, options = {}) {
    this.server = server;
    this.runtime = runtime;
    this.disconnectMessage = options.disconnectMessage ?? "";
    this.sent = [];
    this.disconnected = false;
    this.onMessage = new FakeChromeEvent();
    this.onDisconnect = new FakeChromeEvent();
    server.socket = this;
  }

  postMessage(request) {
    this.sent.push(request);
    this.server.handle(this, request);
  }

  disconnect() {
    if (this.disconnected) {
      return;
    }
    this.disconnected = true;
    if (this.disconnectMessage) {
      this.runtime.lastError = { message: this.disconnectMessage };
    }
    this.onDisconnect.dispatch();
    this.runtime.lastError = undefined;
  }

  close() {
    this.disconnect();
  }

  receiveFailure(id, code, message, data = undefined) {
    this.receive({
      jsonrpc: "2.0",
      id,
      error: {
        code,
        message,
        ...(data === undefined ? {} : { data }),
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
    this.onMessage.dispatch(value);
  }
}

class FakeChromeEvent {
  constructor() {
    this.listeners = [];
  }

  addListener(listener) {
    this.listeners.push(listener);
  }

  dispatch(value) {
    for (const listener of this.listeners) {
      listener(value);
    }
  }
}
