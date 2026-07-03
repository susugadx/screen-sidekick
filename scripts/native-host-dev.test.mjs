import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./native-host-dev.mjs", import.meta.url));

test("locations can preview the Windows registry target from non-Windows hosts", () => {
  const result = runNativeHostDev(["locations", "--browser", "edge", "--target-platform", "win32"]);

  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    result.stdout.trim(),
    "edge: HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.screen_sidekick.host",
  );
});

test("locations can preview a non-current target platform", () => {
  const targetPlatform = nonCurrentTargetPlatform();
  const result = runNativeHostDev(["locations", "--browser", "edge", "--target-platform", targetPlatform]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^edge: /);
});

test("locations use a target-neutral home for non-current Unix target previews", () => {
  const targetPlatform = nonCurrentUnixTargetPlatform();
  const result = runNativeHostDev(["locations", "--browser", "edge", "--target-platform", targetPlatform]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout.trim(), /^edge: ~\//);
  assertDoesNotContainCurrentHome(result.stdout);
});

test("target platform is rejected for install", () => {
  const result = runNativeHostDev([
    "install",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "--target-platform",
    "win32",
    "--dry-run",
  ]);

  assert.equal(result.status, 2);
  assert.match(result.stderr, /--target-platform is not supported for install/);
});

test("install dry-run can include an explicit WSL PATH in generated config", () => {
  const wslPath = "/home/susu/.nvm/versions/node/v22.20.0/bin:/home/susu/.cargo/bin:/usr/local/bin:/usr/bin:/bin";
  const result = runNativeHostDev([
    "install",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "--host-path",
    "C:\\Sidekick\\screen-sidekick-native-host.exe",
    "--wsl-distro",
    "Ubuntu-24.04",
    "--wsl-workdir",
    "/home/susu/screen-sidekick",
    "--wsl-daemon-binary",
    "/home/susu/screen-sidekick/target/debug/screen-sidekick-daemon",
    "--wsl-path",
    wslPath,
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /"wsl_path": "\/home\/susu\/\.nvm\/versions\/node\/v22\.20\.0\/bin:\/home\/susu\/\.cargo\/bin:\/usr\/local\/bin:\/usr\/bin:\/bin"/);
});

test("uninstall dry-run can preview a non-current target platform", () => {
  const targetPlatform = nonCurrentTargetPlatform();
  const result = runNativeHostDev([
    "uninstall",
    "--browser",
    "edge",
    "--target-platform",
    targetPlatform,
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Would (remove|run: reg delete)/);
});

test("uninstall dry-run uses a target-neutral home for non-current Unix target previews", () => {
  const targetPlatform = nonCurrentUnixTargetPlatform();
  const result = runNativeHostDev([
    "uninstall",
    "--browser",
    "edge",
    "--target-platform",
    targetPlatform,
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout.trim(), /^Would remove ~\//);
  assertDoesNotContainCurrentHome(result.stdout);
});

test("uninstall rejects a non-current target platform without dry-run", () => {
  const targetPlatform = nonCurrentTargetPlatform();
  const result = runNativeHostDev([
    "uninstall",
    "--browser",
    "edge",
    "--target-platform",
    targetPlatform,
  ]);

  assert.equal(result.status, 2);
  assert.match(result.stderr, /cross-platform uninstall must use --dry-run/);
  assert.doesNotMatch(result.stdout, /Removed|Would remove|Would run/);
});

function runNativeHostDev(args) {
  return spawnSync(process.execPath, [scriptPath, ...args], {
    encoding: "utf8",
    env: {
      ...process.env,
      APPDATA: "C:\\Users\\tester\\AppData\\Roaming",
    },
    maxBuffer: 1024 * 1024,
  });
}

function nonCurrentTargetPlatform() {
  return process.platform === "win32" ? "linux" : "win32";
}

function nonCurrentUnixTargetPlatform() {
  return process.platform === "darwin" ? "linux" : "darwin";
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function assertDoesNotContainCurrentHome(text) {
  const currentHome = homedir();
  if (currentHome && currentHome !== "/") {
    assert.doesNotMatch(text, new RegExp(escapeRegExp(currentHome)));
  }
}
