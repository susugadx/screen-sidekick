import assert from "node:assert/strict";
import test from "node:test";

import { SidePanelActiveSessionState } from "../dist/side_panel_active_session.js";

const SETTINGS = {
  url: "http://127.0.0.1:43001",
  token: "pairing-token",
};

const CAPTURE_GRANT = {
  tabId: 7,
  origin: "https://example.test",
  hostPermissionPattern: "https://example.test/*",
};

test("recovery guard becomes stale when active chat generation changes", () => {
  const state = new SidePanelActiveSessionState();
  state.setActiveSession(SETTINGS, "sess_1");

  const guard = state.beginRecovery(SETTINGS, "sess_1");

  assert.equal(state.sessionRecoveryRequired, true);
  assert.equal(state.isRecoveryCurrent(guard), true);

  state.clearActiveChat();

  assert.equal(state.isRecoveryCurrent(guard), false);
  assert.equal(state.sessionRecoveryRequired, false);
});

test("matches active daemon identity only for the active session settings", () => {
  const state = new SidePanelActiveSessionState();

  assert.equal(state.activeDaemonSettings(), null);
  assert.equal(state.matchesActiveDaemonIdentity(SETTINGS), false);

  state.setActiveSession(SETTINGS, "sess_1");

  assert.deepEqual(state.activeDaemonSettings(), SETTINGS);
  assert.equal(state.matchesActiveDaemonIdentity(SETTINGS), true);
  assert.equal(
    state.matchesActiveDaemonIdentity({ ...SETTINGS, token: "new-pairing-token" }),
    false,
  );
  assert.equal(
    state.matchesActiveDaemonIdentity({ ...SETTINGS, url: "http://127.0.0.1:43002" }),
    false,
  );
});

test("control state separates request in flight active turn and recovery blocking", () => {
  const state = new SidePanelActiveSessionState();

  assert.deepEqual(state.toControlState(), {
    requestInFlight: false,
    turnActive: false,
    sessionRecoveryRequired: false,
  });

  state.setRequestInFlight(true);
  state.setActiveTurnId("turn_1");
  state.setSessionRecoveryRequired(true);

  assert.deepEqual(state.toControlState(), {
    requestInFlight: true,
    turnActive: true,
    sessionRecoveryRequired: true,
  });
});

test("active chat marker includes daemon identity capture scope and active turn", () => {
  const state = new SidePanelActiveSessionState();

  assert.equal(state.toActiveChatMarker(CAPTURE_GRANT), null);

  state.setActiveSession(SETTINGS, "sess_1");
  assert.deepEqual(state.toActiveChatMarker(CAPTURE_GRANT), {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
  });

  state.setActiveTurnId("turn_1");
  assert.deepEqual(state.toActiveChatMarker(CAPTURE_GRANT), {
    daemonUrl: "http://127.0.0.1:43001",
    daemonToken: "pairing-token",
    tabId: 7,
    origin: "https://example.test",
    sessionId: "sess_1",
    activeTurnId: "turn_1",
  });
});

test("clears recovery blocking state without clearing active chat identity", () => {
  const state = new SidePanelActiveSessionState();
  state.setActiveSession(SETTINGS, "sess_1");
  state.setSubscribedSessionId("sess_1");
  state.setRequestInFlight(true);
  state.setActiveTurnId("turn_1");
  state.setSessionRecoveryRequired(true);

  state.clearRecoveryBlockingState();

  assert.equal(state.sessionId, "sess_1");
  assert.equal(state.subscribedSessionId, "sess_1");
  assert.equal(state.requestInFlight, true);
  assert.equal(state.activeTurnId, null);
  assert.equal(state.sessionRecoveryRequired, false);

  state.clearActiveChat();

  assert.equal(state.sessionId, null);
  assert.equal(state.activeDaemonSettings(), null);
  assert.equal(state.subscribedSessionId, null);
  assert.equal(state.activeTurnId, null);
  assert.equal(state.sessionRecoveryRequired, false);
  assert.equal(state.requestInFlight, true);
});
