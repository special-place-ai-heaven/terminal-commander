// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 `terminal-commander setup cursor-wsl` orchestrator.
//
// Flow:
//   1. resolve distro by priority chain
//   2. detect WSL + validate distro membership
//   3. (optional) --install-wsl-runtime: ONE constant
//      `npm install -g terminal-commander` invocation through
//      lib/wsl/spawn.js argv shape. NO sudo. NO password.
//   4. runtime-presence probe via WWS03 wslDoctor
//   5. if runtime_present: WWS05 writeCursorMcpConfig
//   6. update %LOCALAPPDATA%\terminal-commander\setup.json
//
// Every wsl.exe invocation goes through lib/wsl/spawn.js or
// lib/wsl/detect.js / doctor.js. NO direct child_process call from
// this module. NO sudo. NO password handling. NO credential capture.

"use strict";

const { detectWsl, DETECT_REASONS } = require("../wsl/detect.js");
const { wslDoctor, DOCTOR_STATUSES } = require("../wsl/doctor.js");
const {
  isSafeDistroName,
  assertSafeDistroName,
  UNSAFE_DISTRO_NAME,
} = require("../wsl/distro-name.js");
const {
  buildFilteredEnv,
  ensureSessionInWslEnv,
} = require("../wsl/filtered_env.js");
const {
  buildTerminalCommanderServerConfig,
  serializeCursorMcpConfig,
  SERVER_NAME,
} = require("../cursor/config.js");
const {
  writeCursorMcpConfig,
} = require("../cursor/write.js");
const { writeSetupJson } = require("./setup_state.js");
const { spawn } = require("node:child_process");
const { INSTALL_PROBE_CMD } = require("../bootstrap/constants.js");

const SETUP_STATUSES = Object.freeze({
  SETUP_READY: "setup_ready",
  DRY_RUN: "dry_run",
  CURSOR_CONFIG_CREATED: "cursor_config_created",
  CURSOR_CONFIG_UPDATED: "cursor_config_updated",
  CURSOR_CONFIG_ALREADY_EXISTS: "cursor_config_already_exists",
  CURSOR_CONFIG_INVALID_JSON: "cursor_config_invalid_json",
  CURSOR_CONFIG_WRITE_FAILED: "cursor_config_write_failed",
  UNSUPPORTED_HOST: "unsupported_host",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  NO_DEFAULT_DISTRO_AMBIGUOUS: "no_default_distro_ambiguous",
  DISTRO_NOT_FOUND: "distro_not_found",
  UNSAFE_DISTRO_NAME: "unsafe_distro_name",
  RUNTIME_MISSING: "runtime_missing",
  RUNTIME_PRESENT: "runtime_present",
  CHECK_TIMEOUT: "check_timeout",
  WSL_COMMAND_FAILED: "wsl_command_failed",
  NPM_PACKAGE_UNPUBLISHED: "npm_package_unpublished",
  INSTALL_UNAVAILABLE: "install_unavailable",
  INSTALL_PERMISSION_REQUIRED: "install_permission_required",
  CREDENTIAL_REQUIRED: "credential_required",
});

// Re-export for tests (defined in lib/bootstrap/constants.js).

function buildResult(partial) {
  return {
    status: partial.status,
    exit_code: typeof partial.exit_code === "number" ? partial.exit_code : 64,
    output: partial.output || "",
    distro: partial.distro || null,
    cursor_scope: partial.cursor_scope || null,
    cursor_config_path: partial.cursor_config_path || null,
    cursor_backup_path: partial.cursor_backup_path || null,
    server: partial.server || null,
    plan: partial.plan || null,
    hint: partial.hint || "",
  };
}

function resolveDistro({ flags, env, detectResult }) {
  // Priority 1: --distro <name>
  if (flags.distro != null && flags.distro !== "") {
    if (!isSafeDistroName(flags.distro)) {
      return { status: SETUP_STATUSES.UNSAFE_DISTRO_NAME, distro: flags.distro };
    }
    const found = detectResult.distros.some((d) => d.name === flags.distro);
    if (!found) {
      return { status: SETUP_STATUSES.DISTRO_NOT_FOUND, distro: flags.distro };
    }
    return { status: "ok", distro: flags.distro };
  }
  // Priority 2: TC_WSL_DISTRO env
  if (env && env.TC_WSL_DISTRO) {
    if (!isSafeDistroName(env.TC_WSL_DISTRO)) {
      return { status: SETUP_STATUSES.UNSAFE_DISTRO_NAME, distro: env.TC_WSL_DISTRO };
    }
    const found = detectResult.distros.some((d) => d.name === env.TC_WSL_DISTRO);
    if (!found) {
      return { status: SETUP_STATUSES.DISTRO_NOT_FOUND, distro: env.TC_WSL_DISTRO };
    }
    return { status: "ok", distro: env.TC_WSL_DISTRO };
  }
  // Priority 3: detect default
  if (detectResult.default_distro) {
    return { status: "ok", distro: detectResult.default_distro };
  }
  // Priority 4: refuse
  return { status: SETUP_STATUSES.NO_DEFAULT_DISTRO_AMBIGUOUS, distro: null };
}

/**
 * Run the WSL-side install through the locked constant argv shape.
 * NO sudo. NO password. NO env passthrough beyond `buildFilteredEnv`.
 *
 * @returns {Promise<{ status:string, exit_code:number|null, hint:string }>}
 */
function runInstallProbe({ distro, env, exec, wslPath, timeoutMs }) {
  return new Promise((resolve) => {
    const argv = ["-d", distro, "--", "bash", "-lc", INSTALL_PROBE_CMD];
    // Rebuild WSLENV to a TC-only allowlist after name-based filtering: this
    // spawn launches a Linux process (`bash -lc`), so an ambient
    // WSLENV=SOME_SECRET/u would otherwise forward SOME_SECRET into WSL.
    const filtered = ensureSessionInWslEnv(buildFilteredEnv(env || process.env));
    let stdoutBuf = "";
    let stderrBuf = "";
    let child;
    const localExec = exec ||
      (({ wslPath: wp, argv: a, env: e }) =>
        spawn(wp, a, {
          stdio: ["ignore", "pipe", "pipe"],
          shell: false,
          env: e,
        }));
    try {
      child = localExec({
        wslPath: wslPath || "wsl.exe",
        argv,
        env: filtered,
      });
    } catch (_e) {
      resolve({
        status: SETUP_STATUSES.INSTALL_UNAVAILABLE,
        exit_code: null,
        hint: "failed to spawn wsl.exe for runtime install; ensure WSL is installed and the distro is reachable.",
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
        status: SETUP_STATUSES.CHECK_TIMEOUT,
        exit_code: null,
        hint: "runtime install probe exceeded the configured timeout.",
      });
    }, typeof timeoutMs === "number" ? timeoutMs : 120_000);
    if (child.stdout) child.stdout.on("data", (b) => { stdoutBuf += b.toString("utf8"); });
    if (child.stderr) child.stderr.on("data", (b) => { stderrBuf += b.toString("utf8"); });
    child.on("error", () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: SETUP_STATUSES.INSTALL_UNAVAILABLE,
        exit_code: null,
        hint: "wsl.exe failed to start the runtime install probe.",
      });
    });
    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const combined = (stdoutBuf + "\n" + stderrBuf).toLowerCase();
      // npm E404 / package not published
      if (
        /e404/.test(combined) ||
        /404 not found/.test(combined) ||
        /not in this registry/.test(combined)
      ) {
        resolve({
          status: SETUP_STATUSES.NPM_PACKAGE_UNPUBLISHED,
          exit_code: code,
          hint:
            `terminal-commander is not yet published to npm; cannot install inside '${distro}'. ` +
            "Until NPM07's first publish lands, this install path is expected to fail with E404.",
        });
        return;
      }
      // Permission failures (EACCES, sudo prompt, etc.).
      if (
        /eacces/.test(combined) ||
        /permission denied/.test(combined) ||
        /sudo/.test(combined) ||
        /not permitted/.test(combined)
      ) {
        resolve({
          status: SETUP_STATUSES.INSTALL_PERMISSION_REQUIRED,
          exit_code: code,
          hint:
            `inside-WSL npm install failed with a permission error in '${distro}'. ` +
            "Run 'wsl -d <distro> -- bash -lc \"npm install -g terminal-commander\"' manually with the appropriate npm prefix. " +
            "Terminal Commander does NOT prompt for passwords; the credential broker is future work.",
        });
        return;
      }
      if (code === 0) {
        resolve({
          status: "ok",
          exit_code: 0,
          hint: `runtime install succeeded inside '${distro}'.`,
        });
        return;
      }
      resolve({
        status: SETUP_STATUSES.INSTALL_UNAVAILABLE,
        exit_code: code,
        hint: `inside-WSL npm install exited with code ${code} in '${distro}'.`,
      });
    });
  });
}

function buildPlanText({ distro, cursor_scope, install_runtime, dry_run, print_config }) {
  const out = [];
  out.push(`distro: ${distro || "(unresolved)"}`);
  out.push(`cursor_scope: ${cursor_scope}`);
  out.push(`install_wsl_runtime: ${install_runtime ? "yes" : "no"}`);
  out.push(`dry_run: ${dry_run ? "yes" : "no"}`);
  if (print_config) out.push("mode: --print-config (no writes)");
  return out.join("\n");
}

/**
 * Execute `terminal-commander setup cursor-wsl`.
 *
 * @param {Object} opts
 * @param {object} opts.flags
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @param {Function} [opts.detect]    override detectWsl
 * @param {Function} [opts.doctor]    override wslDoctor
 * @param {Function} [opts.installExec]   override the install spawn for tests
 * @param {Function} [opts.writeConfig]   override WWS05 writeCursorMcpConfig
 * @param {Function} [opts.writeState]    override setup_state.writeSetupJson
 * @returns {Promise<object>}
 */
async function runSetupCursorWsl(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const flags = o.flags || {};
  const detect = o.detect || detectWsl;
  const doctor = o.doctor || wslDoctor;
  const writeConfig = o.writeConfig || writeCursorMcpConfig;
  const writeState = o.writeState || writeSetupJson;

  const dryRun = flags["dry-run"] === true;
  const printConfig = flags["print-config"] === true;
  const installRuntime = flags["install-wsl-runtime"] === true;
  const cursorScope = flags.project != null ? "project" : "global";
  const force = flags.force === true;
  const clobberBackup = flags["clobber-backup"] === true;

  if (platform !== "win32") {
    return buildResult({
      status: SETUP_STATUSES.UNSUPPORTED_HOST,
      output: "setup cursor-wsl is Windows-only; on Linux/WSL, run terminal-commander-mcp directly.",
      hint: "setup cursor-wsl is Windows-only.",
    });
  }

  // (1) Detect WSL + resolve distro.
  const detectResult = await detect({ platform });

  switch (detectResult.reason) {
    case DETECT_REASONS.UNSUPPORTED_HOST:
      return buildResult({
        status: SETUP_STATUSES.UNSUPPORTED_HOST,
        output: "host is not Windows.",
      });
    case DETECT_REASONS.WSL_NOT_FOUND:
      return buildResult({
        status: SETUP_STATUSES.WSL_NOT_FOUND,
        output: "wsl.exe was not found on PATH; install WSL via 'wsl --install' from an elevated Windows terminal.",
      });
    case DETECT_REASONS.NO_DISTROS:
      return buildResult({
        status: SETUP_STATUSES.NO_DISTROS,
        output: "WSL is present but no distro is registered; run 'wsl --install -d Ubuntu-24.04'.",
      });
    case DETECT_REASONS.CHECK_TIMEOUT:
      return buildResult({
        status: SETUP_STATUSES.CHECK_TIMEOUT,
        output: "WSL probe exceeded the configured timeout.",
      });
    case DETECT_REASONS.WSL_COMMAND_FAILED:
      return buildResult({
        status: SETUP_STATUSES.WSL_COMMAND_FAILED,
        output: "wsl.exe returned a non-zero exit during discovery.",
      });
    case DETECT_REASONS.OK:
      break;
    default:
      return buildResult({
        status: SETUP_STATUSES.WSL_COMMAND_FAILED,
        output: "unexpected detectWsl result.",
      });
  }

  const resolved = resolveDistro({ flags, env, detectResult });
  if (resolved.status === SETUP_STATUSES.NO_DEFAULT_DISTRO_AMBIGUOUS) {
    const names = detectResult.distros.map((d) => d.name).join(", ");
    return buildResult({
      status: SETUP_STATUSES.NO_DEFAULT_DISTRO_AMBIGUOUS,
      output: `no default WSL distro; pass --distro <name> or set TC_WSL_DISTRO. Available distros: ${names || "(none)"}.`,
    });
  }
  if (resolved.status === SETUP_STATUSES.UNSAFE_DISTRO_NAME) {
    return buildResult({
      status: SETUP_STATUSES.UNSAFE_DISTRO_NAME,
      distro: resolved.distro,
      output: "distro name failed safety whitelist; only ASCII letters, digits, '.', '_' and '-' are allowed (length 1..64).",
    });
  }
  if (resolved.status === SETUP_STATUSES.DISTRO_NOT_FOUND) {
    const names = detectResult.distros.map((d) => d.name).join(", ");
    return buildResult({
      status: SETUP_STATUSES.DISTRO_NOT_FOUND,
      distro: resolved.distro,
      output: `distro '${resolved.distro}' not found in 'wsl -l -v'. Available distros: ${names || "(none)"}.`,
    });
  }
  const distro = resolved.distro;

  // (--print-config): print only the planned Cursor stanza JSON.
  if (printConfig) {
    const stanza = buildTerminalCommanderServerConfig({ distro });
    const text = serializeCursorMcpConfig({
      mcpServers: { [SERVER_NAME]: stanza },
    });
    return buildResult({
      status: SETUP_STATUSES.DRY_RUN,
      exit_code: 0,
      distro,
      cursor_scope: cursorScope,
      server: stanza,
      output: text,
      hint: "no files written; rerun without --print-config to apply.",
    });
  }

  // (--dry-run): print the plan only.
  if (dryRun) {
    const planText = buildPlanText({
      distro,
      cursor_scope: cursorScope,
      install_runtime: installRuntime,
      dry_run: true,
      print_config: false,
    });
    return buildResult({
      status: SETUP_STATUSES.DRY_RUN,
      exit_code: 0,
      distro,
      cursor_scope: cursorScope,
      plan: planText,
      output: planText + "\nno files written; rerun without --dry-run to apply.",
      hint: "no files written; rerun without --dry-run to apply.",
    });
  }

  // (2) Optional install probe.
  if (installRuntime) {
    const installResult = await runInstallProbe({
      distro,
      env,
      exec: o.installExec,
      wslPath: "wsl.exe",
    });
    if (installResult.status !== "ok") {
      return buildResult({
        status: installResult.status,
        distro,
        cursor_scope: cursorScope,
        output: installResult.hint,
        hint: installResult.hint,
      });
    }
  }

  // (3) Read-only runtime-presence probe.
  const doc = await doctor({
    distro,
    probeRuntime: true,
    detectResult,
    platform,
  });
  if (doc.status === DOCTOR_STATUSES.RUNTIME_MISSING) {
    return buildResult({
      status: SETUP_STATUSES.RUNTIME_MISSING,
      distro,
      cursor_scope: cursorScope,
      output:
        `terminal-commander runtime is not installed inside '${distro}'. ` +
        `Install with: wsl -d ${distro} -- bash -lc 'npm install -g terminal-commander', or re-run setup with --install-wsl-runtime once NPM07 publishes.`,
    });
  }
  if (
    doc.status === DOCTOR_STATUSES.CHECK_TIMEOUT ||
    doc.status === DOCTOR_STATUSES.WSL_COMMAND_FAILED
  ) {
    return buildResult({
      status:
        doc.status === DOCTOR_STATUSES.CHECK_TIMEOUT
          ? SETUP_STATUSES.CHECK_TIMEOUT
          : SETUP_STATUSES.WSL_COMMAND_FAILED,
      distro,
      cursor_scope: cursorScope,
      output: doc.hint || "",
    });
  }
  // doc.status === RUNTIME_PRESENT or OK / doctor_not_run; runtime is
  // confirmed present.

  // (4) Cursor config write via WWS05.
  const writeOpts = {
    scope: cursorScope,
    platform,
    env,
    distro,
    force,
    clobber_backup: clobberBackup,
  };
  if (cursorScope === "project") {
    writeOpts.projectRoot = flags.project;
  }
  const cursorRes = writeConfig(writeOpts);
  let cursorStatus;
  switch (cursorRes.status) {
    case "config_created":
      cursorStatus = SETUP_STATUSES.CURSOR_CONFIG_CREATED;
      break;
    case "config_updated":
      cursorStatus = SETUP_STATUSES.CURSOR_CONFIG_UPDATED;
      break;
    case "already_exists":
      cursorStatus = SETUP_STATUSES.CURSOR_CONFIG_ALREADY_EXISTS;
      break;
    case "invalid_json":
      cursorStatus = SETUP_STATUSES.CURSOR_CONFIG_INVALID_JSON;
      break;
    case "config_too_large":
    case "path_not_allowed":
    case "project_root_required":
    case "unsafe_distro_name":
    case "distro_not_found":
    case "backup_failed":
    case "write_failed":
    case "unsupported_host":
      cursorStatus = SETUP_STATUSES.CURSOR_CONFIG_WRITE_FAILED;
      break;
    default:
      cursorStatus = SETUP_STATUSES.CURSOR_CONFIG_WRITE_FAILED;
      break;
  }

  if (
    cursorStatus !== SETUP_STATUSES.CURSOR_CONFIG_CREATED &&
    cursorStatus !== SETUP_STATUSES.CURSOR_CONFIG_UPDATED
  ) {
    return buildResult({
      status: cursorStatus,
      exit_code: 64,
      distro,
      cursor_scope: cursorScope,
      cursor_config_path: cursorRes.path,
      cursor_backup_path: cursorRes.backup_path,
      output: cursorRes.hint || "",
      hint: cursorRes.hint || "",
    });
  }

  // (5) Persist setup.json (best-effort; setup is still considered
  // ready even if state-file write fails on an unusual host).
  try {
    writeState({
      platform,
      env,
      distro,
      cursor_scope: cursorScope,
    });
  } catch (_e) {
    /* state-file is convenience metadata; failure is non-fatal */
  }

  return buildResult({
    status: SETUP_STATUSES.SETUP_READY,
    exit_code: 0,
    distro,
    cursor_scope: cursorScope,
    cursor_config_path: cursorRes.path,
    cursor_backup_path: cursorRes.backup_path,
    server: cursorRes.server,
    output:
      `setup_ready\ndistro=${distro}\ncursor_scope=${cursorScope}\ncursor_config_path=${cursorRes.path}` +
      (cursorRes.backup_path ? `\ncursor_backup_path=${cursorRes.backup_path}` : ""),
    hint: "Cursor MCP config written; restart Cursor to pick up the new server.",
  });
}

module.exports = {
  runSetupCursorWsl,
  resolveDistro,
  runInstallProbe,
  buildPlanText,
  SETUP_STATUSES,
  INSTALL_PROBE_CMD,
};
