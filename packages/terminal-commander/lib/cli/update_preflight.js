// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { resolveBinary, formatResolveError } = require("../resolve-binary.js");
const { stableBinPath } = require("../harness/stable_bin.js");
const {
  detectRuntimeEnvironment,
  windowsUpdateScopes,
  describeError,
} = require("./runtime_environment.js");

// Explicit facts -> deterministic plan -> sequential side effects. Keeping
// discovery out of execution makes every Windows branch testable on any host.

function diagnostic(message) {
  return message.endsWith("\n") ? message : `${message}\n`;
}

function resolutionDiagnostic(formatResolutionError, resolution, platform, arch) {
  try {
    const message = formatResolutionError(resolution, { platform, arch });
    if (typeof message === "string" && message.length > 0) return message;
  } catch (_err) {
    // The resolver's formatter is diagnostic-only. A malformed resolver result
    // must not prevent the stable-helper or npm-repair recovery paths.
  }
  if (resolution && resolution.error) {
    return `terminal-commander: binary resolver failed: ${describeError(resolution.error)}`;
  }
  const reason = resolution && resolution.reason ? resolution.reason : "invalid_result";
  return `terminal-commander: binary resolver failed (${reason})`;
}

/**
 * Resolve every decision needed by the update preflight without spawning.
 * All host and I/O boundaries are injectable so tests never impersonate an OS
 * or depend on ignored build artifacts in the working tree.
 */
function planUpdatePreflight(opts) {
  const o = opts || {};
  const platform = o.platform ?? process.platform;
  const arch = o.arch ?? process.arch;
  const env = o.env ?? process.env;
  const packageRoot = o.packageRoot ?? path.resolve(__dirname, "../..");
  const detect = o.detectRuntimeEnvironment || detectRuntimeEnvironment;
  const resolve = o.resolveBinary || resolveBinary;
  const resolveStableBinPath = o.stableBinPath || stableBinPath;
  const exists = o.existsSync || fs.existsSync;
  const resolveScopes = o.windowsUpdateScopes || windowsUpdateScopes;
  const formatResolutionError = o.formatResolveError || formatResolveError;
  const diagnostics = [];

  let environment;
  try {
    environment = detect({ platform, env, flags: {} });
  } catch (err) {
    return {
      status: "invalid_environment",
      exitCode: 64,
      diagnostics: [
        diagnostic(
          `terminal-commander: update preflight environment error: ${describeError(err)}`,
        ),
      ],
      commands: [],
    };
  }

  if (!environment || environment.status !== "ok") {
    const evidence = environment && environment.evidence
      ? environment.evidence
      : "detector_returned_no_evidence";
    return {
      status: "unsupported_environment",
      exitCode: 64,
      diagnostics: [
        diagnostic(`terminal-commander: update preflight unsupported environment (${evidence}).`),
      ],
      commands: [],
    };
  }

  if (platform !== "win32") {
    return { status: "not_required", exitCode: 0, diagnostics, commands: [] };
  }

  let resolution;
  try {
    resolution = resolve({ binary: "terminal-commander", platform, arch });
  } catch (err) {
    resolution = { reason: "resolver_failed", error: err };
  }
  if (resolution && resolution.reason === "ok" && !resolution.binaryPath) {
    resolution = { ...resolution, reason: "invalid_ok_result" };
  }

  let helperPath = resolution && resolution.reason === "ok" ? resolution.binaryPath : null;
  if (!helperPath) {
    const resolutionMessage = resolutionDiagnostic(
      formatResolutionError,
      resolution,
      platform,
      arch,
    );
    let stableHelper = null;
    try {
      stableHelper = resolveStableBinPath("terminal-commander", { platform, env });
      if (!exists(stableHelper)) stableHelper = null;
    } catch (err) {
      diagnostics.push(
        diagnostic(
          `terminal-commander: stable update helper lookup failed: ${describeError(err)}.`,
        ),
      );
    }

    if (stableHelper) {
      helperPath = stableHelper;
      diagnostics.push(
        diagnostic(
          `${resolutionMessage}; using stable update helper ${stableHelper}.`,
        ),
      );
    } else {
      diagnostics.push(
        diagnostic(
          `${resolutionMessage}; no update helper is available, continuing with npm repair.`,
        ),
      );
      return { status: "degraded_repair", exitCode: 0, diagnostics, commands: [] };
    }
  }

  let scopes;
  try {
    scopes = resolveScopes({ platform, env, packageRoot });
  } catch (err) {
    return {
      status: "invalid_environment",
      exitCode: 64,
      diagnostics: diagnostics.concat(
        diagnostic(
          `terminal-commander: update preflight environment error: ${describeError(err)}`,
        ),
      ),
      commands: [],
    };
  }
  if (
    !Array.isArray(scopes) ||
    scopes.length === 0 ||
    scopes.some((scopeDir) => typeof scopeDir !== "string" || scopeDir.length === 0)
  ) {
    return {
      status: "invalid_environment",
      exitCode: 64,
      diagnostics: diagnostics.concat(
        diagnostic("terminal-commander: update preflight scope resolver returned invalid data"),
      ),
      commands: [],
    };
  }

  return {
    status: "ready",
    exitCode: 0,
    diagnostics,
    commands: scopes.map((scopeDir) => ({
      file: helperPath,
      args: ["update-locks", "--scope-dir", scopeDir],
      scopeDir,
    })),
  };
}

function runCommand(command, opts) {
  const spawnChild = opts.spawn;
  const env = opts.env;
  const writeStderr = opts.writeStderr;

  return new Promise((resolve) => {
    let settled = false;
    const finish = (code) => {
      if (settled) return;
      settled = true;
      resolve(code);
    };

    let child;
    try {
      child = spawnChild(command.file, command.args, {
        stdio: "inherit",
        shell: false,
        env,
      });
    } catch (err) {
      writeStderr(
        diagnostic(
          `terminal-commander: failed to start update preflight for ${command.scopeDir}: ${describeError(err)}`,
        ),
      );
      finish(126);
      return;
    }
    if (!child || typeof child.once !== "function") {
      writeStderr(
        diagnostic(
          `terminal-commander: failed to start update preflight for ${command.scopeDir}: spawn returned no child process`,
        ),
      );
      finish(126);
      return;
    }

    child.once("error", (err) => {
      writeStderr(
        diagnostic(
          `terminal-commander: failed to start update preflight for ${command.scopeDir}: ${describeError(err)}`,
        ),
      );
      finish(126);
    });
    child.once("exit", (code, signal) => {
      if (signal) {
        finish(1);
        return;
      }
      finish(code == null ? 1 : code);
    });
  });
}

async function executeUpdatePreflight(plan, opts) {
  const o = opts || {};
  const env = o.env ?? process.env;
  const spawnChild = o.spawn || spawn;
  const writeStderr = o.writeStderr || ((message) => process.stderr.write(message));

  for (const message of plan.diagnostics || []) writeStderr(message);
  if (plan.exitCode !== 0) return plan.exitCode;

  for (const command of plan.commands || []) {
    const code = await runCommand(command, { spawn: spawnChild, env, writeStderr });
    if (code !== 0) return code;
  }
  return 0;
}

function runUpdatePreflight(opts) {
  const o = opts || {};
  const plan = planUpdatePreflight(o);
  return executeUpdatePreflight(plan, o);
}

module.exports = {
  planUpdatePreflight,
  executeUpdatePreflight,
  runUpdatePreflight,
};
