// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { runDoctorDaemon } = require("../lib/cli/doctor_daemon.js");

const STATUS_RUNNING = [
  "terminal-commander status:",
  "  version       : 0.3.11",
  "  endpoint      : \\\\.\\pipe\\terminal-commander-tester",
  "  daemon        : running",
  "  pid           : 4242",
  "  log_path      : C:\\tc\\state\\logs\\terminal-commanderd.log",
  "  state_dir     : C:\\tc\\state",
  "",
].join("\n");

function nativeCase({ statusOut, statusCode }) {
  const localAppData = fs.mkdtempSync(path.join(os.tmpdir(), "tc-doctor-win-"));
  const calls = { wsl: 0, detect: 0, status: [] };
  const opts = {
    platform: "win32",
    env: { LOCALAPPDATA: localAppData, USERNAME: "tester" },
    flags: {},
    detect: async () => {
      calls.detect += 1;
      return { reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" };
    },
    // Any WSL spawn would go here; it must not happen in native mode.
    wslPath: path.join(localAppData, "no-such-wsl.exe"),
    resolveBinary: () => ({ reason: "ok", binaryPath: "C:\\fake\\terminal-commander.exe" }),
    execFile: async (file, argv) => {
      calls.status.push([file, ...argv]);
      return { code: statusCode, out: statusOut };
    },
  };
  return { opts, calls, localAppData };
}

test("doctor daemon on Windows native mode probes the native daemon, not WSL", async () => {
  const { opts, calls, localAppData } = nativeCase({ statusOut: STATUS_RUNNING, statusCode: 0 });
  const exe = path.join(localAppData, "terminal-commander", "bin", "terminal-commanderd.exe");
  fs.mkdirSync(path.dirname(exe), { recursive: true });
  fs.writeFileSync(exe, "");

  const r = await runDoctorDaemon(opts);
  const lines = r.output.split("\n");
  assert.match(lines[0], /native Windows mode/, r.output);
  assert.equal(calls.detect, 0, "native mode must not look for WSL distros");
  assert.deepEqual(calls.status, [["C:\\fake\\terminal-commander.exe", "status"]]);
  assert.equal(r.status, "ok", r.output);
  assert.ok(r.output.includes(`daemon_exe: ${exe} (present)`), r.output);
  assert.match(r.output, /daemon_running: yes/);
  assert.match(r.output, /endpoint: \\\\\.\\pipe\\terminal-commander-tester/);
  assert.match(r.output, /data_dir: C:\\tc\\state/);
  assert.doesNotMatch(r.output, /distro|~\/\.local/);
});

test("doctor daemon on Windows native mode reports a daemon that does not answer", async () => {
  const down = STATUS_RUNNING.replace("daemon        : running", "daemon        : unavailable")
    .replace("pid           : 4242", "pid           : -");
  const { opts } = nativeCase({ statusOut: down, statusCode: 1 });
  const r = await runDoctorDaemon(opts);
  assert.equal(r.status, "ok", r.output);
  assert.match(r.output, /daemon_exe: .*\(missing\)/);
  assert.match(r.output, /daemon_running: no/);
});

function withExeRow(row) {
  return STATUS_RUNNING.replace("  pid           : 4242\n", `  pid           : 4242\n  daemon_exe    : ${row}\n`);
}

test("doctor daemon reports the running exe and flags it when it is not the stable copy", async () => {
  const nested = "C:\\nm\\terminal-commander-win32-x64\\bin\\terminal-commanderd.exe";
  const { opts } = nativeCase({ statusOut: withExeRow(nested), statusCode: 0 });
  const r = await runDoctorDaemon(opts);
  assert.ok(r.output.includes(`running_exe: ${nested}`), r.output);
  assert.match(r.output, /note: the running daemon is not the installed stable copy/);
});

test("doctor daemon adds no note when the running exe is the stable copy", async () => {
  const { opts, localAppData } = nativeCase({ statusOut: "", statusCode: 0 });
  const exe = path.join(localAppData, "terminal-commander", "bin", "terminal-commanderd.exe");
  const r = await runDoctorDaemon({
    ...opts,
    execFile: async () => ({ code: 0, out: withExeRow(exe.toUpperCase()) }),
  });
  assert.ok(r.output.includes(`running_exe: ${exe.toUpperCase()}`), r.output);
  assert.doesNotMatch(r.output, /note:/);
});

test("doctor daemon omits running_exe when not running or unknown", async () => {
  const down = STATUS_RUNNING.replace("daemon        : running", "daemon        : unavailable")
    .replace("pid           : 4242", "pid           : -");
  let r = await runDoctorDaemon(nativeCase({ statusOut: down, statusCode: 1 }).opts);
  assert.doesNotMatch(r.output, /running_exe/);
  r = await runDoctorDaemon(nativeCase({ statusOut: withExeRow("unknown (pid unknown)"), statusCode: 0 }).opts);
  assert.doesNotMatch(r.output, /running_exe|note:/);
});

test("doctor daemon on Windows probes WSL only when the bridge mode is configured", async () => {
  const { opts, calls } = nativeCase({ statusOut: STATUS_RUNNING, statusCode: 0 });
  const r = await runDoctorDaemon({ ...opts, env: { ...opts.env, TC_WSL_DISTRO: "Ubuntu" } });
  assert.match(r.output.split("\n")[0], /WSL bridge mode/, r.output);
  assert.equal(calls.status.length, 0, "bridge mode must not run the native status probe");
  assert.equal(calls.detect, 1);
});
