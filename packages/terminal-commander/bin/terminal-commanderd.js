#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for `terminal-commanderd`. Resolves the matching
// Terminal Commander platform binary and exec's it. Bounded behavior:
//
//   - Reads only `process.platform` and `process.arch`.
//   - Calls `child_process.spawn` with `shell: false` and
//     `stdio: 'inherit'`.
//   - Forwards `process.argv.slice(2)` verbatim. No shell interpolation.
//   - Mirrors the child's exit code on parent exit.
//   - Never reads files. Never opens sockets.
//   - On unsupported platform or missing platform package, exits with
//     code 64 (matches TC40 unsupported-platform exit) and writes one
//     bounded stderr line.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const result = resolveBinary({ binary: "terminal-commanderd" });
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
