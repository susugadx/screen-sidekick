import assert from "node:assert/strict";
import test from "node:test";

import { clearPreviewState, setPreviewState } from "../dist/preview_state.js";

test("sets preview content and enables copy buttons after successful capture", () => {
  const elements = makePreviewElements();

  setPreviewState(elements, {
    screenContextJson: "{\"schema_version\":\"screen_context.v0.1\"}",
    promptText: "Explain these controls",
    safetySummaryText: "danger: no",
  });

  assert.equal(elements.screenContextJson.value, "{\"schema_version\":\"screen_context.v0.1\"}");
  assert.equal(elements.promptText.value, "Explain these controls");
  assert.equal(elements.safetySummary.textContent, "danger: no");
  assert.equal(elements.copyJson.disabled, false);
  assert.equal(elements.copyPrompt.disabled, false);
});

test("clears stale preview content and disables copy buttons", () => {
  const elements = makePreviewElements();
  setPreviewState(elements, {
    screenContextJson: "{\"stale\":true}",
    promptText: "stale prompt",
    safetySummaryText: "stale safety",
  });

  clearPreviewState(elements);

  assert.equal(elements.screenContextJson.value, "");
  assert.equal(elements.promptText.value, "");
  assert.equal(elements.safetySummary.textContent, "");
  assert.equal(elements.copyJson.disabled, true);
  assert.equal(elements.copyPrompt.disabled, true);
});

function makePreviewElements() {
  return {
    screenContextJson: { value: "" },
    promptText: { value: "" },
    safetySummary: { textContent: "" },
    copyJson: { disabled: true },
    copyPrompt: { disabled: true },
  };
}
