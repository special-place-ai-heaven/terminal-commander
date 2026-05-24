#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");
const { isWindowsMountedShimPath, tryReexecNativeLinuxMcp } = require("../lib/wsl/native-mcp.js");

if (isWindowsMountedShimPath(__filename)) {
  if (tryReexecNativeLinuxMcp(process.argv.slice(2))) {
    /* native Linux MCP owns stdio */
  } else {
    process.exit(64);
  }
} else {
  const result = resolveBinary({ binary: "terminal-commander-mcp" });
  const legacyBridge =
    process.platform === "win32" && process.env.TC_USE_LEGACY_WSL_BRIDGE === "1";

  if (legacyBridge) {
    const { spawnWslBridge, BRIDGE_STATUSES } = require("../lib/wsl/spawn.js");
    (async () => {
      const bridge = await spawnWslBridge();
      if (bridge.status === BRIDGE_STATUSES.OK) {
        if (bridge.signal) {
          process.kill(process.pid, bridge.signal);
          return;
        }
        process.exit(bridge.exit_code == null ? 0 : bridge.exit_code);
        return;
      }
      process.stderr.write(`${bridge.hint}\n`);
      process.exit(64);
    })().catch(() => process.exit(64));
  } else if (result.reason !== "ok") {
    process.stderr.write(`${formatResolveError(result)}\n`);
    process.exit(64);
  } else {
    const daemonResult = resolveBinary({ binary: "terminal-commanderd" });
    if (daemonResult.reason !== "ok") {
      process.stderr.write(`${formatResolveError(daemonResult)}\n`);
      process.exit(64);
    }
    const { runHarnessMcpSession } = require("../lib/daemon/session_supervisor.js");
    (async () => {
      const outcome = await runHarnessMcpSession({
        daemonBinary: daemonResult.binaryPath,
        mcpBinary: result.binaryPath,
        argv: process.argv.slice(2),
        env: process.env,
      });
      if (outcome.signal) {
        process.kill(process.pid, outcome.signal);
        return;
      }
      process.exit(outcome.code);
    })().catch(() => process.exit(64));
  }
}
