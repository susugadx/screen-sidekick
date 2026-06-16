import assert from "node:assert/strict";

export function assertSameMessageSendRequest(actual, expected, text) {
  assert.equal(actual.idempotency_key, expected.idempotency_key);
  assert.deepEqual(actual.attachment_ids, expected.attachment_ids);
  assert.equal(actual.text, text);
}

export function assertDifferentMessageSendRequest(actual, expected) {
  assert.notEqual(actual.idempotency_key, expected.idempotency_key);
  assert.notDeepEqual(actual.attachment_ids, expected.attachment_ids);
}

export class FakeSidekickServer {
  constructor({
    attachmentSafetyStatus = "clean",
    closeBeforePersistSendNumbers = new Set(),
    closeBeforeSendResponseNumbers = new Set(),
    codexReadiness = {
      available: true,
      version: "codex-fake",
    },
    deferCloseEvents = false,
    deferSessionGetNumbers = new Set(),
    deferSendResponseNumbers = new Set(),
    deferMessageCreatedNumbers = new Set(),
    failAfterPersistSendNumbers = new Set(),
    failReusedSendNumbers = new Set(),
    failSendNumbers = new Set(),
    failSessionCreateNumbers = new Set(),
    failSessionGetNumbers = new Set(),
    malformedSendResponseNumbers = new Set(),
    sessions = {},
    closeAfterTerminalReusedSendNumbers = new Set(),
    terminalReusedSendNumbers = new Set(),
    terminalReusedSendErrorData = {
      message_send_idempotency_disposition: "discard",
    },
    failMissingSessionSubscribe = false,
  } = {}) {
    this.attachmentSafetyStatus = attachmentSafetyStatus;
    this.closeBeforePersistSendNumbers = closeBeforePersistSendNumbers;
    this.closeBeforeSendResponseNumbers = closeBeforeSendResponseNumbers;
    this.codexReadiness = codexReadiness;
    this.deferCloseEvents = deferCloseEvents;
    this.deferSessionGetNumbers = deferSessionGetNumbers;
    this.deferSendResponseNumbers = deferSendResponseNumbers;
    this.deferMessageCreatedNumbers = deferMessageCreatedNumbers;
    this.failAfterPersistSendNumbers = failAfterPersistSendNumbers;
    this.failReusedSendNumbers = failReusedSendNumbers;
    this.failSendNumbers = failSendNumbers;
    this.failSessionCreateNumbers = failSessionCreateNumbers;
    this.failSessionGetNumbers = failSessionGetNumbers;
    this.malformedSendResponseNumbers = malformedSendResponseNumbers;
    this.sessions = new Map(Object.entries(sessions));
    this.closeAfterTerminalReusedSendNumbers = closeAfterTerminalReusedSendNumbers;
    this.terminalReusedSendNumbers = terminalReusedSendNumbers;
    this.terminalReusedSendErrorData = terminalReusedSendErrorData;
    this.failMissingSessionSubscribe = failMissingSessionSubscribe;
    this.deferredCloseEvents = [];
    this.deferredSessionGetResponses = [];
    this.deferredSendResponses = [];
    this.idempotentMessageSends = new Map();
    this.messageSendRequests = [];
    this.reusedSendCount = 0;
    this.sendCount = 0;
    this.attachCount = 0;
    this.sessionCreateCount = 0;
    this.sessionGetCount = 0;
    this.attachSessionIds = [];
    this.sendSessionIds = [];
    this.subscribeSessionIds = [];
    this.socket = null;
    this.sockets = [];
  }

  handle(socket, request) {
    switch (request.method) {
      case "initialize":
        socket.receiveSuccess(request.id, {
          codex_readiness: this.codexReadiness,
          limits: readyProtocolLimits(),
        });
        return;
      case "session/subscribe":
        this.subscribeSessionIds.push(request.params.session_id);
        if (this.failMissingSessionSubscribe && !this.sessions.has(request.params.session_id)) {
          socket.receiveFailure(request.id, "session_not_found", "Session was not found.");
          return;
        }
        socket.receiveSuccess(request.id, {});
        return;
      case "session/create":
        this.sessionCreateCount += 1;
        if (this.failSessionCreateNumbers.has(this.sessionCreateCount)) {
          socket.receiveFailure(request.id, "internal_error", "Session create failed.");
          return;
        }
        {
          const session = {
            id: `sess_${this.sessionCreateCount}`,
            title: "Screen Sidekick",
          };
          this.sessions.set(session.id, {
            session,
            messages: [],
            attachments: [],
            active_turn: null,
          });
          socket.receiveSuccess(request.id, {
            session,
          });
        }
        return;
      case "session/get":
        this.sessionGetCount += 1;
        {
          const sessionGetNumber = this.sessionGetCount;
          const respond = () => {
            if (this.failSessionGetNumbers.has(sessionGetNumber)) {
              socket.receiveFailure(request.id, "internal_error", "Session recovery failed.");
              return;
            }
            const snapshot = this.sessions.get(request.params.session_id);
            if (!snapshot) {
              socket.receiveFailure(request.id, "session_not_found", "Session was not found.");
              return;
            }
            socket.receiveSuccess(request.id, {
              session: snapshot.session,
              messages: snapshot.messages,
              attachments: snapshot.attachments,
              active_turn: snapshot.active_turn,
            });
          };
          if (this.deferSessionGetNumbers.has(sessionGetNumber)) {
            this.deferredSessionGetResponses.push(respond);
            return;
          }
          respond();
        }
        return;
      case "context/attach_browser":
        this.attachCount += 1;
        this.attachSessionIds.push(request.params.session_id);
        socket.receiveSuccess(request.id, {
          attachment: {
            id: `att_${this.attachCount}`,
            session_id: request.params.session_id,
            summary: "Admin",
            safety_status: this.attachmentSafetyStatus,
            debug_available: false,
          },
        });
        return;
      case "message/send":
        this.sendCount += 1;
        this.sendSessionIds.push(request.params.session_id);
        this.messageSendRequests.push(request.params);
        {
          const existingSend = this.idempotentMessageSends.get(
            request.params.idempotency_key,
          );
          if (existingSend) {
            this.reusedSendCount += 1;
            if (this.failReusedSendNumbers.has(this.reusedSendCount)) {
              socket.receiveFailure(request.id, "internal_error", "Retry setup failed.");
              return;
            }
            if (this.terminalReusedSendNumbers.has(this.reusedSendCount)) {
              socket.receiveFailure(
                request.id,
                "codex_not_found",
                "Previous message/send attempt failed.",
                this.terminalReusedSendErrorData,
              );
              if (this.closeAfterTerminalReusedSendNumbers.has(this.reusedSendCount)) {
                socket.close();
              }
              return;
            }
            socket.receiveSuccess(request.id, {
              message_id: existingSend.messageId,
              turn_id: existingSend.turnId,
              reused: true,
            });
            return;
          }
        }
        if (this.failSendNumbers.has(this.sendCount)) {
          socket.receiveFailure(request.id, "turn_already_running", "A turn is already running.");
          return;
        }
        if (this.closeBeforePersistSendNumbers.has(this.sendCount)) {
          socket.close();
          return;
        }
        {
          const turnId = `turn_${this.sendCount}`;
          const messageId = `msg_${this.sendCount}`;
          const snapshot = this.ensureSession(request.params.session_id);
          const message = {
            id: messageId,
            session_id: request.params.session_id,
            role: "user",
            text: request.params.text,
            status: "pending",
            turn_id: turnId,
          };
          snapshot.messages.push(message);
          snapshot.active_turn = {
            id: turnId,
            session_id: request.params.session_id,
            status: "running",
          };
          const notifyMessageCreated = () =>
            socket.receiveNotification("message/created", {
              session_id: request.params.session_id,
              message,
            });
          if (!this.deferMessageCreatedNumbers.has(this.sendCount)) {
            notifyMessageCreated();
          }
          if (this.failAfterPersistSendNumbers.has(this.sendCount)) {
            snapshot.active_turn = null;
            socket.receiveFailure(request.id, "codex_not_found", "Codex start failed.");
            return;
          }
          this.idempotentMessageSends.set(request.params.idempotency_key, {
            messageId,
            turnId,
          });
          if (this.closeBeforeSendResponseNumbers.has(this.sendCount)) {
            socket.close();
            return;
          }
          if (this.malformedSendResponseNumbers.has(this.sendCount)) {
            socket.receive({
              jsonrpc: "2.0",
              id: request.id,
              error: {
                code: "invalid_request",
              },
            });
            return;
          }
          const sendResponse = () => {
            socket.receiveSuccess(request.id, {
              message_id: messageId,
              turn_id: turnId,
              reused: false,
            });
            if (this.deferMessageCreatedNumbers.has(this.sendCount)) {
              setTimeout(notifyMessageCreated, 0);
            }
          };
          if (this.deferSendResponseNumbers.has(this.sendCount)) {
            this.deferredSendResponses.push(sendResponse);
            return;
          }
          sendResponse();
        }
        return;
      default:
        socket.receiveFailure(request.id, "method_not_found", "Method was not found.");
    }
  }

  ensureSession(sessionId) {
    const existing = this.sessions.get(sessionId);
    if (existing) {
      return existing;
    }
    const snapshot = {
      session: {
        id: sessionId,
        title: "Screen Sidekick",
      },
      messages: [],
      attachments: [],
      active_turn: null,
    };
    this.sessions.set(sessionId, snapshot);
    return snapshot;
  }

  finishLatestTurn(sessionId, status) {
    const snapshot = this.sessions.get(sessionId);
    assert.ok(snapshot, `missing session ${sessionId}`);
    const message = snapshot.messages.at(-1);
    assert.ok(message, `missing latest message for ${sessionId}`);
    message.status = status;
    snapshot.active_turn = null;
  }

  releaseDeferredSessionGetResponses() {
    const responses = this.deferredSessionGetResponses.splice(0);
    for (const respond of responses) {
      respond();
    }
  }

  releaseDeferredSendResponses() {
    const responses = this.deferredSendResponses.splice(0);
    for (const respond of responses) {
      respond();
    }
  }

  releaseDeferredCloseEvents() {
    const events = this.deferredCloseEvents.splice(0);
    for (const emitClose of events) {
      emitClose();
    }
  }
}

export class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 3;

  constructor(url, server) {
    this.url = String(url);
    this.server = server;
    this.readyState = FakeWebSocket.CONNECTING;
    this.listeners = new Map();
    this.sent = [];
    server.socket = this;
    server.sockets.push(this);
    queueMicrotask(() => {
      this.readyState = FakeWebSocket.OPEN;
      this.emit("open", {});
    });
  }

  addEventListener(type, listener, options = {}) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push({ listener, once: options.once === true });
    this.listeners.set(type, listeners);
  }

  close() {
    this.readyState = FakeWebSocket.CLOSED;
    const emitClose = () => {
      this.emit("close", {});
    };
    if (this.server.deferCloseEvents) {
      this.server.deferredCloseEvents.push(emitClose);
      return;
    }
    emitClose();
  }

  send(text) {
    const request = JSON.parse(text);
    this.sent.push(request);
    this.server.handle(this, request);
  }

  receiveSuccess(id, result) {
    this.receive({
      jsonrpc: "2.0",
      id,
      result,
    });
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

  receiveNotification(method, params) {
    this.receive({
      jsonrpc: "2.0",
      method,
      params,
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

function readyProtocolLimits() {
  return {
    max_message_bytes: 262144,
    max_attachment_bytes: 131072,
  };
}
