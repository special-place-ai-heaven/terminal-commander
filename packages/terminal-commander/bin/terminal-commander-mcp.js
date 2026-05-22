#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for `terminal-commander-mcp`. See
// `terminal-commanderd.js` for the bounded-behavior contract; this
// shim is the same shape with a different binary name.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const result = resolveBinary({ binary: "terminal-commander-mcp" });
if (result.reason !== "ok") {
  process.stderr.write(formatResolveError(result) + "\n");
  process.exit(64);
}

const child = spawn(result.binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  shell: false,
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  }
  process.exit(code == null ? 1 : code);
});

child.on("error", (err) => {
  process.stderr.write(`terminal-commander: failed to spawn ${result.binaryPath}: ${err.code || err.message}\n`);
  process.exit(126);
});
