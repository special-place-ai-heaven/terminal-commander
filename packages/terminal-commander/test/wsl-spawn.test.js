// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS04 spawnWslBridge() unit tests. Mocks detect / doctor / exec
// so the suite runs deterministically on any host (no wsl.exe
// required).

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  spawnWslBridge,
  resolveBridgeDistro,
  buildFilteredEnv,
  isSecretEnvKey,
  BRIDGE_STATUSES,
  BRIDGE_PROBE_CMD,
  SECRET_ENV_PATTERNS,
  EXPLICIT_SECRET_KEYS,
} = require("../lib/wsl/spawn.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");
const { DOCTOR_STATUSES } = require("../lib/wsl/doctor.js");

function okDetect(distros, defaultName) {
  return {
    host_platform: "win32",
    wsl_callable: true,
    default_distro: defaultName || null,
    distros: distros.map((d) =>
      typeof d === "string"
        ? { name: d, state: "Running", wsl_version: 2, is_default: d === defaultName }
        : d,
    ),
    reason: DETECT_REASONS.OK,
  };
}

function makeMockDetect(detectResult) {
  return async () => detectResult;
}

function makeMockDoctor(status) {
  return async () => ({
    status,
    reason: status,
    distro: null,
    runtime_present: status === DOCTOR_STATUSES.RUNTIME_PRESENT,
    hint: "",
  });
}

function makeRecorder() {
  const calls = [];
  const exec = (args) => {
    calls.push(args);
    return {
      on() {
        // Never called in returnInsteadOfMirror=true path.
      },
      kill() {
        /* noop */
      },
      pid: 9999,
    };
  };
  return { exec, calls };
}

test("BRIDGE_STATUSES exposes the full status enum", () => {
  assert.deepEqual(
    new Set(Object.values(BRIDGE_STATUSES)),
    new Set([
      "ok",
      "unsupported_host",
      "unsafe_distro_name",
      "no_default_distro",
      "distro_not_found",
      "wsl_not_found",
      "no_distros",
      "wsl_command_failed",
      "check_timeout",
      "runtime_missing",
      "bridge_spawn_failed",
      "bridge_child_exit",
    ]),
  );
});

test("BRIDGE_PROBE_CMD uses Linux-first PATH then exec terminal-commander-mcp", () => {
  assert.match(BRIDGE_PROBE_CMD, /exec terminal-commander-mcp$/);
  assert.match(BRIDGE_PROBE_CMD, /npm-global\/bin/);
  assert.equal(BRIDGE_PROBE_CMD.includes("${"), false);
  assert.equal(BRIDGE_PROBE_CMD.includes("sudo"), false);
  assert.equal(BRIDGE_PROBE_CMD.includes("install"), false);
});

test("non-win32 platform short-circuits to unsupported_host", async () => {
  const r = await spawnWslBridge({
    platform: "linux",
    env: {},
    detect: async () => {
      throw new Error("detect must not be called");
    },
  });
  assert.equal(r.status, "unsupported_host");
});

test("detect.wsl_not_found maps to bridge wsl_not_found", async () => {
  const r = await spawnWslBridge({
    platform: "win32",
    env: {},
    detect: makeMockDetect({
      host_platform: "win32",
      wsl_callable: false,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.WSL_NOT_FOUND,
    }),
  });
  assert.equal(r.status, "wsl_not_found");
  assert.match(r.hint, /wsl\.exe not found/);
});

test("detect.no_distros maps to bridge no_distros", async () => {
  const r = await spawnWslBridge({
    platform: "win32",
    env: {},
    detect: makeMockDetect({
      host_platform: "win32",
      wsl_callable: true,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.NO_DISTROS,
    }),
  });
  assert.equal(r.status, "no_distros");
});

test("TC_WSL_DISTRO with unsafe name returns unsafe_distro_name without spawning", async () => {
  const rec = makeRecorder();
  const r = await spawnWslBridge({
    platform: "win32",
    env: { TC_WSL_DISTRO: "Ubuntu; rm -rf /" },
    detect: makeMockDetect(okDetect(["Ubuntu"], "Ubuntu")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "unsafe_distro_name");
  assert.equal(rec.calls.length, 0);
});

test("TC_WSL_DISTRO not in whitelist returns distro_not_found without spawning", async () => {
  const rec = makeRecorder();
  const r = await spawnWslBridge({
    platform: "win32",
    env: { TC_WSL_DISTRO: "Fedora" },
    detect: makeMockDetect(okDetect(["Ubuntu", "Debian"], "Ubuntu")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "distro_not_found");
  assert.equal(rec.calls.length, 0);
});

test("no TC_WSL_DISTRO + no detect default returns no_default_distro without spawning", async () => {
  const rec = makeRecorder();
  const r = await spawnWslBridge({
    platform: "win32",
    env: {},
    detect: makeMockDetect(okDetect(["Ubuntu"], null)),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "no_default_distro");
  assert.equal(rec.calls.length, 0);
  assert.match(r.hint, /TC_WSL_DISTRO/);
});

test("doctor.runtime_missing short-circuits to runtime_missing without spawning", async () => {
  const rec = makeRecorder();
  const r = await spawnWslBridge({
    platform: "win32",
    env: { TC_SKIP_BOOTSTRAP: "1" },
    detect: makeMockDetect(okDetect(["Ubuntu-24.04"], "Ubuntu-24.04")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_MISSING),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "runtime_missing");
  assert.match(r.hint, /not installed inside 'Ubuntu-24\.04'/);
  assert.equal(rec.calls.length, 0);
});

test("TC_WSL_SKIP_DOCTOR=1 skips the runtime gate but keeps distro safety", async () => {
  // doctor mock returns runtime_missing — but TC_WSL_SKIP_DOCTOR=1
  // must bypass that check and proceed to spawn.
  const rec = makeRecorder();
  let doctorCalled = false;
  const doctor = async () => {
    doctorCalled = true;
    return makeMockDoctor(DOCTOR_STATUSES.RUNTIME_MISSING)();
  };
  const r = await spawnWslBridge({
    platform: "win32",
    env: { TC_WSL_SKIP_DOCTOR: "1" },
    detect: makeMockDetect(okDetect(["Ubuntu"], "Ubuntu")),
    doctor,
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(doctorCalled, false, "doctor must not be invoked when TC_WSL_SKIP_DOCTOR=1");
  assert.equal(r.status, "ok");
  assert.equal(rec.calls.length, 1);
});

test("TC_WSL_SKIP_DOCTOR=1 does NOT bypass distro safety whitelist", async () => {
  const rec = makeRecorder();
  const r = await spawnWslBridge({
    platform: "win32",
    env: { TC_WSL_SKIP_DOCTOR: "1", TC_WSL_DISTRO: "Bad; rm" },
    detect: makeMockDetect(okDetect(["Ubuntu"], "Ubuntu")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "unsafe_distro_name");
  assert.equal(rec.calls.length, 0);
});

test("happy path spawns wsl.exe with exact argv + options shape", async () => {
  const rec = makeRecorder();
  const r = await spawnWslBridge({
    platform: "win32",
    env: { PATH: "C:\\Windows;C:\\Windows\\System32" },
    detect: makeMockDetect(okDetect(["Ubuntu-24.04"], "Ubuntu-24.04")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    argv: ["--extra", "flag"],
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "ok");
  assert.equal(rec.calls.length, 1);
  const call = rec.calls[0];
  assert.equal(call.wslPath, "wsl.exe");
  assert.equal(call.argv[5], BRIDGE_PROBE_CMD);
  assert.deepEqual(call.argv, [
    "-d",
    "Ubuntu-24.04",
    "--",
    "bash",
    "-lc",
    BRIDGE_PROBE_CMD,
    "--extra",
    "flag",
  ]);
});

test("happy path passes filtered env (no token-shaped vars) to spawn", async () => {
  const rec = makeRecorder();
  await spawnWslBridge({
    platform: "win32",
    env: {
      PATH: "C:\\Windows",
      USERPROFILE: "C:\\Users\\op",
      NPM_TOKEN: "secret-npm-token-value-aaaaaaaaaa",
      GITHUB_TOKEN: "secret-gh-token-value",
      ANTHROPIC_API_KEY: "secret-anthropic-value",
      MY_PASSWORD: "secret-password-value",
      AWS_SECRET_ACCESS_KEY: "secret-aws-value",
      NORMAL_VAR: "normal-value",
    },
    detect: makeMockDetect(okDetect(["Ubuntu"], "Ubuntu")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec: rec.exec,
    returnInsteadOfMirror: true,
  });
  const passedEnv = rec.calls[0].env;
  assert.equal(passedEnv.PATH, "C:\\Windows");
  assert.equal(passedEnv.USERPROFILE, "C:\\Users\\op");
  assert.equal(passedEnv.NORMAL_VAR, "normal-value");
  // Token-shaped vars must not appear in the child's env.
  assert.equal("NPM_TOKEN" in passedEnv, false);
  assert.equal("GITHUB_TOKEN" in passedEnv, false);
  assert.equal("ANTHROPIC_API_KEY" in passedEnv, false);
  assert.equal("MY_PASSWORD" in passedEnv, false);
  assert.equal("AWS_SECRET_ACCESS_KEY" in passedEnv, false);
});

test("buildFilteredEnv is pure (input env unchanged)", () => {
  const input = { PATH: "x", NPM_TOKEN: "secret", OK: "y" };
  const filtered = buildFilteredEnv(input);
  assert.equal(input.NPM_TOKEN, "secret", "input must not be mutated");
  assert.equal("NPM_TOKEN" in filtered, false);
  assert.equal(filtered.PATH, "x");
  assert.equal(filtered.OK, "y");
});

test("isSecretEnvKey matches explicit + pattern-shaped keys", () => {
  for (const key of EXPLICIT_SECRET_KEYS) {
    assert.equal(isSecretEnvKey(key), true, `${key} must be classified secret`);
  }
  for (const key of [
    "FOO_TOKEN",
    "BAR_SECRET",
    "BAZ_PASSWORD",
    "QUX_PASS",
    "ZAB_API_KEY",
    "ABC_APIKEY",
    "MY_FANCY_TOKEN",
    "AWS_SESSION_TOKEN",
    "AWS_SECRET_ACCESS_KEY",
  ]) {
    assert.equal(isSecretEnvKey(key), true, `${key} must be classified secret`);
  }
  for (const key of ["PATH", "HOME", "USERPROFILE", "NORMAL", "FOO", "PATHTOKEN_X"]) {
    assert.equal(isSecretEnvKey(key), false, `${key} must NOT be classified secret`);
  }
});

test("resolveBridgeDistro priority chain matches the locked contract", () => {
  // P1: env override.
  let r = resolveBridgeDistro({ TC_WSL_DISTRO: "Ubuntu" }, okDetect(["Ubuntu"], null));
  assert.deepEqual(r, { status: "ok", distro: "Ubuntu" });

  // P1: env override but unsafe.
  r = resolveBridgeDistro({ TC_WSL_DISTRO: "Bad name" }, okDetect(["Ubuntu"], null));
  assert.equal(r.status, "unsafe_distro_name");

  // P1: env override but not in whitelist.
  r = resolveBridgeDistro({ TC_WSL_DISTRO: "Fedora" }, okDetect(["Ubuntu"], null));
  assert.equal(r.status, "distro_not_found");

  // P2: detect default.
  r = resolveBridgeDistro({}, okDetect(["Ubuntu"], "Ubuntu"));
  assert.deepEqual(r, { status: "ok", distro: "Ubuntu" });

  // P3: refuse.
  r = resolveBridgeDistro({}, okDetect(["Ubuntu"], null));
  assert.equal(r.status, "no_default_distro");
});

test("exec thrown error during synchronous spawn yields bridge_spawn_failed", async () => {
  const exec = () => {
    throw new Error("ENOENT");
  };
  const r = await spawnWslBridge({
    platform: "win32",
    env: {},
    detect: makeMockDetect(okDetect(["Ubuntu"], "Ubuntu")),
    doctor: makeMockDoctor(DOCTOR_STATUSES.RUNTIME_PRESENT),
    exec,
    returnInsteadOfMirror: true,
  });
  assert.equal(r.status, "bridge_spawn_failed");
});
