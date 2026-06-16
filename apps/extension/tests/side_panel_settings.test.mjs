import assert from "node:assert/strict";
import test from "node:test";

import {
  FakeWebSocket,
  element,
  importFreshSidePanel,
  installSidePanelHarness,
  nextTick,
  submitMessage,
  transcriptText,
  waitFor,
} from "./support/side_panel_harness.mjs";

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

test("saving malformed daemon URL disconnects active stale socket before stale errors update UI", async () => {
  const server = installSidePanelHarness();
  await importFreshSidePanel();

  submitMessage("Old daemon question");
  await waitFor(() => server.sendCount === 1);
  const firstSocket = server.socket;

  element("bridge-url").value = "not a daemon url";
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

test("saving different daemon settings disconnects stale socket without active session", async () => {
  const server = installSidePanelHarness({
    failSessionCreateNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("Session create will fail");
  await waitFor(() => element("status").textContent === "Session create failed.");
  const firstSocket = server.socket;

  assert.equal(server.sessionCreateCount, 1);
  assert.equal(server.attachCount, 0);
  assert.equal(server.sendCount, 0);

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

test("saving non-loopback daemon URL disconnects stale socket without active session", async () => {
  const server = installSidePanelHarness({
    failSessionCreateNumbers: new Set([1]),
  });
  await importFreshSidePanel();

  submitMessage("Session create will fail");
  await waitFor(() => element("status").textContent === "Session create failed.");
  const firstSocket = server.socket;

  assert.equal(server.sessionCreateCount, 1);
  assert.equal(server.attachCount, 0);
  assert.equal(server.sendCount, 0);

  element("bridge-url").value = "http://localhost:43001";
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
