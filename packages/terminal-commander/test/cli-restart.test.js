// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// `terminal-commander restart` handler tests. Mocks detect + exec so
// the suite is portable to any host and never spawns a real daemon.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");

const { runRestart, DAEMON_RESTART_CMD } = require("../lib/cli/restart.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");

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
    env: {},
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
