import assert from "node:assert/strict";
import test from "node:test";

import { installManualTimers, waitForMicrotasks } from "./manual_timers.mjs";
import {
  activeChatMap,
  activeChatMarkerFor,
  activeChatStorage,
  assertDifferentMessageSendRequest,
  assertSameMessageSendRequest,
  completedActiveChatSessions,
  currentActiveChatMarker,
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
  terminalActiveChatSessions,
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
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarkerWithoutTurn())),
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

test("websocket close before message persistence ignores old terminal same-text row", async () => {
  const repeatedQuestion = "Retry old failed question text";
  const server = installSidePanelHarness({
    closeBeforePersistSendNumbers: new Set([1]),
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarkerWithoutTurn())),
    sessions: terminalActiveChatSessions(repeatedQuestion, "failed", "turn_old"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  submitMessage(repeatedQuestion);
  await waitFor(() => server.sendCount === 1);
  const firstRequest = server.messageSendRequests[0];
  await waitFor(() => server.sessionGetCount === 2);

  assert.equal(element("status").textContent, "Ready");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(element("message-input").value, repeatedQuestion);
  assert.equal(messageRows().length, 1);

  submitMessage(repeatedQuestion);
  await waitFor(() => server.sendCount === 2);
  const secondRequest = server.messageSendRequests[1];

  assert.equal(server.attachCount, 1);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_1"]);
  assertSameMessageSendRequest(secondRequest, firstRequest, repeatedQuestion);
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
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
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

test("side panel reload renders failed restored active turn as terminal error", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
    sessions: terminalActiveChatSessions("Failed restored question", "failed"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("status").textContent, "Codex turn failed");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText().includes("Failed restored question"), true);

  const activeChat = await waitForStoredActiveChat(
    (value) => currentActiveChatMarker(value)?.activeTurnId === undefined,
  );
  assert.equal(currentActiveChatMarker(activeChat).activeTurnId, undefined);
});

test("side panel reload renders cancelled restored active turn as terminal state", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
    sessions: terminalActiveChatSessions("Cancelled restored question", "cancelled"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("status").textContent, "Cancelled");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText().includes("Cancelled restored question"), true);
});

test("side panel reload ignores terminal messages from old turns", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
    sessions: terminalActiveChatSessions("Old failed restored question", "failed", "turn_old"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(element("status").textContent, "Ready");
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText().includes("Old failed restored question"), true);
});

test("stored active chat recovery failure re-enables ask controls", async () => {
  const server = installSidePanelHarness({
    failSessionGetNumbers: new Set([1]),
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
    sessions: runningActiveChatSessions(),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").disabled, false);
  assert.equal(element("status").textContent, "Session recovery failed.");
});

test("message send timeout renders recovered failed turn without retained retry", async () => {
  const question = "Recover failed message send timeout";
  const server = installSidePanelHarness({
    deferSendResponseNumbers: new Set([1]),
  });
  const timers = installManualTimers();

  try {
    await importFreshSidePanelWithMicrotasks();

    submitMessage(question);
    await waitForMicrotasks(() => server.sendCount === 1);
    const firstRequest = server.messageSendRequests[0];

    server.finishLatestTurn("sess_1", "failed");
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 45_000);
    timers.fireNext();
    await waitForMicrotasks(() => element("status").textContent === "Codex turn failed");

    assert.equal(element("status").textContent, "Codex turn failed");
    assert.equal(element("ask").disabled, false);
    assert.equal(element("message-input").disabled, false);
    assert.equal(element("message-input").value, "");

    submitMessage(question);
    await waitForMicrotasks(() => server.sendCount === 2);
    const secondRequest = server.messageSendRequests[1];

    assert.equal(server.reusedSendCount, 0);
    assert.equal(server.attachCount, 2);
    assertDifferentMessageSendRequest(secondRequest, firstRequest);
  } finally {
    timers.restore();
  }
});

test("message send timeout renders recovered cancelled turn without retained retry", async () => {
  const question = "Recover cancelled message send timeout";
  const server = installSidePanelHarness({
    deferSendResponseNumbers: new Set([1]),
  });
  const timers = installManualTimers();

  try {
    await importFreshSidePanelWithMicrotasks();

    submitMessage(question);
    await waitForMicrotasks(() => server.sendCount === 1);
    const firstRequest = server.messageSendRequests[0];

    server.finishLatestTurn("sess_1", "cancelled");
    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 45_000);
    timers.fireNext();
    await waitForMicrotasks(() => element("status").textContent === "Cancelled");

    assert.equal(element("ask").disabled, false);
    assert.equal(element("message-input").disabled, false);
    assert.equal(element("message-input").value, "");

    submitMessage(question);
    await waitForMicrotasks(() => server.sendCount === 2);
    const secondRequest = server.messageSendRequests[1];

    assert.equal(server.reusedSendCount, 0);
    assert.equal(server.attachCount, 2);
    assertDifferentMessageSendRequest(secondRequest, firstRequest);
  } finally {
    timers.restore();
  }
});

test("message send timeout keeps idempotent retry state when recovery fails", async () => {
  const question = "Retry after message send timeout";
  const server = installSidePanelHarness({
    deferSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1]),
  });
  const timers = installManualTimers();

  try {
    await importFreshSidePanelWithMicrotasks();

    submitMessage(question);
    await waitForMicrotasks(() => server.sendCount === 1);
    const firstRequest = server.messageSendRequests[0];

    assert.equal(timers.size, 1);
    assert.equal(timers.nextDelay(), 45_000);

    timers.fireNext();
    await waitForMicrotasks(() => element("ask").disabled === false);

    assert.equal(element("message-input").disabled, false);
    assert.equal(element("status").textContent, "Session recovery failed.");
    assert.equal(element("message-input").value, question);

    submitMessage(question);
    await waitForMicrotasks(() => server.sendCount === 2);
    const secondRequest = server.messageSendRequests[1];

    assert.equal(server.reusedSendCount, 1);
    assert.equal(server.attachCount, 1);
    assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_1"]);
    assertSameMessageSendRequest(secondRequest, firstRequest, question);
  } finally {
    timers.restore();
  }
});

test("recovery keeps save enabled and ignores delayed stale snapshots after settings change", async () => {
  const server = installSidePanelHarness({
    deferCloseEvents: true,
    deferSessionGetNumbers: new Set([1]),
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
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
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
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
    storage: activeChatStorage(
      activeChatMap(scopedActiveChatMarker({ daemonToken: "old-token" })),
    ),
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
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker({ tabId: 8 }))),
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
    storage: activeChatStorage(
      activeChatMap(scopedActiveChatMarker({ origin: "https://other.example" })),
    ),
    sessions: runningActiveChatSessions("Different origin question"),
  });
  await importFreshSidePanel();
  await nextTick();

  assert.equal(server.sessionGetCount, 0);
  assert.equal(element("ask").disabled, false);
  assert.equal(element("message-input").disabled, false);
  assert.equal(transcriptText(), "");
});

test("legacy single active chat marker is recovered", async () => {
  const server = installSidePanelHarness({
    storage: activeChatStorage(scopedActiveChatMarker()),
    sessions: runningActiveChatSessions("Legacy single marker question"),
  });
  await importFreshSidePanel();
  await waitFor(() => server.sessionGetCount === 1);

  assert.equal(server.sessionCreateCount, 0);
  assert.deepEqual(server.subscribeSessionIds, ["sess_1"]);
  assert.equal(transcriptText().includes("Legacy single marker question"), true);
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
    (value) => currentActiveChatMarker(value)?.activeTurnId === "turn_1",
  );

  assert.equal(marker.version, 1);
  assert.deepEqual(currentActiveChatMarker(marker), {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
    activeTurnId: "turn_1",
  });
});

test("active chat marker map preserves other tab markers", async () => {
  const otherMarker = scopedActiveChatMarker({
    tabId: 8,
    sessionId: "sess_other",
    activeTurnId: "turn_other",
  });
  installSidePanelHarness({
    storage: activeChatStorage(activeChatMap(otherMarker)),
  });
  await importFreshSidePanel();

  submitMessage("Scoped marker question");
  const activeChat = await waitForStoredActiveChat(
    (value) => currentActiveChatMarker(value)?.activeTurnId === "turn_1",
  );

  assert.deepEqual(
    activeChatMarkerFor(activeChat, otherMarker),
    otherMarker,
  );
  assert.deepEqual(currentActiveChatMarker(activeChat), {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
    activeTurnId: "turn_1",
  });
});

test("settings change clears only the current active chat marker", async () => {
  const otherMarker = scopedActiveChatMarker({
    tabId: 8,
    sessionId: "sess_other",
    activeTurnId: "turn_other",
  });
  installSidePanelHarness({
    storage: activeChatStorage(activeChatMap(otherMarker)),
  });
  await importFreshSidePanel();

  submitMessage("Current marker question");
  await waitForStoredActiveChat(
    (value) => currentActiveChatMarker(value)?.activeTurnId === "turn_1",
  );

  element("bridge-url").value = "http://127.0.0.1:43002";
  element("bridge-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
  await waitFor(() => element("status").textContent === "Saved");

  const stored = await chrome.storage.session.get(["activeChat"]);
  assert.deepEqual(stored.activeChat, activeChatMap(otherMarker));
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

test("fresh ask resets stale active session and continues in a new session", async () => {
  const server = installSidePanelHarness({
    failMissingSessionSubscribe: true,
  });
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

  server.sessions.delete("sess_1");
  server.socket.close();
  await nextTick();

  submitMessage("Second question");
  await waitFor(() => server.sendCount === 2);

  assert.equal(server.sessionCreateCount, 2);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_2"]);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_2"]);
  assert.equal(transcriptText().includes("Second question"), true);
});

test("retained retry discards stale session state instead of reusing attachments in a new session", async () => {
  const question = "Retry after stale session";
  const server = installSidePanelHarness({
    closeBeforePersistSendNumbers: new Set([1]),
    failMissingSessionSubscribe: true,
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];
  server.sessions.delete("sess_1");
  server.socket.close();
  await nextTick();

  submitMessage(question);
  await waitFor(() => element("status").textContent === "Session was not found.");

  assert.equal(server.sendCount, 1);

  submitMessage(question);
  await waitFor(() => server.sendCount === 2);
  const freshRequest = server.messageSendRequests[1];

  assert.equal(server.sessionCreateCount, 2);
  assert.deepEqual(server.sendSessionIds, ["sess_1", "sess_2"]);
  assert.deepEqual(server.attachSessionIds, ["sess_1", "sess_2"]);
  assertDifferentMessageSendRequest(freshRequest, firstRequest);
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
    storage: activeChatStorage(activeChatMap(scopedActiveChatMarker())),
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
