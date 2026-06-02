// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// B3 (folded into F1 Phase 2b): a non-zero autostart-run exit must not be
// swallowed silently. installDaemonAutostart surfaces it in the result.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const os = require("node:os");
const fs = require("node:fs");
const path = require("node:path");

const { installDaemonAutostart } = require("../lib/daemon/autostart.js");

function tmpHome() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "tc-autostart-"));
}

test("non-zero autostart run is surfaced, not swallowed", () => {
  const homeDir = tmpHome();
  const r = installDaemonAutostart({
    platform: "linux",
    homeDir,
    env: { HOME: homeDir },
    daemonBinary: "/fake/terminal-commanderd",
    // Force the profile-hook branch (no systemd in this fake env) and inject
    // a failing autostart run.
    systemdUserAvailable: () => false,
    runAutostartOnce: () => ({ ok: false, exit_code: 7 }),
  });
  assert.equal(r.status, "profile_hook");
  assert.equal(r.autostart_run_exit_code, 7, "non-zero exit must be reported");
  assert.match(r.hint, /autostart/i);
});

test("successful autostart run reports exit 0 without warning noise", () => {
  const homeDir = tmpHome();
  const r = installDaemonAutostart({
    platform: "linux",
    homeDir,
    env: { HOME: homeDir },
    daemonBinary: "/fake/terminal-commanderd",
    systemdUserAvailable: () => false,
    runAutostartOnce: () => ({ ok: true, exit_code: 0 }),
  });
  assert.equal(r.status, "profile_hook");
  assert.equal(r.autostart_run_exit_code, 0);
});
