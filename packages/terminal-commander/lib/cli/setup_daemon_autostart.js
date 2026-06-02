// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { resolveDistro } = require("./setup_cursor_wsl.js");
const { detectWsl, DETECT_REASONS } = require("../wsl/detect.js");
const {
  installDaemonAutostart,
  AUTOSTART_STATUSES,
} = require("../daemon/autostart.js");
const {
  ensureDaemonAutostartInWsl,
  ENSURE_DAEMON_STATUSES,
} = require("../bootstrap/ensure_daemon_autostart.js");

async function runSetupDaemonAutostart(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const flags = o.flags || {};

  if (platform === "win32") {
    const detectResult = await (o.detect || detectWsl)({
      platform,
      wslPath: o.wslPath,
      timeoutMs: o.timeoutMs,
    });
    if (detectResult.reason === DETECT_REASONS.WSL_NOT_FOUND) {
      return {
        status: "wsl_not_found",
        exit_code: 64,
        output: "terminal-commander: WSL not found.\n",
      };
    }
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
    if (flags["dry-run"] === true) {
      return {
        status: "dry_run",
        exit_code: 0,
        output: `terminal-commander: would install daemon autostart in WSL distro ${resolved.distro}.\n`,
      };
    }
    const r = await (o.ensureDaemonAutostartInWsl || ensureDaemonAutostartInWsl)({
      distro: resolved.distro,
      platform,
      env,
      exec: o.exec,
      wslPath: o.wslPath,
      timeoutMs: o.timeoutMs,
    });
    if (r.status === ENSURE_DAEMON_STATUSES.OK) {
      return {
        status: "ok",
        exit_code: 0,
        output: `terminal-commander: ${r.hint}\n`,
      };
    }
    return {
      status: r.status,
      exit_code: 64,
      output: `terminal-commander: daemon autostart install failed: ${r.hint}\n`,
    };
  }

  if (platform === "linux") {
    if (flags["dry-run"] === true) {
      const r = installDaemonAutostart({ platform, env, dry_run: true });
      return {
        status: "dry_run",
        exit_code: 0,
        output: `terminal-commander: ${r.hint}\n`,
      };
    }
    const r = installDaemonAutostart({ platform, env });
    if (
      r.status === AUTOSTART_STATUSES.SYSTEMD_ENABLED ||
      r.status === AUTOSTART_STATUSES.PROFILE_HOOK ||
      r.status === AUTOSTART_STATUSES.OK
    ) {
      return { status: "ok", exit_code: 0, output: `terminal-commander: ${r.hint}\n` };
    }
    return {
      status: r.status,
      exit_code: 64,
      output: `terminal-commander: ${r.hint}\n`,
    };
  }

  return {
    status: "unsupported_host",
    exit_code: 64,
    output: "terminal-commander: daemon autostart is only supported on Linux and Windows (WSL).\n",
  };
}

module.exports = { runSetupDaemonAutostart };
