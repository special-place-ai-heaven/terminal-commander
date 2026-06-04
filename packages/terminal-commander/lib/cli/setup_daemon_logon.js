// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// `terminal-commander setup daemon-logon` — OPT-IN, no-admin, per-user.
//
// Registers (or removes with --uninstall) a per-user logon Scheduled Task that
// pre-starts the daemon at logon. This handler does NO spawning itself: it
// resolves the stable per-user daemon exe (Part A) and hands the argv to
// lib/daemon/logon_task.js, which owns the single schtasks.exe spawn. Keeping
// spawn out of lib/cli/** preserves the CLI spawn-discipline invariant.
//
// Default OFF: the task is registered ONLY when the operator runs this command.

"use strict";

const { ensureStableBinaries } = require("../harness/stable_bin.js");
const {
  installLogonTask,
  uninstallLogonTask,
  LOGON_TASK_STATUSES,
} = require("../daemon/logon_task.js");

async function runSetupDaemonLogon(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const flags = o.flags || {};
  const dryRun = flags["dry-run"] === true;
  const ensure = o.ensureStableBinaries || ensureStableBinaries;
  const install = o.installLogonTask || installLogonTask;
  const uninstall = o.uninstallLogonTask || uninstallLogonTask;

  if (platform !== "win32") {
    return {
      status: "unsupported_host",
      exit_code: 64,
      output:
        "terminal-commander: daemon-logon Scheduled Task is Windows-only; on Linux/WSL use 'terminal-commander setup daemon-autostart'.\n",
    };
  }

  // Uninstall path needs no exe resolution.
  if (flags.uninstall === true) {
    const r = uninstall({ platform, env, dry_run: dryRun, exec: o.exec });
    const ok =
      r.status === LOGON_TASK_STATUSES.OK ||
      r.status === LOGON_TASK_STATUSES.NOT_FOUND ||
      r.status === LOGON_TASK_STATUSES.DRY_RUN;
    return {
      status: r.status,
      exit_code: ok ? 0 : 64,
      output: `terminal-commander: ${r.hint}\n`,
    };
  }

  // Install path: resolve the stable terminal-commanderd.exe (Part A). In
  // dry-run we resolve without copying.
  const stable = ensure({
    platform,
    env,
    dry_run: dryRun,
    primary: "terminal-commanderd",
  });
  if (!stable.exePath) {
    return {
      status: "exe_missing",
      exit_code: 64,
      output:
        "terminal-commander: could not resolve a stable terminal-commanderd.exe; run 'terminal-commander setup harness' first, then retry.\n",
    };
  }

  const r = install({
    platform,
    env,
    exePath: stable.exePath,
    dry_run: dryRun,
    exec: o.exec,
  });
  const ok =
    r.status === LOGON_TASK_STATUSES.OK ||
    r.status === LOGON_TASK_STATUSES.DRY_RUN;
  return {
    status: r.status,
    exit_code: ok ? 0 : 64,
    output: `terminal-commander: ${r.hint}\n`,
  };
}

module.exports = { runSetupDaemonLogon };
