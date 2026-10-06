// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WSL distro resolution shared by setup, restart and the daemon doctors.
// (`setup cursor-wsl` itself routes through setup_harness.js.)

"use strict";

const { isSafeDistroName } = require("../wsl/distro-name.js");

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

module.exports = {
  resolveDistro,
  SETUP_STATUSES,
};
