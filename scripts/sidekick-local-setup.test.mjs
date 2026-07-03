import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  runSetupWithFakeLocal,
  runSetupWithFakeWindows,
  validDaemonStatus,
  validManifest,
  validNativeHostConfig,
} from "./test-support/sidekick-local-setup-fake-runtime.mjs";

const scriptPath = fileURLToPath(new URL("./sidekick-local-setup.mjs", import.meta.url));
const cleanEnvKeys = [
  "SCREEN_SIDEKICK_EXTENSION_ID",
  "SCREEN_SIDEKICK_WINDOWS_HOST_PATH",
  "SCREEN_SIDEKICK_WSL_DISTRO",
  "SCREEN_SIDEKICK_WSL_WORKDIR",
  "SCREEN_SIDEKICK_WSL_DAEMON_BINARY",
  "SCREEN_SIDEKICK_WSL_PATH",
  "SCREEN_SIDEKICK_NATIVE_HOST_CONFIG",
];

const wslPath = "/home/susu/.nvm/versions/node/v22.20.0/bin:/home/susu/.cargo/bin:/usr/local/bin:/usr/bin:/bin";

test("install dry-run delegates Windows WSL manifest and config generation", () => {
  const result = runSetupWithFakeWindows([
    "install",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "--host-path",
    "C:\\Sidekick\\screen-sidekick-native-host.exe",
    "--wsl-distro",
    "Ubuntu",
    "--wsl-workdir",
    "/home/test/screen-sidekick",
    "--wsl-daemon-binary",
    "/home/test/screen-sidekick/target/debug/screen-sidekick-daemon",
    "--wsl-path",
    wslPath,
    "--skip-build",
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Would write .*com\.screen_sidekick\.host\.json/);
  assert.match(result.stdout, /chrome-extension:\/\/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\//);
  assert.match(result.stdout, /"wsl_distro": "Ubuntu"/);
  assert.match(result.stdout, /"wsl_path": "\/home\/susu\/\.nvm\/versions\/node\/v22\.20\.0\/bin:\/home\/susu\/\.cargo\/bin:\/usr\/local\/bin:\/usr\/bin:\/bin"/);
  assert.match(result.stdout, /Would run: reg add HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com\.screen_sidekick\.host/);
});

test("local install dry-run shows local build steps without executing them", () => {
  const result = runSetupWithFakeLocal([
    "install",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "--host-path",
    "C:\\Sidekick\\screen-sidekick-native-host.exe",
    "--wsl-workdir",
    "/home/test/screen-sidekick",
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Would run \(build WSL daemon\): cargo build -p screen-sidekick-sidekick-daemon --bin screen-sidekick-daemon/);
  assert.match(result.stdout, /Would run \(install extension dependencies\): npm ci --prefix apps\/extension/);
  assert.match(result.stdout, /Would run \(build extension\): npm --prefix apps\/extension run build/);
  assert.equal(result.calls.some((call) => call.command === "cargo" || call.command === "npm"), false);
});

test("Windows install dry-run shows WSL build steps without executing them", () => {
  const result = runSetupWithFakeWindows([
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
    "/home/test/screen-sidekick",
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(
    result.stdout,
    /Would run \(build WSL daemon\): wsl\.exe -d Ubuntu-24\.04 --cd \/home\/test\/screen-sidekick -- cargo build -p screen-sidekick-sidekick-daemon --bin screen-sidekick-daemon/,
  );
  assert.match(
    result.stdout,
    /Would run \(install extension dependencies in WSL\): wsl\.exe -d Ubuntu-24\.04 --cd \/home\/test\/screen-sidekick -- npm ci --prefix apps\/extension/,
  );
  assert.match(
    result.stdout,
    /Would run \(build extension in WSL\): wsl\.exe -d Ubuntu-24\.04 --cd \/home\/test\/screen-sidekick -- npm --prefix apps\/extension run build/,
  );
  assert.equal(result.calls.some((call) => call.command === "cargo" || call.command === "npm"), false);
});

test("Windows install dry-run applies explicit WSL PATH to WSL build steps", () => {
  const result = runSetupWithFakeWindows([
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
    "/home/test/screen-sidekick",
    "--wsl-path",
    wslPath,
    "--dry-run",
  ]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(
    result.stdout,
    /Would run \(build WSL daemon\): wsl\.exe -d Ubuntu-24\.04 --cd \/home\/test\/screen-sidekick --exec env PATH=\/home\/susu\/\.nvm\/versions\/node\/v22\.20\.0\/bin:\/home\/susu\/\.cargo\/bin:\/usr\/local\/bin:\/usr\/bin:\/bin cargo build -p screen-sidekick-sidekick-daemon --bin screen-sidekick-daemon/,
  );
  assert.match(
    result.stdout,
    /Would run \(install extension dependencies in WSL\): wsl\.exe -d Ubuntu-24\.04 --cd \/home\/test\/screen-sidekick --exec env PATH=\/home\/susu\/\.nvm\/versions\/node\/v22\.20\.0\/bin:\/home\/susu\/\.cargo\/bin:\/usr\/local\/bin:\/usr\/bin:\/bin npm ci --prefix apps\/extension/,
  );
  assert.match(
    result.stdout,
    /Would run \(build extension in WSL\): wsl\.exe -d Ubuntu-24\.04 --cd \/home\/test\/screen-sidekick --exec env PATH=\/home\/susu\/\.nvm\/versions\/node\/v22\.20\.0\/bin:\/home\/susu\/\.cargo\/bin:\/usr\/local\/bin:\/usr\/bin:\/bin npm --prefix apps\/extension run build/,
  );
});

test("doctor dry-run validates config without process checks", () => {
  const result = runSetup(["doctor", "--browser", "edge", "--dry-run"]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /\[OK\] browser option: edge/);
  assert.match(result.stdout, /\[SKIP\] extension ID comparison/);
  assert.match(result.stdout, /\[SKIP\] process checks: dry run/);
});

test("uninstall dry-run targets Windows registration and config", () => {
  const result = runSetupWithFakeWindows(["uninstall", "--browser", "edge", "--dry-run"]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Would run: reg delete HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com\.screen_sidekick\.host \/f/);
  assert.match(result.stdout, /Would remove C:\\Users\\tester\\AppData\\Roaming\\Screen Sidekick\\native-host-config\.json/);
});

test("uninstall removes Windows config even when registry unregister fails", () => {
  const result = runSetupWithFakeWindows(["uninstall", "--browser", "edge"], {
    nativeHostDevStatus: 1,
  });

  assert.equal(result.status, 1);
  assert.match(result.stdout, /Removed C:\\Users\\tester\\AppData\\Roaming\\Screen Sidekick\\native-host-config\.json/);
  assert.deepEqual(
    result.calls.filter((call) => call.command === "rm").map((call) => call.args),
    [["C:\\Users\\tester\\AppData\\Roaming\\Screen Sidekick\\native-host-config.json"]],
  );
});

test("uninstall keep-config skips Windows config removal after registry unregister failure", () => {
  const result = runSetupWithFakeWindows(["uninstall", "--browser", "edge", "--keep-config"], {
    nativeHostDevStatus: 1,
  });

  assert.equal(result.status, 1);
  assert.equal(result.calls.some((call) => call.command === "rm"), false);
  assert.doesNotMatch(result.stdout, /native-host-config\.json/);
});

test("invalid WSL path is rejected before subprocess execution", () => {
  const result = runSetup([
    "install",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "--host-path",
    "C:\\Sidekick\\screen-sidekick-native-host.exe",
    "--wsl-workdir",
    "/home/test/../screen-sidekick",
    "--dry-run",
  ]);

  assert.equal(result.status, 2);
  assert.match(result.stderr, /--wsl-workdir must be an absolute Linux path without parent traversal/);
});

test("invalid WSL PATH list is rejected before subprocess execution", () => {
  const result = runSetup([
    "install",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "--host-path",
    "C:\\Sidekick\\screen-sidekick-native-host.exe",
    "--wsl-workdir",
    "/home/test/screen-sidekick",
    "--wsl-path",
    "/home/test/.cargo/bin:relative/bin",
    "--dry-run",
  ]);

  assert.equal(result.status, 2);
  assert.match(result.stderr, /--wsl-path must be a colon-separated list of absolute Linux paths without parent traversal/);
});

test("Windows doctor reads installed config without requiring WSL path options", () => {
  const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"]);

  assert.equal(result.status, 0, result.stdout);
  assert.match(result.stdout, /\[OK\] Windows registry manifest:/);
  assert.match(result.stdout, /\[OK\] Windows native host config:/);
  assert.match(result.stdout, /\[OK\] Codex CLI in WSL: codex 1\.2\.3/);
  assert.match(result.stdout, /\[OK\] WSL daemon stdio status: status line parsed/);
  assert.equal(result.calls.some((call) => call.command === "cargo" || call.command === "npm"), false);
});

test("Windows doctor fails invalid manifest path values", () => {
  const cases = [
    {
      name: "missing path",
      manifest: { ...validManifest(), path: undefined },
      expected: /manifest path is missing or not a string/,
    },
    {
      name: "stale host path",
      manifest: validManifest({ path: "C:\\Sidekick\\missing-host.exe" }),
      hostPathExists: false,
      expected: /manifest path does not exist: C:\\Sidekick\\missing-host\.exe/,
    },
    {
      name: "host path mismatch",
      args: ["doctor", "--browser", "edge", "--host-path", "C:\\Other\\screen-sidekick-native-host.exe"],
      expected: /manifest path does not match expected host path/,
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows(fixture.args ?? ["doctor", "--browser", "edge"], fixture);

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(result.stdout, fixture.expected, fixture.name);
  }
});

test("Windows doctor fails manifests without the required description", () => {
  const cases = [
    {
      name: "missing description",
      manifest: validManifest({ description: undefined }),
    },
    {
      name: "wrong description",
      manifest: validManifest({ description: "Other Native Messaging Host" }),
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], fixture);

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(result.stdout, /manifest name\/type\/description is invalid/, fixture.name);
  }
});

test("Windows doctor accepts a valid manifest path with normalized host-path comparison", () => {
  const result = runSetupWithFakeWindows([
    "doctor",
    "--browser",
    "edge",
    "--host-path",
    "c:/sidekick/SCREEN-SIDEKICK-NATIVE-HOST.EXE",
  ]);

  assert.equal(result.status, 0, result.stdout);
  assert.match(result.stdout, /\[OK\] Windows registry manifest:/);
});

test("Windows doctor accepts the exact expected manifest allowed origin", () => {
  const result = runSetupWithFakeWindows([
    "doctor",
    "--browser",
    "edge",
    "--extension-id",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  ]);

  assert.equal(result.status, 0, result.stdout);
  assert.match(result.stdout, /\[OK\] Windows registry manifest:/);
});

test("Windows doctor skips allowed origin exact matching without an expected extension ID", () => {
  const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], {
    manifest: validManifest({
      allowed_origins: [
        "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
        "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/",
      ],
    }),
  });

  assert.equal(result.status, 0, result.stdout);
  assert.match(result.stdout, /\[SKIP\] extension ID comparison/);
  assert.match(result.stdout, /\[OK\] Windows registry manifest:/);
});

test("Windows doctor fails manifest allowed origins that are not the exact expected set", () => {
  const expectedOrigin = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
  const extraOrigin = "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/";
  const cases = [
    {
      name: "extra origin",
      manifest: validManifest({ allowed_origins: [expectedOrigin, extraOrigin] }),
    },
    {
      name: "different origin",
      manifest: validManifest({ allowed_origins: [extraOrigin] }),
    },
    {
      name: "missing allowed origins",
      manifest: validManifest({ allowed_origins: undefined }),
    },
    {
      name: "non-array allowed origins",
      manifest: validManifest({ allowed_origins: expectedOrigin }),
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows([
      "doctor",
      "--browser",
      "edge",
      "--extension-id",
      "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ], fixture);

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(
      result.stdout,
      /allowed_origins must exactly match chrome-extension:\/\/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\//,
      fixture.name,
    );
    assert.doesNotMatch(result.stdout, /bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/, fixture.name);
  }
});

test("Windows doctor mirrors native-host config parser failures", () => {
  const cases = [
    {
      name: "unsupported schema",
      config: validNativeHostConfig({ schema_version: "screen_sidekick_native_host_config.v9" }),
      expected: /config schema_version is unsupported/,
    },
    {
      name: "unknown field",
      config: { ...validNativeHostConfig(), extra: true },
      expected: /config contains unknown field: extra/,
    },
    {
      name: "missing field",
      config: { ...validNativeHostConfig(), wsl_workdir: undefined },
      expected: /config is missing wsl_workdir/,
    },
    {
      name: "invalid Linux path",
      config: validNativeHostConfig({ wsl_workdir: "relative/path" }),
      expected: /config wsl_workdir must be an absolute Linux path without parent traversal/,
    },
    {
      name: "invalid WSL PATH",
      config: validNativeHostConfig({ wsl_path: "/home/susu/.cargo/bin:" }),
      expected: /config wsl_path must be a colon-separated list of absolute Linux paths without parent traversal/,
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], fixture);

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(result.stdout, fixture.expected, fixture.name);
    assert.match(result.stdout, /\[SKIP\] Codex CLI in WSL: native host config must be valid first/);
    assert.match(result.stdout, /\[SKIP\] extension build output in WSL: native host config must be valid first/);
    assert.equal(result.calls.filter((call) => call.command === "wsl.exe").length, 1);
  }
});

test("Windows doctor passes config WSL values into codex and daemon checks", () => {
  const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"]);

  assert.equal(result.status, 0, result.stdout);
  const wslCalls = result.calls.filter((call) => call.command === "wsl.exe").map((call) => call.args);
  assert.deepEqual(wslCalls, [
    ["--status"],
    ["-d", "Ubuntu-24.04", "--", "codex", "--version"],
    [
      "-d",
      "Ubuntu-24.04",
      "--cd",
      "/home/susu/screen-sidekick",
      "--exec",
      "test",
      "-f",
      "apps/extension/dist/side_panel.js",
    ],
    [
      "-d",
      "Ubuntu-24.04",
      "--cd",
      "/home/susu/screen-sidekick",
      "--exec",
      "/home/susu/screen-sidekick/target/debug/screen-sidekick-daemon",
      "--stdio-status",
    ],
  ]);
});

test("Windows doctor applies config WSL PATH to codex and daemon checks", () => {
  const result = runSetupWithFakeWindows([
    "doctor",
    "--browser",
    "edge",
    "--wsl-path",
    wslPath,
  ], {
    config: validNativeHostConfig({ wsl_path: wslPath }),
  });

  assert.equal(result.status, 0, result.stdout);
  const wslCalls = result.calls.filter((call) => call.command === "wsl.exe").map((call) => call.args);
  assert.deepEqual(wslCalls, [
    ["--status"],
    [
      "-d",
      "Ubuntu-24.04",
      "--cd",
      "/home/susu/screen-sidekick",
      "--exec",
      "env",
      `PATH=${wslPath}`,
      "codex",
      "--version",
    ],
    [
      "-d",
      "Ubuntu-24.04",
      "--cd",
      "/home/susu/screen-sidekick",
      "--exec",
      "env",
      `PATH=${wslPath}`,
      "test",
      "-f",
      "apps/extension/dist/side_panel.js",
    ],
    [
      "-d",
      "Ubuntu-24.04",
      "--cd",
      "/home/susu/screen-sidekick",
      "--exec",
      "env",
      `PATH=${wslPath}`,
      "/home/susu/screen-sidekick/target/debug/screen-sidekick-daemon",
      "--stdio-status",
    ],
  ]);
});

test("Windows doctor fails when WSL extension build output is missing", () => {
  const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], {
    extensionBuildOutputExists: false,
  });

  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stdout, /\[FAIL\] extension build output in WSL: missing apps\/extension\/dist\/side_panel\.js/);
});

test("Windows doctor requires daemon status token", () => {
  const cases = [
    {
      name: "missing token",
      daemonStatus: { token: undefined },
      expected: /status token is missing/,
    },
    {
      name: "empty token",
      daemonStatus: { token: "" },
      expected: /status token is missing/,
    },
    {
      name: "control character token",
      daemonStatus: { token: "pairing\u0007token" },
      expected: /status token is invalid/,
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], {
      daemonStatus: fixture.daemonStatus,
    });

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(result.stdout, /\[FAIL\] WSL daemon stdio status:/, fixture.name);
    assert.match(result.stdout, fixture.expected, fixture.name);
    assert.doesNotMatch(result.stdout, /pairing/);
  }
});

test("Windows doctor rejects daemon status ws_url values outside native-host sidecar contract", () => {
  const cases = [
    {
      name: "localhost host",
      wsUrl: "ws://localhost:43001/v0/ws",
    },
    {
      name: "http scheme",
      wsUrl: "http://127.0.0.1:43001/v0/ws",
    },
    {
      name: "missing port",
      wsUrl: "ws://127.0.0.1/v0/ws",
    },
    {
      name: "wrong path",
      wsUrl: "ws://127.0.0.1:43001/other",
    },
    {
      name: "query",
      wsUrl: "ws://127.0.0.1:43001/v0/ws?token=SECRET",
    },
    {
      name: "fragment",
      wsUrl: "ws://127.0.0.1:43001/v0/ws#fragment",
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], {
      daemonStatus: {
        ws_url: fixture.wsUrl,
        token: "secret-token",
      },
    });

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(result.stdout, /\[FAIL\] WSL daemon stdio status:/, fixture.name);
    assert.match(result.stdout, /status ws_url is not sidecar loopback WebSocket endpoint/, fixture.name);
    assert.doesNotMatch(result.stdout, /secret-token/);
  }
});

test("Windows doctor parses only the first newline-terminated daemon status line", () => {
  const validStatusLine = JSON.stringify(validDaemonStatus({ token: "secret-token" }));
  const cases = [
    {
      name: "banner before status",
      daemonStatusStdout: `starting daemon\n${validStatusLine}\n`,
      expected: /status line is not valid JSON/,
    },
    {
      name: "oversized first line",
      daemonStatusStdout: `${"x".repeat(8 * 1024 + 1)}\n${validStatusLine}\n`,
      expected: /status line is too large/,
    },
    {
      name: "missing newline",
      daemonStatusStdout: validStatusLine,
      expected: /status line is not newline terminated/,
    },
  ];

  for (const fixture of cases) {
    const result = runSetupWithFakeWindows(["doctor", "--browser", "edge"], {
      daemonStatusStdout: fixture.daemonStatusStdout,
    });

    assert.equal(result.status, 1, `${fixture.name}\n${result.stdout}`);
    assert.match(result.stdout, /\[FAIL\] WSL daemon stdio status:/, fixture.name);
    assert.match(result.stdout, fixture.expected, fixture.name);
    assert.doesNotMatch(result.stdout, /secret-token/);
  }
});

test("local doctor uses the shared daemon status contract", () => {
  const daemonBinary = "/tmp/screen-sidekick-daemon";
  const result = runSetupWithFakeLocal([
    "doctor",
    "--browser",
    "edge",
    "--wsl-daemon-binary",
    daemonBinary,
  ], {
    daemonBinary,
    daemonStatus: { token: undefined },
  });

  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stdout, /\[FAIL\] daemon stdio status: status token is missing/);
});

test("local doctor checks extension files from the configured WSL workdir", () => {
  const localWorkdir = "/tmp/screen-sidekick configured";
  const daemonBinary = `${localWorkdir}/target/debug/screen-sidekick-daemon`;
  const result = runSetupWithFakeLocal([
    "doctor",
    "--browser",
    "edge",
    "--wsl-workdir",
    localWorkdir,
  ], {
    daemonBinary,
    localWorkdir,
    extensionBuildOutputExists: false,
  });

  assert.equal(result.status, 1, result.stdout);
  assert.match(
    result.stdout,
    /\[FAIL\] extension build output: missing: \/tmp\/screen-sidekick configured\/apps\/extension\/dist\/side_panel\.js/,
  );
});

function runSetup(args) {
  const env = {
    ...process.env,
    APPDATA: "C:\\Users\\tester\\AppData\\Roaming",
  };
  for (const key of cleanEnvKeys) {
    delete env[key];
  }
  return spawnSync(process.execPath, [scriptPath, ...args], {
    encoding: "utf8",
    env,
    maxBuffer: 1024 * 1024,
  });
}
