// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Install / setup bootstrap orchestrator.

"use strict";

const { detectWsl, DETECT_REASONS } = require("../wsl/detect.js");
const { wslDoctor, DOCTOR_STATUSES } = require("../wsl/doctor.js");
const { resolveDistro } = require("../cli/setup_cursor_wsl.js");
const { writeSetupJson, readSetupJson } = require("../cli/setup_state.js");
const { detectAllHarnesses } = require("../harness/detect.js");
const { writeAllHarnesses, HARNESS_WRITE_STATUSES } = require("../harness/write_all.js");
const { ensureWslRuntime, ENSURE_STATUSES } = require("./ensure_wsl_runtime.js");
const { tryAcquireBootstrapLock, releaseBootstrapLock } = require("./lock.js");
const { shouldSkipBootstrap, isGlobalNpmInstall } = require("./skip.js");

const BOOTSTRAP_STATUSES = Object.freeze({
  BOOTSTRAP_READY: "bootstrap_ready",
  BOOTSTRAP_PARTIAL: "bootstrap_partial",
  BOOTSTRAP_SKIPPED: "bootstrap_skipped",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  NO_DEFAULT_DISTRO: "no_default_distro_ambiguous",
  UNSUPPORTED_HOST: "unsupported_host",
  WSL_RUNTIME_FAILED: "wsl_runtime_failed",
});

function logStderr(lines) {
  for (const line of lines) {
    if (line) process.stderr.write(`${line}\n`);
  }
}

/**
 * @param {Object} opts
 * @param {"install"|"cli"|"lazy"} [opts.mode]
 */
async function runBootstrap(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const mode = o.mode || "cli";
  const lines = [];
  const failSoft = mode === "install";

  if (shouldSkipBootstrap(env)) {
    return {
      status: BOOTSTRAP_STATUSES.BOOTSTRAP_SKIPPED,
      exit_code: 0,
      output: "bootstrap skipped (TC_SKIP_BOOTSTRAP=1).",
      lines: [],
    };
  }

  if (mode === "install" && platform === "win32" && !isGlobalNpmInstall(env)) {
    return {
      status: BOOTSTRAP_STATUSES.BOOTSTRAP_SKIPPED,
      exit_code: 0,
      output: "bootstrap skipped (not a global npm install).",
      lines: [],
    };
  }

  const lock =
    o.acquireLock === false
      ? { acquired: true }
      : tryAcquireBootstrapLock({ platform, env, stateDir: o.stateDir });
  if (!lock.acquired) {
    return {
      status: BOOTSTRAP_STATUSES.BOOTSTRAP_SKIPPED,
      exit_code: 0,
      output: "bootstrap skipped (another bootstrap in progress).",
      lines: [],
    };
  }

  let distro = null;
  let knownDistros = [];
  let harnessResults = [];
  let configured = [];

  try {

    if (platform === "win32") {
      const detect = o.detect || detectWsl;
      const detectResult = await detect({ platform, wslPath: o.wslPath, timeoutMs: o.timeoutMs });
      knownDistros = detectResult.distros || [];

      if (detectResult.reason === DETECT_REASONS.WSL_NOT_FOUND) {
        const msg =
          "terminal-commander: WSL not found; install WSL then re-run setup harness.";
        lines.push(msg);
        logStderr(lines);
        return {
          status: BOOTSTRAP_STATUSES.WSL_NOT_FOUND,
          exit_code: failSoft ? 0 : 64,
          output: msg,
          lines,
        };
      }
      if (detectResult.reason === DETECT_REASONS.NO_DISTROS) {
        const msg = "terminal-commander: no WSL distros registered.";
        lines.push(msg);
        logStderr(lines);
        return {
          status: BOOTSTRAP_STATUSES.NO_DISTROS,
          exit_code: failSoft ? 0 : 64,
          output: msg,
          lines,
        };
      }

      const prior = readSetupJson({ platform, env, stateDir: o.stateDir });
      const priorDistro =
        prior.ok && prior.value && prior.value.distro ? prior.value.distro : null;

      const resolved = resolveDistro({
        flags: { distro: o.distro || priorDistro },
        env,
        detectResult,
      });
      if (resolved.status !== "ok") {
        const msg = `terminal-commander: could not resolve WSL distro (${resolved.status}).`;
        lines.push(msg);
        logStderr(lines);
        return {
          status: BOOTSTRAP_STATUSES.NO_DEFAULT_DISTRO,
          exit_code: failSoft ? 0 : 64,
          output: msg,
          lines,
        };
      }
      distro = resolved.distro;

      const skipWslInstall = o.skipWslInstall === true;
      if (!skipWslInstall) {
        const doctor = o.doctor || wslDoctor;
        let needInstall = true;
        const doc = await doctor({
          distro,
          platform,
          probeRuntime: true,
          detectResult,
          wslPath: o.wslPath,
          timeoutMs: o.timeoutMs,
        });
        if (doc.status === DOCTOR_STATUSES.RUNTIME_PRESENT) {
          needInstall = false;
        }
        if (needInstall) {
          const ensure = await (o.ensureWslRuntime || ensureWslRuntime)({
            distro,
            platform,
            env,
            exec: o.exec,
            wslPath: o.wslPath,
            timeoutMs: o.timeoutMs,
          });
          if (ensure.status !== ENSURE_STATUSES.OK) {
            lines.push(`terminal-commander: WSL runtime ensure: ${ensure.status} — ${ensure.hint}`);
            if (!failSoft) {
              logStderr(lines);
              return {
                status: BOOTSTRAP_STATUSES.WSL_RUNTIME_FAILED,
                exit_code: 64,
                output: ensure.hint,
                lines,
                distro,
              };
            }
          } else {
            lines.push("terminal-commander: WSL runtime installed and verified.");
          }
        } else {
          lines.push("terminal-commander: WSL runtime already present.");
        }
      }
    }

    harnessResults = (o.writeAllHarnesses || writeAllHarnesses)({
      platform,
      env,
      distro,
      knownDistros,
      requireKnownDistro: platform === "win32" && distro != null,
      force: o.force === true,
      clobber_backup: o.clobber_backup === true,
      dry_run: o.dry_run === true,
      cursor_scope: o.cursor_scope || "global",
      projectRoot: o.projectRoot,
      providerFilter: o.providerFilter,
      cursorOnly: o.cursorOnly === true,
      randomSuffix: o.randomSuffix,
    });

    configured = [];
    for (const r of harnessResults) {
      if (r.status === HARNESS_WRITE_STATUSES.OK) {
        configured.push(r.id);
        if (r.hint) lines.push(r.hint);
      } else if (r.status === HARNESS_WRITE_STATUSES.STUB_UNVERIFIED) {
        lines.push(r.hint || `${r.id}: stub`);
      } else if (r.status === HARNESS_WRITE_STATUSES.FAILED && r.hint) {
        lines.push(r.hint);
      }
    }

    if (o.dry_run !== true) {
      const writeState = o.writeState || writeSetupJson;
      writeState({
        platform,
        env,
        stateDir: o.stateDir,
        distro,
        cursor_scope: o.cursor_scope || "global",
        providers_configured: configured,
        bootstrap_at: (typeof o.now === "function" ? o.now() : new Date()).toISOString(),
        bootstrap_mode: mode,
      });
    }

    logStderr(lines);
    const status =
      configured.length > 0
        ? BOOTSTRAP_STATUSES.BOOTSTRAP_READY
        : BOOTSTRAP_STATUSES.BOOTSTRAP_PARTIAL;

    return {
      status,
      exit_code: 0,
      output: lines.join("\n"),
      lines,
      harness_results: harnessResults,
      distro,
    };
  } finally {
    if (o.acquireLock !== false) {
      releaseBootstrapLock({ platform, env, stateDir: o.stateDir });
    }
  }
}

module.exports = {
  runBootstrap,
  shouldSkipBootstrap,
  isGlobalNpmInstall,
  BOOTSTRAP_STATUSES,
};
// shouldSkipBootstrap / isGlobalNpmInstall live in ./skip.js (re-exported above).
