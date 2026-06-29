// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
const { ensureStableBinaries, resolveDirectExePath } = require("../harness/stable_bin.js");
const { ensureWslRuntime, ENSURE_STATUSES } = require("./ensure_wsl_runtime.js");
const {
  ensureDaemonAutostartInWsl,
  ENSURE_DAEMON_STATUSES,
} = require("./ensure_daemon_autostart.js");
const {
  installDaemonAutostart,
  shouldInstallDaemonAutostart,
  AUTOSTART_STATUSES,
} = require("../daemon/autostart.js");
const { tryAcquireBootstrapLock, releaseBootstrapLock } = require("./lock.js");
const {
  shouldSkipBootstrap,
  isGlobalNpmInstall,
  isPackageInstallLifecycle,
} = require("./skip.js");
const { harnessNeedsConfiguration } = require("../harness/needs.js");
const { runWslBashLc } = require("./ensure_wsl_runtime.js");
const { LINUX_PATH_PREFIX, RUNTIME_VERSION_CMD } = require("./constants.js");
const { DAEMON_RESTART_CMD } = require("../cli/restart.js");

// Authoritative host runtime version. The WSL runtime must match this: a stale
// WSL runtime serves `health` but not command execution (daemon skew), so the
// install gate compares versions rather than checking presence alone.
const HOST_VERSION = require("../../package.json").version;

const BOOTSTRAP_STATUSES = Object.freeze({
  BOOTSTRAP_READY: "bootstrap_ready",
  BOOTSTRAP_PARTIAL: "bootstrap_partial",
  BOOTSTRAP_SKIPPED: "bootstrap_skipped",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  NO_DEFAULT_DISTRO: "no_default_distro_ambiguous",
  UNSUPPORTED_HOST: "unsupported_host",
  WSL_RUNTIME_FAILED: "wsl_runtime_failed",
  HARNESS_FAILED: "harness_failed",
});

function logStderr(lines) {
  for (const line of lines) {
    if (line) process.stderr.write(`${line}\n`);
  }
}

// clap `--version` prints `terminal-commander-mcp <ver>`; take the last
// whitespace token of the last non-empty line. Returns null when unreadable.
function parseRuntimeVersion(stdout) {
  if (!stdout) return null;
  const text = String(stdout).trim();
  if (!text) return null;
  const lastLine = text.split(/\r?\n/).filter((l) => l.trim().length > 0).pop() || "";
  const token = lastLine.trim().split(/\s+/).pop() || "";
  return token || null;
}

// Default WSL runtime-version probe. Runs RUNTIME_VERSION_CMD in the distro via
// the shared runWslBashLc helper (which surfaces raw stdout) and parses the
// version token. Injectable as `o.probeRuntimeVersion` for deterministic tests.
async function defaultProbeRuntimeVersion({ distro, env, exec, wslPath, timeoutMs }) {
  const res = await runWslBashLc({
    distro,
    cmd: RUNTIME_VERSION_CMD,
    env,
    exec,
    wslPath,
    timeoutMs: typeof timeoutMs === "number" ? timeoutMs : 15_000,
  });
  return parseRuntimeVersion(res && res.stdout);
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
  const failSoft = mode === "install" || mode === "lazy";
  const emitOutput = o.emitOutput === true;
  const autoConfigure = mode === "install" || mode === "lazy" || o.auto_configure === true;
  // `--print-config` is a superset of `--dry-run`: print the planned
  // stanzas and write nothing. Both gate every filesystem write below.
  const printConfig = o.print_config === true;
  const noWrite = o.dry_run === true || printConfig;

  if (shouldSkipBootstrap(env)) {
    return {
      status: BOOTSTRAP_STATUSES.BOOTSTRAP_SKIPPED,
      exit_code: 0,
      output: "bootstrap skipped (TC_SKIP_BOOTSTRAP=1).",
      lines: [],
    };
  }

  if (mode === "install" && !isPackageInstallLifecycle(env) && o.require_install_lifecycle !== false) {
    return {
      status: BOOTSTRAP_STATUSES.BOOTSTRAP_SKIPPED,
      exit_code: 0,
      output: "bootstrap skipped (not an npm install lifecycle run).",
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
      const wantsWslRuntime =
        env.TC_USE_LEGACY_WSL_BRIDGE === "1" ||
        typeof o.distro === "string" ||
        typeof env.TC_WSL_DISTRO === "string";

      if (!wantsWslRuntime) {
        lines.push("terminal-commander: native Windows MCP path selected; WSL bootstrap skipped.");
      } else {
      const detect = o.detect || detectWsl;
      const detectResult = await detect({ platform, wslPath: o.wslPath, timeoutMs: o.timeoutMs });
      knownDistros = detectResult.distros || [];

      if (detectResult.reason === DETECT_REASONS.WSL_NOT_FOUND) {
        const msg =
          "terminal-commander: WSL not found; install WSL (wsl --install), then run terminal-commander setup harness again.";
        lines.push(msg);
        if (emitOutput) logStderr(lines);
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
        if (emitOutput) logStderr(lines);
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
        if (emitOutput) logStderr(lines);
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
        // Only an actual version skew (runtime present but != host) leaves a
        // stale LIVE daemon the upgrade must swap. A fresh install (runtime
        // absent) had no daemon to replace, so the swap stays gated on skew.
        let skewDetected = false;
        const doc = await doctor({
          distro,
          platform,
          probeRuntime: true,
          detectResult,
          wslPath: o.wslPath,
          timeoutMs: o.timeoutMs,
        });
        if (doc.status === DOCTOR_STATUSES.RUNTIME_PRESENT) {
          // Presence is NOT enough: a stale WSL runtime serves `health` but not
          // command execution. Compare the WSL runtime version to the host
          // package version; upgrade on skew (or when the version is unreadable).
          const probeVersion = o.probeRuntimeVersion || defaultProbeRuntimeVersion;
          const wslVersion = await probeVersion({
            distro,
            env,
            exec: o.exec,
            wslPath: o.wslPath,
            timeoutMs: o.timeoutMs,
          });
          if (wslVersion && wslVersion === HOST_VERSION) {
            needInstall = false;
          } else {
            needInstall = true;
            skewDetected = true;
            lines.push(
              wslVersion
                ? `terminal-commander: WSL runtime ${wslVersion} != host ${HOST_VERSION}; upgrading WSL runtime.`
                : `terminal-commander: WSL runtime version unreadable; reinstalling to match host ${HOST_VERSION}.`,
            );
          }
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
              if (emitOutput) logStderr(lines);
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
            // On a DETECTED skew only, swap the live daemon once: re-sourcing
            // autostart.sh won't replace a running stale daemon (its
            // `[ -S "$SOCK" ]` early-exit). failSoft: a swap failure is
            // non-fatal and only logged, matching the surrounding pattern.
            if (skewDetected) {
              const restart = await runWslBashLc({
                distro,
                cmd: `${LINUX_PATH_PREFIX}${DAEMON_RESTART_CMD}`,
                env,
                exec: o.exec,
                wslPath: o.wslPath,
                timeoutMs: o.daemonRestartTimeoutMs || 45_000,
              });
              if (restart.status === ENSURE_STATUSES.OK) {
                lines.push("terminal-commander: live WSL daemon swapped to the upgraded runtime.");
              } else {
                lines.push(
                  `terminal-commander: live WSL daemon swap after upgrade not completed (${restart.status}); restart WSL or run 'terminal-commander restart'.`,
                );
              }
            }
          }
        } else {
          lines.push("terminal-commander: WSL runtime already present.");
        }
      }

      if (distro && shouldInstallDaemonAutostart(env) && o.skipDaemonAutostart !== true) {
        const daemonEnsure = await (o.ensureDaemonAutostartInWsl || ensureDaemonAutostartInWsl)({
          distro,
          platform,
          env,
          exec: o.exec,
          wslPath: o.wslPath,
          timeoutMs: o.timeoutMs,
        });
        if (daemonEnsure.status === ENSURE_DAEMON_STATUSES.OK) {
          lines.push("terminal-commander: WSL daemon autostart installed (systemd or profile).");
        } else if (daemonEnsure.status === ENSURE_DAEMON_STATUSES.SKIPPED) {
          /* no line */
        } else if (!failSoft) {
          lines.push(
            `terminal-commander: WSL daemon autostart: ${daemonEnsure.status} — ${daemonEnsure.hint}`,
          );
        } else {
          lines.push(
            `terminal-commander: WSL daemon autostart not installed (${daemonEnsure.status}); will retry on MCP connect.`,
          );
        }
      }

      if (
        distro &&
        !noWrite &&
        (mode === "install" || mode === "lazy") &&
        shouldInstallDaemonAutostart(env)
      ) {
        const startCmd = `${LINUX_PATH_PREFIX}. "$HOME/.config/terminal-commander/autostart.sh" 2>/dev/null || true`;
        await runWslBashLc({
          distro,
          cmd: startCmd,
          env,
          exec: o.exec,
          wslPath: o.wslPath,
          timeoutMs: o.startDaemonTimeoutMs || 45_000,
        });
      }
      }
    }

    if (platform === "linux" && shouldInstallDaemonAutostart(env) && o.skipDaemonAutostart !== true) {
      const localDaemon = (o.installDaemonAutostart || installDaemonAutostart)({
        platform,
        env,
        dry_run: noWrite,
      });
      if (localDaemon.status === AUTOSTART_STATUSES.SYSTEMD_ENABLED) {
        lines.push(`terminal-commander: ${localDaemon.hint}`);
      } else if (
        localDaemon.status === AUTOSTART_STATUSES.PROFILE_HOOK ||
        localDaemon.status === AUTOSTART_STATUSES.OK
      ) {
        lines.push(`terminal-commander: ${localDaemon.hint}`);
      } else if (localDaemon.status === AUTOSTART_STATUSES.BINARY_MISSING) {
        lines.push(
          "terminal-commander: terminal-commanderd not on PATH; daemon autostart deferred until binary is installed.",
        );
      }
    }

    const needsHarness =
      autoConfigure || o.force === true || harnessNeedsConfiguration({ platform, env });

    // AV-safe direct-exe path: mirror the resolved native exe(s) into a STABLE
    // per-user dir the package owns and point every harness config at that path
    // (command: <stable>\terminal-commander-mcp.exe, args: []). This removes the
    // npm script-launcher shim -> node -> JS-shim -> spawn chain that heuristic
    // AV reads as a loader.
    //
    // If the stable copy cannot be made (locked file mid-update, EACCES), DO NOT
    // fall through to the bare PATH-dependent command: resolve a real ABSOLUTE
    // path to the currently-running node_modules exe instead (the known-good
    // resolution the live codex entry already uses). The bare name is reached
    // only as an absolute last resort, with a loud warning, when NO absolute
    // path can be resolved (e.g. the only candidate lives in the npx cache).
    // In dry-run / print-config the path is resolved but nothing is copied.
    let stableExePath;
    if (autoConfigure || needsHarness || noWrite) {
      const stable = (o.ensureStableBinaries || ensureStableBinaries)({
        platform,
        env,
        dry_run: noWrite,
      });
      if (stable.exePath) {
        stableExePath = stable.exePath;
        lines.push(
          `terminal-commander: harness configs point at stable exe ${stable.exePath}`,
        );
      } else {
        const direct = (o.resolveDirectExePath || resolveDirectExePath)({
          platform,
        });
        if (direct.exePath) {
          stableExePath = direct.exePath;
          lines.push(
            `terminal-commander: stable exe copy unavailable (${stable.reason}); harness configs point at the absolute node_modules exe ${direct.exePath}`,
          );
        } else {
          stableExePath = undefined;
          lines.push(
            `terminal-commander: WARNING could not resolve an absolute MCP binary path (${stable.reason}/${direct.reason}); harness configs fall back to the PATH-dependent bare command 'terminal-commander-mcp' which only works if its shim is on PATH. Run 'npm install -g terminal-commander' then 'terminal-commander setup harness' to write an absolute path.`,
          );
        }
      }
    }

    harnessResults =
      autoConfigure || needsHarness || noWrite
        ? (o.writeAllHarnesses || writeAllHarnesses)({
      platform,
      env,
      distro,
      exePath: stableExePath,
      knownDistros,
      requireKnownDistro: platform === "win32" && distro != null,
      force: o.force === true || autoConfigure,
      clobber_backup: o.clobber_backup === true,
      dry_run: noWrite,
      cursor_scope: o.cursor_scope || "global",
      projectRoot: o.projectRoot,
      providerFilter: o.providerFilter,
      surface: o.surface,
      cursorOnly: o.cursorOnly === true,
      randomSuffix: o.randomSuffix,
    })
        : [];

    configured = [];
    const failedHarnessResults = [];
    for (const r of harnessResults) {
      if (r.status === HARNESS_WRITE_STATUSES.OK) {
        configured.push(r.id);
        if (r.hint) lines.push(r.hint);
      } else if (r.status === HARNESS_WRITE_STATUSES.STUB_UNVERIFIED) {
        lines.push(r.hint || `${r.id}: stub`);
      } else if (r.status === HARNESS_WRITE_STATUSES.FAILED) {
        failedHarnessResults.push(r);
        lines.push(r.hint || `${r.id}: ${r.harness_status || r.status}`);
      }
    }

    if (printConfig) {
      for (const r of harnessResults) {
        if (r && r.stanza) {
          lines.push(`${r.id}: ${JSON.stringify(r.stanza)}`);
        }
      }
    }

    if (failedHarnessResults.length > 0 && !failSoft) {
      if (emitOutput) logStderr(lines);
      return {
        status: BOOTSTRAP_STATUSES.HARNESS_FAILED,
        exit_code: 64,
        output: lines.join("\n"),
        lines,
        harness_results: harnessResults,
        distro,
      };
    }

    if (!noWrite) {
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

    if (emitOutput) logStderr(lines);
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
  isPackageInstallLifecycle,
  BOOTSTRAP_STATUSES,
};
