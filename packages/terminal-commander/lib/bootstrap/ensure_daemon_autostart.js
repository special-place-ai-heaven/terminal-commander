// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { spawn } = require("node:child_process");
const {
  buildFilteredEnv,
  ensureSessionInWslEnv,
} = require("../wsl/filtered_env.js");
const {
  getInstallDaemonAutostartCmd,
  getRepairDaemonAutostartCmd,
  withShellRanSentinel,
  takeShellRanSentinel,
  shellExitedEarlyHint,
} = require("./constants.js");
const { shouldInstallDaemonAutostart } = require("../daemon/autostart.js");

const ENSURE_DAEMON_STATUSES = Object.freeze({
  OK: "ok",
  SKIPPED: "skipped",
  INSTALL_FAILED: "install_failed",
  CHECK_TIMEOUT: "check_timeout",
  UNSUPPORTED_HOST: "unsupported_host",
  SHELL_EXITED_EARLY: "shell_exited_early",
});

function runWslBashLc({ distro, cmd, env, exec, wslPath, timeoutMs }) {
  return new Promise((resolve) => {
    // Non-login `bash -c`: the install command sets its own PATH, and a login
    // shell would first run ~/.profile, where a <= 0.3.11 snippet exits the
    // shell (status 0) once the daemon socket exists -- so the installer that
    // replaces that snippet would silently never run.
    const argv = ["-d", distro, "--", "bash", "-c", withShellRanSentinel(cmd)];
    // Rebuild WSLENV to a TC-only allowlist after name-based filtering: this
    // spawn launches a Linux process (`bash -c`), so an ambient
    // WSLENV=SOME_SECRET/u would otherwise forward SOME_SECRET into WSL.
    const filtered = ensureSessionInWslEnv(buildFilteredEnv(env || process.env));
    let stdoutBuf = "";
    let stderrBuf = "";
    let child;
    const localExec =
      exec ||
      (({ wslPath: wp, argv: a, env: e }) =>
        spawn(wp, a, {
          stdio: ["ignore", "pipe", "pipe"],
          shell: false,
          env: e,
        }));
    try {
      child = localExec({ wslPath: wslPath || "wsl.exe", argv, env: filtered });
    } catch (_e) {
      resolve({
        status: ENSURE_DAEMON_STATUSES.INSTALL_FAILED,
        hint: "failed to spawn wsl.exe for daemon autostart install",
        exit_code: null,
      });
      return;
    }
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      try {
        child.kill("SIGKILL");
      } catch (_e) {
        /* ignore */
      }
      resolve({
        status: ENSURE_DAEMON_STATUSES.CHECK_TIMEOUT,
        hint: "daemon autostart install exceeded timeout",
        exit_code: null,
      });
    }, typeof timeoutMs === "number" ? timeoutMs : 120_000);
    if (child.stdout) child.stdout.on("data", (b) => { stdoutBuf += b.toString("utf8"); });
    if (child.stderr) child.stderr.on("data", (b) => { stderrBuf += b.toString("utf8"); });
    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const shell = takeShellRanSentinel(stdoutBuf);
      stdoutBuf = shell.stdout;
      if (code === 0 && !shell.ran) {
        resolve({
          status: ENSURE_DAEMON_STATUSES.SHELL_EXITED_EARLY,
          hint: shellExitedEarlyHint(distro),
          exit_code: 0,
          stdout: stdoutBuf,
          stderr: stderrBuf,
        });
        return;
      }
      if (code === 0) {
        resolve({
          status: ENSURE_DAEMON_STATUSES.OK,
          hint: "daemon autostart installed in WSL",
          exit_code: 0,
          stdout: stdoutBuf,
          stderr: stderrBuf,
          // Lines the installer reports for the operator (e.g. an rc file
          // with malformed autostart markers that was left unchanged).
          warnings: stdoutBuf
            .split("\n")
            .map((l) => l.replace(/\r$/, ""))
            .filter((l) => l.startsWith("terminal-commander: ")),
        });
        return;
      }
      const tail = (stderrBuf || stdoutBuf).trim().slice(-240);
      resolve({
        status: ENSURE_DAEMON_STATUSES.INSTALL_FAILED,
        hint: tail || `daemon autostart install exited ${code}`,
        exit_code: code,
        stdout: stdoutBuf,
        stderr: stderrBuf,
      });
    });
    child.on("error", () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: ENSURE_DAEMON_STATUSES.INSTALL_FAILED,
        hint: "wsl.exe spawn error during daemon autostart install",
        exit_code: null,
      });
    });
  });
}

/**
 * @param {Object} opts
 * @param {string} opts.distro
 */
async function ensureDaemonAutostartInWsl(opts) {
  const o = opts || {};
  if (o.platform !== "win32" || !o.distro) {
    return {
      status: ENSURE_DAEMON_STATUSES.UNSUPPORTED_HOST,
      hint: "WSL distro required",
    };
  }
  if (!shouldInstallDaemonAutostart(o.env)) {
    return {
      status: ENSURE_DAEMON_STATUSES.SKIPPED,
      hint: "daemon autostart skipped",
    };
  }
  const cmd = getInstallDaemonAutostartCmd();
  return runWslBashLc({
    distro: o.distro,
    cmd,
    env: o.env,
    exec: o.exec,
    wslPath: o.wslPath,
    timeoutMs: o.timeoutMs,
  });
}

/**
 * Rewrite autostart.sh and the profile snippet in WSL when an earlier install
 * left them on disk (see renderRepairBash). Not gated on
 * shouldInstallDaemonAutostart: it installs nothing new, it only replaces
 * files that can otherwise kill the user's shells.
 *
 * @param {Object} opts
 * @param {string} opts.distro
 */
async function repairDaemonAutostartInWsl(opts) {
  const o = opts || {};
  if (o.platform !== "win32" || !o.distro) {
    return {
      status: ENSURE_DAEMON_STATUSES.UNSUPPORTED_HOST,
      hint: "WSL distro required",
    };
  }
  return runWslBashLc({
    distro: o.distro,
    cmd: getRepairDaemonAutostartCmd(),
    env: o.env,
    exec: o.exec,
    wslPath: o.wslPath,
    timeoutMs: o.timeoutMs,
  });
}

module.exports = {
  ensureDaemonAutostartInWsl,
  repairDaemonAutostartInWsl,
  ENSURE_DAEMON_STATUSES,
};
