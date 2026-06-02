// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS03 read-only WSL doctor.
//
// `wslDoctor(opts)` answers, for a given distro: is the WSL host
// callable, does the requested distro exist in the live `wsl.exe -l
// -v` whitelist, and (optionally) does `terminal-commander-mcp` appear
// to be installed inside that distro?
//
// The probe is read-only by default. `opts.probeRuntime === true` is
// the ONLY way to opt into an inside-distro check, and the only
// command issued is the constant string:
//
//   wsl.exe -d <distro> -- bash -lc "command -v terminal-commander-mcp"
//
// Argv shape:
//
//   spawn("wsl.exe", ["-d", distro, "--", "bash", "-lc", PROBE_CMD], {
//     shell: false,
//     stdio: ["ignore", "pipe", "pipe"],
//   })
//
// `distro` is verified TWICE before it is allowed into argv:
//
//   1. `assertSafeDistroName(distro)` — character whitelist.
//   2. Membership in the live `detectWsl()` distro list (caller can
//      override by passing a pre-computed `opts.detectResult`).
//
// PROBE_CMD is a single constant string literal. NO operator value is
// ever concatenated into the `bash -lc` argument. The helper does
// NOT install anything, does NOT mutate the distro, does NOT touch
// any file outside the package directory, does NOT request `sudo`,
// does NOT receive credentials, does NOT write `setup.json`, does
// NOT touch Cursor configuration.

"use strict";

const { detectWsl, normalizeWslOutput, DETECT_REASONS, DEFAULT_TIMEOUT_MS } =
  require("./detect.js");
const {
  assertSafeDistroName,
  UNSAFE_DISTRO_NAME,
} = require("./distro-name.js");
const {
  buildFilteredEnv,
  ensureSessionInWslEnv,
} = require("./filtered_env.js");
const { spawn } = require("node:child_process");

const DOCTOR_STATUSES = Object.freeze({
  OK: "ok",
  UNSUPPORTED_HOST: "unsupported_host",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  DISTRO_NOT_FOUND: "distro_not_found",
  UNSAFE_DISTRO_NAME: "unsafe_distro_name",
  WSL_COMMAND_FAILED: "wsl_command_failed",
  RUNTIME_MISSING: "runtime_missing",
  RUNTIME_PRESENT: "runtime_present",
  DOCTOR_NOT_RUN: "doctor_not_run",
  CHECK_TIMEOUT: "check_timeout",
});

// Constant probe string. The doctor never interpolates operator
// input into this value.
const RUNTIME_PROBE_CMD = "command -v terminal-commander-mcp";

function defaultProbeExec({ wslPath, argv, timeoutMs }) {
  return new Promise((resolve) => {
    let settled = false;
    const stdoutChunks = [];
    const stderrChunks = [];
    let child;
    try {
      child = spawn(wslPath, argv, {
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        // The runtime probe path runs `wsl -d <distro> -- bash -lc ...`, which
        // launches a Linux process. Without an explicit env the child inherits
        // the full process.env, so an ambient WSLENV=SOME_SECRET/u would
        // forward SOME_SECRET into WSL. Rebuild WSLENV to the TC-only allowlist
        // after name-based filtering. (Harmless for the host-side `-l -v`
        // discovery call, which launches no Linux process.)
        env: ensureSessionInWslEnv(buildFilteredEnv(process.env)),
      });
    } catch (err) {
      resolve({
        status: null,
        signal: null,
        stdout: Buffer.alloc(0),
        stderr: Buffer.alloc(0),
        error: err,
      });
      return;
    }

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      try {
        child.kill("SIGKILL");
      } catch (_e) {
        /* ignore */
      }
      const err = new Error("wsl.exe doctor probe timeout");
      err.code = "CHECK_TIMEOUT";
      resolve({
        status: null,
        signal: null,
        stdout: Buffer.concat(stdoutChunks),
        stderr: Buffer.concat(stderrChunks),
        error: err,
      });
    }, timeoutMs);

    child.stdout.on("data", (chunk) => stdoutChunks.push(chunk));
    child.stderr.on("data", (chunk) => stderrChunks.push(chunk));

    child.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: null,
        signal: null,
        stdout: Buffer.concat(stdoutChunks),
        stderr: Buffer.concat(stderrChunks),
        error: err,
      });
    });

    child.on("close", (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: code,
        signal,
        stdout: Buffer.concat(stdoutChunks),
        stderr: Buffer.concat(stderrChunks),
        error: null,
      });
    });
  });
}

function buildResult(partial) {
  return {
    status: partial.status,
    reason: partial.reason || partial.status,
    distro: partial.distro || null,
    runtime_present: partial.runtime_present === true,
    hint: partial.hint || "",
    raw_excerpt_for_debug: partial.raw_excerpt_for_debug,
  };
}

function hintFor(status, distro) {
  switch (status) {
    case DOCTOR_STATUSES.UNSUPPORTED_HOST:
      return "terminal-commander: host is not Windows; wslDoctor is a Windows-only probe.";
    case DOCTOR_STATUSES.WSL_NOT_FOUND:
      return "terminal-commander: wsl.exe was not found on PATH; install WSL via 'wsl --install' from an elevated Windows terminal, then re-run.";
    case DOCTOR_STATUSES.NO_DISTROS:
      return "terminal-commander: WSL is present but no distro is registered; run 'wsl --install -d Ubuntu-24.04' or pick another distro, then re-run.";
    case DOCTOR_STATUSES.DISTRO_NOT_FOUND:
      return `terminal-commander: distro '${distro}' not found in 'wsl -l -v'; re-run setup once the distro is installed.`;
    case DOCTOR_STATUSES.UNSAFE_DISTRO_NAME:
      return "terminal-commander: distro name failed safety whitelist; only ASCII letters, digits, '.', '_' and '-' are allowed (length 1..64).";
    case DOCTOR_STATUSES.WSL_COMMAND_FAILED:
      return "terminal-commander: wsl.exe returned a non-zero exit code; re-run 'terminal-commander doctor' after the next 'wsl -l -v' succeeds.";
    case DOCTOR_STATUSES.RUNTIME_MISSING:
      return `terminal-commander: runtime not installed in distro '${distro}'; run 'wsl -d ${distro} -- bash -lc \"npm install -g terminal-commander\"' from within the distro.`;
    case DOCTOR_STATUSES.RUNTIME_PRESENT:
      return `terminal-commander: runtime appears installed in distro '${distro}'; WWS04 will own the actual bridge spawn.`;
    case DOCTOR_STATUSES.CHECK_TIMEOUT:
      return "terminal-commander: WSL probe exceeded the configured timeout; re-run with --timeout-ms set higher if the distro is unusually slow to start.";
    case DOCTOR_STATUSES.DOCTOR_NOT_RUN:
      return "terminal-commander: runtime probe was not requested (probeRuntime: false); pass probeRuntime: true to check whether terminal-commander-mcp is installed inside the distro.";
    case DOCTOR_STATUSES.OK:
      return `terminal-commander: WSL distro '${distro}' is reachable.`;
    default:
      return "";
  }
}

/**
 * Read-only WSL doctor probe.
 *
 * @param {Object} opts
 * @param {string} opts.distro
 *     Operator-supplied distro name. Validated via
 *     `assertSafeDistroName` BEFORE any executor invocation.
 * @param {boolean} [opts.probeRuntime=false]
 *     When true, runs ONE constant inside-distro probe to check
 *     whether `terminal-commander-mcp` is on the distro's PATH. When
 *     false (the default), returns `doctor_not_run` for the runtime
 *     field without touching the distro.
 * @param {string} [opts.platform=process.platform]
 * @param {(args: {wslPath:string, argv:string[], timeoutMs:number}) => Promise<{status:number|null,signal:string|null,stdout:Buffer,stderr:Buffer,error:Error|null}>} [opts.exec]
 *     Forwarded to `detectWsl` for discovery AND used for the runtime
 *     probe when `probeRuntime` is true. Default is a thin wrapper
 *     around `child_process.spawn('wsl.exe', argv, { shell: false })`.
 * @param {{ default_distro:string|null, distros:Array<{name:string}>, reason:string }} [opts.detectResult]
 *     Pre-computed detect result. When omitted, `wslDoctor` runs
 *     `detectWsl` itself.
 * @param {string} [opts.wslPath="wsl.exe"]
 * @param {number} [opts.timeoutMs=5000]
 * @returns {Promise<{status:string,reason:string,distro:string|null,runtime_present:boolean,hint:string,raw_excerpt_for_debug?:string}>}
 */
async function wslDoctor(opts) {
  const platform = (opts && opts.platform) || process.platform;
  const exec = (opts && opts.exec) || defaultProbeExec;
  const wslPath = (opts && opts.wslPath) || "wsl.exe";
  const timeoutMs =
    opts && typeof opts.timeoutMs === "number" ? opts.timeoutMs : DEFAULT_TIMEOUT_MS;
  const probeRuntime = !!(opts && opts.probeRuntime);
  const requestedDistro = opts && opts.distro;

  // (1) Validate distro-name shape BEFORE anything else. If the
  // string is unsafe, the executor is never invoked.
  try {
    assertSafeDistroName(requestedDistro);
  } catch (err) {
    if (err && err.code === UNSAFE_DISTRO_NAME) {
      return buildResult({
        status: DOCTOR_STATUSES.UNSAFE_DISTRO_NAME,
        reason: DOCTOR_STATUSES.UNSAFE_DISTRO_NAME,
        distro: typeof requestedDistro === "string" ? requestedDistro : null,
        runtime_present: false,
        hint: hintFor(DOCTOR_STATUSES.UNSAFE_DISTRO_NAME, requestedDistro),
      });
    }
    throw err;
  }

  // (2) Discovery. Either reuse a passed-in detect result, or run a
  // fresh `detectWsl` probe.
  const detect =
    opts && opts.detectResult
      ? opts.detectResult
      : await detectWsl({ platform, exec, wslPath, timeoutMs });

  switch (detect.reason) {
    case DETECT_REASONS.UNSUPPORTED_HOST:
      return buildResult({
        status: DOCTOR_STATUSES.UNSUPPORTED_HOST,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.UNSUPPORTED_HOST, requestedDistro),
      });
    case DETECT_REASONS.WSL_NOT_FOUND:
      return buildResult({
        status: DOCTOR_STATUSES.WSL_NOT_FOUND,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.WSL_NOT_FOUND, requestedDistro),
      });
    case DETECT_REASONS.NO_DISTROS:
      return buildResult({
        status: DOCTOR_STATUSES.NO_DISTROS,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.NO_DISTROS, requestedDistro),
      });
    case DETECT_REASONS.WSL_COMMAND_FAILED:
      return buildResult({
        status: DOCTOR_STATUSES.WSL_COMMAND_FAILED,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.WSL_COMMAND_FAILED, requestedDistro),
      });
    case DETECT_REASONS.CHECK_TIMEOUT:
      return buildResult({
        status: DOCTOR_STATUSES.CHECK_TIMEOUT,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.CHECK_TIMEOUT, requestedDistro),
      });
    case DETECT_REASONS.OK:
      // fall through to membership check
      break;
    default:
      return buildResult({
        status: DOCTOR_STATUSES.WSL_COMMAND_FAILED,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.WSL_COMMAND_FAILED, requestedDistro),
      });
  }

  // (3) Whitelist membership. Distros from `wsl.exe -l -v` are the
  // ground truth; operator-supplied strings (even safe-shaped ones)
  // must appear in that list before they are passed to any future
  // bridge spawn.
  const found =
    detect.distros && detect.distros.some((d) => d && d.name === requestedDistro);
  if (!found) {
    return buildResult({
      status: DOCTOR_STATUSES.DISTRO_NOT_FOUND,
      distro: requestedDistro,
      hint: hintFor(DOCTOR_STATUSES.DISTRO_NOT_FOUND, requestedDistro),
    });
  }

  // (4) Optional inside-distro runtime probe. Default OFF.
  if (!probeRuntime) {
    return buildResult({
      status: DOCTOR_STATUSES.OK,
      distro: requestedDistro,
      runtime_present: false,
      hint: hintFor(DOCTOR_STATUSES.DOCTOR_NOT_RUN, requestedDistro),
    });
  }

  const probe = await exec({
    wslPath,
    argv: ["-d", requestedDistro, "--", "bash", "-lc", RUNTIME_PROBE_CMD],
    timeoutMs,
  });

  if (probe.error) {
    if (probe.error.code === "CHECK_TIMEOUT") {
      return buildResult({
        status: DOCTOR_STATUSES.CHECK_TIMEOUT,
        distro: requestedDistro,
        hint: hintFor(DOCTOR_STATUSES.CHECK_TIMEOUT, requestedDistro),
      });
    }
    return buildResult({
      status: DOCTOR_STATUSES.WSL_COMMAND_FAILED,
      distro: requestedDistro,
      hint: hintFor(DOCTOR_STATUSES.WSL_COMMAND_FAILED, requestedDistro),
    });
  }

  const stdoutText = normalizeWslOutput(probe.stdout).trim();
  if (probe.status === 0 && stdoutText.length > 0) {
    return buildResult({
      status: DOCTOR_STATUSES.RUNTIME_PRESENT,
      distro: requestedDistro,
      runtime_present: true,
      hint: hintFor(DOCTOR_STATUSES.RUNTIME_PRESENT, requestedDistro),
      raw_excerpt_for_debug: stdoutText.slice(0, 200),
    });
  }

  return buildResult({
    status: DOCTOR_STATUSES.RUNTIME_MISSING,
    distro: requestedDistro,
    runtime_present: false,
    hint: hintFor(DOCTOR_STATUSES.RUNTIME_MISSING, requestedDistro),
  });
}

module.exports = {
  wslDoctor,
  DOCTOR_STATUSES,
  RUNTIME_PROBE_CMD,
};
