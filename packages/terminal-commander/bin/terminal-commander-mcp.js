#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for `terminal-commander-mcp`, extended at WWS04 to
// transparently bridge from Windows hosts into a WSL distro.
//
// Bounded behavior:
//
//   - Linux + supported arch + platform package installed (`reason ===
//     'ok'`): spawn the resolved Rust binary with `shell: false` and
//     `stdio: 'inherit'`. Mirrors child exit code / signal. Linux
//     behaviour is byte-for-byte unchanged from WWS02 / WWS03.
//
//   - Windows host (`reason === 'bridge_required'`): delegate to the
//     WWS04 `spawnWslBridge()` helper. The helper resolves the
//     distro (TC_WSL_DISTRO env -> detectWsl().default_distro),
//     double-validates it (assertSafeDistroName + live whitelist),
//     optionally runs the WWS03 runtime-presence probe (default ON;
//     opt-out via TC_WSL_SKIP_DOCTOR=1), strips token-shaped env
//     vars, and spawns `wsl.exe -d <distro> -- bash -lc 'exec
//     terminal-commander-mcp' [...userArgv]` with `shell: false`,
//     `windowsHide: true`, `stdio: 'inherit'`. The shim writes
//     NOTHING to stdout — all status lines go to stderr — so
//     Cursor's rmcp framing on stdout/stdin passes through the WSL
//     pipe transparently.
//
//   - Any other unsupported platform / missing platform package:
//     exits 64 with the existing bounded stderr line.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");
const { isWindowsMountedShimPath, tryReexecNativeLinuxMcp } = require("../lib/wsl/native-mcp.js");

if (isWindowsMountedShimPath(__filename)) {
  if (tryReexecNativeLinuxMcp(process.argv.slice(2))) {
    // Native Linux MCP owns stdio until exit.
  } else {
    process.exit(64);
  }
} else {
const result = resolveBinary({ binary: "terminal-commander-mcp" });

if (result.reason === "bridge_required") {
  // Lazy-load the bridge helper so non-Windows hosts pay no import
  // cost.
  const { spawnWslBridge, BRIDGE_STATUSES } = require("../lib/wsl/spawn.js");
  (async () => {
    const bridge = await spawnWslBridge();
    if (bridge.status === BRIDGE_STATUSES.OK) {
      // `spawnWslBridge` already wired signal forwarding + exit
      // mirroring. The child's `close` event resolved the promise;
      // mirror the captured exit code / signal into this process.
      if (bridge.signal) {
        process.kill(process.pid, bridge.signal);
        return;
      }
      process.exit(bridge.exit_code == null ? 0 : bridge.exit_code);
      return;
    }
    // Non-OK statuses: bounded stderr line + exit 64.
    process.stderr.write(`${bridge.hint}\n`);
    process.exit(64);
  })().catch((err) => {
    process.stderr.write(
      `terminal-commander: bridge internal error: ${err && err.code ? err.code : "unknown"}\n`,
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
}
