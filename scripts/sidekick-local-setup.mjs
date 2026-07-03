#!/usr/bin/env node
import { existsSync, readFileSync, rmSync } from "node:fs";
import { platform } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  CONFIG_SCHEMA_VERSION,
  CONFIG_ENV,
  DESCRIPTION,
  HOST_NAME,
  PRINT_CONFIG_SCHEMA_VERSION_ARG,
  browserError,
  extensionIdError,
  hostSchemaProbeEnv,
  isWindowsAbsolutePath,
  joinWindowsPath,
  linuxPathError,
  linuxPathListError,
  validateDaemonStatusOutput,
  validateWslAutoConfigValue,
  windowsRegistryKey,
  wslDistroError,
} from "./native-host-shared.mjs";

const EXTENSION_ID_ENV = "SCREEN_SIDEKICK_EXTENSION_ID";
const WINDOWS_HOST_PATH_ENV = "SCREEN_SIDEKICK_WINDOWS_HOST_PATH";
const WSL_DISTRO_ENV = "SCREEN_SIDEKICK_WSL_DISTRO";
const WSL_WORKDIR_ENV = "SCREEN_SIDEKICK_WSL_WORKDIR";
const WSL_DAEMON_BINARY_ENV = "SCREEN_SIDEKICK_WSL_DAEMON_BINARY";
const WSL_PATH_ENV = "SCREEN_SIDEKICK_WSL_PATH";
const MAX_COMMAND_OUTPUT_BYTES = 64 * 1024;

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const nativeHostDevScript = join(repoRoot, "scripts", "native-host-dev.mjs");

class UsageError extends Error {}

class CommandExit extends Error {
  constructor(status) {
    super(`command exited with status ${status}`);
    this.status = status;
  }
}

function createRealRuntime() {
  return {
    env: process.env,
    execPath: process.execPath,
    platform: () => platform(),
    existsSync,
    readFileSync,
    rmSync,
    spawnSync,
    stdout: (line) => console.log(line),
    stderr: (line) => console.error(line),
  };
}

export function runSidekickLocalSetup(argv, runtime = createRealRuntime()) {
  try {
    return main(argv, runtime);
  } catch (error) {
    if (error instanceof UsageError) {
      printUsage(runtime, error.message);
      return 2;
    }
    if (error instanceof CommandExit) {
      return error.status;
    }
    throw error;
  }
}

function main(argv, runtime) {
  const [command, ...rest] = argv;
  const options = parseArgs(rest);
  switch (command) {
    case "install":
      installLocal(options, runtime);
      return 0;
    case "doctor":
      return doctorLocal(options, runtime);
    case "uninstall":
      return uninstallLocal(options, runtime);
    default:
      usage(command ? `unknown command: ${command}` : null);
  }
}

function installLocal(options, runtime) {
  const config = resolveInstallConfig(options, runtime);
  ensureWindowsWriteAllowed(options.dryRun, runtime);

  if (!options.skipBuild) {
    runLocalBuilds(config, options.dryRun, runtime);
  }

  runNativeHostDev([
    "install",
    "--browser",
    config.browser,
    "--extension-id",
    config.extensionId,
    "--host-path",
    config.hostPath,
    "--wsl-distro",
    config.wslDistro,
    "--wsl-workdir",
    config.wslWorkdir,
    "--wsl-daemon-binary",
    config.wslDaemonBinary,
    ...(config.wslPath ? ["--wsl-path", config.wslPath] : []),
    ...(options.dryRun ? ["--dry-run"] : []),
  ], runtime);
}

function doctorLocal(options, runtime) {
  const config = resolveDoctorConfig(options, runtime);
  const checks = [];

  checks.push(ok("browser option", config.browser));
  if (config.expectedWslDistro) {
    checks.push(ok("WSL distro comparison", config.expectedWslDistro));
  } else {
    checks.push(skip("WSL distro comparison", `set --wsl-distro or ${WSL_DISTRO_ENV}`));
  }
  if (config.expectedWslWorkdir) {
    checks.push(ok("WSL workdir comparison", config.expectedWslWorkdir));
  } else {
    checks.push(skip("WSL workdir comparison", `set --wsl-workdir or ${WSL_WORKDIR_ENV}`));
  }
  if (config.expectedWslDaemonBinary) {
    checks.push(ok("WSL daemon binary comparison", config.expectedWslDaemonBinary));
  } else {
    checks.push(skip("WSL daemon binary comparison", `set --wsl-daemon-binary or ${WSL_DAEMON_BINARY_ENV}`));
  }
  if (config.expectedWslPath) {
    checks.push(ok("WSL PATH comparison", config.expectedWslPath));
  } else {
    checks.push(skip("WSL PATH comparison", `set --wsl-path or ${WSL_PATH_ENV}`));
  }
  if (config.extensionId) {
    checks.push(ok("extension ID comparison", config.extensionId));
  } else {
    checks.push(skip("extension ID comparison", `set --extension-id or ${EXTENSION_ID_ENV}`));
  }
  if (config.hostPath) {
    checks.push(ok("Windows native host path option", config.hostPath));
  } else {
    checks.push(skip("Windows native host path check", `set --host-path or ${WINDOWS_HOST_PATH_ENV}`));
  }

  if (options.dryRun) {
    checks.push(skip("process checks", "dry run"));
    printChecks(checks, runtime);
    return 0;
  }

  if (runtime.platform() === "win32") {
    checks.push(checkCommandAvailable("wsl.exe", ["--status"], runtime));
    const manifestResult = checkWindowsRegistryManifest(config, runtime);
    checks.push(manifestResult.check);
    const configResult = checkWindowsConfig(config, runtime);
    checks.push(configResult.check);
    if (configResult.wslConfig) {
      if (requiresWindowsHostConfigSchemaProbe(configResult.wslConfig)) {
        checks.push(checkWindowsHostConfigSchema(manifestResult.hostPath, CONFIG_SCHEMA_VERSION, runtime));
      }
      checks.push(checkWslCodex(configResult.wslConfig, runtime));
      checks.push(checkWslExtensionBuildOutput(configResult.wslConfig, runtime));
      checks.push(checkWslDaemonStatus(configResult.wslConfig, runtime));
    } else {
      checks.push(skip("Codex CLI in WSL", "native host config must be valid first"));
      checks.push(skip("extension build output in WSL", "native host config must be valid first"));
      checks.push(skip("WSL daemon stdio status", "native host config must be valid first"));
    }
  } else {
    checks.push(checkCommandAvailable("cargo", ["--version"], runtime));
    checks.push(checkCommandAvailable("npm", ["--version"], runtime));
    checks.push(checkPath("extension package", localWorkdirPath(config, "apps/extension/package.json"), runtime));
    checks.push(checkPath("extension lockfile", localWorkdirPath(config, "apps/extension/package-lock.json"), runtime));
    checks.push(checkPath("extension build output", localWorkdirPath(config, "apps/extension/dist/side_panel.js"), runtime));
    checks.push(checkPath("WSL daemon binary", config.localDaemonBinary, runtime));
    checks.push(checkLocalCodex(runtime));
    checks.push(checkLocalDaemonStatus({ wslDaemonBinary: config.localDaemonBinary }, runtime));
    checks.push(skip("Windows registry manifest", "run doctor from Windows to verify HKCU registration"));
    checks.push(skip("Windows native host config", "run doctor from Windows to verify APPDATA config"));
  }

  printChecks(checks, runtime);
  if (checks.some((check) => check.status === "fail")) {
    return 1;
  }
  return 0;
}

function uninstallLocal(options, runtime) {
  const config = resolveUninstallConfig(options);
  ensureWindowsWriteAllowed(options.dryRun, runtime);

  const unregisterStatus = runNativeHostDevStatus([
    "uninstall",
    "--browser",
    config.browser,
    "--target-platform",
    "win32",
    ...(options.dryRun ? ["--dry-run"] : []),
  ], runtime);

  if (options.keepConfig) {
    return unregisterStatus;
  }
  removeWindowsConfig(options.dryRun, runtime);
  return unregisterStatus;
}

function runLocalBuilds(config, dryRun, runtime) {
  if (runtime.platform() === "win32") {
    runRequired(
      "build WSL daemon",
      [
        "wsl.exe",
        ...wslBuildArgs(config, [
          "cargo",
          "build",
          "-p",
          "screen-sidekick-sidekick-daemon",
          "--bin",
          "screen-sidekick-daemon",
        ]),
      ],
      dryRun,
      runtime,
    );
    runRequired(
      "install extension dependencies in WSL",
      ["wsl.exe", ...wslBuildArgs(config, ["npm", "ci", "--prefix", "apps/extension"])],
      dryRun,
      runtime,
    );
    runRequired(
      "build extension in WSL",
      ["wsl.exe", ...wslBuildArgs(config, ["npm", "--prefix", "apps/extension", "run", "build"])],
      dryRun,
      runtime,
    );
    return;
  }

  runRequired(
    "build WSL daemon",
    ["cargo", "build", "-p", "screen-sidekick-sidekick-daemon", "--bin", "screen-sidekick-daemon"],
    dryRun,
    runtime,
  );
  runRequired(
    "install extension dependencies",
    ["npm", "ci", "--prefix", "apps/extension"],
    dryRun,
    runtime,
  );
  runRequired(
    "build extension",
    ["npm", "--prefix", "apps/extension", "run", "build"],
    dryRun,
    runtime,
  );
}

function resolveInstallConfig(options, runtime) {
  const browser = requireBrowser(options.browser);
  const extensionId = options.extensionId ?? runtime.env[EXTENSION_ID_ENV] ?? null;
  requireExtensionId(extensionId);

  const hostPath = options.hostPath ?? runtime.env[WINDOWS_HOST_PATH_ENV] ?? null;
  if (!hostPath) {
    usage(`--host-path or ${WINDOWS_HOST_PATH_ENV} is required for Windows Chrome/Edge setup`);
  }
  if (hostPath && !isWindowsAbsolutePath(hostPath)) {
    usage("--host-path must be an absolute Windows path");
  }

  const wslDistro = options.wslDistro ?? runtime.env[WSL_DISTRO_ENV] ?? "Ubuntu";
  validateWslDistro(wslDistro);

  const defaultWslWorkdir = repoRoot.startsWith("/") ? repoRoot : null;
  const wslWorkdir = options.wslWorkdir ?? runtime.env[WSL_WORKDIR_ENV] ?? defaultWslWorkdir;
  if (!wslWorkdir) {
    usage(`--wsl-workdir or ${WSL_WORKDIR_ENV} is required when running from Windows`);
  }
  validateLinuxPath(wslWorkdir, "--wsl-workdir", true);

  const wslDaemonBinary =
    options.wslDaemonBinary ??
    runtime.env[WSL_DAEMON_BINARY_ENV] ??
    joinLinuxPath(wslWorkdir, "target", "debug", "screen-sidekick-daemon");
  validateLinuxPath(wslDaemonBinary, "--wsl-daemon-binary", false);

  const wslPath = options.wslPath ?? runtime.env[WSL_PATH_ENV] ?? null;
  if (wslPath) {
    validateLinuxPathList(wslPath, "--wsl-path");
  }

  return {
    browser,
    extensionId,
    hostPath,
    wslDistro,
    wslWorkdir,
    wslDaemonBinary,
    wslPath,
  };
}

function resolveDoctorConfig(options, runtime) {
  const browser = requireBrowser(options.browser);
  const extensionId = options.extensionId ?? runtime.env[EXTENSION_ID_ENV] ?? null;
  if (extensionId) {
    requireExtensionId(extensionId);
  }

  const hostPath = options.hostPath ?? runtime.env[WINDOWS_HOST_PATH_ENV] ?? null;
  if (hostPath && !isWindowsAbsolutePath(hostPath)) {
    usage("--host-path must be an absolute Windows path");
  }

  const expectedWslDistro = options.wslDistro ?? runtime.env[WSL_DISTRO_ENV] ?? null;
  if (expectedWslDistro) {
    validateWslDistro(expectedWslDistro);
  }
  const expectedWslWorkdir = options.wslWorkdir ?? runtime.env[WSL_WORKDIR_ENV] ?? null;
  if (expectedWslWorkdir) {
    validateLinuxPath(expectedWslWorkdir, "--wsl-workdir", true);
  }
  const expectedWslDaemonBinary = options.wslDaemonBinary ?? runtime.env[WSL_DAEMON_BINARY_ENV] ?? null;
  if (expectedWslDaemonBinary) {
    validateLinuxPath(expectedWslDaemonBinary, "--wsl-daemon-binary", false);
  }
  const expectedWslPath = options.wslPath ?? runtime.env[WSL_PATH_ENV] ?? null;
  if (expectedWslPath) {
    validateLinuxPathList(expectedWslPath, "--wsl-path");
  }

  const defaultWslWorkdir = repoRoot.startsWith("/") ? repoRoot : null;
  const localWorkdir = expectedWslWorkdir ?? defaultWslWorkdir;
  const localDaemonBinary =
    expectedWslDaemonBinary ??
    (localWorkdir ? joinLinuxPath(localWorkdir, "target", "debug", "screen-sidekick-daemon") : null);

  return {
    browser,
    extensionId,
    hostPath,
    expectedWslDistro,
    expectedWslWorkdir,
    expectedWslDaemonBinary,
    expectedWslPath,
    localWorkdir,
    localDaemonBinary,
  };
}

function resolveUninstallConfig(options) {
  return {
    browser: requireBrowser(options.browser),
  };
}

function checkWindowsRegistryManifest(config, runtime) {
  const location = windowsRegistryKey(config.browser);
  const query = runCapture("reg", ["query", location, "/ve"], { timeoutMs: 10_000 }, runtime);
  if (query.status !== 0) {
    return { check: fail("Windows registry manifest", "native host registry entry is missing") };
  }
  const manifestPath = parseRegDefaultValue(query.stdout);
  if (!manifestPath) {
    return { check: fail("Windows registry manifest", "registry entry did not contain a default REG_SZ path") };
  }
  if (!isWindowsAbsolutePath(manifestPath)) {
    return { check: fail("Windows registry manifest", `registry manifest path is not absolute: ${manifestPath}`) };
  }
  if (!runtime.existsSync(manifestPath)) {
    return { check: fail("Windows registry manifest", `manifest does not exist: ${manifestPath}`) };
  }
  const manifest = readJson(manifestPath, runtime);
  if (!manifest.ok) {
    return { check: fail("Windows registry manifest", manifest.error) };
  }
  if (
    manifest.value.name !== HOST_NAME ||
    manifest.value.description !== DESCRIPTION ||
    manifest.value.type !== "stdio"
  ) {
    return { check: fail("Windows registry manifest", "manifest name/type/description is invalid") };
  }
  const hostPath = manifest.value.path;
  if (typeof hostPath !== "string") {
    return { check: fail("Windows registry manifest", "manifest path is missing or not a string") };
  }
  if (!isWindowsAbsolutePath(hostPath)) {
    return { check: fail("Windows registry manifest", `manifest path is not an absolute Windows path: ${hostPath}`) };
  }
  if (!runtime.existsSync(hostPath)) {
    return { check: fail("Windows registry manifest", `manifest path does not exist: ${hostPath}`) };
  }
  if (config.hostPath && !sameWindowsPath(hostPath, config.hostPath)) {
    return { check: fail("Windows registry manifest", `manifest path does not match expected host path: ${hostPath}`) };
  }
  if (config.extensionId) {
    const expectedOrigin = `chrome-extension://${config.extensionId}/`;
    if (!allowedOriginsExactlyMatch(manifest.value.allowed_origins, expectedOrigin)) {
      return { check: fail("Windows registry manifest", `allowed_origins must exactly match ${expectedOrigin}`) };
    }
  }
  return { check: ok("Windows registry manifest", manifestPath), hostPath };
}

function allowedOriginsExactlyMatch(value, expectedOrigin) {
  return Array.isArray(value) && value.length === 1 && value[0] === expectedOrigin;
}

function checkWindowsConfig(config, runtime) {
  const configPath = windowsConfigPath(runtime);
  if (!configPath) {
    return { check: fail("Windows native host config", `APPDATA or ${CONFIG_ENV} is required`) };
  }
  if (!runtime.existsSync(configPath)) {
    return { check: fail("Windows native host config", `config does not exist: ${configPath}`) };
  }
  const parsed = readJson(configPath, runtime);
  if (!parsed.ok) {
    return { check: fail("Windows native host config", parsed.error) };
  }
  const validated = validateWslAutoConfigValue(parsed.value);
  if (!validated.ok) {
    return { check: fail("Windows native host config", validated.error) };
  }
  const mismatch = compareExpectedWslConfig(validated.config, config);
  if (mismatch) {
    return { check: fail("Windows native host config", mismatch) };
  }
  return { check: ok("Windows native host config", configPath), wslConfig: validated.config };
}

function requiresWindowsHostConfigSchemaProbe(wslConfig) {
  return wslConfig.schemaVersion === CONFIG_SCHEMA_VERSION || Boolean(wslConfig.wslPath);
}

function checkWindowsHostConfigSchema(hostPath, schemaVersion, runtime) {
  if (!hostPath) {
    return skip("Windows native host config schema compatibility", "registry manifest must be valid first");
  }
  const result = runCapture(hostPath, [PRINT_CONFIG_SCHEMA_VERSION_ARG], {
    env: hostSchemaProbeEnv(runtime.env),
    timeoutMs: 5_000,
  }, runtime);
  const observed = firstLine(result.stdout);
  if (result.status !== 0 || observed !== schemaVersion) {
    return fail(
      "Windows native host config schema compatibility",
      `host binary must report ${schemaVersion} before using this config`,
    );
  }
  return ok("Windows native host config schema compatibility", observed);
}

function checkWslCodex(config, runtime) {
  const result = runCapture("wsl.exe", wslCodexArgs(config), {
    timeoutMs: 10_000,
  }, runtime);
  if (result.status !== 0) {
    return fail("Codex CLI in WSL", "codex --version failed through wsl.exe");
  }
  return ok("Codex CLI in WSL", firstLine(result.stdout));
}

function checkLocalCodex(runtime) {
  const result = runCapture("codex", ["--version"], { timeoutMs: 10_000 }, runtime);
  if (result.status !== 0) {
    return fail("Codex CLI", "codex --version failed");
  }
  return ok("Codex CLI", firstLine(result.stdout));
}

function checkWslExtensionBuildOutput(config, runtime) {
  const result = runCapture(
    "wsl.exe",
    wslExecArgs(config, ["test", "-f", "apps/extension/dist/side_panel.js"]),
    { timeoutMs: 10_000 },
    runtime,
  );
  if (result.status !== 0) {
    return fail("extension build output in WSL", "missing apps/extension/dist/side_panel.js");
  }
  return ok("extension build output in WSL", joinLinuxPath(config.wslWorkdir, "apps/extension/dist/side_panel.js"));
}

function checkWslDaemonStatus(config, runtime) {
  const result = runCapture(
    "wsl.exe",
    wslExecArgs(config, [config.wslDaemonBinary, "--stdio-status"]),
    { input: "", timeoutMs: 15_000 },
    runtime,
  );
  return parseDaemonStatusCheck("WSL daemon stdio status", result);
}

function wslExecArgs(config, command) {
  const commandArgs = config.wslPath ? ["env", `PATH=${config.wslPath}`, ...command] : command;
  return ["-d", config.wslDistro, "--cd", config.wslWorkdir, "--exec", ...commandArgs];
}

function wslBuildArgs(config, command) {
  if (config.wslPath) {
    return wslExecArgs(config, command);
  }
  return ["-d", config.wslDistro, "--cd", config.wslWorkdir, "--", ...command];
}

function wslCodexArgs(config) {
  if (config.wslPath) {
    return wslExecArgs(config, ["codex", "--version"]);
  }
  return ["-d", config.wslDistro, "--", "codex", "--version"];
}

function checkLocalDaemonStatus(config, runtime) {
  const result = runCapture(config.wslDaemonBinary, ["--stdio-status"], {
    input: "",
    timeoutMs: 15_000,
  }, runtime);
  return parseDaemonStatusCheck("daemon stdio status", result);
}

function parseDaemonStatusCheck(name, result) {
  if (result.status !== 0) {
    return fail(name, "daemon --stdio-status failed");
  }
  const status = validateDaemonStatusOutput(result.stdout);
  return status.ok ? ok(name, "status line parsed") : fail(name, status.error);
}

function compareExpectedWslConfig(actual, expected) {
  const mismatches = [];
  if (expected.expectedWslDistro && expected.expectedWslDistro !== actual.wslDistro) {
    mismatches.push("wsl_distro");
  }
  if (expected.expectedWslWorkdir && expected.expectedWslWorkdir !== actual.wslWorkdir) {
    mismatches.push("wsl_workdir");
  }
  if (expected.expectedWslDaemonBinary && expected.expectedWslDaemonBinary !== actual.wslDaemonBinary) {
    mismatches.push("wsl_daemon_binary");
  }
  if (expected.expectedWslPath && expected.expectedWslPath !== actual.wslPath) {
    mismatches.push("wsl_path");
  }
  if (mismatches.length > 0) {
    return `config does not match expected WSL settings: ${mismatches.join(", ")}`;
  }
  return null;
}

function sameWindowsPath(left, right) {
  return normalizeWindowsPathForComparison(left) === normalizeWindowsPathForComparison(right);
}

function normalizeWindowsPathForComparison(value) {
  return value.replace(/\//g, "\\").toLowerCase();
}

function removeWindowsConfig(dryRun, runtime) {
  const configPath = windowsConfigPath(runtime, dryRun);
  if (!configPath) {
    usage(`APPDATA or ${CONFIG_ENV} is required to remove the Windows native host config`);
  }
  if (dryRun) {
    runtime.stdout(`Would remove ${configPath}`);
    return;
  }
  runtime.rmSync(configPath, { force: true });
  runtime.stdout(`Removed ${configPath}`);
}

function windowsConfigPath(runtime, dryRun = false) {
  if (runtime.env[CONFIG_ENV]) {
    return runtime.env[CONFIG_ENV];
  }
  if (runtime.env.APPDATA) {
    return joinWindowsPath(runtime.env.APPDATA, "Screen Sidekick", "native-host-config.json");
  }
  return dryRun ? joinWindowsPath("%APPDATA%", "Screen Sidekick", "native-host-config.json") : null;
}

function runNativeHostDev(args, runtime) {
  runRequired("native host dev helper", [runtime.execPath, nativeHostDevScript, ...args], false, runtime);
}

function runNativeHostDevStatus(args, runtime) {
  return runCommandStatus([runtime.execPath, nativeHostDevScript, ...args], runtime);
}

function runRequired(label, command, dryRun, runtime) {
  if (dryRun) {
    runtime.stdout(`Would run (${label}): ${formatCommand(command)}`);
    return;
  }
  const status = runCommandStatus(command, runtime);
  if (status !== 0) {
    throw new CommandExit(status);
  }
}

function runCommandStatus(command, runtime) {
  const result = runtime.spawnSync(command[0], command.slice(1), {
    stdio: "inherit",
    shell: false,
  });
  return typeof result.status === "number" ? result.status : 1;
}

function runCapture(command, args, options = {}, runtime) {
  try {
    const result = runtime.spawnSync(command, args, {
      encoding: "utf8",
      input: options.input,
      env: options.env,
      maxBuffer: MAX_COMMAND_OUTPUT_BYTES,
      shell: false,
      timeout: options.timeoutMs ?? 10_000,
    });
    return {
      status: result.status ?? (result.error ? 1 : 0),
      stdout: result.stdout ?? "",
      stderr: result.stderr ?? "",
      error: result.error,
    };
  } catch (error) {
    return { status: 1, stdout: "", stderr: "", error };
  }
}

function checkCommandAvailable(command, args, runtime) {
  const result = runCapture(command, args, { timeoutMs: 10_000 }, runtime);
  if (result.status !== 0) {
    return fail(`${command} available`, `${formatCommand([command, ...args])} failed`);
  }
  return ok(`${command} available`, firstLine(result.stdout));
}

function checkPath(name, path, runtime) {
  return path && runtime.existsSync(path) ? ok(name, path) : fail(name, `missing: ${path}`);
}

function localWorkdirPath(config, relativePath) {
  return config.localWorkdir ? joinLinuxPath(config.localWorkdir, relativePath) : null;
}

function readJson(path, runtime) {
  try {
    return { ok: true, value: JSON.parse(runtime.readFileSync(path, "utf8")) };
  } catch (error) {
    return { ok: false, error: `failed to read JSON: ${error.message}` };
  }
}

function ok(name, detail) {
  return { status: "ok", name, detail };
}

function fail(name, detail) {
  return { status: "fail", name, detail };
}

function skip(name, detail) {
  return { status: "skip", name, detail };
}

function printChecks(checks, runtime = createRealRuntime()) {
  for (const check of checks) {
    const label = check.status === "ok" ? "OK" : check.status === "skip" ? "SKIP" : "FAIL";
    runtime.stdout(`[${label}] ${check.name}: ${check.detail}`);
  }
}

function parseRegDefaultValue(stdout) {
  for (const line of stdout.split(/\r?\n/)) {
    const match = line.match(/\s+REG_SZ\s+(.+)\s*$/);
    if (match) {
      return match[1].trim();
    }
  }
  return null;
}

function requireBrowser(browser) {
  const error = browserError(browser);
  if (error) {
    usage(error);
  }
  return browser;
}

function requireExtensionId(extensionId) {
  const error = extensionIdError(extensionId);
  if (error) {
    usage(error);
  }
  return extensionId;
}

function ensureWindowsWriteAllowed(dryRun, runtime) {
  if (runtime.platform() !== "win32" && !dryRun) {
    usage("Windows local setup writes HKCU/APPDATA; run from Windows or pass --dry-run");
  }
}

function parseArgs(args) {
  const options = {
    dryRun: false,
    skipBuild: false,
    keepConfig: false,
  };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    switch (arg) {
      case "--browser":
        options.browser = takeValue(args, ++index, arg);
        break;
      case "--extension-id":
        options.extensionId = takeValue(args, ++index, arg);
        break;
      case "--host-path":
        options.hostPath = takeValue(args, ++index, arg);
        break;
      case "--wsl-distro":
        options.wslDistro = takeValue(args, ++index, arg);
        break;
      case "--wsl-workdir":
        options.wslWorkdir = takeValue(args, ++index, arg);
        break;
      case "--wsl-daemon-binary":
        options.wslDaemonBinary = takeValue(args, ++index, arg);
        break;
      case "--wsl-path":
        options.wslPath = takeValue(args, ++index, arg);
        break;
      case "--dry-run":
        options.dryRun = true;
        break;
      case "--skip-build":
        options.skipBuild = true;
        break;
      case "--keep-config":
        options.keepConfig = true;
        break;
      default:
        usage(`unknown option: ${arg}`);
    }
  }
  return options;
}

function takeValue(args, index, option) {
  const value = args[index];
  if (!value || value.startsWith("--")) {
    usage(`missing value for ${option}`);
  }
  return value;
}

function validateWslDistro(value) {
  const error = wslDistroError(value);
  if (error) {
    usage(error);
  }
}

function validateLinuxPath(value, option, allowRoot) {
  const error = linuxPathError(value, option, allowRoot);
  if (error) {
    usage(error);
  }
}

function validateLinuxPathList(value, option) {
  const error = linuxPathListError(value, option);
  if (error) {
    usage(error);
  }
}

function joinLinuxPath(...parts) {
  return parts
    .join("/")
    .replace(/\/+/g, "/")
    .replace(/\/$/, "");
}

function firstLine(text) {
  return text.split(/\r?\n/).find((line) => line.trim().length > 0)?.trim() ?? "";
}

function formatCommand(command) {
  return command.map(shellQuote).join(" ");
}

function shellQuote(value) {
  return /[^A-Za-z0-9_./:=\\-]/.test(value) ? JSON.stringify(value) : value;
}

function usage(error) {
  throw new UsageError(error ?? "");
}

function printUsage(runtime, error) {
  if (error) {
    runtime.stderr(error);
  }
  runtime.stderr(`Usage:
  node scripts/sidekick-local-setup.mjs install --browser <chrome|chrome-for-testing|chromium|edge> --extension-id <32-char-id> --host-path <windows-exe> [--wsl-distro <name>] [--wsl-workdir <path>] [--wsl-daemon-binary <path>] [--wsl-path <path-list>] [--skip-build] [--dry-run]
  node scripts/sidekick-local-setup.mjs doctor --browser <chrome|chrome-for-testing|chromium|edge> [--extension-id <32-char-id>] [--host-path <windows-exe>] [--wsl-distro <name>] [--wsl-workdir <path>] [--wsl-daemon-binary <path>] [--wsl-path <path-list>] [--dry-run]
  node scripts/sidekick-local-setup.mjs uninstall --browser <chrome|chrome-for-testing|chromium|edge> [--keep-config] [--dry-run]

Environment defaults:
  ${EXTENSION_ID_ENV}, ${WINDOWS_HOST_PATH_ENV}, ${WSL_DISTRO_ENV}, ${WSL_WORKDIR_ENV}, ${WSL_DAEMON_BINARY_ENV}, ${WSL_PATH_ENV}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(runSidekickLocalSetup(process.argv.slice(2)));
}
