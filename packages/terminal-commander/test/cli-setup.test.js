// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 setup cursor-wsl tests. Mocks detect / doctor / writeConfig
// / writeState / installExec so the suite runs deterministically.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  runSetupCursorWsl,
  resolveDistro,
  SETUP_STATUSES,
  INSTALL_PROBE_CMD,
} = require("../lib/cli/setup_cursor_wsl.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");
const { DOCTOR_STATUSES } = require("../lib/wsl/doctor.js");

function okDetect(distros, defaultName) {
  return async () => ({
    host_platform: "win32",
    wsl_callable: true,
    default_distro: defaultName || null,
    distros: distros.map((n) => ({ name: n, state: "Running", wsl_version: 2, is_default: n === defaultName })),
    reason: DETECT_REASONS.OK,
  });
}

function doctorReturning(status) {
  return async () => ({ status, hint: "" });
}

function makeStubWriter() {
  const calls = [];
  const writeConfig = (opts) => {
    calls.push(opts);
    return { status: "config_created", path: "/tmp/.cursor/mcp.json", backup_path: null, server: { type: "stdio", command: "terminal-commander-mcp" }, was_present: false, hint: "" };
  };
  return { writeConfig, calls };
}

function makeStubStateWriter() {
  const calls = [];
  const writeState = (opts) => {
    calls.push(opts);
    return { status: "ok", path: "/tmp/state/setup.json", value: opts };
  };
  return { writeState, calls };
}

test("INSTALL_PROBE_CMD is the locked constant", () => {
  assert.equal(INSTALL_PROBE_CMD, "npm install -g terminal-commander");
});

test("SETUP_STATUSES includes the full closed enum", () => {
  const required = [
    "setup_ready",
    "dry_run",
    "cursor_config_created",
    "cursor_config_updated",
    "cursor_config_already_exists",
    "cursor_config_invalid_json",
    "cursor_config_write_failed",
    "unsupported_host",
    "wsl_not_found",
    "no_distros",
    "no_default_distro_ambiguous",
    "distro_not_found",
    "unsafe_distro_name",
    "runtime_missing",
    "runtime_present",
    "check_timeout",
    "wsl_command_failed",
    "npm_package_unpublished",
    "install_unavailable",
    "install_permission_required",
    "credential_required",
  ];
  for (const s of required) {
    assert.ok(Object.values(SETUP_STATUSES).includes(s), `missing status: ${s}`);
  }
});

test("setup on non-win32 returns unsupported_host without spawn", async () => {
  const r = await runSetupCursorWsl({
    platform: "linux",
    flags: {},
    detect: async () => {
      throw new Error("detect must not be called");
    },
  });
  assert.equal(r.status, "unsupported_host");
});

test("--print-config emits the WWS04 bridge stanza and writes nothing", async () => {
  const writer = makeStubWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { "print-config": true },
    detect: okDetect(["Ubuntu-24.04"], "Ubuntu-24.04"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig: writer.writeConfig,
    writeState: makeStubStateWriter().writeState,
  });
  assert.equal(r.status, "dry_run");
  assert.equal(r.exit_code, 0);
  assert.equal(writer.calls.length, 0);
  assert.match(r.output, /terminal-commander-mcp/);
  assert.match(r.output, /"type": "stdio"/);
  assert.match(r.output, /TC_WSL_DISTRO/);
  // No wsl direct command in the generated stanza.
  assert.equal(/wsl\.exe/.test(r.output), false);
});

test("--dry-run prints the plan and writes nothing", async () => {
  const writer = makeStubWriter();
  const stateWriter = makeStubStateWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { "dry-run": true },
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig: writer.writeConfig,
    writeState: stateWriter.writeState,
  });
  assert.equal(r.status, "dry_run");
  assert.equal(r.exit_code, 0);
  assert.equal(writer.calls.length, 0);
  assert.equal(stateWriter.calls.length, 0);
  assert.match(r.output, /no files written/);
});

test("setup refuses unsafe distro before any write or doctor call", async () => {
  let doctorCalled = false;
  const writer = makeStubWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: { TC_WSL_DISTRO: "Bad; rm" },
    flags: {},
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: async () => {
      doctorCalled = true;
      return { status: DOCTOR_STATUSES.RUNTIME_PRESENT, hint: "" };
    },
    writeConfig: writer.writeConfig,
  });
  assert.equal(r.status, "unsafe_distro_name");
  assert.equal(writer.calls.length, 0);
  assert.equal(doctorCalled, false);
});

test("setup refuses unknown --distro before any write", async () => {
  const writer = makeStubWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { distro: "Fedora" },
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig: writer.writeConfig,
  });
  assert.equal(r.status, "distro_not_found");
  assert.equal(writer.calls.length, 0);
});

test("setup with no default distro + no override returns no_default_distro_ambiguous", async () => {
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: {},
    detect: async () => ({
      host_platform: "win32",
      wsl_callable: true,
      default_distro: null,
      distros: [{ name: "Ubuntu", state: "Stopped", wsl_version: 2, is_default: false }],
      reason: DETECT_REASONS.OK,
    }),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
  });
  assert.equal(r.status, "no_default_distro_ambiguous");
  assert.match(r.output, /Available distros: Ubuntu/);
});

test("setup with runtime_missing refuses to install and returns runtime_missing", async () => {
  const writer = makeStubWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: {},
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_MISSING),
    writeConfig: writer.writeConfig,
  });
  assert.equal(r.status, "runtime_missing");
  assert.equal(writer.calls.length, 0);
  assert.match(r.output, /Install with: wsl -d Ubuntu/);
});

test("setup with runtime_present calls WWS05 writeCursorMcpConfig and persists setup.json", async () => {
  const writer = makeStubWriter();
  const stateWriter = makeStubStateWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: {},
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig: writer.writeConfig,
    writeState: stateWriter.writeState,
  });
  assert.equal(r.status, "setup_ready");
  assert.equal(r.exit_code, 0);
  assert.equal(writer.calls.length, 1);
  assert.equal(writer.calls[0].scope, "global");
  assert.equal(writer.calls[0].distro, "Ubuntu");
  assert.equal(stateWriter.calls.length, 1);
  assert.equal(stateWriter.calls[0].distro, "Ubuntu");
  assert.equal(stateWriter.calls[0].cursor_scope, "global");
});

test("setup --force passes through to writeCursorMcpConfig", async () => {
  const writer = makeStubWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { force: true },
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig: writer.writeConfig,
    writeState: makeStubStateWriter().writeState,
  });
  assert.equal(r.status, "setup_ready");
  assert.equal(writer.calls[0].force, true);
});

test("setup --project <path> passes through as scope=project + projectRoot", async () => {
  const writer = makeStubWriter();
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { project: "/repo/x" },
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig: writer.writeConfig,
    writeState: makeStubStateWriter().writeState,
  });
  assert.equal(writer.calls[0].scope, "project");
  assert.equal(writer.calls[0].projectRoot, "/repo/x");
});

test("setup maps WWS05 already_exists to cursor_config_already_exists", async () => {
  const writeConfig = () => ({ status: "already_exists", path: "/x", backup_path: null, server: null, was_present: true, hint: "" });
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: {},
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    writeConfig,
    writeState: makeStubStateWriter().writeState,
  });
  assert.equal(r.status, "cursor_config_already_exists");
});

test("--install-wsl-runtime npm E404 maps to npm_package_unpublished", async () => {
  const fakeChild = {
    stdout: { on: () => {} },
    stderr: { on: () => {} },
    on(event, cb) {
      if (event === "close") setImmediate(() => cb(1));
    },
    kill: () => {},
  };
  const installExec = ({ argv, env }) => {
    // Simulate npm E404 by triggering a 'close' with stderr containing E404.
    return {
      stdout: {
        on: (event, cb) => {
          if (event === "data") setImmediate(() => cb(Buffer.from("npm error code E404\nnpm error 404 Not Found\n")));
        },
      },
      stderr: { on: () => {} },
      on(event, cb) {
        if (event === "close") setImmediate(() => cb(1));
      },
      kill: () => {},
    };
  };
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { "install-wsl-runtime": true },
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    installExec,
  });
  assert.equal(r.status, "npm_package_unpublished");
});

test("--install-wsl-runtime EACCES maps to install_permission_required (no sudo retry)", async () => {
  const installExec = () => ({
    stdout: {
      on: (event, cb) => {
        if (event === "data") setImmediate(() => cb(Buffer.from("EACCES: permission denied")));
      },
    },
    stderr: { on: () => {} },
    on(event, cb) {
      if (event === "close") setImmediate(() => cb(1));
    },
    kill: () => {},
  });
  const r = await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { "install-wsl-runtime": true },
    detect: okDetect(["Ubuntu"], "Ubuntu"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    installExec,
  });
  assert.equal(r.status, "install_permission_required");
  // Hint must NOT promise to run sudo.
  assert.equal(/will (try|retry) (with )?sudo/i.test(r.hint), false);
  assert.match(r.hint, /does NOT prompt for passwords/);
});

test("--install-wsl-runtime constructs the locked argv shape", async () => {
  let capturedArgv = null;
  let capturedShell = "absent";
  const installExec = ({ argv, env }) => {
    capturedArgv = argv;
    capturedShell = "absent"; // injected exec receives only filtered args
    return {
      stdout: { on: () => {} },
      stderr: { on: () => {} },
      on(event, cb) {
        if (event === "close") setImmediate(() => cb(0));
      },
      kill: () => {},
    };
  };
  await runSetupCursorWsl({
    platform: "win32",
    env: {},
    flags: { "install-wsl-runtime": true },
    detect: okDetect(["Ubuntu-24.04"], "Ubuntu-24.04"),
    doctor: doctorReturning(DOCTOR_STATUSES.RUNTIME_PRESENT),
    installExec,
    writeConfig: makeStubWriter().writeConfig,
    writeState: makeStubStateWriter().writeState,
  });
  assert.deepEqual(capturedArgv, [
    "-d",
    "Ubuntu-24.04",
    "--",
    "bash",
    "-lc",
    "npm install -g terminal-commander",
  ]);
});

test("resolveDistro priority chain matches the locked contract", () => {
  const detect = {
    distros: [{ name: "Ubuntu" }, { name: "Debian" }],
    default_distro: "Debian",
  };
  // P1: --distro
  assert.deepEqual(resolveDistro({ flags: { distro: "Ubuntu" }, env: {}, detectResult: detect }), { status: "ok", distro: "Ubuntu" });
  // P1 unsafe
  assert.equal(resolveDistro({ flags: { distro: "Bad; rm" }, env: {}, detectResult: detect }).status, "unsafe_distro_name");
  // P1 not in whitelist
  assert.equal(resolveDistro({ flags: { distro: "Fedora" }, env: {}, detectResult: detect }).status, "distro_not_found");
  // P2: TC_WSL_DISTRO
  assert.deepEqual(resolveDistro({ flags: {}, env: { TC_WSL_DISTRO: "Ubuntu" }, detectResult: detect }), { status: "ok", distro: "Ubuntu" });
  // P3: detect default
  assert.deepEqual(resolveDistro({ flags: {}, env: {}, detectResult: detect }), { status: "ok", distro: "Debian" });
  // P4: refuse
  assert.equal(resolveDistro({ flags: {}, env: {}, detectResult: { distros: [{ name: "x" }], default_distro: null } }).status, "no_default_distro_ambiguous");
});
