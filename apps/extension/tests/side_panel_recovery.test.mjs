import assert from "node:assert/strict";
import test from "node:test";

import { installManualTimers, waitForMicrotasks } from "./manual_timers.mjs";
import {
  activeChatStorage,
  assertDifferentMessageSendRequest,
  completedActiveChatSessions,
  element,
  importFreshSidePanel,
  importFreshSidePanelWithMicrotasks,
  installSidePanelHarness,
  legacyActiveChatMarker,
  messageRows,
  nextTick,
  runningActiveChatSessions,
  scopedActiveChatMarker,
  scopedActiveChatMarkerWithoutTurn,
  submitMessage,
  transcriptText,
  waitFor,
  waitForStoredActiveChat,
} from "./support/side_panel_harness.mjs";

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

test("connection loss recovery uses active daemon identity instead of unsaved inputs", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Keep recovering on the original daemon");
  await waitFor(() => server.sendCount === 1);
  const firstSocket = server.socket;
  element("bridge-url").value = "http://127.0.0.1:43002";

  firstSocket.close();
  await waitFor(() => server.sessionGetCount === 1);

  assert.notEqual(server.socket, firstSocket);
  assert.equal(server.socket.url, "ws://127.0.0.1:43001/v0/ws");
  assert.equal(element("ask").disabled, true);
  assert.equal(element("message-input").disabled, true);
  assert.equal(element("status").textContent, "Asking");
  assert.equal(transcriptText().includes("Keep recovering on the original daemon"), true);

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
  assert.equal(element("status").textContent, "Ready");
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
  const firstRequest = server.messageSendRequests[0];

  submitMessage("Retry question");
  await waitFor(() => server.sendCount === 2);
  const secondRequest = server.messageSendRequests[1];

  assert.equal(server.sessionCreateCount, 1);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_1"]);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_1"]);
  assert.equal(server.attachCount, 2);
  assertDifferentMessageSendRequest(secondRequest, firstRequest);
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
