import assert from "node:assert/strict";
import test from "node:test";

import { JSDOM } from "jsdom";

import { collectBrowserContext } from "../dist/dom_capture.js";

test("captures input submit button values as button text", () => {
  installDom(`
    <form>
      <input type="submit" value="Delete user" />
      <input id="password" type="password" name="password" value="swordfish" />
      <label for="password">Password</label>
    </form>
  `);

  const capture = collectBrowserContext();

  assert.equal(capture.buttons[0]?.text, "Delete user");
  assert.equal(capture.inputs.length, 1);
  assert.equal(capture.inputs[0]?.kind, "password");
  assert.equal("value" in capture.inputs[0], false);
});

test("captures plaintext-only contenteditable elements as inputs", () => {
  installDom(`
    <section>
      <div contenteditable="plaintext-only" aria-label="Comment editor"></div>
    </section>
  `);

  const capture = collectBrowserContext();

  assert.equal(capture.inputs.length, 1);
  assert.equal(capture.inputs[0]?.kind, "content_editable");
  assert.equal(capture.inputs[0]?.aria_label, "Comment editor");
});

test("does not capture contenteditable false elements as empty inputs", () => {
  installDom(`
    <section>
      <div contenteditable="false" aria-label="Read only content"></div>
      <div contenteditable="FALSE" aria-label="Also read only"></div>
    </section>
  `);

  const capture = collectBrowserContext();

  assert.equal(capture.inputs.length, 0);
});

test("excludes nested textarea contents from captured labels", () => {
  installDom(`
    <form>
      <label>
        Notes
        <textarea name="notes">private note</textarea>
      </label>
    </form>
  `);

  const capture = collectBrowserContext();

  assert.equal(capture.inputs.length, 1);
  assert.equal(capture.inputs[0]?.kind, "textarea");
  assert.equal(capture.inputs[0]?.label, "Notes");
  assert.equal(JSON.stringify(capture).includes("private note"), false);
});

test("excludes nested contenteditable contents from captured labels", () => {
  installDom(`
    <form>
      <label>
        Notes
        <div contenteditable="plaintext-only">private note</div>
        <input name="title" />
      </label>
    </form>
  `);

  const capture = collectBrowserContext();
  const labeledInput = capture.inputs.find((input) => input.name === "title");

  assert.equal(labeledInput?.label, "Notes");
  assert.equal(JSON.stringify(capture).includes("private note"), false);
});

test("captures only controls that intersect the viewport", () => {
  installDom(`
    <section>
      ${Array.from(
        { length: 45 },
        (_, index) => `<button data-rect="0,900,100,24">Offscreen ${index}</button>`,
      ).join("")}
      <button data-rect="0,-60,100,24">Above viewport</button>
      <button data-rect="0,12,100,24">Visible action</button>
      <input name="hidden-offscreen" data-rect="0,900,100,24" />
      <input name="visible-input" aria-label="Visible input" data-rect="0,48,100,24" />
    </section>
  `);

  const capture = collectBrowserContext();

  assert.deepEqual(
    capture.buttons.map((button) => button.text),
    ["Visible action"],
  );
  assert.deepEqual(
    capture.inputs.map((input) => input.name),
    ["visible-input"],
  );
});

test("captures DOM text and attributes as well-formed truncated strings", () => {
  installDom(`<section id="controls"></section>`);
  const button = document.createElement("button");
  button.textContent = `${"x".repeat(511)}\u{1F600}tail`;
  button.setAttribute("aria-label", "broken-\uD83D");
  document.getElementById("controls").append(button);

  const capture = collectBrowserContext();
  const body = JSON.stringify(capture);

  assert.equal(capture.buttons[0]?.text, `${"x".repeat(511)}\u{1F600}`);
  assert.equal(capture.buttons[0]?.aria_label, "broken-\uFFFD");
  assert.equal(body.includes("\\ud83d"), false);
});

function installDom(html) {
  const dom = new JSDOM(html, { pretendToBeVisual: true });
  const { window } = dom;

  globalThis.window = window;
  globalThis.document = window.document;
  globalThis.Element = window.Element;
  globalThis.HTMLElement = window.HTMLElement;
  globalThis.HTMLButtonElement = window.HTMLButtonElement;
  globalThis.HTMLInputElement = window.HTMLInputElement;
  globalThis.HTMLSelectElement = window.HTMLSelectElement;
  globalThis.HTMLTextAreaElement = window.HTMLTextAreaElement;

  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    value: 800,
  });
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    value: 600,
  });

  Object.defineProperty(window.HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value() {
      const rect = parseRectAttribute(this.getAttribute("data-rect"));
      return {
        x: rect.left,
        y: rect.top,
        top: rect.top,
        left: rect.left,
        right: rect.left + rect.width,
        bottom: rect.top + rect.height,
        width: rect.width,
        height: rect.height,
        toJSON() {
          return this;
        },
      };
    },
  });

  Object.defineProperty(window.HTMLElement.prototype, "isContentEditable", {
    configurable: true,
    get() {
      const value = this.getAttribute("contenteditable");
      return value !== null && value.toLowerCase() !== "false";
    },
  });
}

function parseRectAttribute(value) {
  if (!value) {
    return { left: 0, top: 0, width: 100, height: 24 };
  }
  const [left, top, width, height] = value.split(",").map(Number);
  return { left, top, width, height };
}
