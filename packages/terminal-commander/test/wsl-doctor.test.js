// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS03 wslDoctor tests. Mocks the discovery + probe executor so the
// suite is portable to non-Windows hosts.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { wslDoctor, DOCTOR_STATUSES, RUNTIME_PROBE_CMD } = require("../lib/wsl/doctor.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");

function okDetect(distros, defaultName) {
  return {
    host_platform: "win32",
    wsl_callable: true,
    default_distro: defaultName || (distros[0] && distros[0].name) || null,
    distros: distros.map((d) =>
      typeof d === "string"
        ? { name: d, state: "Running", wsl_version: 2, is_default: false }
        : d,
    ),
    reason: DETECT_REASONS.OK,
  };
}

function failingDetect(reason) {
  return {
    host_platform: "win32",
    wsl_callable: reason !== DETECT_REASONS.UNSUPPORTED_HOST && reason !== DETECT_REASONS.WSL_NOT_FOUND,
    default_distro: null,
    distros: [],
    reason,
  };
}

test("DOCTOR_STATUSES exposes the full status enum", () => {
  assert.deepEqual(
    new Set(Object.values(DOCTOR_STATUSES)),
    new Set([
      "ok",
      "unsupported_host",
      "wsl_not_found",
      "no_distros",
      "distro_not_found",
      "unsafe_distro_name",
      "wsl_command_failed",
      "runtime_missing",
      "runtime_present",
      "doctor_not_run",
      "check_timeout",
    ]),
  );
});

test("RUNTIME_PROBE_CMD is the constant we expect, with no operator interpolation token", () => {
  assert.equal(RUNTIME_PROBE_CMD, "command -v terminal-commander-mcp");
  // Defense-in-depth: no `${...}` or `%...%` template syntax.
  assert.equal(RUNTIME_PROBE_CMD.includes("${"), false);
  assert.equal(RUNTIME_PROBE_CMD.includes("%"), false);
  assert.equal(RUNTIME_PROBE_CMD.includes("sudo"), false);
  assert.equal(RUNTIME_PROBE_CMD.includes("install"), false);
});

test("rejects unsafe distro name BEFORE invoking any executor", async () => {
  let execCalled = false;
  const exec = async () => {
    execCalled = true;
    throw new Error("exec must not be called for unsafe distro");
  };
  const r = await wslDoctor({
    distro: "Ubuntu; rm -rf /",
    exec,
    platform: "win32",
  });
  assert.equal(execCalled, false);
  assert.equal(r.status, "unsafe_distro_name");
  assert.equal(r.runtime_present, false);
  assert.match(r.hint, /safety whitelist/);
});

test("non-string distro is rejected as unsafe_distro_name", async () => {
  const r = await wslDoctor({
    distro: undefined,
    exec: async () => {
      throw new Error("should not be called");
    },
    platform: "win32",
  });
  assert.equal(r.status, "unsafe_distro_name");
});

test("unsupported_host status when platform is not win32", async () => {
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "linux",
    exec: async () => {
      throw new Error("should not be called");
    },
  });
  assert.equal(r.status, "unsupported_host");
});

test("wsl_not_found status when detect returns wsl_not_found", async () => {
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    detectResult: failingDetect(DETECT_REASONS.WSL_NOT_FOUND),
  });
  assert.equal(r.status, "wsl_not_found");
  assert.equal(r.runtime_present, false);
});

test("no_distros status when detect returns no_distros", async () => {
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    detectResult: failingDetect(DETECT_REASONS.NO_DISTROS),
  });
  assert.equal(r.status, "no_distros");
});

test("wsl_command_failed status when detect returns wsl_command_failed", async () => {
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    detectResult: failingDetect(DETECT_REASONS.WSL_COMMAND_FAILED),
  });
  assert.equal(r.status, "wsl_command_failed");
});

test("check_timeout status when detect returns check_timeout", async () => {
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    detectResult: failingDetect(DETECT_REASONS.CHECK_TIMEOUT),
  });
  assert.equal(r.status, "check_timeout");
});

test("distro_not_found when name is safe but not in the whitelist", async () => {
  const r = await wslDoctor({
    distro: "Fedora",
    platform: "win32",
    detectResult: okDetect(["Ubuntu", "Debian"], "Ubuntu"),
  });
  assert.equal(r.status, "distro_not_found");
  assert.match(r.hint, /not found/);
});

test("default OK when probeRuntime is false and distro is in whitelist (doctor_not_run hint)", async () => {
  let execCalled = false;
  const exec = async () => {
    execCalled = true;
    throw new Error("exec must not run when probeRuntime=false");
  };
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    exec,
    detectResult: okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(execCalled, false);
  assert.equal(r.status, "ok");
  assert.equal(r.runtime_present, false);
  assert.match(r.hint, /probeRuntime: true/);
});

test("runtime_present when probeRuntime=true and probe exits 0 with non-empty stdout", async () => {
  let captured = null;
  const exec = async (args) => {
    captured = args;
    return {
      status: 0,
      signal: null,
      stdout: Buffer.from("/usr/local/bin/terminal-commander-mcp\n", "utf8"),
      stderr: Buffer.alloc(0),
      error: null,
    };
  };
  const r = await wslDoctor({
    distro: "Ubuntu-24.04",
    platform: "win32",
    probeRuntime: true,
    exec,
    detectResult: okDetect(["Ubuntu-24.04"], "Ubuntu-24.04"),
  });
  assert.equal(r.status, "runtime_present");
  assert.equal(r.runtime_present, true);
  assert.deepEqual(captured.argv, [
    "-d",
    "Ubuntu-24.04",
    "--",
    "bash",
    "-lc",
    "command -v terminal-commander-mcp",
  ]);
  // No operator string concatenation: argv[5] is the constant
  // RUNTIME_PROBE_CMD, byte-for-byte.
  assert.equal(captured.argv[5], RUNTIME_PROBE_CMD);
});

test("runtime_missing when probeRuntime=true and probe exits non-zero or empty stdout", async () => {
  // Non-zero exit, empty stdout.
  let r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    probeRuntime: true,
    exec: async () => ({
      status: 1,
      signal: null,
      stdout: Buffer.alloc(0),
      stderr: Buffer.from("not found", "utf8"),
      error: null,
    }),
    detectResult: okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(r.status, "runtime_missing");
  assert.equal(r.runtime_present, false);

  // Zero exit, empty stdout (shouldn't happen with `command -v` but
  // belt-and-braces).
  r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    probeRuntime: true,
    exec: async () => ({
      status: 0,
      signal: null,
      stdout: Buffer.alloc(0),
      stderr: Buffer.alloc(0),
      error: null,
    }),
    detectResult: okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(r.status, "runtime_missing");
});

test("check_timeout when probeRuntime=true and probe returns a timeout error", async () => {
  const err = new Error("timeout");
  err.code = "CHECK_TIMEOUT";
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    probeRuntime: true,
    exec: async () => ({
      status: null,
      signal: null,
      stdout: Buffer.alloc(0),
      stderr: Buffer.alloc(0),
      error: err,
    }),
    detectResult: okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(r.status, "check_timeout");
});

test("wsl_command_failed when probeRuntime=true and probe returns a generic error", async () => {
  const err = new Error("boom");
  err.code = "EACCES";
  const r = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    probeRuntime: true,
    exec: async () => ({
      status: null,
      signal: null,
      stdout: Buffer.alloc(0),
      stderr: Buffer.alloc(0),
      error: err,
    }),
    detectResult: okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(r.status, "wsl_command_failed");
});

test("wslDoctor argv shape never includes a literal 'wsl.exe' first arg from this module", async () => {
  // The caller passes wslPath, never a literal 'wsl.exe' built inside
  // the argv array. We assert the captured argv elements 0..2 are
  // the documented constants.
  let argv = null;
  const exec = async (args) => {
    argv = args.argv;
    return {
      status: 0,
      signal: null,
      stdout: Buffer.from("/usr/bin/x"),
      stderr: Buffer.alloc(0),
      error: null,
    };
  };
  await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    probeRuntime: true,
    exec,
    detectResult: okDetect(["Ubuntu"], "Ubuntu"),
  });
  assert.equal(argv[0], "-d");
  assert.equal(argv[1], "Ubuntu");
  assert.equal(argv[2], "--");
  assert.equal(argv[3], "bash");
  assert.equal(argv[4], "-lc");
});
