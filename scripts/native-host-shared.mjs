import { Buffer } from "node:buffer";

export const HOST_NAME = "com.screen_sidekick.host";
export const DESCRIPTION = "Screen Sidekick Native Messaging Host";
export const CONFIG_SCHEMA_VERSION = "screen_sidekick_native_host_config.v0.1";
export const CONFIG_ENV = "SCREEN_SIDEKICK_NATIVE_HOST_CONFIG";
export const BROWSERS = new Set(["chrome", "chrome-for-testing", "chromium", "edge"]);
export const DAEMON_STATUS_SCHEMA_VERSION = "sidekick_daemon_status.v0.1";

const DAEMON_STATUS_MAX_LINE_BYTES = 8 * 1024;

export function browserError(browser) {
  return !browser || !BROWSERS.has(browser) ? "missing or invalid --browser" : null;
}

export function extensionIdError(extensionId) {
  return !extensionId || !/^[a-p]{32}$/.test(extensionId)
    ? "--extension-id must be a 32-character Chrome extension ID"
    : null;
}

export function wslDistroError(value) {
  return value.trim() !== value ||
    value.length === 0 ||
    value.length > 128 ||
    /[/"'\\\x00-\x1f\x7f]/.test(value)
    ? "--wsl-distro is invalid"
    : null;
}

export function linuxPathError(value, option, allowRoot) {
  return value.trim() !== value ||
    !value.startsWith("/") ||
    (!allowRoot && value === "/") ||
    value.includes("\\") ||
    /[\x00-\x1f\x7f]/.test(value) ||
    value.split("/").includes("..")
    ? `${option} must be an absolute Linux path without parent traversal`
    : null;
}

export function linuxPathListError(value, option) {
  if (value.trim() !== value || value.length === 0 || value.includes("\\") || /[\x00-\x1f\x7f]/.test(value)) {
    return `${option} must be a colon-separated list of absolute Linux paths without parent traversal`;
  }
  const parts = value.split(":");
  if (parts.length === 0 || parts.some((part) => part.length === 0 || linuxPathError(part, option, false))) {
    return `${option} must be a colon-separated list of absolute Linux paths without parent traversal`;
  }
  return null;
}

export function validateWslAutoConfigValue(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { ok: false, error: "config must be a JSON object" };
  }

  const allowedFields = new Set([
    "schema_version",
    "mode",
    "wsl_distro",
    "wsl_workdir",
    "wsl_daemon_binary",
    "wsl_path",
  ]);
  for (const key of Object.keys(value)) {
    if (!allowedFields.has(key)) {
      return { ok: false, error: `config contains unknown field: ${key}` };
    }
  }

  if (value.schema_version !== CONFIG_SCHEMA_VERSION) {
    return { ok: false, error: "config schema_version is unsupported" };
  }
  if (value.mode !== "wsl_auto") {
    return { ok: false, error: "config mode is unsupported" };
  }

  const distro = requiredConfigString(value, "wsl_distro");
  if (!distro.ok) {
    return distro;
  }
  const workdir = requiredConfigString(value, "wsl_workdir");
  if (!workdir.ok) {
    return workdir;
  }
  const daemonBinary = requiredConfigString(value, "wsl_daemon_binary");
  if (!daemonBinary.ok) {
    return daemonBinary;
  }

  if (wslDistroError(distro.value)) {
    return { ok: false, error: "config wsl_distro is invalid" };
  }
  const workdirError = linuxPathError(workdir.value, "wsl_workdir", true);
  if (workdirError) {
    return { ok: false, error: `config ${workdirError}` };
  }
  const daemonBinaryError = linuxPathError(daemonBinary.value, "wsl_daemon_binary", false);
  if (daemonBinaryError) {
    return { ok: false, error: `config ${daemonBinaryError}` };
  }
  let wslPath = null;
  if (value.wsl_path !== undefined) {
    if (typeof value.wsl_path !== "string" || value.wsl_path.trim().length === 0) {
      return { ok: false, error: "config wsl_path is invalid" };
    }
    const wslPathError = linuxPathListError(value.wsl_path, "wsl_path");
    if (wslPathError) {
      return { ok: false, error: `config ${wslPathError}` };
    }
    wslPath = value.wsl_path;
  }

  return {
    ok: true,
    config: {
      wslDistro: distro.value,
      wslWorkdir: workdir.value,
      wslDaemonBinary: daemonBinary.value,
      wslPath,
    },
  };
}

export function validateDaemonStatusOutput(stdout) {
  const statusLine = firstDaemonStatusLine(stdout);
  if (!statusLine.ok) {
    return statusLine;
  }

  let value;
  try {
    value = JSON.parse(statusLine.line.trimEnd());
  } catch {
    return { ok: false, error: "status line is not valid JSON" };
  }

  return validateDaemonStatusValue(value);
}

function firstDaemonStatusLine(stdout) {
  if (typeof stdout !== "string") {
    return { ok: false, error: "status stdout is invalid" };
  }
  const newlineIndex = stdout.indexOf("\n");
  const candidate = newlineIndex === -1 ? stdout : stdout.slice(0, newlineIndex + 1);
  if (Buffer.byteLength(candidate, "utf8") > DAEMON_STATUS_MAX_LINE_BYTES) {
    return { ok: false, error: "status line is too large" };
  }
  if (newlineIndex === -1) {
    return { ok: false, error: "status line is not newline terminated" };
  }
  return { ok: true, line: candidate };
}

function validateDaemonStatusValue(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { ok: false, error: "status must be a JSON object" };
  }
  if (value.schema_version !== DAEMON_STATUS_SCHEMA_VERSION) {
    return { ok: false, error: "unexpected status schema_version" };
  }

  const wsUrl = requiredStatusString(value, "ws_url");
  if (!wsUrl.ok) {
    return wsUrl;
  }
  const wsUrlError = daemonWsUrlError(wsUrl.value);
  if (wsUrlError) {
    return { ok: false, error: wsUrlError };
  }

  const token = requiredStatusString(value, "token");
  if (!token.ok) {
    return token;
  }
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(token.value)) {
    return { ok: false, error: "status token is invalid" };
  }

  return { ok: true };
}

function requiredStatusString(value, field) {
  if (typeof value[field] !== "string" || value[field].length === 0) {
    return { ok: false, error: `status ${field} is missing` };
  }
  return { ok: true, value: value[field] };
}

function daemonWsUrlError(value) {
  let url;
  try {
    url = new URL(value);
  } catch {
    return "status ws_url is invalid";
  }
  if (
    url.protocol !== "ws:" ||
    url.hostname !== "127.0.0.1" ||
    url.port === "" ||
    url.pathname !== "/v0/ws" ||
    url.search !== "" ||
    url.hash !== ""
  ) {
    return "status ws_url is not sidecar loopback WebSocket endpoint";
  }
  return null;
}

function requiredConfigString(value, field) {
  if (typeof value[field] !== "string" || value[field].trim().length === 0) {
    return { ok: false, error: `config is missing ${field}` };
  }
  return { ok: true, value: value[field] };
}

export function isWindowsAbsolutePath(filePath) {
  return /^[A-Za-z]:[\\/]/.test(filePath) || /^\\\\/.test(filePath);
}

export function joinWindowsPath(...parts) {
  return parts.join("\\");
}

export function windowsRegistryKey(browser) {
  const roots = {
    chrome: "Google\\Chrome",
    "chrome-for-testing": "Google\\ChromeForTesting",
    chromium: "Chromium",
    edge: "Microsoft\\Edge",
  };
  return `HKCU\\Software\\${roots[browser]}\\NativeMessagingHosts\\${HOST_NAME}`;
}
