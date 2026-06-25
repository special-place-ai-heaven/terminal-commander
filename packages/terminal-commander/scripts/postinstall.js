#!/usr/bin/env node
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// npm `postinstall` trigger (Fix 4): after `npm install`, auto-configure every
// detected coding-agent harness with a WORKING absolute-path MCP entry, so the
// MCP just works with zero user action.
//
// HARD SAFETY CONTRACT (must never break `npm install`):
//   - The whole body is wrapped in try/catch and ALWAYS exits 0. A setup
//     failure prints a one-line hint and is swallowed — `npm install` succeeds.
//   - SAFE no-op in CI / non-interactive (isCiOrNonInteractive) and when
//     TC_NO_AUTO_SETUP=1 / TC_SKIP_BOOTSTRAP=1 (shouldSkipBootstrap).
//   - Routed through runBootstrap({ mode:"install" }), which is fail-soft,
//     autoConfigures (force-refresh, idempotent), and uses the atomic +
//     timestamped-backup writers — so a re-install never corrupts a config.
//
// This script does NO heavy work itself: it delegates to runBootstrap and only
// owns the guard + try/catch envelope.

"use strict";

function main() {
  const env = process.env;

  // Guard 1: explicit opt-out (either env flag).
  let shouldSkipBootstrap;
  let isCiOrNonInteractive;
  let runBootstrap;
  try {
    ({ shouldSkipBootstrap, isCiOrNonInteractive } = require("../lib/bootstrap/skip.js"));
    ({ runBootstrap } = require("../lib/bootstrap/orchestrator.js"));
  } catch (_e) {
    // If the package tree is incomplete mid-install, do nothing (exit 0).
    return;
  }

  if (shouldSkipBootstrap(env)) {
    return;
  }

  // Guard 2: CI / non-interactive installs are a SAFE no-op. The user can run
  // `terminal-commander setup harness` explicitly when they want it.
  if (isCiOrNonInteractive(env)) {
    return;
  }

  runBootstrap({ mode: "install", env, emitOutput: true })
    .then((r) => {
      if (r && r.status === "bootstrap_ready") {
        process.stdout.write(
          "terminal-commander: MCP harnesses configured. Restart your coding agent to pick up the new server.\n",
        );
      }
    })
    .catch((err) => {
      process.stderr.write(
        `terminal-commander: auto-setup skipped (${(err && (err.code || err.message)) || "error"}); ` +
          "run 'terminal-commander setup harness' to configure your coding agent manually.\n",
      );
    });
}

try {
  main();
} catch (err) {
  // A postinstall must NEVER fail `npm install`. Log a hint and exit 0.
  process.stderr.write(
    `terminal-commander: auto-setup skipped (${(err && (err.code || err.message)) || "error"}); ` +
      "run 'terminal-commander setup harness' to configure your coding agent manually.\n",
  );
}

// Always succeed: a setup hiccup must not break the install.
process.exitCode = 0;
