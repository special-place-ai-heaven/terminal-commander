#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// npm `install` lifecycle — local bootstrap only (INSTALL01).
// Logs to stderr only. Never downloads from GitHub Releases.

"use strict";

const { runBootstrap, shouldSkipBootstrap } = require("../lib/bootstrap/orchestrator.js");

async function main() {
  if (shouldSkipBootstrap(process.env)) {
    process.exit(0);
    return;
  }
  try {
    const result = await runBootstrap({
      mode: "install",
      platform: process.platform,
      env: process.env,
    });
    process.exit(typeof result.exit_code === "number" ? result.exit_code : 0);
  } catch (err) {
    process.stderr.write(
      `terminal-commander: bootstrap install script error: ${err && err.message ? err.message : "unknown"}\n`,
    );
    process.exit(0);
  }
}

main();
