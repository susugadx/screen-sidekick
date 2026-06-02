import assert from "node:assert/strict";
import test from "node:test";

import {
  EXTENSION_CAPTURE_BODY_LIMIT_BYTES,
  RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
  captureBodyByteLength,
  serializeCaptureContextForBridge,
} from "../dist/capture_contract.js";

test("serializes oversized page captures under the bridge request budget", () => {
  const longText = "x".repeat(8000);
  const context = {
    schema_version: RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
    page: {
      url: `https://example.test/${longText}`,
      title: longText,
    },
    selected_text: longText,
    buttons: Array.from({ length: 80 }, () => ({
      text: longText,
      aria_label: longText,
      title: longText,
      visible: true,
    })),
    inputs: Array.from({ length: 80 }, () => ({
      kind: "text",
      name: longText,
      label: longText,
      aria_label: longText,
      title: longText,
      placeholder: longText,
      visible: true,
    })),
  };

  const body = serializeCaptureContextForBridge(context);
  const limited = JSON.parse(body);

  assert.ok(Buffer.byteLength(body, "utf8") <= EXTENSION_CAPTURE_BODY_LIMIT_BYTES);
  assert.ok(captureBodyByteLength(limited) <= EXTENSION_CAPTURE_BODY_LIMIT_BYTES);
  assert.ok(limited.buttons.length + limited.inputs.length <= 40);
  assert.ok(limited.buttons.every((button) => button.text.length <= 256));
});

test("serializes truncated external text as well-formed JSON strings", () => {
  const boundaryText = `${"x".repeat(255)}\u{1F600}tail`;
  const context = {
    schema_version: RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
    buttons: [
      {
        text: boundaryText,
        aria_label: "broken-\uD83D",
        title: "low-\uDE00",
        visible: true,
      },
    ],
    inputs: [
      {
        kind: "text",
        label: "input-\uD83D",
        placeholder: `${"y".repeat(255)}\u{1F600}tail`,
        visible: true,
      },
    ],
  };

  const body = serializeCaptureContextForBridge(context);
  const limited = JSON.parse(body);

  assert.equal(limited.buttons[0].text, `${"x".repeat(255)}\u{1F600}`);
  assert.equal(limited.buttons[0].aria_label, "broken-\uFFFD");
  assert.equal(limited.buttons[0].title, "low-\uFFFD");
  assert.equal(limited.inputs[0].label, "input-\uFFFD");
  assert.equal(limited.inputs[0].placeholder, `${"y".repeat(255)}\u{1F600}`);
  assert.equal(body.includes("\\ud83d"), false);
  assert.equal(body.includes("\\ude00"), false);
});
