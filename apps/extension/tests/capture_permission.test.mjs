import assert from "node:assert/strict";
import test from "node:test";

import {
  REOPEN_SIDEKICK_FOR_TAB_MESSAGE,
  UNSUPPORTED_CAPTURE_TAB_MESSAGE,
  assertFreshCaptureGrant,
  captureHostPermissionPatternFromUrl,
  captureOriginFromUrl,
  createCaptureGrant,
  isMissingCaptureVisibleTabPermissionError,
  isMissingManifestHostPermissionError,
} from "../dist/capture_permission.js";

test("creates capture grants for http and https tab origins", () => {
  assert.deepEqual(createCaptureGrant({ id: 7, url: "https://example.test/path" }), {
    tabId: 7,
    origin: "https://example.test",
    hostPermissionPattern: "https://example.test/*",
  });
  assert.deepEqual(createCaptureGrant({ id: 8, url: "http://127.0.0.1:5173/ui" }), {
    tabId: 8,
    origin: "http://127.0.0.1:5173",
    hostPermissionPattern: "http://127.0.0.1/*",
  });
});

test("creates host permission patterns from capturable tab URLs", () => {
  assert.equal(
    captureHostPermissionPatternFromUrl("https://zed.dev/docs?token=secret"),
    "https://zed.dev/*",
  );
  assert.equal(
    captureHostPermissionPatternFromUrl("http://localhost:5173/admin"),
    "http://localhost/*",
  );
});

test("rejects tabs without capturable web origins", () => {
  assert.equal(captureOriginFromUrl("chrome://extensions"), null);
  assert.equal(captureHostPermissionPatternFromUrl("chrome://extensions"), null);
  assert.equal(captureOriginFromUrl("file:///tmp/page.html"), null);
  assert.equal(captureHostPermissionPatternFromUrl("file:///tmp/page.html"), null);
  assert.throws(
    () => createCaptureGrant({ id: 7, url: "chrome://extensions" }),
    new RegExp(UNSUPPORTED_CAPTURE_TAB_MESSAGE),
  );
});

test("requires capture to stay on the originally granted tab and origin", () => {
  const grant = createCaptureGrant({ id: 7, url: "https://example.test/admin" });

  assert.doesNotThrow(() =>
    assertFreshCaptureGrant({ id: 7, url: "https://example.test/settings" }, grant),
  );
  assert.throws(
    () => assertFreshCaptureGrant({ id: 9, url: "https://example.test/admin" }, grant),
    new RegExp(REOPEN_SIDEKICK_FOR_TAB_MESSAGE),
  );
  assert.throws(
    () => assertFreshCaptureGrant({ id: 7, url: "https://other.test/admin" }, grant),
    new RegExp(REOPEN_SIDEKICK_FOR_TAB_MESSAGE),
  );
});

test("classifies Chrome capture permission errors", () => {
  assert.equal(
    isMissingManifestHostPermissionError(
      new Error(
        'Cannot access contents of url "https://zed.dev/". Extension manifest must request permission to access this host.',
      ),
    ),
    true,
  );
  assert.equal(
    isMissingManifestHostPermissionError(new Error("Page context capture failed")),
    false,
  );

  assert.equal(
    isMissingCaptureVisibleTabPermissionError(
      new Error("Either the '<all_urls>' or 'activeTab' permission is required."),
    ),
    true,
  );
  assert.equal(
    isMissingCaptureVisibleTabPermissionError(new Error("Screenshot metadata capture failed")),
    false,
  );
});
