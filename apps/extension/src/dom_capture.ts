import type { DomCapture, InputKind, RawBrowserButton, RawBrowserInput } from "./capture_contract.js";

export function collectBrowserContext(): DomCapture {
  const maxControls = 40;
  const maxTextChars = 512;

  function cleanText(value: string | null | undefined): string | undefined {
    const text = value?.replace(/\s+/g, " ").trim();
    if (!text) {
      return undefined;
    }
    return truncateText(text, maxTextChars);
  }

  function truncateText(value: string, maxChars: number): string {
    if (maxChars <= 0) {
      return "";
    }

    let text = "";
    let index = 0;
    let charCount = 0;
    while (index < value.length && charCount < maxChars) {
      const codeUnit = value.charCodeAt(index);
      if (isHighSurrogate(codeUnit)) {
        const nextCodeUnit = value.charCodeAt(index + 1);
        if (isLowSurrogate(nextCodeUnit)) {
          text += value.slice(index, index + 2);
          index += 2;
        } else {
          text += "\uFFFD";
          index += 1;
        }
      } else if (isLowSurrogate(codeUnit)) {
        text += "\uFFFD";
        index += 1;
      } else {
        text += String.fromCharCode(codeUnit);
        index += 1;
      }
      charCount += 1;
    }
    return text;
  }

  function isHighSurrogate(codeUnit: number): boolean {
    return codeUnit >= 0xd800 && codeUnit <= 0xdbff;
  }

  function isLowSurrogate(codeUnit: number): boolean {
    return codeUnit >= 0xdc00 && codeUnit <= 0xdfff;
  }

  function optionalAttribute(element: Element, name: string): string | undefined {
    return cleanText(element.getAttribute(name));
  }

  function visible(element: Element): boolean {
    if (!(element instanceof HTMLElement)) {
      return false;
    }
    const style = window.getComputedStyle(element);
    if (
      style.display === "none" ||
      style.visibility === "hidden" ||
      style.visibility === "collapse" ||
      style.opacity === "0"
    ) {
      return false;
    }
    const rect = element.getBoundingClientRect();
    const viewportWidth = window.innerWidth || document.documentElement.clientWidth;
    const viewportHeight = window.innerHeight || document.documentElement.clientHeight;
    return (
      rect.width > 0 &&
      rect.height > 0 &&
      rect.bottom > 0 &&
      rect.right > 0 &&
      rect.top < viewportHeight &&
      rect.left < viewportWidth
    );
  }

  function disabled(element: Element): boolean | undefined {
    if (
      element instanceof HTMLButtonElement ||
      element instanceof HTMLInputElement ||
      element instanceof HTMLSelectElement ||
      element instanceof HTMLTextAreaElement
    ) {
      return element.disabled;
    }
    return element.getAttribute("aria-disabled") === "true" ? true : undefined;
  }

  function makeButton(
    text: string | undefined,
    ariaLabel: string | undefined,
    title: string | undefined,
    disabledValue: boolean | undefined,
    visibleValue: boolean | undefined,
  ): RawBrowserButton {
    const button: RawBrowserButton = {};
    if (text) {
      button.text = text;
    }
    if (ariaLabel) {
      button.aria_label = ariaLabel;
    }
    if (title) {
      button.title = title;
    }
    if (disabledValue !== undefined) {
      button.disabled = disabledValue;
    }
    if (visibleValue !== undefined) {
      button.visible = visibleValue;
    }
    return button;
  }

  function makeInput(
    kind: InputKind | undefined,
    name: string | undefined,
    label: string | undefined,
    ariaLabel: string | undefined,
    title: string | undefined,
    placeholder: string | undefined,
    disabledValue: boolean | undefined,
    visibleValue: boolean | undefined,
  ): RawBrowserInput {
    const input: RawBrowserInput = {};
    if (kind) {
      input.kind = kind;
    }
    if (name) {
      input.name = name;
    }
    if (label) {
      input.label = label;
    }
    if (ariaLabel) {
      input.aria_label = ariaLabel;
    }
    if (title) {
      input.title = title;
    }
    if (placeholder) {
      input.placeholder = placeholder;
    }
    if (disabledValue !== undefined) {
      input.disabled = disabledValue;
    }
    if (visibleValue !== undefined) {
      input.visible = visibleValue;
    }
    return input;
  }

  function buttonText(element: Element): string | undefined {
    if (element instanceof HTMLInputElement) {
      return cleanText(element.value);
    }
    return cleanText(element.textContent);
  }

  function buttonFromElement(element: Element): RawBrowserButton {
    return makeButton(
      buttonText(element),
      optionalAttribute(element, "aria-label"),
      optionalAttribute(element, "title"),
      disabled(element),
      visible(element),
    );
  }

  function inputKind(element: Element): InputKind | undefined {
    if (element instanceof HTMLTextAreaElement) {
      return "textarea";
    }
    if (element instanceof HTMLSelectElement) {
      return "select";
    }
    if (element instanceof HTMLElement && element.isContentEditable) {
      return "content_editable";
    }
    if (!(element instanceof HTMLInputElement)) {
      return undefined;
    }

    switch (element.type) {
      case "search":
        return "search";
      case "email":
        return "email";
      case "password":
        return "password";
      case "number":
        return "number";
      case "tel":
        return "tel";
      case "url":
        return "url";
      case "checkbox":
        return "checkbox";
      case "radio":
        return "radio";
      default:
        return "text";
    }
  }

  function inputName(element: Element): string | undefined {
    if (
      element instanceof HTMLInputElement ||
      element instanceof HTMLTextAreaElement ||
      element instanceof HTMLSelectElement
    ) {
      return cleanText(element.name);
    }
    return optionalAttribute(element, "name");
  }

  function inputLabel(element: Element): string | undefined {
    if (
      element instanceof HTMLInputElement ||
      element instanceof HTMLTextAreaElement ||
      element instanceof HTMLSelectElement
    ) {
      const labels = Array.from(element.labels ?? []);
      const labelText = labels.map(labelTextWithoutNestedControls).find(Boolean);
      if (labelText) {
        return labelText;
      }
    }
    return undefined;
  }

  function labelTextWithoutNestedControls(label: HTMLLabelElement): string | undefined {
    const clone = label.cloneNode(true);
    if (!(clone instanceof HTMLElement)) {
      return undefined;
    }
    clone.querySelectorAll("button,input,select,textarea,[contenteditable]").forEach((control) => {
      control.remove();
    });
    return cleanText(clone.textContent);
  }

  function inputPlaceholder(element: Element): string | undefined {
    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
      return cleanText(element.placeholder);
    }
    return undefined;
  }

  function inputFromElement(element: Element): RawBrowserInput {
    return makeInput(
      inputKind(element),
      inputName(element),
      inputLabel(element),
      optionalAttribute(element, "aria-label"),
      optionalAttribute(element, "title"),
      inputPlaceholder(element),
      disabled(element),
      visible(element),
    );
  }

  function isCapturableInput(element: Element): boolean {
    return (
      element instanceof HTMLInputElement ||
      element instanceof HTMLTextAreaElement ||
      element instanceof HTMLSelectElement ||
      (element instanceof HTMLElement && element.isContentEditable)
    );
  }

  const buttonSelector = [
    "button",
    "[role='button']",
    "[role='menuitem']",
    "input[type='button']",
    "input[type='submit']",
    "input[type='reset']",
  ].join(",");
  const inputSelector = [
    "input:not([type='hidden']):not([type='button']):not([type='submit']):not([type='reset']):not([type='image'])",
    "textarea",
    "select",
    "[contenteditable]:not([contenteditable='false'])",
  ].join(",");
  const selectedText = cleanText(window.getSelection()?.toString());
  const buttons = Array.from(document.querySelectorAll(buttonSelector))
    .filter((element) => element instanceof HTMLElement && visible(element))
    .slice(0, maxControls)
    .map(buttonFromElement);
  const inputs = Array.from(document.querySelectorAll(inputSelector))
    .filter(
      (element) =>
        element instanceof HTMLElement && isCapturableInput(element) && visible(element),
    )
    .slice(0, maxControls)
    .map(inputFromElement);

  const capture: DomCapture = { buttons, inputs };
  if (selectedText) {
    capture.selectedText = selectedText;
  }
  return capture;
}
