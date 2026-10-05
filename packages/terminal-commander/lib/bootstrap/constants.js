// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Locked bootstrap command strings. NO operator interpolation.

"use strict";

// Linux-first PATH avoids Windows node/npm shims visible inside WSL.
const LINUX_PATH_PREFIX =
  'export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$HOME/.local/bin:$HOME/.npm-global/bin"; ';

const INSTALL_PROBE_CMD = `${LINUX_PATH_PREFIX}npm install -g terminal-commander`;

const RUNTIME_VERIFY_CMD = `${LINUX_PATH_PREFIX}command -v terminal-commander-mcp && node -e "const a=process.arch==='arm64'?'arm64':'x64';require.resolve('@terminal-commander/linux-'+a)"`;

// Version probe for skew detection. clap prints `terminal-commander-mcp <ver>`
// to stdout; the caller splits the last whitespace token and compares it to the
// host package version. (Documented fallback if the flag proves unreliable:
// read `"$(npm root -g)/terminal-commander/package.json"`.)
const RUNTIME_VERSION_CMD = `${LINUX_PATH_PREFIX}terminal-commander-mcp --version`;

// Run autostart.sh as a process, never source it: a sourced `exit` ends the
// calling shell, so the command chained after it never runs. Silent, because
// BRIDGE_PROBE_CMD's stdout is the MCP stdio channel.
const AUTOSTART_RUN = 'bash "$HOME/.config/terminal-commander/autostart.sh" >/dev/null 2>&1 || true';

// Bootstrap's explicit daemon start: same script, but its exit status counts.
const DAEMON_START_CMD = `${LINUX_PATH_PREFIX}bash "$HOME/.config/terminal-commander/autostart.sh"`;

// WSL must not resolve terminal-commander-mcp from /mnt/c (Windows npm shim);
// that runs Node as linux and fails optionalDependency resolve.
const BRIDGE_DAEMON_ENSURE = `${AUTOSTART_RUN}; `;

const BRIDGE_PROBE_CMD = `${LINUX_PATH_PREFIX}${BRIDGE_DAEMON_ENSURE}exec terminal-commander-mcp`;

// Proof that a WSL shell got past its startup files and reached our command.
// A startup file that calls `exit` (e.g. the <= 0.3.11 autostart snippet once
// the daemon socket existed) ends `bash -lc` with status 0 before the command
// runs; without this line that read as success with nothing done.
const SHELL_RAN_SENTINEL = "__TC_SHELL_RAN__";

function withShellRanSentinel(cmd) {
  return `echo ${SHELL_RAN_SENTINEL}; ${cmd}`;
}

// Returns whether the sentinel was printed, and stdout without it.
function takeShellRanSentinel(stdout) {
  const lines = String(stdout || "").split("\n");
  const idx = lines.findIndex((l) => l.replace(/\r$/, "") === SHELL_RAN_SENTINEL);
  if (idx === -1) return { ran: false, stdout: String(stdout || "") };
  lines.splice(idx, 1);
  return { ran: true, stdout: lines.join("\n") };
}

function shellExitedEarlyHint(distro) {
  return (
    `the WSL shell in '${distro}' exited before running the command: a shell startup file ` +
    "(/etc/profile, ~/.profile, ~/.bash_profile, ~/.bash_login, or a file they source) ends the shell early. " +
    `Find it with: wsl -d ${distro} -- bash -c 'grep -n exit ~/.profile ~/.bash_profile ~/.bash_login'`
  );
}

module.exports = {
  LINUX_PATH_PREFIX,
  INSTALL_PROBE_CMD,
  RUNTIME_VERIFY_CMD,
  RUNTIME_VERSION_CMD,
  AUTOSTART_RUN,
  DAEMON_START_CMD,
  BRIDGE_DAEMON_ENSURE,
  BRIDGE_PROBE_CMD,
  SHELL_RAN_SENTINEL,
  withShellRanSentinel,
  takeShellRanSentinel,
  shellExitedEarlyHint,
  getInstallDaemonAutostartCmd,
  getRepairDaemonAutostartCmd,
};

function getInstallDaemonAutostartCmd() {
  const { buildWslInstallCommand } = require("../daemon/autostart.js");
  return buildWslInstallCommand();
}

function getRepairDaemonAutostartCmd() {
  const { buildWslRepairCommand } = require("../daemon/autostart.js");
  return buildWslRepairCommand();
}
