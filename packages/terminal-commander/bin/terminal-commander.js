#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for the admin CLI `terminal-commander`, extended at
// WWS02 to recognize the bridge-required branch on Windows hosts
// and at WWS06 to delegate that branch to `lib/cli/run.js` (the
// setup / doctor / pair subcommands).
//
// Linux branch is byte-equivalent to WWS02 / WWS03 / WWS04: spawn
// the resolved Rust admin CLI with shell:false + stdio:inherit;
// mirror exit code / signal.
//
// Windows branch (resolver returns `bridge_required`): delegate to
// `lib/cli/run.js`. The CLI is JS-only; it never invokes wsl.exe
// directly (every wsl.exe call flows through `lib/wsl/spawn.js`
// when needed) and never invokes sudo / passwords / credentials.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const result = resolveBinary({ binary: "terminal-commander" });

if (result.reason === "bridge_required") {
  const { run } = require("../lib/cli/run.js");
  (async () => {
    const r = await run();
    if (r.output) {
      process.stderr.write(r.output);
      if (!r.output.endsWith("\n")) process.stderr.write("\n");
    }
    process.exit(typeof r.exit_code === "number" ? r.exit_code : 64);
  })().catch((err) => {
    process.stderr.write(
      `terminal-commander: CLI internal error: ${err && err.code ? err.code : "unknown"}\n`,
    );
    process.exit(64);
  });
} else if (result.reason !== "ok") {
  process.stderr.write(formatResolveError(result) + "\n");
  process.exit(64);
} else {
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
    process.stderr.write(
      `terminal-commander: failed to spawn ${result.binaryPath}: ${err.code || err.message}\n`,
    );
    process.exit(126);
  });
}
