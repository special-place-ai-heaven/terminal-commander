// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// `terminal-commander restart` handler.
//
// Replaces the running terminal-commander daemon with the installed
// binary by invoking `terminal-commanderd update --force` (the F4
// forced-replace path). This is the operator verb for "I upgraded the
// package; swap the running daemon."
//
// Windows defaults to the native daemon. WSL is selected only by explicit
// operator configuration, then dispatched through the resolved distro:
//   wsl.exe -d <distro> -- bash -lc '<prefix>terminal-commanderd update --force'
// Native Windows, Linux, and WSL invoke their local daemon binary directly.
//
// NO sudo. NO credential. NO npm install. The distro name is double-
// validated by resolveDistro (whitelist + live membership) before it
// reaches argv. env is filtered through buildFilteredEnv so no secret-
// shaped variable is forwarded into the child.

"use strict";

const { resolveDistro } = require("./setup_cursor_wsl.js");
const { detectWsl } = require("../wsl/detect.js");
const { LINUX_PATH_PREFIX } = require("../bootstrap/constants.js");
const { buildFilteredEnv } = require("../wsl/filtered_env.js");
const { ensureSessionInWslEnv } = require("../wsl/spawn.js");
const { resolveBinary, formatResolveError } = require("../resolve-binary.js");
const { detectRuntimeEnvironment } = require("./runtime_environment.js");

// The daemon-side command. `--force` is always passed: restart's whole
// purpose is to replace even a same-version daemon.
const DAEMON_RESTART_CMD = "terminal-commanderd update --force";

function defaultExec({ file, argv, env }) {
  const { spawn } = require("node:child_process");
  return spawn(file, argv, {
    stdio: ["ignore", "pipe", "pipe"],
    shell: false,
    env,
  });
}

async function collectChild(child) {
  return new Promise((resolve) => {
    let out = "";
    let err = "";
    if (child.stdout) child.stdout.on("data", (b) => { out += b.toString("utf8"); });
    if (child.stderr) child.stderr.on("data", (b) => { err += b.toString("utf8"); });
    child.on("close", (code) => resolve({ code: typeof code === "number" ? code : 1, out, err }));
    child.on("error", () => resolve({ code: 1, out, err: `${err}spawn_failed` }));
  });
}

/**
 * Run `terminal-commander restart`.
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @param {Object} [opts.flags]                Parsed flags ({ distro?, force? }).
 * @param {Function} [opts.detect]             Override for detectWsl (Windows).
 * @param {Function} [opts.exec]               Injected spawn ({file,argv,env}) -> child.
 * @param {string} [opts.wslPath="wsl.exe"]
 * @returns {Promise<{status:string, exit_code:number, output:string}>}
 */
async function runRestart(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const flags = o.flags || {};
  const exec = o.exec || defaultExec;
  const wslPath = o.wslPath || "wsl.exe";

  const environment = detectRuntimeEnvironment({ platform, env, flags });
  if (environment.status !== "ok") {
    return {
      status: "unsupported_environment",
      exit_code: 64,
      output: `terminal-commander: restart unsupported environment (${environment.evidence}).\n`,
    };
  }

  if (platform === "win32" && environment.runtime === "wsl") {
    const detectResult = await (o.detect || detectWsl)({ platform });
    const resolved = resolveDistro({
      flags: { distro: flags.distro },
      env,
      detectResult,
    });
    if (resolved.status !== "ok") {
      return {
        status: resolved.status,
        exit_code: 64,
        output: `terminal-commander: could not resolve WSL distro (${resolved.status}).\n`,
      };
    }
    const argv = [
      "-d",
      resolved.distro,
      "--",
      "bash",
      "-lc",
      `${LINUX_PATH_PREFIX}${DAEMON_RESTART_CMD}`,
    ];
    // Apply the same WSLENV defense the bridge spawn uses: reduce
    // WSLENV to a TC-only allowlist (or drop it) so wsl.exe cannot
    // forward an operator's ambient credential-shaped vars (e.g.
    // WSL_SUDO_CREDENTIAL) across the Windows->Linux boundary on
    // restart. buildFilteredEnv only strips by variable *name*; WSLENV
    // forwarding is keyed on names listed in WSLENV, so it needs this
    // separate, explicit allowlist step.
    const child = exec({
      file: wslPath,
      argv,
      env: ensureSessionInWslEnv(buildFilteredEnv(env)),
    });
    const { code, out, err } = await collectChild(child);
    const tail = `${out}${err}`.trim();
    return {
      status: code === 0 ? "ok" : "restart_failed",
      exit_code: code,
      output:
        code === 0
          ? `terminal-commander: daemon restarted in '${resolved.distro}'.\n${tail ? `${tail}\n` : ""}`
          : `terminal-commander: daemon restart failed in '${resolved.distro}' (exit ${code}).\n${tail ? `${tail}\n` : ""}`,
    };
  }

  if (platform === "win32") {
    const resolve = o.resolveBinary || resolveBinary;
    const arch = o.arch || process.arch;
    const resolved = resolve({ binary: "terminal-commanderd", platform, arch });
    if (resolved.reason !== "ok" || !resolved.binaryPath) {
      return {
        status: "restart_unavailable",
        exit_code: 64,
        output: `${formatResolveError(resolved, { platform, arch })}\n`,
      };
    }
    const child = exec({
      file: resolved.binaryPath,
      argv: ["update", "--force"],
      env: buildFilteredEnv(env),
    });
    const { code, out, err } = await collectChild(child);
    const tail = `${out}${err}`.trim();
    return {
      status: code === 0 ? "ok" : "restart_failed",
      exit_code: code,
      output:
        code === 0
          ? `terminal-commander: native Windows daemon restarted.\n${tail ? `${tail}\n` : ""}`
          : `terminal-commander: native Windows daemon restart failed (exit ${code}).\n${tail ? `${tail}\n` : ""}`,
    };
  }

  // Linux / WSL: invoke the daemon binary directly.
  const child = exec({
    file: "terminal-commanderd",
    argv: ["update", "--force"],
    env: buildFilteredEnv(env),
  });
  const { code, out, err } = await collectChild(child);
  const tail = `${out}${err}`.trim();
  return {
    status: code === 0 ? "ok" : "restart_failed",
    exit_code: code,
    output:
      code === 0
        ? `terminal-commander: daemon restarted.\n${tail ? `${tail}\n` : ""}`
        : `terminal-commander: daemon restart failed (exit ${code}).\n${tail ? `${tail}\n` : ""}`,
  };
}

module.exports = { runRestart, DAEMON_RESTART_CMD };
