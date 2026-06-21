#!/usr/bin/env node
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir, platform } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const HOST_NAME = "com.screen_sidekick.host";
const DESCRIPTION = "Screen Sidekick Native Messaging Host";
const BROWSERS = new Set(["chrome", "chrome-for-testing", "chromium", "edge"]);
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
      printLocations();
      return;
    default:
      usage(command ? `unknown command: ${command}` : null);
  }
}

function generate(options) {
  const extensionId = requireExtensionId(options);
  const hostPath = requireHostPath(options);
  const manifestPath = options.out
    ? resolve(options.out)
    : defaultGeneratedManifestPath();
  const manifest = buildManifest(hostPath, extensionId);
  writeJsonFile(manifestPath, manifest, options.dryRun);
}

function install(options) {
  const browser = requireBrowser(options);
  const extensionId = requireExtensionId(options);
  const hostPath = requireHostPath(options);
  const manifestPath = options.manifestPath
    ? resolve(options.manifestPath)
    : defaultGeneratedManifestPath();
  const manifest = buildManifest(hostPath, extensionId);
  writeJsonFile(manifestPath, manifest, options.dryRun);

  if (platform() === "win32") {
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
  if (platform() === "win32") {
    const key = windowsRegistryKey(browser);
    runOrPrint(["reg", "delete", key, "/f"], options.dryRun);
    return;
  }
  const targetPath = userManifestPath(browser);
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
  if (!browser || !BROWSERS.has(browser)) {
    usage("missing or invalid --browser");
  }
  return browser;
}

function requireExtensionId(options) {
  const extensionId = options.extensionId;
  if (!extensionId || !/^[a-p]{32}$/.test(extensionId)) {
    usage("--extension-id must be a 32-character Chrome extension ID");
  }
  return extensionId;
}

function requireHostPath(options) {
  const hostPath = options.hostPath ? resolve(options.hostPath) : defaultHostBinaryPath();
  if (!isAbsolute(hostPath)) {
    usage("--host-path must resolve to an absolute path");
  }
  if (!options.dryRun && !existsSync(hostPath)) {
    usage(`host binary does not exist: ${hostPath}`);
  }
  return hostPath;
}

function defaultHostBinaryPath() {
  const suffix = platform() === "win32" ? ".exe" : "";
  return join(repoRoot, "target", "debug", `screen-sidekick-native-host${suffix}`);
}

function defaultGeneratedManifestPath() {
  return join(repoRoot, "target", "native-host", `${HOST_NAME}.json`);
}

function userManifestPath(browser) {
  const home = homedir();
  if (platform() === "darwin") {
    const dirs = {
      chrome: ["Library", "Application Support", "Google", "Chrome"],
      "chrome-for-testing": ["Library", "Application Support", "Google", "ChromeForTesting"],
      chromium: ["Library", "Application Support", "Chromium"],
      edge: ["Library", "Application Support", "Microsoft Edge"],
    };
    return join(home, ...dirs[browser], "NativeMessagingHosts", `${HOST_NAME}.json`);
  }
  if (platform() === "linux") {
    const dirs = {
      chrome: [".config", "google-chrome"],
      "chrome-for-testing": [".config", "google-chrome-for-testing"],
      chromium: [".config", "chromium"],
      edge: [".config", "microsoft-edge"],
    };
    return join(home, ...dirs[browser], "NativeMessagingHosts", `${HOST_NAME}.json`);
  }
  return defaultGeneratedManifestPath();
}

function windowsRegistryKey(browser) {
  const roots = {
    chrome: "Google\\Chrome",
    "chrome-for-testing": "Google\\ChromeForTesting",
    chromium: "Chromium",
    edge: "Microsoft\\Edge",
  };
  return `HKCU\\Software\\${roots[browser]}\\NativeMessagingHosts\\${HOST_NAME}`;
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

function printLocations() {
  for (const browser of BROWSERS) {
    if (platform() === "win32") {
      console.log(`${browser}: ${windowsRegistryKey(browser)}`);
    } else {
      console.log(`${browser}: ${userManifestPath(browser)}`);
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
      case "--out":
        options.out = takeValue(args, ++index, arg);
        break;
      case "--dry-run":
        options.dryRun = true;
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

function usage(error) {
  if (error) {
    console.error(error);
  }
  console.error(`Usage:
  node scripts/native-host-dev.mjs generate --extension-id <32-char-id> [--host-path <path>] [--out <path>] [--dry-run]
  node scripts/native-host-dev.mjs install --browser <chrome|chrome-for-testing|chromium|edge> --extension-id <32-char-id> [--host-path <path>] [--dry-run]
  node scripts/native-host-dev.mjs uninstall --browser <chrome|chrome-for-testing|chromium|edge> [--dry-run]
  node scripts/native-host-dev.mjs locations`);
  process.exit(2);
}

function shellQuote(value) {
  return /[^A-Za-z0-9_./:=\\-]/.test(value) ? JSON.stringify(value) : value;
}

main(process.argv.slice(2));
