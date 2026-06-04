// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// OPT-IN per-user logon Scheduled Task that pre-starts the Terminal Commander
// daemon at logon (Windows). By the time an MCP server launches, the daemon is
// already up, so ensure_daemon finds it AlreadyRunning and the MCP exe spawns
// nothing.
//
// USER-SPACE ONLY — registered with the NORMAL user mechanism, no elevation:
//
//   schtasks.exe /Create /SC ONLOGON /TN "TerminalCommander Daemon"
//     /TR "<stable>\terminal-commanderd.exe start" /F
//
// A developer creating a logon task for their own tool via schtasks is the
// documented, ordinary, user-space way to schedule one's own task. There is
// NO /RU SYSTEM, NO /RL HIGHEST, NO admin, NO certificate/allowance — the task
// runs as the same interactive user with the same privileges the user already
// has. argv form, shell:false, no hidden window.
//
// OPT-IN + idempotent (/F overwrites the same task) + uninstallable
// (/Delete /TN ... /F). It is registered ONLY when the operator explicitly runs
// `terminal-commander setup daemon-logon`; setup never registers it silently.

"use strict";

const { spawnSync } = require("node:child_process");

// Stable, human-readable task name. Used verbatim for /Create and /Delete so
// install + uninstall always address the same task.
const TASK_NAME = "TerminalCommander Daemon";

const LOGON_TASK_STATUSES = Object.freeze({
  OK: "ok",
  DRY_RUN: "dry_run",
  UNSUPPORTED_HOST: "unsupported_host",
  EXE_MISSING: "exe_missing",
  SCHTASKS_FAILED: "schtasks_failed",
  NOT_FOUND: "not_found",
});

/**
 * Build the schtasks argv that registers the per-user logon task.
 *
 * Pure: returns the exact argv array spawnSync receives (shell:false). The
 * caller is responsible for spawning. No admin flags are ever emitted: this is
 * deliberately a per-user ONLOGON task running at the user's own privilege.
 *
 * The /TR action quotes the exe path (it may contain spaces, e.g. a username
 * with a space) and appends the `start` subcommand the daemon expects. schtasks
 * stores /TR as a single string; quoting the exe keeps the path one token.
 *
 * @param {Object} opts
 * @param {string} opts.exePath  Absolute path to terminal-commanderd.exe (the
 *     stable per-user path from Part A).
 * @param {string} [opts.taskName=TASK_NAME]
 * @returns {string[]} argv for schtasks.exe
 */
function buildCreateArgv(opts) {
  const o = opts || {};
  if (!o.exePath || typeof o.exePath !== "string") {
    throw new Error("terminal-commander: logon task requires an exePath");
  }
  const taskName = o.taskName || TASK_NAME;
  const action = `"${o.exePath}" start`;
  return [
    "/Create",
    "/SC",
    "ONLOGON",
    "/TN",
    taskName,
    "/TR",
    action,
    "/F",
  ];
}

/**
 * Build the schtasks argv that removes the per-user logon task.
 *
 * @param {Object} [opts]
 * @param {string} [opts.taskName=TASK_NAME]
 * @returns {string[]} argv for schtasks.exe
 */
function buildDeleteArgv(opts) {
  const o = opts || {};
  const taskName = o.taskName || TASK_NAME;
  return ["/Delete", "/TN", taskName, "/F"];
}

function runSchtasks(argv, exec) {
  const run =
    exec ||
    ((args) =>
      spawnSync("schtasks.exe", args, {
        encoding: "utf8",
        shell: false,
      }));
  return run(argv);
}

/**
 * Register the opt-in per-user logon task pointing at the stable daemon exe.
 *
 * @param {Object} opts
 * @param {string} opts.exePath  Stable terminal-commanderd.exe path.
 * @param {string} [opts.platform=process.platform]
 * @param {boolean} [opts.dry_run=false]
 * @param {string} [opts.taskName=TASK_NAME]
 * @param {(argv:string[])=>{status:number|null,stdout?:string,stderr?:string}} [opts.exec]
 *     Test seam; defaults to spawnSync('schtasks.exe', argv, {shell:false}).
 * @returns {{status:string, task_name:string, argv:string[], hint:string}}
 */
function installLogonTask(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const taskName = o.taskName || TASK_NAME;

  if (platform !== "win32") {
    return {
      status: LOGON_TASK_STATUSES.UNSUPPORTED_HOST,
      task_name: taskName,
      argv: [],
      hint: "logon Scheduled Task is Windows-only; on Linux/WSL use 'terminal-commander setup daemon-autostart'.",
    };
  }
  if (!o.exePath) {
    return {
      status: LOGON_TASK_STATUSES.EXE_MISSING,
      task_name: taskName,
      argv: [],
      hint: "stable terminal-commanderd.exe path not available; run 'terminal-commander setup harness' first.",
    };
  }

  const argv = buildCreateArgv({ exePath: o.exePath, taskName });

  if (o.dry_run === true) {
    return {
      status: LOGON_TASK_STATUSES.DRY_RUN,
      task_name: taskName,
      argv,
      hint: `would register per-user logon task "${taskName}" -> ${o.exePath} start`,
    };
  }

  const r = runSchtasks(argv, o.exec);
  if (r && r.status === 0) {
    return {
      status: LOGON_TASK_STATUSES.OK,
      task_name: taskName,
      argv,
      hint: `registered per-user logon task "${taskName}"; remove with 'terminal-commander setup daemon-logon --uninstall'`,
    };
  }
  return {
    status: LOGON_TASK_STATUSES.SCHTASKS_FAILED,
    task_name: taskName,
    argv,
    hint: `schtasks /Create failed (${(r && (r.stderr || r.stdout) ? String(r.stderr || r.stdout).trim() : `exit ${r ? r.status : "?"}`)})`,
  };
}

/**
 * Remove the opt-in per-user logon task. Idempotent: a not-found delete is
 * reported as NOT_FOUND, not an error.
 *
 * @param {Object} [opts]  Same shape as installLogonTask (minus exePath).
 * @returns {{status:string, task_name:string, argv:string[], hint:string}}
 */
function uninstallLogonTask(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const taskName = o.taskName || TASK_NAME;

  if (platform !== "win32") {
    return {
      status: LOGON_TASK_STATUSES.UNSUPPORTED_HOST,
      task_name: taskName,
      argv: [],
      hint: "logon Scheduled Task is Windows-only.",
    };
  }

  const argv = buildDeleteArgv({ taskName });

  if (o.dry_run === true) {
    return {
      status: LOGON_TASK_STATUSES.DRY_RUN,
      task_name: taskName,
      argv,
      hint: `would remove per-user logon task "${taskName}"`,
    };
  }

  const r = runSchtasks(argv, o.exec);
  if (r && r.status === 0) {
    return {
      status: LOGON_TASK_STATUSES.OK,
      task_name: taskName,
      argv,
      hint: `removed per-user logon task "${taskName}"`,
    };
  }
  // schtasks /Delete exits non-zero when the task does not exist. Treat that as
  // an idempotent no-op so uninstall is safe to run repeatedly.
  const text = r && (r.stderr || r.stdout) ? String(r.stderr || r.stdout) : "";
  if (/cannot find|does not exist|ERROR: The system cannot find/i.test(text)) {
    return {
      status: LOGON_TASK_STATUSES.NOT_FOUND,
      task_name: taskName,
      argv,
      hint: `no per-user logon task "${taskName}" was registered`,
    };
  }
  return {
    status: LOGON_TASK_STATUSES.SCHTASKS_FAILED,
    task_name: taskName,
    argv,
    hint: `schtasks /Delete failed (${text.trim() || `exit ${r ? r.status : "?"}`})`,
  };
}

module.exports = {
  TASK_NAME,
  LOGON_TASK_STATUSES,
  buildCreateArgv,
  buildDeleteArgv,
  installLogonTask,
  uninstallLogonTask,
};
