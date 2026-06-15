import assert from "node:assert/strict";
import test from "node:test";

import { installManualTimers, waitForMicrotasks } from "./manual_timers.mjs";
import {
  assertDifferentMessageSendRequest,
  assertSameMessageSendRequest,
  captureBridgeResponse,
  element,
  importFreshSidePanel,
  installSidePanelHarness,
  messageRows,
  nextTick,
  submitMessage,
  transcriptText,
  waitFor,
  waitForStoredActiveChat,
} from "./support/side_panel_harness.mjs";

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

test("codex unavailable initialize stops ask before session or message send", async () => {
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

test("ask requests host permission before daemon websocket or session create", async () => {
  const server = installSidePanelHarness();
  const permissionSessionCreateCounts = [];
  let executeScriptCount = 0;
  chrome.scripting.executeScript = async () => {
    executeScriptCount += 1;
    if (executeScriptCount === 1) {
      throw new Error("Extension manifest must request permission to access this host.");
    }
    return [
      {
        result: {
          selectedText: "",
          buttons: [],
          inputs: [],
        },
      },
    ];
  };
  chrome.permissions.request = async (details) => {
    permissionSessionCreateCounts.push(server.sessionCreateCount);
    assert.deepEqual(details, {
      origins: ["https://example.test/*"],
    });
    return true;
  };
  await importFreshSidePanel();

  submitMessage("Needs site access first");
  await waitFor(() => server.sendCount === 1);

  assert.deepEqual(permissionSessionCreateCounts, [0]);
  assert.equal(executeScriptCount, 2);
  assert.equal(server.sessionCreateCount, 1);
  assert.equal(server.attachCount, 1);
});

test("capture failure stops ask before daemon session side effects", async () => {
  const server = installSidePanelHarness();
  chrome.scripting.executeScript = async () => {
    throw new Error("DOM capture failed before daemon I/O");
  };
  await importFreshSidePanel();

  submitMessage("Do not create a session");
  await waitFor(
    () => element("status").textContent === "DOM capture failed before daemon I/O",
  );

  assert.equal(server.sockets.length, 0);
  assert.equal(server.sessionCreateCount, 0);
  assert.equal(server.attachCount, 0);
  assert.equal(server.sendCount, 0);
  assert.equal(element("message-input").value, "Do not create a session");
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

test("retry keeps pending send reuse state until a reused snapshot renders", async () => {
  const question = "Retry the same pending send";
  const server = installSidePanelHarness({
    closeBeforeSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1, 2]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  assert.equal(element("message-input").value, question);
  const firstRequest = server.messageSendRequests[0];

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 2);
  await waitFor(() => element("ask").disabled === false);

  const secondRequest = server.messageSendRequests[1];
  assert.equal(server.sendCount, 2);
  assert.equal(server.attachCount, 1);
  assert.equal(server.reusedSendCount, 1);
  assertSameMessageSendRequest(secondRequest, firstRequest, question);
  assert.equal(element("message-input").value, question);
  assert.equal(element("status").textContent, "Session recovery failed.");

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 3);
  await nextTick();

  const thirdRequest = server.messageSendRequests[2];
  assert.equal(server.sendCount, 3);
  assert.equal(server.attachCount, 1);
  assert.equal(server.reusedSendCount, 2);
  assertSameMessageSendRequest(thirdRequest, firstRequest, question);
  assert.equal(element("message-input").value, "");
  assert.equal(element("status").textContent, "Asking");
  assert.equal(
    messageRows().filter(
      (row) => row.className === "message user" && row.textContent.includes(question),
    ).length,
    1,
  );
  assert.equal(messageRows().length, 2);
});

test("retry preserves pending reuse state when reused response fails before persisted ids", async () => {
  const question = "Retry after reused send setup fails";
  const server = installSidePanelHarness({
    closeBeforeSendResponseNumbers: new Set([1]),
    failReusedSendNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];
  assert.equal(element("message-input").value, question);

  submitMessage(question);
  await waitFor(() => server.reusedSendCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const secondRequest = server.messageSendRequests[1];
  assert.equal(server.sendCount, 2);
  assert.equal(server.attachCount, 1);
  assertSameMessageSendRequest(secondRequest, firstRequest, question);
  assert.equal(element("message-input").value, question);
  assert.equal(element("status").textContent, "Retry setup failed.");

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 2);
  await nextTick();

  const thirdRequest = server.messageSendRequests[2];
  assert.equal(server.sendCount, 3);
  assert.equal(server.attachCount, 1);
  assert.equal(server.reusedSendCount, 2);
  assertSameMessageSendRequest(thirdRequest, firstRequest, question);
  assert.equal(element("message-input").value, "");
  assert.equal(element("status").textContent, "Asking");
  assert.equal(
    messageRows().filter(
      (row) => row.className === "message user" && row.textContent.includes(question),
    ).length,
    1,
  );
  assert.equal(messageRows().length, 2);
});

test("terminal idempotency replay failure drops pending reuse state for the next submit", async () => {
  const question = "Retry after terminal idempotency failure";
  const server = installSidePanelHarness({
    closeBeforeSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1]),
    terminalReusedSendNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];

  submitMessage(question);
  await waitFor(() => server.reusedSendCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const terminalReplayRequest = server.messageSendRequests[1];
  assertSameMessageSendRequest(terminalReplayRequest, firstRequest, question);
  assert.equal(element("message-input").value, question);
  assert.equal(element("status").textContent, "Previous message/send attempt failed.");

  submitMessage(question);
  await waitFor(() => server.sendCount === 3);

  const freshRequest = server.messageSendRequests[2];
  assert.equal(server.attachCount, 2);
  assertDifferentMessageSendRequest(freshRequest, firstRequest);
  assert.equal(freshRequest.text, question);
});

test("terminal idempotency replay failure drops pending state before recovery retention", async () => {
  const question = "Retry after terminal idempotency failure during recovery";
  const server = installSidePanelHarness({
    closeAfterTerminalReusedSendNumbers: new Set([1]),
    closeBeforeSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1, 2]),
    terminalReusedSendNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 2);
  await waitFor(() => element("ask").disabled === false);

  const terminalReplayRequest = server.messageSendRequests[1];
  assertSameMessageSendRequest(terminalReplayRequest, firstRequest, question);
  assert.equal(element("message-input").value, question);
  assert.equal(element("status").textContent, "Session recovery failed.");

  submitMessage(question);
  await waitFor(() => server.sendCount === 3);

  const freshRequest = server.messageSendRequests[2];
  assert.equal(server.attachCount, 2);
  assertDifferentMessageSendRequest(freshRequest, firstRequest);
  assert.equal(freshRequest.text, question);
});

test("retained warning attachment retry preserves review status when send starts fresh", async () => {
  const question = "Retry warning attachment";
  const server = installSidePanelHarness({
    attachmentSafetyStatus: "warning",
    closeBeforePersistSendNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];

  submitMessage(question);
  await waitFor(() => server.sendCount === 2);

  const retryRequest = server.messageSendRequests[1];
  assert.equal(server.attachCount, 1);
  assert.equal(server.reusedSendCount, 0);
  assertSameMessageSendRequest(retryRequest, firstRequest, question);
  assert.equal(element("status").textContent, "Review");
});

test("retained warning attachment retry preserves review status when send is reused", async () => {
  const question = "Retry warning attachment with reused send";
  const server = installSidePanelHarness({
    attachmentSafetyStatus: "warning",
    closeBeforeSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 2);
  await nextTick();

  const retryRequest = server.messageSendRequests[1];
  assert.equal(server.attachCount, 1);
  assert.equal(server.reusedSendCount, 1);
  assertSameMessageSendRequest(retryRequest, firstRequest, question);
  assert.equal(element("status").textContent, "Review");
});

test("retry rejects stale capture scope before reusing pending attachment", async () => {
  const question = "Retry after tab changes";
  const server = installSidePanelHarness({
    closeBeforeSendResponseNumbers: new Set([1]),
    failSessionGetNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage(question);
  await waitFor(() => server.sessionGetCount === 1);
  await waitFor(() => element("ask").disabled === false);

  const firstRequest = server.messageSendRequests[0];
  chrome.tabs.query = async () => [
    {
      id: 8,
      url: "https://other.test/admin",
      title: "Other admin",
      windowId: 1,
    },
  ];

  submitMessage(question);
  await waitFor(
    () => element("status").textContent === "Reopen Screen Sidekick on this tab before capture",
  );

  assert.equal(server.sendCount, 1);
  assert.equal(server.attachCount, 1);
  assert.equal(server.reusedSendCount, 0);
  assert.equal(server.messageSendRequests.length, 1);
  assert.equal(element("message-input").value, question);

  chrome.tabs.query = async () => [
    {
      id: 7,
      url: "https://example.test/admin",
      title: "Admin",
      windowId: 1,
    },
  ];

  submitMessage(question);
  await waitFor(() => server.sendCount === 2);

  const secondRequest = server.messageSendRequests[1];
  assert.equal(server.attachCount, 2);
  assertDifferentMessageSendRequest(secondRequest, firstRequest);
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
