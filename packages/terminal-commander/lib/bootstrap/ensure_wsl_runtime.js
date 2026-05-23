// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Ensure terminal-commander is installed inside a WSL distro with the
// Linux platform optionalDependency resolved.

"use strict";

const { spawn } = require("node:child_process");
const { buildFilteredEnv } = require("../wsl/filtered_env.js");
const { INSTALL_PROBE_CMD, RUNTIME_VERIFY_CMD } = require("./constants.js");

const ENSURE_STATUSES = Object.freeze({
  OK: "ok",
  INSTALL_UNAVAILABLE: "install_unavailable",
  INSTALL_PERMISSION_REQUIRED: "install_permission_required",
  NPM_PACKAGE_UNPUBLISHED: "npm_package_unpublished",
  CHECK_TIMEOUT: "check_timeout",
  RUNTIME_VERIFY_FAILED: "runtime_verify_failed",
  UNSUPPORTED_HOST: "unsupported_host",
});

function classifyNpmOutput(combined, code) {
  if (
    /e404/.test(combined) ||
    /404 not found/.test(combined) ||
    /not in this registry/.test(combined)
  ) {
    return {
      status: ENSURE_STATUSES.NPM_PACKAGE_UNPUBLISHED,
      hint: "terminal-commander is not published to npm (E404 inside WSL).",
      exit_code: code,
    };
  }
  if (
    /eacces/.test(combined) ||
    /permission denied/.test(combined) ||
    /sudo/.test(combined) ||
    /not permitted/.test(combined)
  ) {
    return {
      status: ENSURE_STATUSES.INSTALL_PERMISSION_REQUIRED,
      hint:
        "inside-WSL npm install failed with a permission error; install manually in WSL with an appropriate npm prefix.",
      exit_code: code,
    };
  }
  if (code === 0) {
    return { status: ENSURE_STATUSES.OK, hint: "install succeeded inside WSL.", exit_code: 0 };
  }
  return {
    status: ENSURE_STATUSES.INSTALL_UNAVAILABLE,
    hint: `inside-WSL npm install exited with code ${code}.`,
    exit_code: code,
  };
}

function runWslBashLc({ distro, cmd, env, exec, wslPath, timeoutMs }) {
  return new Promise((resolve) => {
    const argv = ["-d", distro, "--", "bash", "-lc", cmd];
    const filtered = buildFilteredEnv(env || process.env);
    let stdoutBuf = "";
    let stderrBuf = "";
    let child;
    const localExec =
      exec ||
      (({ wslPath: wp, argv: a, env: e }) =>
        spawn(wp, a, {
          stdio: ["ignore", "pipe", "pipe"],
          shell: false,
          windowsHide: true,
          env: e,
        }));
    try {
      child = localExec({ wslPath: wslPath || "wsl.exe", argv, env: filtered });
    } catch (_e) {
      resolve({
        status: ENSURE_STATUSES.INSTALL_UNAVAILABLE,
        hint: "failed to spawn wsl.exe",
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
        status: ENSURE_STATUSES.CHECK_TIMEOUT,
        hint: "WSL command exceeded timeout.",
        exit_code: null,
      });
    }, typeof timeoutMs === "number" ? timeoutMs : 180_000);
    if (child.stdout) child.stdout.on("data", (b) => { stdoutBuf += b.toString("utf8"); });
    if (child.stderr) child.stderr.on("data", (b) => { stderrBuf += b.toString("utf8"); });
    child.on("error", () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: ENSURE_STATUSES.INSTALL_UNAVAILABLE,
        hint: "wsl.exe failed to start",
        exit_code: null,
      });
    });
    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const combined = (stdoutBuf + "\n" + stderrBuf).toLowerCase();
      const classified = classifyNpmOutput(combined, code);
      resolve(classified);
    });
  });
}

/**
 * @param {Object} opts
 * @param {string} opts.distro
 * @param {boolean} [opts.skipInstall=false]  Only verify, do not npm install.
 */
async function ensureWslRuntime(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  if (platform !== "win32") {
    return { status: ENSURE_STATUSES.UNSUPPORTED_HOST, hint: "ensureWslRuntime is Windows-only." };
  }
  const distro = o.distro;
  if (!distro) {
    return { status: ENSURE_STATUSES.INSTALL_UNAVAILABLE, hint: "no WSL distro resolved." };
  }
  if (o.skipInstall !== true) {
    const install = await runWslBashLc({
      distro,
      cmd: INSTALL_PROBE_CMD,
      env: o.env,
      exec: o.exec,
      wslPath: o.wslPath,
      timeoutMs: o.timeoutMs,
    });
    if (install.status !== ENSURE_STATUSES.OK) {
      return install;
    }
  }
  const verify = await runWslBashLc({
    distro,
    cmd: RUNTIME_VERIFY_CMD,
    env: o.env,
    exec: o.exec,
    wslPath: o.wslPath,
    timeoutMs: o.verifyTimeoutMs || 60_000,
  });
  if (verify.status !== ENSURE_STATUSES.OK) {
    return {
      status: ENSURE_STATUSES.RUNTIME_VERIFY_FAILED,
      hint:
        "terminal-commander-mcp or @terminal-commander/linux-* platform package missing inside WSL; check Linux npm PATH.",
      exit_code: verify.exit_code,
    };
  }
  return { status: ENSURE_STATUSES.OK, hint: "WSL runtime present with platform package." };
}

module.exports = {
  ensureWslRuntime,
  runWslBashLc,
  ENSURE_STATUSES,
  INSTALL_PROBE_CMD: INSTALL_PROBE_CMD,
};
