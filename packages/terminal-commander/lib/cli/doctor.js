// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 `terminal-commander doctor` and `terminal-commander doctor
// wsl` implementation. Read-only. Calls WWS03 detectWsl and (when
// --probe-runtime is set) wslDoctor. Never writes files. Never spawns
// anything beyond what lib/wsl/detect.js + lib/wsl/doctor.js already
// do.

"use strict";

const { detectWsl, DETECT_REASONS } = require("../wsl/detect.js");
const { wslDoctor, DOCTOR_STATUSES } = require("../wsl/doctor.js");
const {
  isSafeDistroName,
  assertSafeDistroName,
  UNSAFE_DISTRO_NAME,
} = require("../wsl/distro-name.js");

const DOCTOR_CLI_STATUSES = Object.freeze({
  OK: "ok",
  UNSUPPORTED_HOST: "unsupported_host",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  DISTRO_NOT_FOUND: "distro_not_found",
  UNSAFE_DISTRO_NAME: "unsafe_distro_name",
  RUNTIME_MISSING: "runtime_missing",
  RUNTIME_PRESENT: "runtime_present",
  CHECK_TIMEOUT: "check_timeout",
  WSL_COMMAND_FAILED: "wsl_command_failed",
});

function buildSummary({ status, host_platform, wsl_callable, default_distro, distros, requested_distro, runtime_present, hint }) {
  return {
    status,
    host_platform: host_platform || null,
    wsl_callable: wsl_callable === true,
    default_distro: default_distro || null,
    distros: Array.isArray(distros) ? distros : [],
    requested_distro: requested_distro || null,
    runtime_present: runtime_present === true,
    hint: hint || "",
  };
}

function renderHuman(summary) {
  const out = [];
  out.push(`status: ${summary.status}`);
  out.push(`host_platform: ${summary.host_platform}`);
  out.push(`wsl_callable: ${summary.wsl_callable}`);
  if (summary.default_distro != null) {
    out.push(`default_distro: ${summary.default_distro}`);
  }
  if (summary.distros.length > 0) {
    out.push("distros:");
    for (const d of summary.distros) {
      const marker = d.is_default ? "*" : " ";
      out.push(`  ${marker} ${d.name}\tstate=${d.state}\twsl_version=${d.wsl_version}`);
    }
  } else {
    out.push("distros: (none)");
  }
  if (summary.requested_distro != null) {
    out.push(`requested_distro: ${summary.requested_distro}`);
  }
  if (summary.runtime_present) {
    out.push("runtime_present: true");
  }
  if (summary.hint) {
    out.push(`hint: ${summary.hint}`);
  }
  return out.join("\n");
}

/**
 * Execute `terminal-commander doctor` / `doctor wsl`.
 *
 * @param {Object} opts
 * @param {string|null} [opts.subcommand]  null = Windows host overview;
 *     "wsl" = WSL discovery + optional runtime probe.
 * @param {object} [opts.flags]            { distro?, "probe-runtime"? }
 * @param {string} [opts.platform=process.platform]
 * @param {Function} [opts.detect]         override detectWsl for tests
 * @param {Function} [opts.doctor]         override wslDoctor for tests
 * @returns {Promise<{ status:string, exit_code:number, output:string, summary:object }>}
 */
async function runDoctor(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const detect = o.detect || detectWsl;
  const doctor = o.doctor || wslDoctor;
  const flags = o.flags || {};
  const subcommand = o.subcommand || null;

  // doctor (no subcommand) -> Windows host summary only, no spawn.
  if (subcommand == null) {
    const summary = buildSummary({
      status: platform === "win32" ? DOCTOR_CLI_STATUSES.OK : DOCTOR_CLI_STATUSES.UNSUPPORTED_HOST,
      host_platform: platform,
      wsl_callable: false,
      hint:
        platform === "win32"
          ? "run 'terminal-commander doctor wsl' for WSL discovery."
          : "doctor is most useful on a Windows host with WSL installed; on Linux/WSL the daemon + MCP server run natively.",
    });
    return {
      status: summary.status,
      exit_code: 0,
      output: renderHuman(summary),
      summary,
    };
  }

  // doctor wsl
  if (flags.distro != null) {
    if (!isSafeDistroName(flags.distro)) {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.UNSAFE_DISTRO_NAME,
        host_platform: platform,
        requested_distro: flags.distro,
        hint: "distro name failed safety whitelist; only ASCII letters, digits, '.', '_' and '-' are allowed (length 1..64).",
      });
      return {
        status: summary.status,
        exit_code: 64,
        output: renderHuman(summary),
        summary,
      };
    }
  }

  const detectResult = await detect({ platform });

  // Map detect reason to doctor CLI status when no further probe is needed.
  switch (detectResult.reason) {
    case DETECT_REASONS.UNSUPPORTED_HOST: {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.UNSUPPORTED_HOST,
        host_platform: platform,
        hint: "doctor wsl is Windows-only.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
    case DETECT_REASONS.WSL_NOT_FOUND: {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.WSL_NOT_FOUND,
        host_platform: platform,
        hint: "wsl.exe was not found on PATH; install WSL via 'wsl --install' from an elevated Windows terminal.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
    case DETECT_REASONS.NO_DISTROS: {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.NO_DISTROS,
        host_platform: platform,
        wsl_callable: detectResult.wsl_callable === true,
        hint: "WSL is present but no distro is registered; run 'wsl --install -d Ubuntu-24.04'.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
    case DETECT_REASONS.CHECK_TIMEOUT: {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.CHECK_TIMEOUT,
        host_platform: platform,
        wsl_callable: true,
        hint: "WSL probe exceeded the configured timeout.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
    case DETECT_REASONS.WSL_COMMAND_FAILED: {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.WSL_COMMAND_FAILED,
        host_platform: platform,
        wsl_callable: true,
        hint: "wsl.exe returned a non-zero exit during discovery.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
    case DETECT_REASONS.OK:
      break;
    default: {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.WSL_COMMAND_FAILED,
        host_platform: platform,
        hint: "unexpected detectWsl result.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
  }

  // ok detect result. If --distro was supplied, check membership.
  if (flags.distro != null) {
    const found = detectResult.distros.some((d) => d.name === flags.distro);
    if (!found) {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.DISTRO_NOT_FOUND,
        host_platform: platform,
        wsl_callable: true,
        default_distro: detectResult.default_distro,
        distros: detectResult.distros,
        requested_distro: flags.distro,
        hint: `distro '${flags.distro}' not found in 'wsl -l -v'.`,
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
  }

  // Optional --probe-runtime.
  if (flags["probe-runtime"] === true) {
    const distroToProbe = flags.distro || detectResult.default_distro;
    if (distroToProbe == null) {
      const summary = buildSummary({
        status: DOCTOR_CLI_STATUSES.NO_DISTROS,
        host_platform: platform,
        wsl_callable: true,
        default_distro: null,
        distros: detectResult.distros,
        hint: "no default distro; pass --distro <name> to probe a specific distro.",
      });
      return { status: summary.status, exit_code: 64, output: renderHuman(summary), summary };
    }
    const doc = await doctor({
      distro: distroToProbe,
      probeRuntime: true,
      detectResult,
      platform,
    });
    let status;
    switch (doc.status) {
      case DOCTOR_STATUSES.RUNTIME_PRESENT:
        status = DOCTOR_CLI_STATUSES.RUNTIME_PRESENT;
        break;
      case DOCTOR_STATUSES.RUNTIME_MISSING:
        status = DOCTOR_CLI_STATUSES.RUNTIME_MISSING;
        break;
      case DOCTOR_STATUSES.CHECK_TIMEOUT:
        status = DOCTOR_CLI_STATUSES.CHECK_TIMEOUT;
        break;
      case DOCTOR_STATUSES.WSL_COMMAND_FAILED:
        status = DOCTOR_CLI_STATUSES.WSL_COMMAND_FAILED;
        break;
      default:
        status = DOCTOR_CLI_STATUSES.WSL_COMMAND_FAILED;
        break;
    }
    const summary = buildSummary({
      status,
      host_platform: platform,
      wsl_callable: true,
      default_distro: detectResult.default_distro,
      distros: detectResult.distros,
      requested_distro: distroToProbe,
      runtime_present: status === DOCTOR_CLI_STATUSES.RUNTIME_PRESENT,
      hint: doc.hint || "",
    });
    return {
      status: summary.status,
      exit_code: status === DOCTOR_CLI_STATUSES.RUNTIME_PRESENT ? 0 : 64,
      output: renderHuman(summary),
      summary,
    };
  }

  // No probe -> just print the discovery summary.
  const summary = buildSummary({
    status: DOCTOR_CLI_STATUSES.OK,
    host_platform: platform,
    wsl_callable: true,
    default_distro: detectResult.default_distro,
    distros: detectResult.distros,
    requested_distro: flags.distro || null,
    hint: "wsl discovery ok; pass --probe-runtime to check for terminal-commander-mcp inside a distro.",
  });
  return {
    status: summary.status,
    exit_code: 0,
    output: renderHuman(summary),
    summary,
  };
}

module.exports = {
  runDoctor,
  DOCTOR_CLI_STATUSES,
  renderHuman,
};
