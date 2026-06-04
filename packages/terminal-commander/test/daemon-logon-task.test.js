// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Part B: OPT-IN per-user logon Scheduled Task. The schtasks spawn is mocked —
// these tests assert the exact argv (no admin flags) and never register a real
// task on the test machine. Default-OFF is verified at the orchestrator level.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  TASK_NAME,
  LOGON_TASK_STATUSES,
  buildCreateArgv,
  buildDeleteArgv,
  installLogonTask,
  uninstallLogonTask,
} = require("../lib/daemon/logon_task.js");
const { runSetupDaemonLogon } = require("../lib/cli/setup_daemon_logon.js");
const { runBootstrap } = require("../lib/bootstrap/orchestrator.js");

const STABLE_DAEMON =
  "C:\\Users\\op\\AppData\\Local\\terminal-commander\\bin\\terminal-commanderd.exe";

test("buildCreateArgv is a per-user ONLOGON task with NO admin flags", () => {
  const argv = buildCreateArgv({ exePath: STABLE_DAEMON });
  assert.deepEqual(argv, [
    "/Create",
    "/SC",
    "ONLOGON",
    "/TN",
    TASK_NAME,
    "/TR",
    `"${STABLE_DAEMON}" start`,
    "/F",
  ]);
  // Hard guard: never elevate / never run as SYSTEM / never highest-privilege.
  const joined = argv.join(" ");
  assert.equal(/\/RU\b/i.test(joined), false, "must not set /RU (run-as user)");
  assert.equal(/SYSTEM/i.test(joined), false, "must not run as SYSTEM");
  assert.equal(/\/RL\b/i.test(joined), false, "must not set /RL (run level)");
  assert.equal(/HIGHEST/i.test(joined), false, "must not request HIGHEST privileges");
});

test("buildCreateArgv quotes the exe path so spaces in a username survive", () => {
  const spaced =
    "C:\\Users\\Joe Op\\AppData\\Local\\terminal-commander\\bin\\terminal-commanderd.exe";
  const argv = buildCreateArgv({ exePath: spaced });
  const tr = argv[argv.indexOf("/TR") + 1];
  assert.equal(tr, `"${spaced}" start`);
});

test("buildCreateArgv throws without an exePath", () => {
  assert.throws(() => buildCreateArgv({}), /exePath/);
});

test("buildDeleteArgv targets the same task name with /F (idempotent uninstall)", () => {
  assert.deepEqual(buildDeleteArgv(), ["/Delete", "/TN", TASK_NAME, "/F"]);
});

test("installLogonTask spawns schtasks with the create argv (mocked exec)", () => {
  let captured = null;
  const r = installLogonTask({
    platform: "win32",
    exePath: STABLE_DAEMON,
    exec: (argv) => {
      captured = argv;
      return { status: 0 };
    },
  });
  assert.equal(r.status, LOGON_TASK_STATUSES.OK);
  assert.deepEqual(captured, buildCreateArgv({ exePath: STABLE_DAEMON }));
});

test("installLogonTask dry-run returns the argv WITHOUT spawning", () => {
  let spawned = false;
  const r = installLogonTask({
    platform: "win32",
    exePath: STABLE_DAEMON,
    dry_run: true,
    exec: () => {
      spawned = true;
      return { status: 0 };
    },
  });
  assert.equal(r.status, LOGON_TASK_STATUSES.DRY_RUN);
  assert.deepEqual(r.argv, buildCreateArgv({ exePath: STABLE_DAEMON }));
  assert.equal(spawned, false);
});

test("installLogonTask reports schtasks_failed on a non-zero exit", () => {
  const r = installLogonTask({
    platform: "win32",
    exePath: STABLE_DAEMON,
    exec: () => ({ status: 1, stderr: "ERROR: Access is denied." }),
  });
  assert.equal(r.status, LOGON_TASK_STATUSES.SCHTASKS_FAILED);
});

test("installLogonTask refuses on non-win32 without spawning", () => {
  let spawned = false;
  const r = installLogonTask({
    platform: "linux",
    exePath: STABLE_DAEMON,
    exec: () => {
      spawned = true;
      return { status: 0 };
    },
  });
  assert.equal(r.status, LOGON_TASK_STATUSES.UNSUPPORTED_HOST);
  assert.equal(spawned, false);
});

test("uninstallLogonTask spawns schtasks /Delete (mocked exec)", () => {
  let captured = null;
  const r = uninstallLogonTask({
    platform: "win32",
    exec: (argv) => {
      captured = argv;
      return { status: 0 };
    },
  });
  assert.equal(r.status, LOGON_TASK_STATUSES.OK);
  assert.deepEqual(captured, ["/Delete", "/TN", TASK_NAME, "/F"]);
});

test("uninstallLogonTask treats a missing task as an idempotent NOT_FOUND", () => {
  const r = uninstallLogonTask({
    platform: "win32",
    exec: () => ({
      status: 1,
      stderr: "ERROR: The system cannot find the file specified.",
    }),
  });
  assert.equal(r.status, LOGON_TASK_STATUSES.NOT_FOUND);
});

test("runSetupDaemonLogon install resolves the stable daemon exe then registers", async () => {
  let captured = null;
  const r = await runSetupDaemonLogon({
    platform: "win32",
    env: { LOCALAPPDATA: "C:\\L" },
    flags: {},
    ensureStableBinaries: ({ primary }) => {
      assert.equal(primary, "terminal-commanderd", "logon task targets the daemon exe");
      return { exePath: STABLE_DAEMON, copied: [], reason: "ok" };
    },
    installLogonTask: (o) =>
      installLogonTask({
        ...o,
        exec: (argv) => {
          captured = argv;
          return { status: 0 };
        },
      }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(r.status, LOGON_TASK_STATUSES.OK);
  assert.deepEqual(captured, buildCreateArgv({ exePath: STABLE_DAEMON }));
});

test("runSetupDaemonLogon --uninstall removes without resolving an exe", async () => {
  let captured = null;
  let resolved = false;
  const r = await runSetupDaemonLogon({
    platform: "win32",
    env: {},
    flags: { uninstall: true },
    ensureStableBinaries: () => {
      resolved = true;
      return { exePath: null };
    },
    uninstallLogonTask: (o) =>
      uninstallLogonTask({
        ...o,
        exec: (argv) => {
          captured = argv;
          return { status: 0 };
        },
      }),
  });
  assert.equal(r.exit_code, 0);
  assert.deepEqual(captured, ["/Delete", "/TN", TASK_NAME, "/F"]);
  assert.equal(resolved, false, "uninstall must not need exe resolution");
});

test("runSetupDaemonLogon fails cleanly when no stable daemon exe resolves", async () => {
  const r = await runSetupDaemonLogon({
    platform: "win32",
    env: { LOCALAPPDATA: "C:\\L" },
    flags: {},
    ensureStableBinaries: () => ({ exePath: null, reason: "resolve_failed" }),
  });
  assert.equal(r.exit_code, 64);
  assert.equal(r.status, "exe_missing");
});

test("OPT-IN: bootstrap setup NEVER registers the logon task (default OFF)", async () => {
  // The orchestrator wires harness config + (Linux/WSL) daemon autostart, but
  // must NOT register the Windows logon Scheduled Task. Registering that is only
  // ever done by the explicit `setup daemon-logon` subcommand.
  let logonInstalled = false;
  // Spy on the real module to prove it is never called during bootstrap.
  const logonMod = require("../lib/daemon/logon_task.js");
  const origInstall = logonMod.installLogonTask;
  logonMod.installLogonTask = () => {
    logonInstalled = true;
    return { status: "ok" };
  };
  try {
    const r = await runBootstrap({
      mode: "cli",
      platform: "win32",
      env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\L" },
      dry_run: true,
      acquireLock: false,
      ensureStableBinaries: () => ({ exePath: null, reason: "dry_run" }),
      writeAllHarnesses: () => [{ id: "cursor", status: "ok" }],
    });
    assert.equal(r.exit_code, 0);
    assert.equal(
      logonInstalled,
      false,
      "bootstrap must never auto-register the per-user logon task",
    );
  } finally {
    logonMod.installLogonTask = origInstall;
  }
});
