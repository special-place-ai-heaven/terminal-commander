// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 doctor CLI tests. Mocks detect / doctor so the suite is
// portable to Linux/WSL hosts.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { runDoctor, DOCTOR_CLI_STATUSES } = require("../lib/cli/doctor.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");
const { DOCTOR_STATUSES } = require("../lib/wsl/doctor.js");

function okDetect(distros, defaultName) {
  return {
    host_platform: "win32",
    wsl_callable: true,
    default_distro: defaultName || null,
    distros: distros.map((n) => ({ name: n, state: "Running", wsl_version: 2, is_default: n === defaultName })),
    reason: DETECT_REASONS.OK,
  };
}

test("DOCTOR_CLI_STATUSES enum is the locked set", () => {
  assert.deepEqual(
    new Set(Object.values(DOCTOR_CLI_STATUSES)),
    new Set([
      "ok",
      "unsupported_host",
      "wsl_not_found",
      "no_distros",
      "distro_not_found",
      "unsafe_distro_name",
      "runtime_missing",
      "runtime_present",
      "check_timeout",
      "wsl_command_failed",
    ]),
  );
});

test("doctor (no subcommand) returns Windows host overview without spawn", async () => {
  let detectCalled = false;
  const r = await runDoctor({
    subcommand: null,
    platform: "win32",
    detect: async () => {
      detectCalled = true;
      return { reason: DETECT_REASONS.OK };
    },
  });
  assert.equal(detectCalled, false);
  assert.equal(r.status, "ok");
  assert.equal(r.exit_code, 0);
  assert.match(r.output, /host_platform: win32/);
});

test("doctor on non-win32 host reports unsupported_host (but still exits 0)", async () => {
  const r = await runDoctor({ subcommand: null, platform: "linux", detect: async () => ({}) });
  assert.equal(r.status, "unsupported_host");
  assert.equal(r.exit_code, 0);
});

test("doctor wsl returns ok + distro list when detectWsl returns ok", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: {},
    detect: async () => okDetect(["Ubuntu", "Debian"], "Ubuntu"),
  });
  assert.equal(r.status, "ok");
  assert.equal(r.exit_code, 0);
  assert.equal(r.summary.default_distro, "Ubuntu");
  assert.equal(r.summary.distros.length, 2);
});

test("doctor wsl --distro rejects unsafe name before invoking detect", async () => {
  let detectCalled = false;
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: { distro: "Bad; rm" },
    detect: async () => {
      detectCalled = true;
      return okDetect(["Ubuntu"], "Ubuntu");
    },
  });
  assert.equal(detectCalled, false);
  assert.equal(r.status, "unsafe_distro_name");
  assert.equal(r.exit_code, 64);
});

test("doctor wsl --distro maps unknown distro to distro_not_found", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: { distro: "Fedora" },
    detect: async () => okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(r.status, "distro_not_found");
  assert.equal(r.exit_code, 64);
});

test("doctor wsl maps detect wsl_not_found to wsl_not_found", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: {},
    detect: async () => ({ host_platform: "win32", wsl_callable: false, default_distro: null, distros: [], reason: DETECT_REASONS.WSL_NOT_FOUND }),
  });
  assert.equal(r.status, "wsl_not_found");
});

test("doctor wsl maps detect no_distros to no_distros", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: {},
    detect: async () => ({ host_platform: "win32", wsl_callable: true, default_distro: null, distros: [], reason: DETECT_REASONS.NO_DISTROS }),
  });
  assert.equal(r.status, "no_distros");
});

test("doctor wsl --probe-runtime maps doctor.RUNTIME_PRESENT to runtime_present", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: { "probe-runtime": true },
    detect: async () => okDetect(["Ubuntu"], "Ubuntu"),
    doctor: async () => ({ status: DOCTOR_STATUSES.RUNTIME_PRESENT, hint: "" }),
  });
  assert.equal(r.status, "runtime_present");
  assert.equal(r.exit_code, 0);
  assert.equal(r.summary.runtime_present, true);
});

test("doctor wsl --probe-runtime maps doctor.RUNTIME_MISSING to runtime_missing with exit 64", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: { "probe-runtime": true },
    detect: async () => okDetect(["Ubuntu"], "Ubuntu"),
    doctor: async () => ({ status: DOCTOR_STATUSES.RUNTIME_MISSING, hint: "missing" }),
  });
  assert.equal(r.status, "runtime_missing");
  assert.equal(r.exit_code, 64);
});

test("doctor wsl --probe-runtime requires a distro or detect default", async () => {
  const r = await runDoctor({
    subcommand: "wsl",
    platform: "win32",
    flags: { "probe-runtime": true },
    detect: async () => ({
      host_platform: "win32",
      wsl_callable: true,
      default_distro: null,
      distros: [{ name: "x", state: "Running", wsl_version: 2, is_default: false }],
      reason: DETECT_REASONS.OK,
    }),
  });
  // detect returned ok but no default distro -> --probe-runtime
  // needs a target distro and emits no_distros hint.
  assert.equal(r.status, "no_distros");
});
