import assert from "node:assert/strict";
import test from "node:test";

import { JSDOM } from "jsdom";
import { installManualTimers, waitForMicrotasks } from "./manual_timers.mjs";

let moduleCounter = 0;

test("keeps ask controls disabled after message send response until turn completes", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("What should I do next?");
  await waitFor(() => server.sendCount === 1);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);

  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
});

test("renders the user message before the assistant placeholder when send response wins the race", async () => {
  const server = installSidePanelHarness({
    deferMessageCreatedNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("Ordering question");
  await waitFor(() => messageRows().length === 2);
  await nextTick();

  const rows = messageRows();
  assert.equal(rows.length, 2);
  assert.equal(rows[0]?.className, "message user");
  assert.equal(rows[1]?.className, "message assistant");
  assert.equal(
    rows.filter((row) => (row.textContent ?? "").includes("Ordering question")).length,
    1,
  );
});

test("turn already running rejection keeps the active turn and avoids unsent transcript rows", async () => {
  const server = installSidePanelHarness({ failSendNumbers: new Set([2]) });
  await importFreshSidePanel();

  submitMessage("First question");
  await waitFor(() => server.sendCount === 1);
  assert.equal(transcriptText().includes("First question"), true);
  assert.equal(messageRows().length, 2);

  submitMessage("Second question");
  await waitFor(() => server.sendCount === 2);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(element("message-input").value, "Second question");
  assert.equal(transcriptText().includes("Second question"), false);
  assert.equal(messageRows().length, 2);
});

test("failed send renders persisted user message without clearing draft from text-only match", async () => {
  const server = installSidePanelHarness({
    failAfterPersistSendNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("Question before Codex fails");
  await waitFor(() => server.sendCount === 1);
  await waitFor(() => element("ask").disabled === false);

  assert.equal(transcriptText().includes("Question before Codex fails"), true);
  assert.equal(messageRows().length, 1);
  assert.equal(element("message-input").value, "Question before Codex fails");
  assert.equal(element("status").textContent, "Codex start failed.");
});

test("codex unavailable initialize stops ask before capture or message send", async () => {
  const server = installSidePanelHarness({
    codexReadiness: {
      available: false,
      error_code: "unsupported_codex_version",
    },
  });
  await importFreshSidePanel();

  submitMessage("Will this reach Codex?");
  await waitFor(
    () => element("status").textContent === "Codex app-server version is unsupported.",
  );

  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(server.sessionCreateCount, 0);
  assert.equal(server.attachCount, 0);
  assert.equal(server.sendCount, 0);
  assert.equal(transcriptText(), "");
});

test("debug capture normalizes websocket daemon URL to http capture endpoint", async () => {
  const previousFetch = globalThis.fetch;
  let capturedFetch = null;
  installSidePanelHarness({
    storage: {
      daemonSettings: {
        url: "ws://127.0.0.1:43001?token=SECRET#debug",
        token: "pairing-token",
      },
    },
  });
  globalThis.fetch = async (url, options) => {
    capturedFetch = {
      url: String(url),
      method: options.method,
      authorization: options.headers.Authorization,
    };
    return {
      ok: true,
      status: 200,
      async text() {
        return JSON.stringify(captureBridgeResponse());
      },
    };
  };

  try {
    await importFreshSidePanel("ws://127.0.0.1:43001?token=SECRET#debug");

    element("debug-capture").click();
    await waitFor(() => element("status").textContent === "Ready");

    assert.deepEqual(capturedFetch, {
      url: "http://127.0.0.1:43001/v0/capture",
      method: "POST",
      authorization: "Bearer pairing-token",
    });
    assert.equal(element("screen-context-json").value, "{}");
    assert.equal(element("prompt-text").value, "Prompt");
  } finally {
    globalThis.fetch = previousFetch;
  }
});

test("turn failed notification clears active turn controls", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Will this fail?");
  await waitFor(() => server.sendCount === 1);
  server.socket.receiveNotification("turn/failed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "failed",
    },
  });
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "Codex turn failed");
  assert.equal(messageRows().length, 1);
  assert.equal(transcriptText().includes("Will this fail?"), true);
});

test("turn failed notification displays concrete daemon message", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Will this fail?");
  await waitFor(() => server.sendCount === 1);
  server.socket.receiveNotification("turn/failed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "failed",
    },
    message: "model failed",
  });
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "model failed");
});

test("turn cancelled notification clears active turn controls", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Cancel this");
  await waitFor(() => server.sendCount === 1);
  server.socket.receiveNotification("turn/cancelled", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "cancelled",
    },
  });
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "Cancelled");
  assert.equal(messageRows().length, 1);
  assert.equal(transcriptText().includes("Cancel this"), true);
});

test("unexpected daemon websocket close recovers the in-flight session before the next ask", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("First question");
  await waitFor(() => server.sendCount === 1);

  const firstSocket = server.socket;
  firstSocket.close();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(element("status").textContent, "Asking");
  assert.equal(transcriptText().includes("First question"), true);

  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);
  submitMessage("Second question");
  await waitFor(() => server.sendCount === 2);

  assert.notEqual(server.socket, firstSocket);
  assert.equal(server.sockets.length, 2);
  assert.equal(server.sessionCreateCount, 1);
  assert.deepEqual(server.subscribeSessionIds, ["sess_1", "sess_1"]);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_1"]);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_1"]);
});

test("websocket close before message send response recovers session without a known turn id", async () => {
  const server = installSidePanelHarness({
    closeBeforeSendResponseNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("First question");
  await waitFor(() => server.sendCount === 1);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(element("status").textContent, "Asking");
  assert.equal(transcriptText().includes("First question"), true);
  assert.equal(element("message-input").value, "");
  assert.equal(server.sessionCreateCount, 1);
  assert.deepEqual(server.subscribeSessionIds, ["sess_1", "sess_1"]);

  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);
  submitMessage("Second question");
  await waitFor(() => server.sendCount === 2);

  assert.equal(server.sessionCreateCount, 1);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_1"]);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_1"]);
});

test("websocket close before message persistence keeps draft when restored session has same text", async () => {
  const repeatedQuestion = "Repeat this question";
  const server = installSidePanelHarness({
    closeBeforePersistSendNumbers: new Set([1]),
    storage: activeChatStorage(scopedActiveChatMarkerWithoutTurn()),
    sessions: completedActiveChatSessions(repeatedQuestion),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  submitMessage(repeatedQuestion);
  await waitFor(() => server.sendCount === 1);
  await waitFor(() => server.sessionGetCount === 2);

  assert.equal(element("status").textContent, "Ready");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(element("message-input").value, repeatedQuestion);
  assert.equal(messageRows().length, 1);
  assert.equal(transcriptText().includes(repeatedQuestion), true);
  assert.equal(server.sessionCreateCount, 0);
  assert.deepEqual(server.subscribeSessionIds, ["sess_1", "sess_1"]);
});

test("recovery failure after message send response loss re-enables ask controls", async () => {
  const server = installSidePanelHarness({
    closeBeforeSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("First question");
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "Session recovery failed.");

  submitMessage("Retry question");
  await waitFor(() => server.sendCount === 2);

  assert.equal(server.sessionCreateCount, 1);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_1"]);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_1"]);
});

test("side panel reload restores a persisted in-flight session", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(scopedActiveChatMarker()),
    sessions: runningActiveChatSessions("Persisted question"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(server.sessionCreateCount, 0);
  assert.deepEqual(server.subscribeSessionIds, ["sess_1"]);
  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(transcriptText().includes("Persisted question"), true);

  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);
});

test("stored active chat recovery failure re-enables ask controls", async () => {
  const server = installSidePanelHarness({
    failSessionGetNumbers: new Set([1]),
    storage: activeChatStorage(scopedActiveChatMarker()),
    sessions: runningActiveChatSessions(),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "Session recovery failed.");
});

test("recovery keeps save enabled and ignores delayed stale snapshots after settings change", async () => {
  const server = installSidePanelHarness({
    deferCloseEvents: true,
    deferSessionGetNumbers: new Set([1]),
    storage: activeChatStorage(scopedActiveChatMarker()),
    sessions: runningActiveChatSessions("Old recovery question"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(element("save-bridge").disabled, false);

  element("bridge-url").value = "http://127.0.0.1:43002";
  element("bridge-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
  await waitFor(() => element("status").textContent === "Saved");

  assert.equal(transcriptText(), "");

  server.releaseDeferredSessionGetResponses();
  await nextTick();

  assert.equal(element("status").textContent, "Saved");
  assert.equal(transcriptText(), "");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(element("save-bridge").disabled, false);

  server.releaseDeferredCloseEvents();
  await nextTick();

  assert.equal(element("status").textContent, "Saved");
});

test("hung recovery keeps save available to clear active chat state", async () => {
  const server = installSidePanelHarness({
    deferSessionGetNumbers: new Set([1]),
    storage: activeChatStorage(scopedActiveChatMarker()),
    sessions: runningActiveChatSessions("Hung recovery question"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(element("save-bridge").disabled, false);
  assert.equal(transcriptText().includes("Hung recovery question"), false);

  element("bridge-url").value = "http://127.0.0.1:43002";
  element("bridge-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
  await waitFor(() => element("status").textContent === "Saved");

  assert.equal(transcriptText(), "");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(element("save-bridge").disabled, false);
});

test("stored active chat with a different token is not recovered", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(scopedActiveChatMarker({ daemonToken: "old-token" })),
    sessions: runningActiveChatSessions("Stale token question"),
  });
  await importFreshSidePanel();
  await nextTick();

  assert.equal(server.sessionGetCount, 0);
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText(), "");
});

test("stored active chat with a different tab is not recovered", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(scopedActiveChatMarker({ tabId: 8 })),
    sessions: runningActiveChatSessions("Different tab question"),
  });
  await importFreshSidePanel();
  await nextTick();

  assert.equal(server.sessionGetCount, 0);
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText(), "");
});

test("stored active chat with a different origin is not recovered", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(scopedActiveChatMarker({ origin: "https://other.example" })),
    sessions: runningActiveChatSessions("Different origin question"),
  });
  await importFreshSidePanel();
  await nextTick();

  assert.equal(server.sessionGetCount, 0);
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText(), "");
});

test("legacy stored active chat without tab scope is not recovered", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(legacyActiveChatMarker()),
    sessions: runningActiveChatSessions("Legacy marker question"),
  });
  await importFreshSidePanel();
  await nextTick();

  assert.equal(server.sessionGetCount, 0);
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText(), "");
});

test("new active chat marker stores current tab and origin scope", async () => {
  installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Scoped marker question");
  const marker = await waitForStoredActiveChat(
    (value) => value?.activeTurnId === "turn_1",
  );

  assert.deepEqual(marker, {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
    activeTurnId: "turn_1",
  });
});

test("session not found recovery clears stale transcript", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Question from missing session");
  await waitFor(() => server.sendCount === 1);
  assert.equal(transcriptText().includes("Question from missing session"), true);

  server.sessions.delete("sess_1");
  server.socket.close();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(transcriptText(), "");
  assert.equal(element("status").textContent, "Daemon session was not found");
});

test("reconnects instead of reusing a closed daemon websocket", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("First question");
  await waitFor(() => server.sendCount === 1);
  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);

  const firstSocket = server.socket;
  firstSocket.close();
  submitMessage("Second question");
  await waitFor(() => server.sendCount === 2);

  assert.notEqual(server.socket, firstSocket);
  assert.equal(server.sockets.length, 2);
  assert.equal(transcriptText().includes("Second question"), true);
});

test("saving a different daemon URL clears stale transcript before next ask", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Old daemon question");
  await waitFor(() => server.sendCount === 1);
  assert.equal(transcriptText().includes("Old daemon question"), true);

  element("bridge-url").value = "http://127.0.0.1:43002";
  element("bridge-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
  await waitFor(() => element("status").textContent === "Saved");

  assert.equal(transcriptText(), "");

  submitMessage("Fresh daemon question");
  await waitFor(() => server.sendCount === 2);

  assert.equal(transcriptText().includes("Old daemon question"), false);
  assert.equal(transcriptText().includes("Fresh daemon question"), true);
  assert.equal(server.sessionCreateCount, 2);
});

test("saving different daemon settings disconnects old socket before stale errors update UI", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Old daemon question");
  await waitFor(() => server.sendCount === 1);
  const firstSocket = server.socket;

  element("bridge-url").value = "http://127.0.0.1:43002";
  element("bridge-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
  await waitFor(() => element("status").textContent === "Saved");

  firstSocket.emit("error", {});
  firstSocket.receiveNotification("error", {
    code: "internal_error",
    message: "Old daemon error",
  });
  await nextTick();

  assert.equal(firstSocket.readyState, FakeWebSocket.CLOSED);
  assert.equal(element("status").textContent, "Saved");
  assert.equal(transcriptText(), "");
});

test("malformed send response rejects ask and re-enables controls", async () => {
  const server = installSidePanelHarness({
    malformedSendResponseNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("Malformed response question");
  await waitFor(() => element("status").textContent === "Daemon message shape is invalid");

  assert.equal(server.sendCount, 1);
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(element("message-input").value, "Malformed response question");
});

test("delayed message send response keeps ask disabled until turn completes", async () => {
  const server = installSidePanelHarness({
    deferSendResponseNumbers: new Set([1]),
  });
  await importFreshSidePanel();
  const timers = installManualTimers();

  try {
    submitMessage("Slow question");
    await waitForMicrotasks(() => server.sendCount === 1);

    assert.equal(element("ask").disabled, true);
    assert.equal(element("message-input").disabled, true);
    assert.equal(element("status").textContent, "Capturing");
    assert.equal(transcriptText().includes("Slow question"), true);
    assert.equal(messageRows().length, 1);
    assert.equal(timers.size, 0);
  } finally {
    timers.restore();
  }

  server.releaseDeferredSendResponses();
  await waitFor(() => element("status").textContent === "Asking");
  const activeChat = await waitForStoredActiveChat(
    (storedActiveChat) => storedActiveChat?.activeTurnId === "turn_1",
  );

  assert.equal(activeChat.activeTurnId, "turn_1");
  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(messageRows().length, 2);

  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "Ready");
});

test("stored active chat session get timeout releases recovery controls", async () => {
  const server = installSidePanelHarness({
    deferSessionGetNumbers: new Set([1]),
    storage: activeChatStorage(scopedActiveChatMarker()),
    sessions: runningActiveChatSessions("Persisted question"),
  });
  const timers = installManualTimers();

  try {
    await importFreshSidePanelWithMicrotasks();
    await waitForMicrotasks(() => server.sessionGetCount === 1);

    assert.equal(element("ask").disabled, true);
    assert.equal(element("message-input").disabled, true);
    assert.equal(element("status").textContent, "Reconnecting to daemon");
    assert.equal(timers.size, 1);

    timers.fireNext();
    await waitForMicrotasks(() => element("ask").disabled === false);

    assert.equal(element("message-input").disabled, false);
    assert.equal(element("status").textContent, "Daemon request timed out");

    server.releaseDeferredSessionGetResponses();
    await waitForMicrotasks(() => element("ask").disabled === false);
    assert.equal(element("status").textContent, "Daemon request timed out");
  } finally {
    timers.restore();
  }
});

test("saving a different pairing token clears stale session state before next ask", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Old token question");
  await waitFor(() => server.sendCount === 1);
  assert.equal(transcriptText().includes("Old token question"), true);
  server.socket.receiveNotification("turn/completed", {
    session_id: "sess_1",
    turn: {
      id: "turn_1",
      session_id: "sess_1",
      status: "completed",
    },
  });
  await waitFor(() => element("ask").disabled === false);

  element("bridge-token").value = "new-pairing-token";
  element("bridge-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
  await waitFor(() => element("status").textContent === "Saved");

  assert.equal(transcriptText(), "");

  submitMessage("Fresh token question");
  await waitFor(() => server.sendCount === 2);

  assert.equal(transcriptText().includes("Old token question"), false);
  assert.equal(transcriptText().includes("Fresh token question"), true);
  assert.equal(server.sessionCreateCount, 2);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_2"]);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_2"]);
});

async function importFreshSidePanel(expectedDaemonUrl = "http://127.0.0.1:43001") {
  await import(`../dist/side_panel.js?test=${++moduleCounter}`);
  await waitFor(() => element("bridge-url").value === expectedDaemonUrl);
}

async function importFreshSidePanelWithMicrotasks(
  expectedDaemonUrl = "http://127.0.0.1:43001",
) {
  await import(`../dist/side_panel.js?test=${++moduleCounter}`);
  await waitForMicrotasks(() => element("bridge-url").value === expectedDaemonUrl);
}

function installSidePanelHarness(options = {}) {
  installDom();
  installChrome(options.storage);
  const server = new FakeSidekickServer(options);

  globalThis.WebSocket = class extends FakeWebSocket {
    constructor(url) {
      super(url, server);
    }
  };
  globalThis.WebSocket.CONNECTING = FakeWebSocket.CONNECTING;
  globalThis.WebSocket.OPEN = FakeWebSocket.OPEN;
  globalThis.WebSocket.CLOSED = FakeWebSocket.CLOSED;

  return server;
}

function installDom() {
  const dom = new JSDOM(`
    <main>
      <form id="bridge-form">
        <input id="bridge-url" />
        <input id="bridge-token" />
        <button id="save-bridge" type="submit"></button>
      </form>
      <form id="message-form">
        <textarea id="message-input"></textarea>
        <button id="ask" type="submit"></button>
      </form>
      <div id="transcript"></div>
      <button id="debug-capture" type="button"></button>
      <button id="copy-json" type="button"></button>
      <button id="copy-prompt" type="button"></button>
      <textarea id="screen-context-json"></textarea>
      <textarea id="prompt-text"></textarea>
      <pre id="safety-summary"></pre>
      <span id="status"></span>
    </main>
  `);
  const { window } = dom;

  globalThis.window = window;
  globalThis.document = window.document;
  globalThis.Event = window.Event;
  globalThis.HTMLButtonElement = window.HTMLButtonElement;
  globalThis.HTMLDivElement = window.HTMLDivElement;
  globalThis.HTMLFormElement = window.HTMLFormElement;
  globalThis.HTMLInputElement = window.HTMLInputElement;
  globalThis.HTMLPreElement = window.HTMLPreElement;
  globalThis.HTMLSpanElement = window.HTMLSpanElement;
  globalThis.HTMLTextAreaElement = window.HTMLTextAreaElement;
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      clipboard: {
        writeText: async () => {},
      },
    },
  });
}

function installChrome(initialStorage = {}) {
  const storage = {
    daemonSettings: {
      url: "http://127.0.0.1:43001",
      token: "pairing-token",
    },
    ...initialStorage,
  };
  globalThis.chrome = {
    runtime: {
      getManifest() {
        return { version: "test" };
      },
    },
    storage: {
      session: {
        async get(keys) {
          if (!Array.isArray(keys)) {
            return { ...storage };
          }
          const result = {};
          for (const key of keys) {
            if (Object.hasOwn(storage, key)) {
              result[key] = storage[key];
            }
          }
          return result;
        },
        async set(values) {
          Object.assign(storage, values);
        },
        async remove(keys) {
          for (const key of Array.isArray(keys) ? keys : [keys]) {
            delete storage[key];
          }
        },
      },
    },
    tabs: {
      async query() {
        return [
          {
            id: 7,
            url: "https://example.test/admin",
            title: "Admin",
            windowId: 1,
          },
        ];
      },
      async captureVisibleTab() {
        throw new Error("Either the '<all_urls>' or 'activeTab' permission is required.");
      },
    },
    scripting: {
      async executeScript() {
        return [
          {
            result: {
              selectedText: "",
              buttons: [],
              inputs: [],
            },
          },
        ];
      },
    },
    permissions: {
      async request() {
        return true;
      },
    },
  };
}

function submitMessage(text) {
  element("message-input").value = text;
  element("message-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
}

function element(id) {
  const found = document.getElementById(id);
  assert.ok(found, `missing element ${id}`);
  return found;
}

function messageRows() {
  return [...document.querySelectorAll(".message")];
}

function transcriptText() {
  return element("transcript").textContent ?? "";
}

function activeChatStorage(activeChat) {
  return {
    daemonSettings: {
      url: "http://127.0.0.1:43001",
      token: "pairing-token",
    },
    activeChat,
  };
}

function scopedActiveChatMarker(overrides = {}) {
  return {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
    activeTurnId: "turn_1",
    ...overrides,
  };
}

function scopedActiveChatMarkerWithoutTurn() {
  const { activeTurnId: _activeTurnId, ...marker } = scopedActiveChatMarker();
  return marker;
}

function legacyActiveChatMarker() {
  const { tabId: _tabId, origin: _origin, ...legacyMarker } = scopedActiveChatMarker();
  return legacyMarker;
}

function completedActiveChatSessions(userText) {
  return {
    sess_1: {
      session: {
        id: "sess_1",
        title: "Screen Sidekick",
      },
      messages: [
        {
          id: "msg_old",
          session_id: "sess_1",
          role: "user",
          text: userText,
          status: "completed",
          turn_id: "turn_old",
        },
      ],
      attachments: [],
      active_turn: null,
    },
  };
}

function runningActiveChatSessions(userText = null) {
  const messages = userText
    ? [
        {
          id: "msg_1",
          session_id: "sess_1",
          role: "user",
          text: userText,
          status: "pending",
          turn_id: "turn_1",
        },
      ]
    : [];
  return {
    sess_1: {
      session: {
        id: "sess_1",
        title: "Screen Sidekick",
      },
      messages,
      attachments: [],
      active_turn: {
        id: "turn_1",
        session_id: "sess_1",
        status: "running",
      },
    },
  };
}

function captureBridgeResponse() {
  return {
    schema_version: "sidekick_capture_bridge.v0.1",
    screen_context_json: "{}",
    prompt_text: "Prompt",
    safety: {
      has_danger: false,
      warning_count: 0,
      warnings: [],
      masked_input_values: 0,
      masked_secret_texts: 0,
    },
  };
}

function readyProtocolLimits() {
  return {
    max_message_bytes: 262144,
    max_attachment_bytes: 131072,
  };
}

async function waitFor(predicate) {
  for (let index = 0; index < 100; index += 1) {
    if (predicate()) {
      return;
    }
    await nextTick();
  }
  assert.fail("condition was not met");
}

async function waitForStoredActiveChat(predicate) {
  for (let index = 0; index < 100; index += 1) {
    const stored = await chrome.storage.session.get(["activeChat"]);
    if (predicate(stored.activeChat)) {
      return stored.activeChat;
    }
    await nextTick();
  }
  assert.fail("stored active chat condition was not met");
}

function nextTick() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

class FakeSidekickServer {
  constructor({
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
    failSendNumbers = new Set(),
    failSessionGetNumbers = new Set(),
    malformedSendResponseNumbers = new Set(),
    sessions = {},
  } = {}) {
    this.closeBeforePersistSendNumbers = closeBeforePersistSendNumbers;
    this.closeBeforeSendResponseNumbers = closeBeforeSendResponseNumbers;
    this.codexReadiness = codexReadiness;
    this.deferCloseEvents = deferCloseEvents;
    this.deferSessionGetNumbers = deferSessionGetNumbers;
    this.deferSendResponseNumbers = deferSendResponseNumbers;
    this.deferMessageCreatedNumbers = deferMessageCreatedNumbers;
    this.failAfterPersistSendNumbers = failAfterPersistSendNumbers;
    this.failSendNumbers = failSendNumbers;
    this.failSessionGetNumbers = failSessionGetNumbers;
    this.malformedSendResponseNumbers = malformedSendResponseNumbers;
    this.sessions = new Map(Object.entries(sessions));
    this.deferredCloseEvents = [];
    this.deferredSessionGetResponses = [];
    this.deferredSendResponses = [];
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
        socket.receiveSuccess(request.id, {});
        return;
      case "session/create":
        this.sessionCreateCount += 1;
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
            safety_status: "clean",
            debug_available: false,
          },
        });
        return;
      case "message/send":
        this.sendCount += 1;
        this.sendSessionIds.push(request.params.session_id);
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
          const notifyMessageCreated = () => socket.receiveNotification("message/created", {
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

class FakeWebSocket {
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
