#!/usr/bin/env node
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir, platform } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  BROWSERS,
  CONFIG_ENV,
  CONFIG_SCHEMA_VERSION,
  DESCRIPTION,
  HOST_NAME,
  browserError,
  extensionIdError,
  isWindowsAbsolutePath,
  joinWindowsPath,
  linuxPathError,
  windowsRegistryKey,
  wslDistroError,
} from "./native-host-shared.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function main(argv) {
  const [command, ...rest] = argv;
  const options = parseArgs(rest);
  switch (command) {
    case "generate":
      generate(options);
      return;
    case "install":
      install(options);
      return;
    case "uninstall":
      uninstall(options);
      return;
    case "locations":
      printLocations(options);
      return;
    default:
      usage(command ? `unknown command: ${command}` : null);
  }
}

function generate(options) {
  rejectTargetPlatformOption(options, "generate");
  const extensionId = requireExtensionId(options);
  const targetPlatform = targetPlatformForOptions(options);
  const wslConfig = buildWslConfig(options);
  ensureTargetCanBeWritten(targetPlatform, options.dryRun, "generate");
  const hostPath = requireHostPath(options, targetPlatform, Boolean(wslConfig));
  const manifestPath = options.out
    ? resolve(options.out)
    : defaultGeneratedManifestPath();
  const manifest = buildManifest(hostPath, extensionId);
  writeJsonFile(manifestPath, manifest, options.dryRun);
  if (wslConfig) {
    writeJsonFile(nativeHostConfigPath(targetPlatform, options.dryRun), wslConfig, options.dryRun);
  }
}

function install(options) {
  rejectTargetPlatformOption(options, "install");
  const browser = requireBrowser(options);
  const extensionId = requireExtensionId(options);
  const targetPlatform = targetPlatformForOptions(options);
  const wslConfig = buildWslConfig(options);
  ensureTargetCanBeWritten(targetPlatform, options.dryRun, "install");
  requireWindowsInstallRuntimeConfig(targetPlatform, wslConfig);
  const hostPath = requireHostPath(options, targetPlatform, Boolean(wslConfig));
  const manifestPath = options.manifestPath
    ? resolve(options.manifestPath)
    : defaultGeneratedManifestPath();
  const manifest = buildManifest(hostPath, extensionId);
  writeJsonFile(manifestPath, manifest, options.dryRun);
  if (wslConfig) {
    writeJsonFile(nativeHostConfigPath(targetPlatform, options.dryRun), wslConfig, options.dryRun);
  }

  if (targetPlatform === "win32") {
    const key = windowsRegistryKey(browser);
    runOrPrint(
      ["reg", "add", key, "/ve", "/t", "REG_SZ", "/d", manifestPath, "/f"],
      options.dryRun,
    );
    return;
  }

  const targetPath = userManifestPath(browser);
  writeJsonFile(targetPath, manifest, options.dryRun);
}

function uninstall(options) {
  const browser = requireBrowser(options);
  const targetPlatform = targetPlatformFromOption(options) ?? platform();
  ensureTargetCanBeWritten(targetPlatform, options.dryRun, "uninstall");
  if (targetPlatform === "win32") {
    const key = windowsRegistryKey(browser);
    runOrPrint(["reg", "delete", key, "/f"], options.dryRun);
    return;
  }
  const targetPath = userManifestPath(browser, targetPlatform);
  if (options.dryRun) {
    console.log(`Would remove ${targetPath}`);
    return;
  }
  rmSync(targetPath, { force: true });
  console.log(`Removed ${targetPath}`);
}

function buildManifest(hostPath, extensionId) {
  return {
    name: HOST_NAME,
    description: DESCRIPTION,
    path: hostPath,
    type: "stdio",
    allowed_origins: [`chrome-extension://${extensionId}/`],
  };
}

function buildWslConfig(options) {
  const hasAny = Boolean(options.wslDistro || options.wslWorkdir || options.wslDaemonBinary);
  if (!hasAny) {
    return null;
  }
  const distro = requireOption(options.wslDistro, "--wsl-distro");
  const workdir = requireOption(options.wslWorkdir, "--wsl-workdir");
  const daemonBinary = requireOption(options.wslDaemonBinary, "--wsl-daemon-binary");
  validateWslDistro(distro);
  validateLinuxPath(workdir, "--wsl-workdir", true);
  validateLinuxPath(daemonBinary, "--wsl-daemon-binary", false);
  return {
    schema_version: CONFIG_SCHEMA_VERSION,
    mode: "wsl_auto",
    wsl_distro: distro,
    wsl_workdir: workdir,
    wsl_daemon_binary: daemonBinary,
  };
}

function writeJsonFile(filePath, value, dryRun) {
  const text = `${JSON.stringify(value, null, 2)}\n`;
  if (dryRun) {
    console.log(`Would write ${filePath}:`);
    console.log(text.trimEnd());
    return;
  }
  mkdirSync(dirname(filePath), { recursive: true });
  writeFileSync(filePath, text, { mode: 0o600 });
  console.log(`Wrote ${filePath}`);
}

function requireBrowser(options) {
  const browser = options.browser;
  const error = browserError(browser);
  if (error) {
    usage(error);
  }
  return browser;
}

function requireExtensionId(options) {
  const extensionId = options.extensionId;
  const error = extensionIdError(extensionId);
  if (error) {
    usage(error);
  }
  return extensionId;
}

function requireHostPath(options, targetPlatform, requireExplicit) {
  if (requireExplicit && !options.hostPath) {
    usage("--host-path is required when generating WSL auto-start config");
  }
  const hostPath = options.hostPath
    ? normalizeHostPath(options.hostPath, targetPlatform)
    : defaultHostBinaryPath(targetPlatform);
  if (!isAbsoluteForTarget(hostPath, targetPlatform)) {
    usage("--host-path must resolve to an absolute path");
  }
  if (!options.dryRun && platform() === targetPlatform && !existsSync(hostPath)) {
    usage(`host binary does not exist: ${hostPath}`);
  }
  return hostPath;
}

function defaultHostBinaryPath(targetPlatform) {
  const suffix = targetPlatform === "win32" ? ".exe" : "";
  return join(repoRoot, "target", "debug", `screen-sidekick-native-host${suffix}`);
}

function defaultGeneratedManifestPath() {
  return join(repoRoot, "target", "native-host", `${HOST_NAME}.json`);
}

function userManifestPath(browser, targetPlatform = platform()) {
  const home = targetHomeDirectory(targetPlatform);
  if (targetPlatform === "darwin") {
    const dirs = {
      chrome: ["Library", "Application Support", "Google", "Chrome"],
      "chrome-for-testing": ["Library", "Application Support", "Google", "ChromeForTesting"],
      chromium: ["Library", "Application Support", "Chromium"],
      edge: ["Library", "Application Support", "Microsoft Edge"],
    };
    return joinUnixPath(home, ...dirs[browser], "NativeMessagingHosts", `${HOST_NAME}.json`);
  }
  if (targetPlatform === "linux") {
    const dirs = {
      chrome: [".config", "google-chrome"],
      "chrome-for-testing": [".config", "google-chrome-for-testing"],
      chromium: [".config", "chromium"],
      edge: [".config", "microsoft-edge"],
    };
    return joinUnixPath(home, ...dirs[browser], "NativeMessagingHosts", `${HOST_NAME}.json`);
  }
  return defaultGeneratedManifestPath();
}

function targetHomeDirectory(targetPlatform) {
  return targetPlatform === platform() ? homedir() : "~";
}

function joinUnixPath(...parts) {
  return parts.join("/").replace(/\/+/g, "/");
}

function targetPlatformFromOption(options) {
  if (!options.targetPlatform) {
    return null;
  }
  if (!["win32", "linux", "darwin"].includes(options.targetPlatform)) {
    usage("--target-platform must be win32, linux, or darwin");
  }
  return options.targetPlatform;
}

function rejectTargetPlatformOption(options, command) {
  if (options.targetPlatform) {
    usage(`--target-platform is not supported for ${command}`);
  }
}

function targetPlatformForOptions(options) {
  return hasWslConfigOptions(options) || isWindowsAbsolutePath(options.hostPath ?? "")
    ? "win32"
    : platform();
}

function hasWslConfigOptions(options) {
  return Boolean(options.wslDistro || options.wslWorkdir || options.wslDaemonBinary);
}

function ensureTargetCanBeWritten(targetPlatform, dryRun, command) {
  if (dryRun || targetPlatform === platform()) {
    return;
  }
  if (command === "uninstall") {
    usage("cross-platform uninstall must use --dry-run");
  }
  usage("cross-platform native host setup must use --dry-run");
}

function requireWindowsInstallRuntimeConfig(targetPlatform, wslConfig) {
  if (targetPlatform === "win32" && !wslConfig) {
    usage(
      "Windows native host install requires --wsl-distro, --wsl-workdir, and --wsl-daemon-binary",
    );
  }
}

function normalizeHostPath(rawPath, targetPlatform) {
  if (targetPlatform === "win32") {
    if (isWindowsAbsolutePath(rawPath)) {
      return rawPath;
    }
    if (platform() === "win32") {
      return resolve(rawPath);
    }
    usage("--host-path must be an absolute Windows path for WSL auto-start setup");
  }
  return resolve(rawPath);
}

function isAbsoluteForTarget(filePath, targetPlatform) {
  if (targetPlatform === "win32") {
    return isWindowsAbsolutePath(filePath);
  }
  return isAbsolute(filePath);
}

function nativeHostConfigPath(targetPlatform, dryRun) {
  if (process.env[CONFIG_ENV]) {
    return process.env[CONFIG_ENV];
  }
  if (targetPlatform !== "win32") {
    usage("native host config is only defined for Windows WSL auto-start setup");
  }
  const appData = process.env.APPDATA;
  if (!appData && !dryRun) {
    usage("APPDATA is required to write the Windows native host config");
  }
  return joinWindowsPath(appData ?? "%APPDATA%", "Screen Sidekick", "native-host-config.json");
}

function runOrPrint(command, dryRun) {
  if (dryRun) {
    console.log(`Would run: ${command.map(shellQuote).join(" ")}`);
    return;
  }
  const result = spawnSync(command[0], command.slice(1), { stdio: "inherit" });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function printLocations(options) {
  const targetPlatform = targetPlatformFromOption(options) ?? platform();
  const browsers = options.browser ? [requireBrowser(options)] : BROWSERS;
  for (const browser of browsers) {
    if (targetPlatform === "win32") {
      console.log(`${browser}: ${windowsRegistryKey(browser)}`);
    } else {
      console.log(`${browser}: ${userManifestPath(browser, targetPlatform)}`);
    }
  }
}

function parseArgs(args) {
  const options = {
    dryRun: false,
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
      case "--manifest-path":
        options.manifestPath = takeValue(args, ++index, arg);
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
      case "--out":
        options.out = takeValue(args, ++index, arg);
        break;
      case "--dry-run":
        options.dryRun = true;
        break;
      case "--target-platform":
        options.targetPlatform = takeValue(args, ++index, arg);
        break;
      default:
        usage(`unknown option: ${arg}`);
    }
  }
  return options;
}

function requireOption(value, option) {
  if (!value) {
    usage(`missing value for ${option}`);
  }
  return value;
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

function usage(error) {
  if (error) {
    console.error(error);
  }
  console.error(`Usage:
  node scripts/native-host-dev.mjs generate --extension-id <32-char-id> [--host-path <path>] [--out <path>] [--wsl-distro <name> --wsl-workdir <path> --wsl-daemon-binary <path>] [--dry-run]
  node scripts/native-host-dev.mjs install --browser <chrome|chrome-for-testing|chromium|edge> --extension-id <32-char-id> [--host-path <path>] [--wsl-distro <name> --wsl-workdir <path> --wsl-daemon-binary <path>] [--dry-run]
  node scripts/native-host-dev.mjs uninstall --browser <chrome|chrome-for-testing|chromium|edge> [--target-platform <win32|linux|darwin>] [--dry-run]
  node scripts/native-host-dev.mjs locations [--browser <chrome|chrome-for-testing|chromium|edge>] [--target-platform <win32|linux|darwin>]`);
  process.exit(2);
}

function shellQuote(value) {
  return /[^A-Za-z0-9_./:=\\-]/.test(value) ? JSON.stringify(value) : value;
}

main(process.argv.slice(2));
