// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const {
  applyManagedBlock,
  hasManagedBlock,
  extractManagedBlock,
} = require("../lib/daemon/managed_block.js");
const {
  shouldInstallDaemonAutostart,
  renderAutostartScript,
  renderSystemdUnit,
  buildWslInstallCommand,
} = require("../lib/daemon/autostart.js");

test("shouldInstallDaemonAutostart defaults on", () => {
  assert.equal(shouldInstallDaemonAutostart({}), true);
  assert.equal(shouldInstallDaemonAutostart({ TC_SKIP_DAEMON_AUTOSTART: "1" }), false);
  assert.equal(shouldInstallDaemonAutostart({ TC_BOOTSTRAP_START_DAEMON: "0" }), false);
});

test("renderAutostartScript checks socket before start", () => {
  const s = renderAutostartScript();
  assert.match(s, /terminal-commanderd\.sock/);
  assert.match(s, /start --mode ipc-server/);
  assert.match(s, /nohup/);
});

test("renderSystemdUnit uses ipc-server mode", () => {
  const u = renderSystemdUnit("/usr/bin/terminal-commanderd");
  assert.match(u, /ExecStart=\/usr\/bin\/terminal-commanderd/);
  assert.match(u, /ipc-server/);
});

test("buildWslInstallCommand uses base64 pipe", () => {
  const cmd = buildWslInstallCommand();
  assert.match(cmd, /base64/);
  assert.match(cmd, /npm-global\/bin/);
});

test("applyManagedBlock is idempotent", () => {
  const first = applyManagedBlock("", "autostart", "echo hi");
  assert.ok(hasManagedBlock(first, "autostart"));
  const second = applyManagedBlock(first, "autostart", "echo hi");
  assert.equal(first, second);
  assert.equal(extractManagedBlock(second, "autostart"), "echo hi");
});
