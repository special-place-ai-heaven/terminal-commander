// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { spawn } = require("node:child_process");
const {
  buildFilteredEnv,
  ensureSessionInWslEnv,
} = require("../wsl/filtered_env.js");
const { getInstallDaemonAutostartCmd } = require("./constants.js");
const { shouldInstallDaemonAutostart } = require("../daemon/autostart.js");

const ENSURE_DAEMON_STATUSES = Object.freeze({
  OK: "ok",
  SKIPPED: "skipped",
  INSTALL_FAILED: "install_failed",
  CHECK_TIMEOUT: "check_timeout",
  UNSUPPORTED_HOST: "unsupported_host",
});

function runWslBashLc({ distro, cmd, env, exec, wslPath, timeoutMs }) {
  return new Promise((resolve) => {
    const argv = ["-d", distro, "--", "bash", "-lc", cmd];
    // Rebuild WSLENV to a TC-only allowlist after name-based filtering: this
    // spawn launches a Linux process (`bash -lc`), so an ambient
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
      if (code === 0) {
        resolve({
          status: ENSURE_DAEMON_STATUSES.OK,
          hint: "daemon autostart installed in WSL",
          exit_code: 0,
          stdout: stdoutBuf,
          stderr: stderrBuf,
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

module.exports = {
  ensureDaemonAutostartInWsl,
  ENSURE_DAEMON_STATUSES,
};
