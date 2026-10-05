// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WSL distro resolution shared by setup, restart and the daemon doctors.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { resolveDistro, SETUP_STATUSES } = require("../lib/cli/setup_cursor_wsl.js");

test("SETUP_STATUSES includes the full closed enum", () => {
  const required = [
    "setup_ready",
    "dry_run",
    "cursor_config_created",
    "cursor_config_updated",
    "cursor_config_already_exists",
    "cursor_config_invalid_json",
    "cursor_config_write_failed",
    "unsupported_host",
    "wsl_not_found",
    "no_distros",
    "no_default_distro_ambiguous",
    "distro_not_found",
    "unsafe_distro_name",
    "runtime_missing",
    "runtime_present",
    "check_timeout",
    "wsl_command_failed",
    "npm_package_unpublished",
    "install_unavailable",
    "install_permission_required",
    "credential_required",
  ];
  for (const s of required) {
    assert.ok(Object.values(SETUP_STATUSES).includes(s), `missing status: ${s}`);
  }
});

test("resolveDistro priority chain matches the locked contract", () => {
  const detect = {
    distros: [{ name: "Ubuntu" }, { name: "Debian" }],
    default_distro: "Debian",
  };
  // P1: --distro
  assert.deepEqual(resolveDistro({ flags: { distro: "Ubuntu" }, env: {}, detectResult: detect }), { status: "ok", distro: "Ubuntu" });
  // P1 unsafe
  assert.equal(resolveDistro({ flags: { distro: "Bad; rm" }, env: {}, detectResult: detect }).status, "unsafe_distro_name");
  // P1 not in whitelist
  assert.equal(resolveDistro({ flags: { distro: "Fedora" }, env: {}, detectResult: detect }).status, "distro_not_found");
  // P2: TC_WSL_DISTRO
  assert.deepEqual(resolveDistro({ flags: {}, env: { TC_WSL_DISTRO: "Ubuntu" }, detectResult: detect }), { status: "ok", distro: "Ubuntu" });
  // P3: detect default
  assert.deepEqual(resolveDistro({ flags: {}, env: {}, detectResult: detect }), { status: "ok", distro: "Debian" });
  // P4: refuse
  assert.equal(resolveDistro({ flags: {}, env: {}, detectResult: { distros: [{ name: "x" }], default_distro: null } }).status, "no_default_distro_ambiguous");
});
