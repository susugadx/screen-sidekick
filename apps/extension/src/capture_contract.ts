export const RAW_BROWSER_CONTEXT_SCHEMA_VERSION = "raw_browser_context.v0.1";
export const BRIDGE_CAPTURE_BODY_LIMIT_BYTES = 128 * 1024;
export const EXTENSION_CAPTURE_BODY_LIMIT_BYTES = 96 * 1024;

const MAX_CAPTURE_CONTROLS = 40;
const MAX_CAPTURE_FIELD_CHARS = 256;
const MAX_CAPTURE_SELECTED_TEXT_CHARS = 4096;
const MAX_CAPTURE_PAGE_URL_CHARS = 4096;
const MAX_CAPTURE_PAGE_TITLE_CHARS = 512;

export type InputKind =
  | "text"
  | "search"
  | "email"
  | "password"
  | "number"
  | "tel"
  | "url"
  | "checkbox"
  | "radio"
  | "select"
  | "textarea"
  | "content_editable";

export interface RawBrowserContext {
  schema_version: typeof RAW_BROWSER_CONTEXT_SCHEMA_VERSION;
  page?: RawBrowserPage;
  selected_text?: string;
  screenshot?: RawBrowserScreenshot;
  buttons?: RawBrowserButton[];
  inputs?: RawBrowserInput[];
}

export interface RawBrowserPage {
  url?: string;
  title?: string;
}

export interface RawBrowserScreenshot {
  format?: string;
  width?: number;
  height?: number;
  captured_at?: string;
}

export interface RawBrowserButton {
  text?: string;
  aria_label?: string;
  title?: string;
  disabled?: boolean;
  visible?: boolean;
}

export interface RawBrowserInput {
  kind?: InputKind;
  name?: string;
  label?: string;
  aria_label?: string;
  title?: string;
  placeholder?: string;
  disabled?: boolean;
  visible?: boolean;
}

export interface DomCapture {
  selectedText?: string;
  buttons: RawBrowserButton[];
  inputs: RawBrowserInput[];
}

export interface BridgeSettings {
  url: string;
  token: string;
}

export interface CaptureBridgeResponse {
  schema_version: string;
  screen_context_json: string;
  prompt_text: string;
  safety: SafetySummary;
}

export interface SafetySummary {
  has_danger: boolean;
  warning_count: number;
  warnings: SafetyWarning[];
  masked_input_values: number;
  masked_secret_texts: number;
}

export interface SafetyWarning {
  category: string;
  category_label: string;
  source: string;
  source_label: string;
}

export function serializeCaptureContextForBridge(context: RawBrowserContext): string {
  const limitedContext = limitCaptureContextForBridge(context);
  const body = JSON.stringify(limitedContext);
  if (encodedByteLength(body) > EXTENSION_CAPTURE_BODY_LIMIT_BYTES) {
    throw new Error("Captured context exceeds the bridge request budget");
  }
  return body;
}

export function limitCaptureContextForBridge(context: RawBrowserContext): RawBrowserContext {
  const limited = cloneWithFieldCaps(context);
  while (
    captureBodyByteLength(limited) > EXTENSION_CAPTURE_BODY_LIMIT_BYTES &&
    dropTrailingControl(limited)
  ) {
    continue;
  }

  if (captureBodyByteLength(limited) <= EXTENSION_CAPTURE_BODY_LIMIT_BYTES) {
    return limited;
  }

  delete limited.selected_text;
  if (captureBodyByteLength(limited) <= EXTENSION_CAPTURE_BODY_LIMIT_BYTES) {
    return limited;
  }

  if (limited.page) {
    delete limited.page.title;
  }
  if (captureBodyByteLength(limited) <= EXTENSION_CAPTURE_BODY_LIMIT_BYTES) {
    return limited;
  }

  delete limited.screenshot;
  if (captureBodyByteLength(limited) <= EXTENSION_CAPTURE_BODY_LIMIT_BYTES) {
    return limited;
  }

  if (limited.page?.url) {
    limited.page.url = truncateText(limited.page.url, 1024);
  }
  return limited;
}

export function captureBodyByteLength(context: RawBrowserContext): number {
  return encodedByteLength(JSON.stringify(context));
}

export function buildPage(
  url: string | undefined,
  title: string | undefined,
): RawBrowserPage | undefined {
  const page: RawBrowserPage = {};
  if (url) {
    page.url = url;
  }
  if (title) {
    page.title = title;
  }
  return page.url || page.title ? page : undefined;
}

export function buildButton(
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

export function buildInput(
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

function cloneWithFieldCaps(context: RawBrowserContext): RawBrowserContext {
  const limited: RawBrowserContext = {
    schema_version: RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
  };

  if (context.page) {
    const page = buildPage(
      truncateOptionalText(context.page.url, MAX_CAPTURE_PAGE_URL_CHARS),
      truncateOptionalText(context.page.title, MAX_CAPTURE_PAGE_TITLE_CHARS),
    );
    if (page) {
      limited.page = page;
    }
  }

  const selectedText = truncateOptionalText(
    context.selected_text,
    MAX_CAPTURE_SELECTED_TEXT_CHARS,
  );
  if (selectedText) {
    limited.selected_text = selectedText;
  }

  if (context.screenshot) {
    limited.screenshot = {
      ...context.screenshot,
    };
  }

  const buttons = (context.buttons ?? []).map((button) =>
    buildButton(
      truncateOptionalText(button.text, MAX_CAPTURE_FIELD_CHARS),
      truncateOptionalText(button.aria_label, MAX_CAPTURE_FIELD_CHARS),
      truncateOptionalText(button.title, MAX_CAPTURE_FIELD_CHARS),
      button.disabled,
      button.visible,
    ),
  );
  const inputs = (context.inputs ?? []).map((input) =>
    buildInput(
      input.kind,
      truncateOptionalText(input.name, MAX_CAPTURE_FIELD_CHARS),
      truncateOptionalText(input.label, MAX_CAPTURE_FIELD_CHARS),
      truncateOptionalText(input.aria_label, MAX_CAPTURE_FIELD_CHARS),
      truncateOptionalText(input.title, MAX_CAPTURE_FIELD_CHARS),
      truncateOptionalText(input.placeholder, MAX_CAPTURE_FIELD_CHARS),
      input.disabled,
      input.visible,
    ),
  );

  let remainingControls = MAX_CAPTURE_CONTROLS;
  limited.buttons = buttons.slice(0, remainingControls);
  remainingControls -= limited.buttons.length;
  limited.inputs = inputs.slice(0, remainingControls);

  return limited;
}

function dropTrailingControl(context: RawBrowserContext): boolean {
  const buttonCount = context.buttons?.length ?? 0;
  const inputCount = context.inputs?.length ?? 0;

  if (buttonCount === 0 && inputCount === 0) {
    return false;
  }

  if (inputCount >= buttonCount && context.inputs) {
    context.inputs.pop();
    return true;
  }

  context.buttons?.pop();
  return true;
}

function truncateOptionalText(value: string | undefined, maxChars: number): string | undefined {
  if (!value) {
    return undefined;
  }
  return truncateText(value, maxChars);
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

function encodedByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}
