// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { runBootstrap, BOOTSTRAP_STATUSES } = require("../bootstrap/orchestrator.js");

const SETUP_HARNESS_STATUSES = Object.freeze({
  ...BOOTSTRAP_STATUSES,
  DEPRECATED_CURSOR_WSL: "deprecated_cursor_wsl",
});

async function runSetupHarness(opts) {
  const o = opts || {};
  const flags = o.flags || {};
  return runBootstrap({
    mode: "cli",
    platform: o.platform || process.platform,
    env: o.env || process.env,
    distro: flags.distro,
    // The manual `setup harness` command always REFRESHES the terminal_commander
    // entry: a stale or hand-written stanza must be rewritten with a fresh,
    // correct one, not skipped with ALREADY_EXISTS. auto_configure flows to the
    // writers' `force` (orchestrator.js), which overwrites the single named key
    // and preserves every other server/key (idempotent by construction). The
    // explicit `--force` flag remains supported for parity.
    auto_configure: true,
    force: flags.force === true,
    clobber_backup: flags["clobber-backup"] === true,
    dry_run: flags["dry-run"] === true,
    print_config: flags["print-config"] === true,
    providerFilter: flags.provider,
    surface: flags.surface,
    cursor_scope: flags.project != null ? "project" : "global",
    projectRoot: flags.project,
    detect: o.detect,
    doctor: o.doctor,
    ensureWslRuntime: o.ensureWslRuntime,
    exec: o.exec,
    writeState: o.writeState,
    writeAllHarnesses: o.writeAllHarnesses,
    failSoft: false,
    emitOutput: false,
  }).then((r) => ({
    status: r.status,
    exit_code: typeof r.exit_code === "number" ? r.exit_code : 0,
    output: r.output || r.lines?.join("\n") || "",
    harness_results: r.harness_results,
  }));
}

async function runSetupDefault(opts) {
  return runSetupHarness(opts);
}

async function runSetupCursorWslDeprecated(opts) {
  process.stderr.write(
    "terminal-commander: setup cursor-wsl is deprecated; use 'terminal-commander setup harness'.\n",
  );
  const o = opts || {};
  const flags = { ...(o.flags || {}), "install-wsl-runtime": true };
  const boot = await runBootstrap({
    mode: "cli",
    platform: o.platform || process.platform,
    env: o.env || process.env,
    distro: flags.distro,
    force: flags.force === true,
    clobber_backup: flags["clobber-backup"] === true,
    dry_run: flags["dry-run"] === true,
    print_config: flags["print-config"] === true,
    surface: flags.surface,
    cursorOnly: true,
    cursor_scope: flags.project != null ? "project" : "global",
    projectRoot: flags.project,
    detect: o.detect,
    doctor: o.doctor,
    ensureWslRuntime: o.ensureWslRuntime,
    exec: o.installExec || o.exec,
    writeState: o.writeState,
    writeConfig: o.writeConfig,
    skipWslInstall: flags["dry-run"] === true,
    emitOutput: false,
  });
  if (boot.exit_code !== 0) {
    return {
      status: boot.status,
      exit_code: boot.exit_code,
      output: boot.output,
    };
  }
  return {
    status: SETUP_HARNESS_STATUSES.DEPRECATED_CURSOR_WSL,
    exit_code: 0,
    output: boot.output,
  };
}

module.exports = {
  runSetupHarness,
  runSetupDefault,
  runSetupCursorWslDeprecated,
  SETUP_HARNESS_STATUSES,
};
