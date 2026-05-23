// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Locked bootstrap command strings. NO operator interpolation.

"use strict";

// Linux-first PATH avoids Windows node/npm shims visible inside WSL.
const LINUX_PATH_PREFIX =
  'export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$HOME/.local/bin:$HOME/.npm-global/bin"; ';

const INSTALL_PROBE_CMD = `${LINUX_PATH_PREFIX}npm install -g terminal-commander`;

const RUNTIME_VERIFY_CMD = `${LINUX_PATH_PREFIX}command -v terminal-commander-mcp && node -e "const a=process.arch==='arm64'?'arm64':'x64';require.resolve('@terminal-commander/linux-'+a)"`;

module.exports = {
  LINUX_PATH_PREFIX,
  INSTALL_PROBE_CMD,
  RUNTIME_VERIFY_CMD,
};
