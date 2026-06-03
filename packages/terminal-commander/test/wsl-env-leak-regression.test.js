// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// SECURITY regression: ambient WSLENV must NOT survive into the env handed to
// any wsl.exe spawn that launches a Linux process.
//
// `wsl.exe` forwards every Windows env var NAMED in WSLENV into the Linux
// child. `buildFilteredEnv` strips secrets by variable *name* but does NOT
// rebuild WSLENV, so an ambient `WSLENV=SECRET_NAME/u` would still forward
// SECRET_NAME across the boundary. The defense is `ensureSessionInWslEnv`,
// which reduces WSLENV to the TC-only allowlist (`TC_SESSION/u`) or drops it
// entirely. This suite asserts every wrapped spawn site applies it.
//
// Two flavours of site:
//   1. exec-injectable modules (runWslBashLc / runInstallProbe) — we record
//      the `env` the module hands to its spawn wrapper.
//   2. direct-spawn modules (wsl/doctor.js, cli/doctor_daemon.js) — we patch
//      child_process.spawn to capture options.env from the live default path.
//
// `child_process.spawn` is patched BEFORE requiring the direct-spawn modules,
// because lib/wsl/doctor.js destructures `spawn` at module load.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const childProcess = require("node:child_process");

// ---- Patch spawn so direct-spawn modules never launch a real process. ----
const spawnCalls = [];
const realSpawn = childProcess.spawn;
function fakeChild() {
  const handlers = {};
  return {
    stdout: { on(ev, cb) { handlers[`stdout:${ev}`] = cb; } },
    stderr: { on(ev, cb) { handlers[`stderr:${ev}`] = cb; } },
    on(ev, cb) {
      // Fire close immediately so the awaiting promise settles.
      if (ev === "close") {
        setImmediate(() => cb(0, null));
      }
    },
    kill() {},
    pid: 4242,
  };
}
childProcess.spawn = function patchedSpawn(file, argv, options) {
  spawnCalls.push({ file, argv, env: options && options.env });
  return fakeChild();
};

// Require modules AFTER the patch so the destructured `spawn` is the fake one
// where applicable.
const {
  runWslBashLc: ensureRuntimeRunWsl,
} = require("../lib/bootstrap/ensure_wsl_runtime.js");
const {
  ensureDaemonAutostartInWsl,
} = require("../lib/bootstrap/ensure_daemon_autostart.js");
const { runInstallProbe } = require("../lib/cli/setup_cursor_wsl.js");
const { detectWsl } = require("../lib/wsl/detect.js");
const { wslDoctor } = require("../lib/wsl/doctor.js");
const { runDoctorDaemon } = require("../lib/cli/doctor_daemon.js");

test.after(() => {
  childProcess.spawn = realSpawn;
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Records the env each module passes to its injected exec wrapper.
function makeExecRecorder() {
  const calls = [];
  const exec = ({ wslPath, argv, env }) => {
    calls.push({ wslPath, argv, env });
    // Return a minimal child that resolves its consumers.
    const handlers = {};
    return {
      stdout: { on() {} },
      stderr: { on() {} },
      on(ev, cb) {
        handlers[ev] = cb;
        if (ev === "close") setImmediate(() => cb(0));
      },
      kill() {},
      pid: 7777,
    };
  };
  return { exec, calls };
}

const AMBIENT_SECRET = "WSL_SUDO_CREDENTIAL/u:SOME_OTHER_SECRET/p";

function assertWslenvSanitizedWithSession(env, label) {
  assert.equal(
    env.WSLENV,
    "TC_SESSION/u",
    `${label}: WSLENV must be exactly TC_SESSION/u, got ${env.WSLENV}`,
  );
  assert.doesNotMatch(
    env.WSLENV,
    /WSL_SUDO_CREDENTIAL/,
    `${label}: ambient credential must NOT cross the boundary`,
  );
  assert.doesNotMatch(
    env.WSLENV,
    /SOME_OTHER_SECRET/,
    `${label}: ambient entries must be dropped`,
  );
}

function assertWslenvDropped(env, label) {
  assert.equal(
    "WSLENV" in env,
    false,
    `${label}: ambient WSLENV must be dropped when there is no TC_SESSION`,
  );
}

// ---------------------------------------------------------------------------
// 1. exec-injectable sites
// ---------------------------------------------------------------------------

test("ensure_wsl_runtime: ambient WSLENV does not survive into the wsl spawn (with TC_SESSION)", async () => {
  const rec = makeExecRecorder();
  await ensureRuntimeRunWsl({
    distro: "Ubuntu",
    cmd: "true",
    env: { PATH: "C:\\Windows", TC_SESSION: "tc-abc123", WSLENV: AMBIENT_SECRET },
    exec: rec.exec,
    wslPath: "wsl.exe",
    timeoutMs: 5000,
  });
  assert.equal(rec.calls.length, 1);
  assertWslenvSanitizedWithSession(rec.calls[0].env, "ensure_wsl_runtime");
});

test("ensure_wsl_runtime: ambient WSLENV is dropped when TC_SESSION is absent", async () => {
  const rec = makeExecRecorder();
  await ensureRuntimeRunWsl({
    distro: "Ubuntu",
    cmd: "true",
    env: { PATH: "C:\\Windows", WSLENV: AMBIENT_SECRET },
    exec: rec.exec,
    wslPath: "wsl.exe",
    timeoutMs: 5000,
  });
  assert.equal(rec.calls.length, 1);
  assertWslenvDropped(rec.calls[0].env, "ensure_wsl_runtime");
});

test("ensure_daemon_autostart: ambient WSLENV does not survive into the wsl spawn", async () => {
  const rec = makeExecRecorder();
  await ensureDaemonAutostartInWsl({
    platform: "win32",
    distro: "Ubuntu",
    // Force the install path to run (autostart not skipped).
    env: {
      PATH: "C:\\Windows",
      TC_SESSION: "tc-def456",
      WSLENV: AMBIENT_SECRET,
      TC_SKIP_DAEMON_AUTOSTART: "0",
    },
    exec: rec.exec,
    wslPath: "wsl.exe",
    timeoutMs: 5000,
  });
  assert.equal(
    rec.calls.length,
    1,
    "daemon autostart install must have spawned exactly once",
  );
  assertWslenvSanitizedWithSession(rec.calls[0].env, "ensure_daemon_autostart");
});

test("setup_cursor_wsl runInstallProbe: ambient WSLENV does not survive into the wsl spawn", async () => {
  const rec = makeExecRecorder();
  await runInstallProbe({
    distro: "Ubuntu",
    env: { PATH: "C:\\Windows", TC_SESSION: "tc-ghi789", WSLENV: AMBIENT_SECRET },
    exec: rec.exec,
    wslPath: "wsl.exe",
    timeoutMs: 5000,
  });
  assert.equal(rec.calls.length, 1);
  assertWslenvSanitizedWithSession(rec.calls[0].env, "setup_cursor_wsl");
});

// ---------------------------------------------------------------------------
// 2. direct-spawn sites (live default path through the patched child_process)
// ---------------------------------------------------------------------------

test("wsl/doctor runtime probe: ambient WSLENV does not survive into the live wsl spawn", async () => {
  spawnCalls.length = 0;
  const prevSession = process.env.TC_SESSION;
  const prevWslenv = process.env.WSLENV;
  process.env.TC_SESSION = "tc-doctor01";
  process.env.WSLENV = AMBIENT_SECRET;
  try {
    await wslDoctor({
      distro: "Ubuntu",
      platform: "win32",
      probeRuntime: true,
      // Pre-computed detect result so the runtime probe path is reached without
      // a real `wsl -l -v` discovery call.
      detectResult: {
        host_platform: "win32",
        wsl_callable: true,
        default_distro: "Ubuntu",
        distros: [{ name: "Ubuntu" }],
        reason: "ok",
      },
      wslPath: "wsl.exe",
      timeoutMs: 5000,
    });
  } finally {
    if (prevSession === undefined) delete process.env.TC_SESSION;
    else process.env.TC_SESSION = prevSession;
    if (prevWslenv === undefined) delete process.env.WSLENV;
    else process.env.WSLENV = prevWslenv;
  }
  // The runtime probe is the spawn that launches a Linux process.
  const probe = spawnCalls.find(
    (c) => Array.isArray(c.argv) && c.argv.includes("-d"),
  );
  assert.ok(probe, "expected a `wsl -d <distro> -- bash -lc ...` runtime probe spawn");
  assertWslenvSanitizedWithSession(probe.env, "wsl/doctor");
});

test("cli/doctor_daemon: ambient WSLENV does not survive into the live wsl spawn", async () => {
  spawnCalls.length = 0;
  const prevSession = process.env.TC_SESSION;
  const prevWslenv = process.env.WSLENV;
  process.env.TC_SESSION = "tc-daemon01";
  process.env.WSLENV = AMBIENT_SECRET;
  try {
    await runDoctorDaemon({
      platform: "win32",
      // Resolve a distro deterministically without a real `wsl -l -v` call.
      detect: async () => ({
        host_platform: "win32",
        wsl_callable: true,
        default_distro: "Ubuntu",
        distros: [{ name: "Ubuntu" }],
        reason: "ok",
      }),
      flags: { distro: "Ubuntu" },
      env: { ...process.env, TC_SESSION: "tc-daemon01", WSLENV: AMBIENT_SECRET },
      wslPath: "wsl.exe",
    });
  } finally {
    if (prevSession === undefined) delete process.env.TC_SESSION;
    else process.env.TC_SESSION = prevSession;
    if (prevWslenv === undefined) delete process.env.WSLENV;
    else process.env.WSLENV = prevWslenv;
  }
  const probe = spawnCalls.find(
    (c) => Array.isArray(c.argv) && c.argv.includes("-d"),
  );
  assert.ok(probe, "expected a `wsl -d <distro> -- bash -lc ...` daemon probe spawn");
  assertWslenvSanitizedWithSession(probe.env, "cli/doctor_daemon");
});

// M10: wsl/detect.js `defaultExec` is a direct-spawn site. Its only call is the
// host-side discovery `wsl.exe -l -v`, which launches no Linux process, so
// WSLENV forwarding is moot for it in practice — but it previously passed NO
// `env` at all, so the spawned wsl.exe inherited the FULL ambient process.env
// (every secret-shaped var) by default. The fix routes the spawn env through
// `ensureSessionInWslEnv(buildFilteredEnv(process.env))`, identical to the
// doctor runtime probe. This asserts (a) an ambient secret-shaped var is
// stripped from the spawn env and (b) WSLENV is reduced to the TC-only
// allowlist. The detect spawn is identified by its `-l` argv (no `-d`).
test("wsl/detect discovery: spawn drops ambient secrets and sanitizes WSLENV", async () => {
  spawnCalls.length = 0;
  const prevSession = process.env.TC_SESSION;
  const prevWslenv = process.env.WSLENV;
  const prevExplicitSecret = process.env.ANTHROPIC_API_KEY;
  const prevPatternSecret = process.env.MY_AMBIENT_SECRET;
  process.env.TC_SESSION = "tc-detect01";
  process.env.WSLENV = AMBIENT_SECRET;
  // (a) Two flavours of secret-shaped var that buildFilteredEnv must strip:
  //   - explicit allowlist key
  //   - name pattern match (/_SECRET$/i)
  process.env.ANTHROPIC_API_KEY = "sk-ant-LEAK-ME-IF-YOU-DARE";
  process.env.MY_AMBIENT_SECRET = "do-not-forward";
  try {
    await detectWsl({
      platform: "win32",
      wslPath: "wsl.exe",
      timeoutMs: 5000,
    });
  } finally {
    if (prevSession === undefined) delete process.env.TC_SESSION;
    else process.env.TC_SESSION = prevSession;
    if (prevWslenv === undefined) delete process.env.WSLENV;
    else process.env.WSLENV = prevWslenv;
    if (prevExplicitSecret === undefined) delete process.env.ANTHROPIC_API_KEY;
    else process.env.ANTHROPIC_API_KEY = prevExplicitSecret;
    if (prevPatternSecret === undefined) delete process.env.MY_AMBIENT_SECRET;
    else process.env.MY_AMBIENT_SECRET = prevPatternSecret;
  }
  // The discovery spawn is `wsl.exe -l -v` (no `-d`).
  const discovery = spawnCalls.find(
    (c) => Array.isArray(c.argv) && c.argv.includes("-l") && !c.argv.includes("-d"),
  );
  assert.ok(discovery, "expected a `wsl -l -v` discovery spawn");
  assert.ok(
    discovery.env,
    "wsl/detect: spawn must pass an explicit env (not inherit ambient process.env)",
  );
  // (a) ambient secret-shaped vars must NOT be present in the spawn env.
  assert.equal(
    "ANTHROPIC_API_KEY" in discovery.env,
    false,
    "wsl/detect: explicit secret key must be stripped from the spawn env",
  );
  assert.equal(
    "MY_AMBIENT_SECRET" in discovery.env,
    false,
    "wsl/detect: pattern-matched secret var must be stripped from the spawn env",
  );
  // (b) WSLENV must be reduced to the TC-only allowlist, with no ambient
  //     credential forwarding.
  assertWslenvSanitizedWithSession(discovery.env, "wsl/detect");
});
