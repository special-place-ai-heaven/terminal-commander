// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS04 Windows -> WSL MCP bridge spawn helper.
//
// Cursor on Windows launches `terminal-commander-mcp`. On a Windows
// host the WWS02 resolver returns `bridge_required`; at WWS04 the
// shim calls `spawnWslBridge()` here. This helper:
//
//   1. Resolves the WSL distro:
//        Priority 1: `process.env.TC_WSL_DISTRO` (operator override)
//        Priority 2: `detectWsl().default_distro`
//        Priority 3: refuse with `no_default_distro`
//   2. Double-validates the chosen distro:
//        (a) `assertSafeDistroName(distro)` — character whitelist
//        (b) membership in the live `detectWsl()` distro list
//   3. Optionally runs the WWS03 `wslDoctor({ probeRuntime: true })`
//      gate (skipped iff `env.TC_WSL_SKIP_DOCTOR === '1'`); refuses
//      with `runtime_missing` when the WSL distro does not yet have
//      `terminal-commander-mcp` installed. Runtime setup is an
//      explicit operator action; the bridge never performs lazy
//      bootstrap or install work.
//   4. Builds a defensive `filteredEnv` copy of `process.env` with
//      token-shaped variables stripped (see SECRET_ENV_PATTERNS).
//   5. Spawns:
//        wsl.exe -d <distro> -- bash -lc 'exec terminal-commander-mcp' [...userArgv]
//      with EXACTLY:
//        { shell: false, stdio: 'inherit', env: filteredEnv }
//      `BRIDGE_PROBE_CMD` is a literal constant; no operator value is
//      interpolated into the `bash -lc` argument.
//   6. Forwards SIGINT / SIGTERM from the parent process to the child.
//   7. Mirrors the child's exit code / signal into the parent
//      `process.exit()` so Cursor sees the right shape.
//
// The shim writes nothing to stdout. All status lines go to stderr.
// rmcp framing on stdout/stdin passes through the WSL pipe
// transparently.

"use strict";

const { spawn } = require("node:child_process");
const { detectWsl, DETECT_REASONS } = require("./detect.js");
const { wslDoctor, DOCTOR_STATUSES } = require("./doctor.js");
const {
  buildFilteredEnv,
  isSecretEnvKey,
  SECRET_ENV_PATTERNS,
  EXPLICIT_SECRET_KEYS,
} = require("./filtered_env.js");
const {
  assertSafeDistroName,
  isSafeDistroName,
  UNSAFE_DISTRO_NAME,
} = require("./distro-name.js");
const { BRIDGE_PROBE_CMD } = require("../bootstrap/constants.js");

const BRIDGE_STATUSES = Object.freeze({
  OK: "ok",
  UNSUPPORTED_HOST: "unsupported_host",
  UNSAFE_DISTRO_NAME: "unsafe_distro_name",
  NO_DEFAULT_DISTRO: "no_default_distro",
  DISTRO_NOT_FOUND: "distro_not_found",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  WSL_COMMAND_FAILED: "wsl_command_failed",
  CHECK_TIMEOUT: "check_timeout",
  RUNTIME_MISSING: "runtime_missing",
  BRIDGE_SPAWN_FAILED: "bridge_spawn_failed",
  BRIDGE_CHILD_EXIT: "bridge_child_exit",
});

function buildResult(partial) {
  return {
    status: partial.status,
    distro: partial.distro || null,
    exit_code: typeof partial.exit_code === "number" ? partial.exit_code : null,
    signal: partial.signal || null,
    hint: partial.hint || "",
  };
}

function hintFor(status, distro) {
  switch (status) {
    case BRIDGE_STATUSES.UNSUPPORTED_HOST:
      return "terminal-commander: bridge is Windows-only; on Linux/WSL run terminal-commander-mcp directly.";
    case BRIDGE_STATUSES.UNSAFE_DISTRO_NAME:
      return "terminal-commander: distro name failed safety whitelist; only ASCII letters, digits, '.', '_' and '-' are allowed (length 1..64).";
    case BRIDGE_STATUSES.NO_DEFAULT_DISTRO:
      return "terminal-commander: no WSL distro selected; set TC_WSL_DISTRO=<name> or run 'terminal-commander setup cursor-wsl' (pending WWS06) to persist a default.";
    case BRIDGE_STATUSES.DISTRO_NOT_FOUND:
      return `terminal-commander: distro '${distro}' not found in 'wsl -l -v'; install it or set TC_WSL_DISTRO to a registered distro.`;
    case BRIDGE_STATUSES.WSL_NOT_FOUND:
      return "terminal-commander: wsl.exe not found on PATH; install WSL via 'wsl --install' from an elevated Windows terminal, then re-run.";
    case BRIDGE_STATUSES.NO_DISTROS:
      return "terminal-commander: WSL is present but no distro is registered; run 'wsl --install -d Ubuntu-24.04' or pick another, then re-run.";
    case BRIDGE_STATUSES.WSL_COMMAND_FAILED:
      return "terminal-commander: wsl.exe returned a non-zero exit during discovery; re-run after 'wsl -l -v' succeeds.";
    case BRIDGE_STATUSES.CHECK_TIMEOUT:
      return "terminal-commander: WSL probe exceeded the configured timeout; the distro may be unusually slow to start.";
    case BRIDGE_STATUSES.RUNTIME_MISSING:
      return `terminal-commander runtime is not installed inside '${distro}'; run 'terminal-commander setup cursor-wsl --install-wsl-runtime' or install terminal-commander manually inside that WSL distro.`;
    case BRIDGE_STATUSES.BRIDGE_SPAWN_FAILED:
      return "terminal-commander: failed to spawn wsl.exe for the MCP bridge; check that wsl.exe is on PATH and the distro is reachable.";
    case BRIDGE_STATUSES.BRIDGE_CHILD_EXIT:
      return `terminal-commander: bridge child terminated abnormally inside '${distro}'.`;
    case BRIDGE_STATUSES.OK:
      return `terminal-commander: bridge active via wsl.exe -d '${distro}'.`;
    default:
      return "";
  }
}

/**
 * Resolve the bridge distro per the locked priority chain.
 *
 * @param {NodeJS.ProcessEnv} env
 * @param {{ default_distro:string|null, distros:Array<{name:string}>, reason:string }} detect
 * @returns {{ status: string, distro: string|null }}
 */
function resolveBridgeDistro(env, detect) {
  // Priority 1: operator override.
  if (env.TC_WSL_DISTRO && env.TC_WSL_DISTRO.length > 0) {
    const requested = env.TC_WSL_DISTRO;
    if (!isSafeDistroName(requested)) {
      return { status: BRIDGE_STATUSES.UNSAFE_DISTRO_NAME, distro: requested };
    }
    const found =
      detect.distros && detect.distros.some((d) => d && d.name === requested);
    if (!found) {
      return { status: BRIDGE_STATUSES.DISTRO_NOT_FOUND, distro: requested };
    }
    return { status: BRIDGE_STATUSES.OK, distro: requested };
  }
  // Priority 2: WSL default distro.
  if (detect.default_distro) {
    const def = detect.default_distro;
    if (!isSafeDistroName(def)) {
      return { status: BRIDGE_STATUSES.UNSAFE_DISTRO_NAME, distro: def };
    }
    return { status: BRIDGE_STATUSES.OK, distro: def };
  }
  // Priority 3: refuse.
  return { status: BRIDGE_STATUSES.NO_DEFAULT_DISTRO, distro: null };
}

function defaultBridgeExec({ wslPath, argv, env }) {
  // Production path. Returns a child handle so the shim can wire
  // signal forwarding + exit mirroring on top. `env` is the
  // already-filtered env produced by `buildFilteredEnv()`; we name
  // it `env` here on both sides of the colon so the static guard
  // can pattern-match `env: env` and prove there is no raw
  // `process.env` forwarded.
  return spawn(wslPath, argv, {
    stdio: "inherit",
    shell: false,
    env: env,
  });
}

/**
 * Windows -> WSL MCP bridge entrypoint.
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @param {string[]} [opts.argv=process.argv.slice(2)]
 *     Extra arguments forwarded to the WSL-side `terminal-commander-mcp`.
 *     Pushed AFTER the constant `BRIDGE_PROBE_CMD` so `bash -lc`
 *     receives them in $0..$N. No operator value is interpolated into
 *     `BRIDGE_PROBE_CMD` itself.
 * @param {(args:{wslPath:string, argv:string[], env:object}) => {on:Function, kill:Function, pid:number|null}|object} [opts.exec]
 *     Injected spawn function. Defaults to a thin wrapper around
 *     `child_process.spawn('wsl.exe', argv, { shell:false,
 *     stdio:'inherit', env })`.
 * @param {Function} [opts.detect]  Override for `detectWsl`.
 * @param {Function} [opts.doctor]  Override for `wslDoctor`.
 * @param {string} [opts.wslPath="wsl.exe"]
 * @param {number} [opts.timeoutMs=5000]
 * @param {boolean} [opts.returnInsteadOfMirror=false]
 *     Test-only path. When true, returns the bridge result instead
 *     of calling `process.exit()` / wiring signals.
 * @returns {Promise<{status:string, distro:string|null, exit_code:number|null, signal:string|null, hint:string}>}
 *
 * Production-path contract:
 *   - All status text goes to stderr; nothing is written to stdout
 *     (rmcp framing lives there).
 *   - On `ok`, the function never returns to the caller; instead it
 *     wires signal forwarding and calls `process.exit(child.exit_code)`
 *     when the child closes.
 *   - On non-OK statuses, the function returns the result; the shim
 *     writes `hint` to stderr and exits 64.
 */
async function spawnWslBridge(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const userArgv = Array.isArray(o.argv) ? o.argv : process.argv.slice(2);
  const exec = o.exec || defaultBridgeExec;
  const detect = o.detect || detectWsl;
  const doctor = o.doctor || wslDoctor;
  const wslPath = o.wslPath || "wsl.exe";
  const timeoutMs = typeof o.timeoutMs === "number" ? o.timeoutMs : 5000;
  const returnInsteadOfMirror = o.returnInsteadOfMirror === true;

  if (platform !== "win32") {
    return buildResult({
      status: BRIDGE_STATUSES.UNSUPPORTED_HOST,
      hint: hintFor(BRIDGE_STATUSES.UNSUPPORTED_HOST),
    });
  }

  // (1) Discovery.
  const detectResult = await detect({ platform, wslPath, timeoutMs });

  if (detectResult.reason !== DETECT_REASONS.OK) {
    let status;
    switch (detectResult.reason) {
      case DETECT_REASONS.UNSUPPORTED_HOST:
        status = BRIDGE_STATUSES.UNSUPPORTED_HOST;
        break;
      case DETECT_REASONS.WSL_NOT_FOUND:
        status = BRIDGE_STATUSES.WSL_NOT_FOUND;
        break;
      case DETECT_REASONS.NO_DISTROS:
        status = BRIDGE_STATUSES.NO_DISTROS;
        break;
      case DETECT_REASONS.CHECK_TIMEOUT:
        status = BRIDGE_STATUSES.CHECK_TIMEOUT;
        break;
      default:
        status = BRIDGE_STATUSES.WSL_COMMAND_FAILED;
        break;
    }
    return buildResult({ status, hint: hintFor(status) });
  }

  // (2) Distro resolution + safety check.
  const resolved = resolveBridgeDistro(env, detectResult);
  if (resolved.status !== BRIDGE_STATUSES.OK) {
    return buildResult({
      status: resolved.status,
      distro: resolved.distro,
      hint: hintFor(resolved.status, resolved.distro),
    });
  }
  const distro = resolved.distro;

  // Defense in depth: assertSafeDistroName again at the spawn-site
  // boundary. resolveBridgeDistro already screened, but a future
  // refactor must not be able to slip an unvetted name into argv.
  try {
    assertSafeDistroName(distro);
  } catch (err) {
    if (err && err.code === UNSAFE_DISTRO_NAME) {
      return buildResult({
        status: BRIDGE_STATUSES.UNSAFE_DISTRO_NAME,
        distro,
        hint: hintFor(BRIDGE_STATUSES.UNSAFE_DISTRO_NAME, distro),
      });
    }
    throw err;
  }

  // (3) Optional runtime-presence gate.
  if (env.TC_WSL_SKIP_DOCTOR !== "1") {
    const doc = await doctor({
      distro,
      platform,
      probeRuntime: true,
      detectResult,
      wslPath,
      timeoutMs,
    });
    if (doc.status === DOCTOR_STATUSES.RUNTIME_MISSING) {
      return buildResult({
        status: BRIDGE_STATUSES.RUNTIME_MISSING,
        distro,
        hint: hintFor(BRIDGE_STATUSES.RUNTIME_MISSING, distro),
      });
    }
    if (doc.status === DOCTOR_STATUSES.UNSAFE_DISTRO_NAME) {
      // Should never happen given the prior assertion, but mirror
      // the bounded error if it does.
      return buildResult({
        status: BRIDGE_STATUSES.UNSAFE_DISTRO_NAME,
        distro,
        hint: hintFor(BRIDGE_STATUSES.UNSAFE_DISTRO_NAME, distro),
      });
    }
    // Other doctor failures (wsl_command_failed, check_timeout)
    // fall through to bridge launch; the spawn itself will surface
    // any persistent error.
  }

  // (4) Build argv + filtered env, then spawn.
  const filteredEnv = buildFilteredEnv(env);
  const argv = [
    "-d",
    distro,
    "--",
    "bash",
    "-lc",
    BRIDGE_PROBE_CMD,
    ...userArgv,
  ];

  let child;
  try {
    child = exec({ wslPath, argv, env: filteredEnv });
  } catch (err) {
    return buildResult({
      status: BRIDGE_STATUSES.BRIDGE_SPAWN_FAILED,
      distro,
      hint: hintFor(BRIDGE_STATUSES.BRIDGE_SPAWN_FAILED, distro),
    });
  }

  if (returnInsteadOfMirror) {
    // Test path: synchronously return ok so unit tests can assert
    // argv / env without having to drive a real child process.
    return buildResult({
      status: BRIDGE_STATUSES.OK,
      distro,
      hint: hintFor(BRIDGE_STATUSES.OK, distro),
    });
  }

  // (5) Wire signal forwarding + exit mirroring.
  let exited = false;
  const forwardSignal = (sig) => {
    if (exited) return;
    try {
      child.kill(sig);
    } catch (_e) {
      /* ignore */
    }
  };
  process.on("SIGINT", () => forwardSignal("SIGINT"));
  process.on("SIGTERM", () => forwardSignal("SIGTERM"));

  return new Promise((resolve) => {
    child.on("error", (err) => {
      if (exited) return;
      exited = true;
      // Bounded stderr; no env dump, no raw error stack.
      process.stderr.write(
        `terminal-commander: bridge child error: ${err && err.code ? err.code : "spawn_failed"}\n`,
      );
      const result = buildResult({
        status: BRIDGE_STATUSES.BRIDGE_SPAWN_FAILED,
        distro,
        hint: hintFor(BRIDGE_STATUSES.BRIDGE_SPAWN_FAILED, distro),
      });
      resolve(result);
    });

    child.on("close", (code, signal) => {
      if (exited) return;
      exited = true;
      const result = buildResult({
        status: BRIDGE_STATUSES.OK,
        distro,
        exit_code: typeof code === "number" ? code : null,
        signal: signal || null,
        hint: hintFor(BRIDGE_STATUSES.OK, distro),
      });
      resolve(result);
    });
  });
}

module.exports = {
  spawnWslBridge,
  resolveBridgeDistro,
  buildFilteredEnv,
  isSecretEnvKey,
  BRIDGE_STATUSES,
  BRIDGE_PROBE_CMD,
  SECRET_ENV_PATTERNS,
  EXPLICIT_SECRET_KEYS,
};
