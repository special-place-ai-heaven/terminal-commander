#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for `terminal-commanderd`, extended at WWS02 to add a
// bridge-required branch for Windows hosts.
//
// Bounded behavior:
//
//   - Reads only `process.platform` and `process.arch`.
//   - On Linux + supported arch + platform package installed:
//     `child_process.spawn` the Rust binary with `shell: false` and
//     `stdio: 'inherit'`. Forwards `process.argv.slice(2)` verbatim
//     (no shell interpolation). Mirrors the child's exit code on
//     parent exit.
//   - On Windows (any arch): refuses with a single bounded stderr
//     line + exits 64. The daemon does NOT run inside the Windows
//     bridge; the operator must run it from a WSL distro. WWS04
//     wires the MCP shim into the bridge; the daemon shim stays a
//     hard refusal because the Unix-only runtime invariants (UDS,
//     PTY, peer-cred) cannot honor a Windows-native daemon.
//   - On any other unsupported platform / missing platform package:
//     exits 64 with the existing bounded stderr line.
//   - Never reads files. Never opens sockets. Never invokes
//     wsl.exe.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const result = resolveBinary({ binary: "terminal-commanderd" });

if (result.reason === "bridge_required") {
  process.stderr.write(
    "terminal-commander: terminal-commanderd runs only inside Linux / WSL. " +
      "Run it from a WSL distro (e.g. 'wsl -d <distro> -- bash -lc \"terminal-commanderd start --mode ipc-server\"'), " +
      "or use 'terminal-commander setup cursor-wsl' on Windows after WWS06 lands.\n",
  );
  process.exit(64);
}

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
