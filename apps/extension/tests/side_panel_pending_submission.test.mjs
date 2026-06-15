import assert from "node:assert/strict";
import test from "node:test";

import {
  PendingSubmittedQuestionState,
  messageMatchesPendingSubmittedQuestion,
} from "../dist/side_panel_pending_submission.js";

const SETTINGS = {
  url: "http://127.0.0.1:43001",
  token: "pairing-token",
};

test("reuses pending submission only for the same text settings and session", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());

  assert.equal(state.findRetryable(SETTINGS, "Question", "sess_1"), pending);
  assert.equal(
    state.findRetryable(SETTINGS, "Different question", "sess_1"),
    null,
  );
  assert.equal(
    state.findRetryable({ ...SETTINGS, token: "other-token" }, "Question", "sess_1"),
    null,
  );
  assert.equal(state.findRetryable(SETTINGS, "Question", "sess_2"), null);
});

test("retained retry setup failure keeps pending submission for idempotent reuse", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());

  assert.equal(
    state.shouldDiscardAfterFailure(pending, {
      recoveryRequired: false,
    }),
    true,
  );

  state.retainForIdempotentRetry(pending);

  assert.equal(
    state.shouldDiscardAfterFailure(pending, {
      recoveryRequired: false,
    }),
    false,
  );
  assert.equal(state.findRetryable(SETTINGS, "Question", "sess_1"), pending);
});

test("discard removes retained pending submission", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());

  state.retainForIdempotentRetry(pending);
  state.discard(pending);

  assert.equal(state.current(), null);
  assert.equal(state.findRetryable(SETTINGS, "Question", "sess_1"), null);
});

test("recovery failure without terminal replay keeps retained pending submission", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());

  state.retainForIdempotentRetry(pending);

  assert.equal(
    state.shouldDiscardAfterFailure(pending, {
      recoveryRequired: true,
    }),
    false,
  );
});

test("persisted message id match clears pending submission", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());
  state.recordPersistedIds(pending, "msg_1", "turn_1");

  const clearedText = state.clearIfPersisted(
    sidekickMessage({ id: "msg_1", turnId: "turn_other" }),
  );

  assert.equal(clearedText, "Question");
  assert.equal(state.current(), null);
});

test("persisted turn id match clears pending submission", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());
  state.recordPersistedIds(pending, "msg_1", "turn_1");

  const clearedText = state.clearIfPersisted(
    sidekickMessage({ id: "msg_other", turnId: "turn_1" }),
  );

  assert.equal(clearedText, "Question");
  assert.equal(state.current(), null);
});

test("text-only match does not clear pending submission without persisted ids", () => {
  const state = new PendingSubmittedQuestionState();
  const pending = state.set(pendingQuestion());

  assert.equal(state.clearIfPersisted(sidekickMessage()), null);
  assert.equal(state.current(), pending);
});

test("active turn match clears pending submission after restored snapshot", () => {
  const state = new PendingSubmittedQuestionState();
  state.set(pendingQuestion());

  const clearedText = state.clearIfPersisted(sidekickMessage({ turnId: "turn_1" }), {
    id: "turn_1",
    sessionId: "sess_1",
    status: "running",
  });

  assert.equal(clearedText, "Question");
  assert.equal(state.current(), null);
});

test("message match rejects same text from a different session", () => {
  const pending = pendingQuestion();

  assert.equal(
    messageMatchesPendingSubmittedQuestion(
      sidekickMessage({ sessionId: "sess_2", id: "msg_1", turnId: "turn_1" }),
      pending,
    ),
    false,
  );
});

function pendingQuestion(overrides = {}) {
  return {
    sessionId: "sess_1",
    text: "Question",
    idempotencyKey: "idem_1",
    attachmentIds: ["att_1"],
    captureGrant: {
      tabId: 7,
      origin: "https://example.test",
      hostPermissionPattern: "https://example.test/*",
    },
    safetyStatus: "clean",
    daemonUrl: SETTINGS.url,
    daemonToken: SETTINGS.token,
    retainForIdempotentRetry: false,
    ...overrides,
  };
}

function sidekickMessage(overrides = {}) {
  return {
    id: "msg_current",
    sessionId: "sess_1",
    role: "user",
    text: "Question",
    status: "pending",
    turnId: "turn_current",
    ...overrides,
  };
}
