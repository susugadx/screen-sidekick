import {
  serializeCaptureContextForBridge,
  type CaptureBridgeResponse,
  type RawBrowserContext,
  type SafetySummary,
  type SafetyWarning,
} from "./capture_contract.js";
import { buildDaemonCaptureUrl, type DaemonSettings } from "./sidekick_protocol.js";

const FETCH_TIMEOUT_MS = 10_000;
const MAX_BRIDGE_RESPONSE_CHARS = 512 * 1024;
const STATIC_BRIDGE_REJECTION_MESSAGES = new Set([
  "extension Origin header is required",
  "only chrome-extension origins are allowed",
  "invalid CORS preflight",
  "bearer token is required",
  "bearer token is invalid",
  "capture request JSON is invalid",
  "capture request is invalid",
  "failed to serialize capture response",
]);

export async function postCaptureToBridge(
  settings: DaemonSettings,
  context: RawBrowserContext,
): Promise<CaptureBridgeResponse> {
  return postCapture(buildDaemonCaptureUrl(settings.url), settings.token, context);
}

export function formatSafetySummary(safety: SafetySummary): string {
  const warningLines =
    safety.warnings.length === 0
      ? ["warnings: none"]
      : safety.warnings.map(
          (warning) => `warning: ${warning.category_label} (${warning.source_label})`,
        );

  return [
    `danger: ${safety.has_danger ? "yes" : "no"}`,
    `warning_count: ${safety.warning_count}`,
    `masked_input_values: ${safety.masked_input_values}`,
    `masked_secret_texts: ${safety.masked_secret_texts}`,
    ...warningLines,
  ].join("\n");
}

async function postCapture(
  endpoint: URL,
  token: string,
  context: RawBrowserContext,
): Promise<CaptureBridgeResponse> {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);

  try {
    const body = serializeCaptureContextForBridge(context);
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body,
      signal: controller.signal,
    });
    const text = await response.text();

    if (text.length > MAX_BRIDGE_RESPONSE_CHARS) {
      throw new Error("Daemon response is too large");
    }

    if (!response.ok) {
      throw new Error(formatDaemonRejectionStatus(response.status, text));
    }

    const payload: unknown = JSON.parse(text);
    const parsed = parseCaptureBridgeResponse(payload);
    if (!parsed) {
      throw new Error("Daemon response shape is invalid");
    }
    return parsed;
  } finally {
    window.clearTimeout(timeout);
  }
}

function formatDaemonRejectionStatus(status: number, body: string): string {
  const trimmedBody = body.trim();
  if (STATIC_BRIDGE_REJECTION_MESSAGES.has(trimmedBody)) {
    return `Daemon rejected capture (${status}): ${trimmedBody}`;
  }
  return `Daemon rejected capture (${status})`;
}

function parseCaptureBridgeResponse(value: unknown): CaptureBridgeResponse | null {
  if (!isRecord(value)) {
    return null;
  }

  const schemaVersion = getString(value, "schema_version");
  const screenContextJson = getString(value, "screen_context_json");
  const promptText = getString(value, "prompt_text");
  const safety = parseSafetySummary(value.safety);

  if (!schemaVersion || !screenContextJson || !promptText || !safety) {
    return null;
  }

  return {
    schema_version: schemaVersion,
    screen_context_json: screenContextJson,
    prompt_text: promptText,
    safety,
  };
}

function parseSafetySummary(value: unknown): SafetySummary | null {
  if (!isRecord(value)) {
    return null;
  }

  const hasDanger = getBoolean(value, "has_danger");
  const warningCount = getNumber(value, "warning_count");
  const maskedInputValues = getNumber(value, "masked_input_values");
  const maskedSecretTexts = getNumber(value, "masked_secret_texts");
  const warnings = parseSafetyWarnings(value.warnings);

  if (
    hasDanger === null ||
    warningCount === null ||
    maskedInputValues === null ||
    maskedSecretTexts === null ||
    !warnings
  ) {
    return null;
  }

  return {
    has_danger: hasDanger,
    warning_count: warningCount,
    warnings,
    masked_input_values: maskedInputValues,
    masked_secret_texts: maskedSecretTexts,
  };
}

function parseSafetyWarnings(value: unknown): SafetyWarning[] | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const warnings: SafetyWarning[] = [];
  for (const item of value) {
    if (!isRecord(item)) {
      return null;
    }
    const category = getString(item, "category");
    const categoryLabel = getString(item, "category_label");
    const source = getString(item, "source");
    const sourceLabel = getString(item, "source_label");
    if (!category || !categoryLabel || !source || !sourceLabel) {
      return null;
    }
    warnings.push({
      category,
      category_label: categoryLabel,
      source,
      source_label: sourceLabel,
    });
  }
  return warnings;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function getString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function getBoolean(record: Record<string, unknown>, key: string): boolean | null {
  const value = record[key];
  return typeof value === "boolean" ? value : null;
}

function getNumber(record: Record<string, unknown>, key: string): number | null {
  const value = record[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
