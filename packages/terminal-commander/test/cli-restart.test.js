// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// `terminal-commander restart` handler tests. Mocks detect + exec so
// the suite is portable to any host and never spawns a real daemon.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const path = require("node:path");

const { runRestart, DAEMON_RESTART_CMD } = require("../lib/cli/restart.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");
const {
  detectRuntimeEnvironment,
  windowsUpdateScopes,
  describeError,
} = require("../lib/cli/runtime_environment.js");
const {
  planUpdatePreflight,
  executeUpdatePreflight,
} = require("../lib/cli/update_preflight.js");

// A fake child that emits the given exit code on next tick.
function fakeChild(code, stdout) {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  setImmediate(() => {
    if (stdout) child.stdout.emit("data", Buffer.from(stdout, "utf8"));
    child.emit("close", code);
  });
  return child;
}

function okDetect(defaultName) {
  return {
    host_platform: "win32",
    wsl_callable: true,
    default_distro: defaultName,
    distros: [{ name: defaultName, state: "Running", wsl_version: 2, is_default: true }],
    reason: DETECT_REASONS.OK,
  };
}

test("DAEMON_RESTART_CMD always forces (--force)", () => {
  assert.match(DAEMON_RESTART_CMD, /terminal-commanderd update --force/);
});

test("restart on Windows selects the native daemon by default", async () => {
  let seen = null;
  const r = await runRestart({
    platform: "win32",
    env: {},
    flags: {},
    detect: async () => {
      throw new Error("native Windows restart must not probe WSL");
    },
    resolveBinary: () => ({
      reason: "ok",
      binaryPath: "C:\\tc\\terminal-commanderd.exe",
    }),
    exec: ({ file, argv }) => {
      seen = { file, argv };
      return fakeChild(0, "terminal-commanderd: daemon running");
    },
  });

  assert.equal(r.status, "ok");
  assert.deepEqual(seen, {
    file: "C:\\tc\\terminal-commanderd.exe",
    argv: ["update", "--force"],
  });
  assert.match(r.output, /native Windows daemon restarted/);
});

test("restart on Windows dispatches daemon update --force through WSL", async () => {
  let seenArgv = null;
  const r = await runRestart({
    platform: "win32",
    env: { TC_WSL_DISTRO: "Ubuntu-24.04" },
    flags: {},
    detect: async () => okDetect("Ubuntu-24.04"),
    exec: ({ file, argv }) => {
      seenArgv = { file, argv };
      return fakeChild(0, "terminal-commanderd: replaced 0.1.17 -> 0.1.18");
    },
  });
  assert.equal(r.status, "ok");
  assert.equal(r.exit_code, 0);
  assert.equal(seenArgv.file, "wsl.exe");
  assert.deepEqual(seenArgv.argv.slice(0, 3), ["-d", "Ubuntu-24.04", "--"]);
  // The bash -lc payload must invoke the forced daemon update.
  const payload = seenArgv.argv[seenArgv.argv.length - 1];
  assert.match(payload, /terminal-commanderd update --force/);
  assert.match(r.output, /restarted in 'Ubuntu-24\.04'/);
});

test("restart on Windows neutralizes ambient WSLENV (no credential crosses into WSL)", async () => {
  // With a session token, WSLENV must be reduced to the TC-only
  // allowlist so wsl.exe forwards ONLY TC_SESSION -- never an operator's
  // ambient WSL_SUDO_CREDENTIAL carried by an ambient WSLENV entry.
  let withSession = null;
  await runRestart({
    platform: "win32",
    env: {
      TC_WSL_DISTRO: "Ubuntu-24.04",
      TC_SESSION: "sess-abc",
      WSLENV: "WSL_SUDO_CREDENTIAL/u",
      WSL_SUDO_CREDENTIAL: "super-secret",
    },
    flags: {},
    detect: async () => okDetect("Ubuntu-24.04"),
    exec: ({ env }) => {
      withSession = env;
      return fakeChild(0, "ok");
    },
  });
  assert.equal(withSession.WSLENV, "TC_SESSION/u");

  // Without a session token, the ambient WSLENV is dropped entirely so
  // nothing crosses the boundary.
  let noSession = null;
  await runRestart({
    platform: "win32",
    env: {
      TC_WSL_DISTRO: "Ubuntu-24.04",
      WSLENV: "WSL_SUDO_CREDENTIAL/u",
      WSL_SUDO_CREDENTIAL: "super-secret",
    },
    flags: {},
    detect: async () => okDetect("Ubuntu-24.04"),
    exec: ({ env }) => {
      noSession = env;
      return fakeChild(0, "ok");
    },
  });
  assert.equal(noSession.WSLENV, undefined);
});

test("restart on Linux invokes the daemon binary directly", async () => {
  let seenArgv = null;
  const r = await runRestart({
    platform: "linux",
    env: {},
    flags: {},
    exec: ({ file, argv }) => {
      seenArgv = { file, argv };
      return fakeChild(0, "");
    },
  });
  assert.equal(r.status, "ok");
  assert.equal(seenArgv.file, "terminal-commanderd");
  assert.deepEqual(seenArgv.argv, ["update", "--force"]);
});

test("restart surfaces a non-zero daemon exit as restart_failed", async () => {
  const r = await runRestart({
    platform: "linux",
    env: {},
    flags: {},
    exec: () => fakeChild(2, "boom"),
  });
  assert.equal(r.status, "restart_failed");
  assert.equal(r.exit_code, 2);
  assert.match(r.output, /restart failed/);
});

test("restart on Windows refuses when no distro resolves", async () => {
  const r = await runRestart({
    platform: "win32",
    env: { TC_USE_LEGACY_WSL_BRIDGE: "1" },
    flags: {},
    detect: async () => ({
      host_platform: "win32",
      wsl_callable: true,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.OK,
    }),
    exec: () => {
      throw new Error("must not spawn when distro unresolved");
    },
  });
  assert.notEqual(r.status, "ok");
  assert.equal(r.exit_code, 64);
});

test("Windows selects the native runtime unless WSL is explicitly requested", () => {
  assert.deepEqual(
    detectRuntimeEnvironment({ platform: "win32", env: {}, flags: {} }),
    { status: "ok", host: "windows", runtime: "native", evidence: "win32_default" },
  );
  assert.equal(
    detectRuntimeEnvironment({
      platform: "win32",
      env: { TC_USE_LEGACY_WSL_BRIDGE: "1" },
      flags: {},
    }).runtime,
    "wsl",
  );
  assert.equal(
    detectRuntimeEnvironment({
      platform: "win32",
      env: { TC_WSL_DISTRO: "Ubuntu-24.04" },
      flags: {},
    }).runtime,
    "wsl",
  );
  assert.equal(
    detectRuntimeEnvironment({
      platform: "win32",
      env: {},
      flags: { distro: "Ubuntu-24.04" },
    }).runtime,
    "wsl",
  );
});

test("unsupported hosts are explicit and never classified as unknown", () => {
  assert.deepEqual(
    detectRuntimeEnvironment({ platform: "plan9", env: {}, flags: {} }),
    {
      status: "unsupported",
      host: "plan9",
      runtime: null,
      evidence: "unsupported_platform:plan9",
    },
  );
});

test("Windows update owns both npm staging and stable native binary scopes", () => {
  const packageRoot = path.join("C:\\", "npm", "node_modules", "terminal-commander");
  const scopes = windowsUpdateScopes({
    platform: "win32",
    env: { LOCALAPPDATA: path.join("C:\\", "Users", "me", "AppData", "Local") },
    packageRoot,
  });
  assert.deepEqual(scopes, [
    path.dirname(packageRoot),
    path.join("C:\\", "Users", "me", "AppData", "Local", "terminal-commander", "bin"),
  ]);
});

test("error diagnostics preserve messages instead of collapsing to unknown", () => {
  assert.equal(describeError(new Error("LOCALAPPDATA is missing")), "LOCALAPPDATA is missing");
  assert.equal(describeError({ code: "ELOCKED" }), "ELOCKED");
  assert.match(describeError(null), /non-Error rejection/);
});

test("Windows update planning is hermetic when the package helper exists", () => {
  const packageRoot = path.join("tmp", "npm", "node_modules", "terminal-commander");
  const localAppData = path.join("tmp", "local-app-data");
  const helperPath = path.join("tmp", "package", "terminal-commander.exe");
  const plan = planUpdatePreflight({
    platform: "win32",
    arch: "x64",
    env: { LOCALAPPDATA: localAppData },
    packageRoot,
    resolveBinary: ({ platform, arch }) => {
      assert.equal(platform, "win32");
      assert.equal(arch, "x64");
      return { reason: "ok", binaryPath: helperPath };
    },
    stableBinPath: () => assert.fail("stable helper lookup must not run"),
  });

  assert.equal(plan.status, "ready");
  assert.deepEqual(
    plan.commands.map((command) => command.file),
    [helperPath, helperPath],
  );
  assert.deepEqual(
    plan.commands.map((command) => command.scopeDir),
    [path.dirname(packageRoot), path.join(localAppData, "terminal-commander", "bin")],
  );
});

test("Windows update planning uses an explicitly resolved stable helper", () => {
  const stableHelper = path.join("tmp", "stable", "terminal-commander.exe");
  const plan = planUpdatePreflight({
    platform: "win32",
    env: { LOCALAPPDATA: path.join("tmp", "local-app-data") },
    packageRoot: path.join("tmp", "npm", "node_modules", "terminal-commander"),
    resolveBinary: () => ({ reason: "missing_platform_package" }),
    formatResolveError: () => "terminal-commander: platform package missing",
    stableBinPath: () => stableHelper,
    existsSync: (candidate) => candidate === stableHelper,
  });

  assert.equal(plan.status, "ready");
  assert.equal(plan.commands.length, 2);
  assert.ok(plan.commands.every((command) => command.file === stableHelper));
  assert.match(plan.diagnostics.join(""), /using stable update helper/);
});

test("Windows update planning degrades to npm repair when no helper exists", () => {
  const plan = planUpdatePreflight({
    platform: "win32",
    env: { LOCALAPPDATA: path.join("tmp", "local-app-data") },
    packageRoot: path.join("tmp", "npm", "node_modules", "terminal-commander"),
    resolveBinary: () => ({ reason: "missing_platform_package" }),
    formatResolveError: () => "terminal-commander: platform package missing",
    stableBinPath: () => path.join("tmp", "stable", "terminal-commander.exe"),
    existsSync: () => false,
  });

  assert.equal(plan.status, "degraded_repair");
  assert.equal(plan.exitCode, 0);
  assert.deepEqual(plan.commands, []);
  assert.match(plan.diagnostics.join(""), /continuing with npm repair/);
});

test("Windows update planning survives resolver and stable lookup exceptions", () => {
  const plan = planUpdatePreflight({
    platform: "win32",
    env: {},
    packageRoot: path.join("tmp", "npm", "node_modules", "terminal-commander"),
    resolveBinary: () => {
      throw new Error("resolver exploded");
    },
    stableBinPath: () => {
      throw new Error("LOCALAPPDATA missing");
    },
  });

  assert.equal(plan.status, "degraded_repair");
  assert.equal(plan.exitCode, 0);
  assert.match(plan.diagnostics.join(""), /stable update helper lookup failed/);
  assert.match(plan.diagnostics.join(""), /binary resolver failed: resolver exploded/);
});

test("Windows update planning rejects malformed scope data before spawning", () => {
  const plan = planUpdatePreflight({
    platform: "win32",
    env: {},
    packageRoot: "package",
    resolveBinary: () => ({ reason: "ok", binaryPath: "helper.exe" }),
    windowsUpdateScopes: () => [],
  });

  assert.equal(plan.status, "invalid_environment");
  assert.equal(plan.exitCode, 64);
  assert.deepEqual(plan.commands, []);
  assert.match(plan.diagnostics.join(""), /scope resolver returned invalid data/);
});

test("update preflight execution is sequential and stops on the first failure", async () => {
  const seen = [];
  const plan = {
    exitCode: 0,
    diagnostics: ["planned\n"],
    commands: [
      { file: "helper", args: ["update-locks", "--scope-dir", "one"], scopeDir: "one" },
      { file: "helper", args: ["update-locks", "--scope-dir", "two"], scopeDir: "two" },
      { file: "helper", args: ["update-locks", "--scope-dir", "three"], scopeDir: "three" },
    ],
  };
  const diagnostics = [];
  const code = await executeUpdatePreflight(plan, {
    env: { TEST: "1" },
    writeStderr: (message) => diagnostics.push(message),
    spawn: (file, args, options) => {
      seen.push({ file, args, options });
      const child = new EventEmitter();
      const exitCode = seen.length === 2 ? 7 : 0;
      setImmediate(() => child.emit("exit", exitCode, null));
      return child;
    },
  });

  assert.equal(code, 7);
  assert.equal(seen.length, 2);
  assert.deepEqual(seen.map((call) => call.args[2]), ["one", "two"]);
  assert.ok(seen.every((call) => call.options.shell === false));
  assert.deepEqual(diagnostics, ["planned\n"]);
});

test("update preflight maps a malformed spawn result to a clear executable error", async () => {
  const diagnostics = [];
  const code = await executeUpdatePreflight(
    {
      exitCode: 0,
      diagnostics: [],
      commands: [{ file: "helper", args: [], scopeDir: "scope" }],
    },
    {
      spawn: () => null,
      writeStderr: (message) => diagnostics.push(message),
    },
  );

  assert.equal(code, 126);
  assert.match(diagnostics.join(""), /spawn returned no child process/);
});
