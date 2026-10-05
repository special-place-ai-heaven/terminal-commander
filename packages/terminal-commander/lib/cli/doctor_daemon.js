// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const fs = require("node:fs");
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
const { detectRuntimeEnvironment } = require("./runtime_environment.js");
const { stableBinPath } = require("../harness/stable_bin.js");
const { resolveBinary, formatResolveError } = require("../resolve-binary.js");
const { buildFilteredEnv } = require("../wsl/filtered_env.js");

function defaultExecFile(file, argv, env) {
  const { execFile } = require("node:child_process");
  return new Promise((resolve) => {
    execFile(file, argv, { env, timeout: 15_000 }, (err, stdout) => {
      resolve({ code: err ? (typeof err.code === "number" ? err.code : 1) : 0, out: String(stdout || "") });
    });
  });
}

// Native Windows (the default mode): the stable daemon exe, and the installed
// admin CLI's `status`, which resolves the named pipe the way the adapter does
// and checks it with a Health handshake. It never starts a daemon.
async function doctorNativeWindows(o, env, environment) {
  const platform = "win32";
  const lines = [`terminal-commander daemon doctor (native Windows mode, ${environment.evidence}):`];
  let exe = null;
  try {
    exe = stableBinPath("terminal-commanderd", { platform, env });
  } catch {
    // LOCALAPPDATA unset: reported below.
  }
  lines.push(`  daemon_exe: ${exe ? `${exe} (${fs.existsSync(exe) ? "present" : "missing"})` : "unknown (LOCALAPPDATA not set)"}`);
  const arch = o.arch || process.arch;
  const cli = (o.resolveBinary || resolveBinary)({ binary: "terminal-commander", platform, arch });
  if (cli.reason !== "ok" || !cli.binaryPath) {
    lines.push("  daemon_running: unknown", `  error: ${formatResolveError(cli, { platform, arch })}`);
    return { status: "probe_failed", exit_code: 69, output: `${lines.join("\n")}\n` };
  }
  const { code, out } = await (o.execFile || defaultExecFile)(cli.binaryPath, ["status"], buildFilteredEnv(env));
  // `status` prints `  <name>   : <value>` lines.
  const field = (name) => {
    const line = out.split(/\r?\n/).find((l) => l.trimStart().startsWith(`${name} `) || l.trimStart().startsWith(`${name}:`));
    return line ? line.slice(line.indexOf(":") + 1).trim() : null;
  };
  const daemon = field("daemon");
  if (daemon !== "running" && daemon !== "unavailable") {
    lines.push("  daemon_running: unknown", `  error: \`terminal-commander status\` exited ${code} without a daemon line`);
    return { status: "probe_failed", exit_code: 69, output: `${lines.join("\n")}\n` };
  }
  const pid = field("pid");
  lines.push(`  endpoint: ${field("endpoint")}`, `  daemon_running: ${daemon === "running" ? "yes" : "no"}`);
  if (daemon === "running" && pid && pid !== "-") lines.push(`  pid: ${pid}`);
  lines.push(`  data_dir: ${field("state_dir")}`);
  return { status: "ok", exit_code: 0, output: `${lines.join("\n")}\n` };
}

async function runDoctorDaemon(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;

  if (platform === "win32") {
    // The mode bootstrap and restart use: native unless WSL is configured.
    const environment = detectRuntimeEnvironment({ platform, env, flags: o.flags || {} });
    if (environment.runtime !== "wsl") return doctorNativeWindows(o, env, environment);
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
    const { ensureSessionInWslEnv } = require("../wsl/filtered_env.js");
    const probe = await new Promise((resolve) => {
      // A non-login shell run directly (`-e`, no default shell around it):
      // the probe reads two files and must not run a profile, which can
      // start a daemon.
      const argv = ["-d", resolved.distro, "-e", "bash", "-c", withShellRanSentinel(probeCmd)];
      const child = spawn(o.wslPath || "wsl.exe", argv, {
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        // Rebuild WSLENV to a TC-only allowlist after name-based filtering:
        // this spawn launches a Linux process (`bash -c`), so an ambient
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
      `terminal-commander daemon doctor (WSL bridge mode, ${environment.evidence}):`,
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
