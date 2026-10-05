// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const {
  doctorDaemonAutostart,
  renderDaemonAliveTest,
  STALE_SOCKET_LINE,
} = require("../daemon/autostart.js");
const { resolveDistro } = require("./setup_cursor_wsl.js");
const { detectWsl } = require("../wsl/detect.js");
const {
  LINUX_PATH_PREFIX,
  withShellRanSentinel,
  takeShellRanSentinel,
  shellExitedEarlyHint,
} = require("../bootstrap/constants.js");

async function runDoctorDaemon(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;

  if (platform === "win32") {
    const detectResult = await (o.detect || detectWsl)({ platform });
    const resolved = resolveDistro({
      flags: { distro: (o.flags || {}).distro },
      env,
      detectResult,
    });
    if (resolved.status !== "ok") {
      return {
        status: resolved.status,
        exit_code: 64,
        output: `terminal-commander: could not resolve WSL distro (${resolved.status}).\n`,
      };
    }
    // Shell only on this side: the pidfile rule autostart.sh uses decides.
    const probeCmd =
      `${LINUX_PATH_PREFIX}D="$HOME/.local/share/terminal-commanderd"; ` +
      `if ${renderDaemonAliveTest("$D")}; then echo running; ` +
      `elif [ -S "$D/terminal-commanderd.sock" ]; then echo stale; else echo stopped; fi`;
    const { spawn } = require("node:child_process");
    const {
      buildFilteredEnv,
      ensureSessionInWslEnv,
    } = require("../wsl/filtered_env.js");
    const probe = await new Promise((resolve) => {
      const argv = ["-d", resolved.distro, "--", "bash", "-lc", withShellRanSentinel(probeCmd)];
      const child = spawn(o.wslPath || "wsl.exe", argv, {
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        // Rebuild WSLENV to a TC-only allowlist after name-based filtering:
        // this spawn launches a Linux process (`bash -lc`), so an ambient
        // WSLENV=SOME_SECRET/u would otherwise forward SOME_SECRET into WSL.
        env: ensureSessionInWslEnv(buildFilteredEnv(env)),
      });
      let out = "";
      if (child.stdout) child.stdout.on("data", (b) => { out += b.toString("utf8"); });
      child.on("close", (code) => {
        const shell = takeShellRanSentinel(out);
        const state = shell.stdout.trim();
        resolve({ ran: shell.ran, code, running: state.endsWith("running"), stale: state.endsWith("stale") });
      });
      child.on("error", () => resolve({ ran: false, error: "wsl.exe failed to start" }));
    });
    const lines = [
      "terminal-commander daemon doctor (WSL):",
      `  distro: ${resolved.distro}`,
      `  socket: ~/.local/share/terminal-commanderd/terminal-commanderd.sock`,
      `  daemon_running: ${!probe.ran ? "unknown" : probe.running ? "yes" : "no"}`,
    ];
    if (probe.stale) lines.push(`  note: ${STALE_SOCKET_LINE}`);
    if (!probe.ran) {
      lines.push(
        `  error: ${probe.error || (probe.code === 0 ? shellExitedEarlyHint(resolved.distro) : `probe exited ${probe.code}`)}`,
      );
      return { status: "probe_failed", exit_code: 69, output: `${lines.join("\n")}\n` };
    }
    return { status: "ok", exit_code: 0, output: `${lines.join("\n")}\n` };
  }

  const d = await doctorDaemonAutostart({ env, homeDir: o.homeDir });
  const lines = [
    "terminal-commander daemon doctor:",
    `  socket: ${d.socket_path}`,
    `  daemon_running: ${d.daemon_running ? "yes" : "no"}`,
    ...(d.stale_socket ? [`  note: ${STALE_SOCKET_LINE}`] : []),
    `  autostart_installed: ${d.autostart_installed ? "yes" : "no"}`,
    `  systemd_user: ${d.systemd_user}`,
  ];
  return { status: "ok", exit_code: 0, output: `${lines.join("\n")}\n` };
}

module.exports = { runDoctorDaemon };
