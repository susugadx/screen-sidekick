import {
  CONFIG_SCHEMA_VERSION,
  DAEMON_STATUS_SCHEMA_VERSION,
  DESCRIPTION,
  HOST_NAME,
  windowsRegistryKey,
} from "../native-host-shared.mjs";
import { runSidekickLocalSetup } from "../sidekick-local-setup.mjs";

export function runSetupWithFakeWindows(args, overrides = {}) {
  const stdout = [];
  const stderr = [];
  const calls = [];
  const manifestPath = "C:\\Users\\tester\\AppData\\Roaming\\Screen Sidekick\\com.screen_sidekick.host.json";
  const configPath = "C:\\Users\\tester\\AppData\\Roaming\\Screen Sidekick\\native-host-config.json";
  const manifest = stripUndefined(overrides.manifest ?? validManifest());
  const config = stripUndefined(overrides.config ?? validNativeHostConfig());
  const daemonStatusStdout = daemonStatusStdoutForOverrides(overrides);
  const extensionBuildOutputExists = overrides.extensionBuildOutputExists !== false;
  const files = new Map();

  if (overrides.manifestExists !== false) {
    files.set(manifestPath, `${JSON.stringify(manifest)}\n`);
  }
  if (overrides.configExists !== false) {
    files.set(configPath, `${JSON.stringify(config)}\n`);
  }
  if (typeof manifest.path === "string" && overrides.hostPathExists !== false) {
    files.set(manifest.path, "");
  }

  const runtime = {
    env: {
      APPDATA: "C:\\Users\\tester\\AppData\\Roaming",
      ...(overrides.env ?? {}),
    },
    execPath: process.execPath,
    platform: () => "win32",
    existsSync: (path) => files.has(path),
    readFileSync: (path) => {
      if (!files.has(path)) {
        throw new Error(`missing fake file: ${path}`);
      }
      return files.get(path);
    },
    rmSync: (path) => {
      calls.push({ command: "rm", args: [path] });
      files.delete(path);
    },
    spawnSync: (command, commandArgs, spawnOptions) => {
      calls.push({ command, args: commandArgs, options: spawnOptions });
      return fakeWindowsSpawn(command, commandArgs, {
        manifestPath,
        configPath,
        daemonStatusStdout,
        extensionBuildOutputExists,
        nativeHostDevStatus: overrides.nativeHostDevStatus ?? 0,
        stdout,
        stderr,
      });
    },
    stdout: (line) => stdout.push(line),
    stderr: (line) => stderr.push(line),
  };

  return {
    status: runSidekickLocalSetup(args, runtime),
    stdout: stdout.join("\n"),
    stderr: stderr.join("\n"),
    calls,
  };
}

export function runSetupWithFakeLocal(args, overrides = {}) {
  const stdout = [];
  const stderr = [];
  const calls = [];
  const daemonStatusStdout = daemonStatusStdoutForOverrides(overrides);
  const localWorkdir = overrides.localWorkdir ?? "/repo";
  const daemonBinary = overrides.daemonBinary ?? joinLinuxPath(localWorkdir, "target/debug/screen-sidekick-daemon");
  const extensionPackagePath = joinLinuxPath(localWorkdir, "apps/extension/package.json");
  const extensionLockfilePath = joinLinuxPath(localWorkdir, "apps/extension/package-lock.json");
  const extensionBuildOutputPath = joinLinuxPath(localWorkdir, "apps/extension/dist/side_panel.js");
  const extensionBuildOutputExists = overrides.extensionBuildOutputExists !== false;
  const files = new Set([
    daemonBinary,
    extensionPackagePath,
    extensionLockfilePath,
    ...(extensionBuildOutputExists ? [extensionBuildOutputPath] : []),
    "/repo/apps/extension/package.json",
    "/repo/apps/extension/package-lock.json",
    "/repo/apps/extension/dist/side_panel.js",
  ]);

  const runtime = {
    env: overrides.env ?? {},
    execPath: process.execPath,
    platform: () => "linux",
    existsSync: (path) => {
      if (!extensionBuildOutputExists && path === extensionBuildOutputPath) {
        return false;
      }
      return files.has(path) ||
        path.endsWith("/apps/extension/package.json") ||
        path.endsWith("/apps/extension/package-lock.json") ||
        path.endsWith("/apps/extension/dist/side_panel.js");
    },
    readFileSync: (path) => {
      throw new Error(`unexpected fake read: ${path}`);
    },
    rmSync: (path) => {
      calls.push({ command: "rm", args: [path] });
      files.delete(path);
    },
    spawnSync: (command, commandArgs, spawnOptions) => {
      calls.push({ command, args: commandArgs, options: spawnOptions });
      return fakeLocalSpawn(command, commandArgs, {
        daemonBinary,
        daemonStatusStdout,
        nativeHostDevStatus: overrides.nativeHostDevStatus ?? 0,
        stdout,
        stderr,
      });
    },
    stdout: (line) => stdout.push(line),
    stderr: (line) => stderr.push(line),
  };

  return {
    status: runSidekickLocalSetup(args, runtime),
    stdout: stdout.join("\n"),
    stderr: stderr.join("\n"),
    calls,
  };
}

export function validManifest(overrides = {}) {
  return {
    name: HOST_NAME,
    description: DESCRIPTION,
    type: "stdio",
    path: "C:\\Sidekick\\screen-sidekick-native-host.exe",
    allowed_origins: ["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"],
    ...overrides,
  };
}

export function validNativeHostConfig(overrides = {}) {
  return {
    schema_version: CONFIG_SCHEMA_VERSION,
    mode: "wsl_auto",
    wsl_distro: "Ubuntu-24.04",
    wsl_workdir: "/home/susu/screen-sidekick",
    wsl_daemon_binary: "/home/susu/screen-sidekick/target/debug/screen-sidekick-daemon",
    ...overrides,
  };
}

export function validDaemonStatus(overrides = {}) {
  return {
    schema_version: DAEMON_STATUS_SCHEMA_VERSION,
    url: "http://127.0.0.1:43001",
    ws_url: "ws://127.0.0.1:43001/v0/ws",
    token: "pairing-token",
    status: "running",
    ...overrides,
  };
}

function daemonStatusStdoutForOverrides(overrides) {
  if (typeof overrides.daemonStatusStdout === "string") {
    return overrides.daemonStatusStdout;
  }
  return `${JSON.stringify(stripUndefined(validDaemonStatus(overrides.daemonStatus ?? {})))}\n`;
}

function fakeWindowsSpawn(command, commandArgs, fixture) {
  const nativeHostDev = fakeNativeHostDevSpawn(command, commandArgs, fixture);
  if (nativeHostDev) {
    return nativeHostDev;
  }
  if (command === "cargo" || command === "npm") {
    throw new Error(`unexpected Windows-local tool check: ${command}`);
  }
  if (command === "reg") {
    return {
      status: 0,
      stdout: `HKEY_CURRENT_USER\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\${HOST_NAME}\n    (Default)    REG_SZ    ${fixture.manifestPath}\n`,
      stderr: "",
    };
  }
  if (command === "wsl.exe" && commandArgs.length === 1 && commandArgs[0] === "--status") {
    return { status: 0, stdout: "Default Distribution: Ubuntu-24.04\n", stderr: "" };
  }
  if (command === "wsl.exe" && commandArgs.includes("codex")) {
    return { status: 0, stdout: "codex 1.2.3\n", stderr: "" };
  }
  if (command === "wsl.exe" && isWslExec(commandArgs, ["test", "-f", "apps/extension/dist/side_panel.js"])) {
    return {
      status: fixture.extensionBuildOutputExists ? 0 : 1,
      stdout: "",
      stderr: fixture.extensionBuildOutputExists ? "" : "missing",
    };
  }
  if (command === "wsl.exe" && commandArgs.includes("--stdio-status") && commandArgs.includes("--exec")) {
    return {
      status: 0,
      stdout: fixture.daemonStatusStdout,
      stderr: "",
    };
  }
  return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
}

function fakeLocalSpawn(command, commandArgs, fixture) {
  const nativeHostDev = fakeNativeHostDevSpawn(command, commandArgs, {
    manifestPath: "/repo/target/native-host/com.screen_sidekick.host.json",
    configPath: "%APPDATA%\\Screen Sidekick\\native-host-config.json",
    ...fixture,
  });
  if (nativeHostDev) {
    return nativeHostDev;
  }
  if (command === "cargo" && commandArgs.length === 1 && commandArgs[0] === "--version") {
    return { status: 0, stdout: "cargo 1.96.0\n", stderr: "" };
  }
  if (command === "npm" && commandArgs.length === 1 && commandArgs[0] === "--version") {
    return { status: 0, stdout: "11.0.0\n", stderr: "" };
  }
  if (command === "codex" && commandArgs.length === 1 && commandArgs[0] === "--version") {
    return { status: 0, stdout: "codex 1.2.3\n", stderr: "" };
  }
  if (command === fixture.daemonBinary && commandArgs.length === 1 && commandArgs[0] === "--stdio-status") {
    return { status: 0, stdout: fixture.daemonStatusStdout, stderr: "" };
  }
  return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
}

function fakeNativeHostDevSpawn(command, commandArgs, fixture) {
  if (command !== process.execPath || !commandArgs[0]?.endsWith("native-host-dev.mjs")) {
    return null;
  }

  const args = commandArgs.slice(1);
  const [subcommand] = args;
  const dryRun = args.includes("--dry-run");
  if (subcommand === "install" && dryRun) {
    const browser = optionValue(args, "--browser");
    const extensionId = optionValue(args, "--extension-id");
    const hostPath = optionValue(args, "--host-path");
    const wslConfig = {
      schema_version: CONFIG_SCHEMA_VERSION,
      mode: "wsl_auto",
      wsl_distro: optionValue(args, "--wsl-distro"),
      wsl_workdir: optionValue(args, "--wsl-workdir"),
      wsl_daemon_binary: optionValue(args, "--wsl-daemon-binary"),
    };
    const wslPath = optionValue(args, "--wsl-path");
    if (wslPath) {
      wslConfig.wsl_path = wslPath;
    }
    fixture.stdout.push(`Would write ${fixture.manifestPath}:`);
    fixture.stdout.push(JSON.stringify({
      name: HOST_NAME,
      description: DESCRIPTION,
      path: hostPath,
      type: "stdio",
      allowed_origins: [`chrome-extension://${extensionId}/`],
    }, null, 2));
    fixture.stdout.push(`Would write ${fixture.configPath}:`);
    fixture.stdout.push(JSON.stringify(wslConfig, null, 2));
    fixture.stdout.push(`Would run: reg add ${windowsRegistryKey(browser)} /ve /t REG_SZ /d ${fixture.manifestPath} /f`);
    return { status: 0, stdout: "", stderr: "" };
  }

  if (subcommand === "uninstall" && dryRun) {
    const browser = optionValue(args, "--browser");
    fixture.stdout.push(`Would run: reg delete ${windowsRegistryKey(browser)} /f`);
    return { status: 0, stdout: "", stderr: "" };
  }

  if (subcommand === "install" || subcommand === "uninstall") {
    if (fixture.nativeHostDevStatus !== 0) {
      fixture.stderr.push(`native-host-dev ${subcommand} failed`);
    }
    return { status: fixture.nativeHostDevStatus, stdout: "", stderr: "" };
  }

  return { status: 1, stdout: "", stderr: `unexpected native-host-dev command: ${subcommand}` };
}

function stripUndefined(value) {
  return Object.fromEntries(
    Object.entries(value).filter(([, fieldValue]) => fieldValue !== undefined),
  );
}

function joinLinuxPath(...parts) {
  return parts.join("/").replace(/\/+/g, "/");
}

function optionValue(args, option) {
  const index = args.indexOf(option);
  return index === -1 ? null : args[index + 1];
}

function isWslExec(commandArgs, expectedCommand) {
  const execIndex = commandArgs.indexOf("--exec");
  const actualCommand = execIndex === -1 ? [] : stripWslEnvPrefix(commandArgs.slice(execIndex + 1));
  return execIndex !== -1 &&
    JSON.stringify(actualCommand) === JSON.stringify(expectedCommand);
}

function stripWslEnvPrefix(commandArgs) {
  if (
    commandArgs.length >= 3 &&
    commandArgs[0] === "env" &&
    typeof commandArgs[1] === "string" &&
    commandArgs[1].startsWith("PATH=")
  ) {
    return commandArgs.slice(2);
  }
  return commandArgs;
}
