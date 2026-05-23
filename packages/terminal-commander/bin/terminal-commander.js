#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for the admin CLI `terminal-commander`, extended at
// WWS02 to recognize the bridge-required branch on Windows hosts.
// The real setup / doctor / pair subcommands belong to WWS06
// (`lib/cli/**`); WWS02 only emits a bounded "pending WWS06" stderr
// line and exits 64. NO wsl.exe call in WWS02.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const result = resolveBinary({ binary: "terminal-commander" });

if (result.reason === "bridge_required") {
  process.stderr.write(
    "terminal-commander: Windows host detected; setup / doctor / pair subcommands are pending WWS06. " +
      "To use Terminal Commander today, run 'npm install -g terminal-commander' from inside a WSL distro, " +
      "or wait for the WWS06 release that adds 'terminal-commander setup cursor-wsl' and 'terminal-commander doctor'.\n",
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
