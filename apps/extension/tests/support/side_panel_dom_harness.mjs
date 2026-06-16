import assert from "node:assert/strict";

import { JSDOM } from "jsdom";
import { waitForMicrotasks } from "../manual_timers.mjs";
import { FakeLockManager } from "./side_panel_chrome_harness.mjs";

let moduleCounter = 0;

export async function importFreshSidePanel(expectedDaemonUrl = "http://127.0.0.1:43001") {
  await import(`../../dist/side_panel.js?test=${++moduleCounter}`);
  await waitFor(() => element("bridge-url").value === expectedDaemonUrl);
}

export async function importFreshSidePanelWithMicrotasks(
  expectedDaemonUrl = "http://127.0.0.1:43001",
) {
  await import(`../../dist/side_panel.js?test=${++moduleCounter}`);
  await waitForMicrotasks(() => element("bridge-url").value === expectedDaemonUrl);
}

export function submitMessage(text) {
  element("message-input").value = text;
  element("message-form").dispatchEvent(
    new window.Event("submit", {
      bubbles: true,
      cancelable: true,
    }),
  );
}

export function element(id) {
  const found = document.getElementById(id);
  assert.ok(found, `missing element ${id}`);
  return found;
}

export function messageRows() {
  return [...document.querySelectorAll(".message")];
}

export function transcriptText() {
  return element("transcript").textContent ?? "";
}

export async function waitFor(predicate) {
  for (let index = 0; index < 100; index += 1) {
    if (predicate()) {
      return;
    }
    await nextTick();
  }
  assert.fail("condition was not met");
}

export async function waitForStoredActiveChat(predicate) {
  for (let index = 0; index < 100; index += 1) {
    const stored = await chrome.storage.session.get(["activeChat"]);
    if (predicate(stored.activeChat)) {
      return stored.activeChat;
    }
    await nextTick();
  }
  assert.fail("stored active chat condition was not met");
}

export function nextTick() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

export function installDom() {
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
      locks: new FakeLockManager(),
    },
  });
}
