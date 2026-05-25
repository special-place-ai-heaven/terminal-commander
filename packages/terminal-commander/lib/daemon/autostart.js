// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Install idempotent daemon autostart for Linux / WSL: systemd user
// unit when available, otherwise a profile hook + background start.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { spawnSync } = require("node:child_process");
const { applyManagedBlock, hasManagedBlock } = require("./managed_block.js");
const { LINUX_PATH_PREFIX } = require("../bootstrap/constants.js");

const AUTOSTART_STATUSES = Object.freeze({
  OK: "ok",
  SKIPPED: "skipped",
  SYSTEMD_ENABLED: "systemd_enabled",
  PROFILE_HOOK: "profile_hook",
  BINARY_MISSING: "binary_missing",
  UNSUPPORTED_HOST: "unsupported_host",
  INSTALL_FAILED: "install_failed",
});

const DEFAULT_DATA_DIR = "$HOME/.local/share/terminal-commanderd";
const CONFIG_DIR = "$HOME/.config/terminal-commander";
const AUTOSTART_SH = `${CONFIG_DIR}/autostart.sh`;
const PROFILE_SNIPPET = `${CONFIG_DIR}/profile.d/terminal-commander.sh`;
const SYSTEMD_UNIT_PATH = "$HOME/.config/systemd/user/terminal-commanderd.service";

const PROFILE_BLOCK_BODY = `[ -f "$HOME/.config/terminal-commander/profile.d/terminal-commander.sh" ] && . "$HOME/.config/terminal-commander/profile.d/terminal-commander.sh"`;

function shouldInstallDaemonAutostart(env) {
  const e = env || process.env;
  if (e.TC_SKIP_DAEMON_AUTOSTART === "1") return false;
  if (e.TC_BOOTSTRAP_START_DAEMON === "0") return false;
  return true;
}

function renderAutostartScript() {
  return `#!/usr/bin/env bash
# terminal-commander autostart — managed by terminal-commander; do not edit.
set -eu
TC_DATA="\${TC_DATA:-$HOME/.local/share/terminal-commanderd}"
SOCK="\$TC_DATA/terminal-commanderd.sock"
export PATH="$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
if [ -S "\$SOCK" ]; then
  exit 0
fi
if ! command -v terminal-commanderd >/dev/null 2>&1; then
  exit 0
fi
mkdir -p "\$TC_DATA" "$HOME/.local/state/terminal-commander"
nohup terminal-commanderd --data-dir "\$TC_DATA" start --mode ipc-server \\
  >>"$HOME/.local/state/terminal-commander/daemon.log" 2>&1 &
`;
}

function renderProfileSnippet() {
  return `. "$HOME/.config/terminal-commander/autostart.sh" 2>/dev/null || true
`;
}

function renderSystemdUnit(daemonBinary) {
  const bin = daemonBinary || "terminal-commanderd";
  return `[Unit]
Description=Terminal Commander daemon (user)
Documentation=https://github.com/special-place-ai-heaven/terminal-commander
After=default.target

[Service]
Type=simple
ExecStart=${bin} --data-dir %h/.local/share/terminal-commanderd start --mode ipc-server
Restart=on-failure
RestartSec=3
Environment=PATH=%h/.npm-global/bin:%h/.local/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy=default.target
`;
}

function expandHome(p, homeDir) {
  const home = homeDir || os.homedir();
  return String(p).replace(/\$HOME/g, home).replace(/%h/g, home);
}

function resolveDaemonBinary(env) {
  const e = env || process.env;
  const pathPrefix =
    `${e.HOME || os.homedir()}/.npm-global/bin:` +
    `${e.HOME || os.homedir()}/.local/bin:` +
    `${e.HOME || os.homedir()}/.cargo/bin:/usr/local/bin:/usr/bin:/bin`;
  const r = spawnSync("bash", ["-lc", `export PATH="${pathPrefix}"; command -v terminal-commanderd`], {
    encoding: "utf8",
    shell: false,
    env: e,
  });
  if (r.status !== 0) return null;
  const line = (r.stdout || "").trim().split(/\r?\n/)[0];
  if (!line || line.startsWith("/mnt/")) return null;
  return line;
}

function systemdUserAvailable(env) {
  const r = spawnSync(
    "bash",
    [
      "-lc",
      'command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] && systemctl is-system-running --quiet 2>/dev/null',
    ],
    { encoding: "utf8", shell: false, env: env || process.env },
  );
  return r.status === 0;
}

function writeFileAtomic(filePath, content, mode) {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(dir, `.${path.basename(filePath)}.tmp-${process.pid}`);
  fs.writeFileSync(tmp, content, { encoding: "utf8", mode: mode || 0o644 });
  fs.renameSync(tmp, filePath);
}

function patchProfileFile(profilePath, homeDir) {
  const p = expandHome(profilePath, homeDir);
  let content = "";
  if (fs.existsSync(p)) {
    content = fs.readFileSync(p, "utf8");
  }
  if (hasManagedBlock(content, "autostart")) {
    return { path: p, changed: false };
  }
  const next = applyManagedBlock(content, "autostart", PROFILE_BLOCK_BODY);
  writeFileAtomic(p, next, 0o644);
  return { path: p, changed: true };
}

function installSystemdUserUnit(daemonBinary, homeDir) {
  const unitPath = expandHome(SYSTEMD_UNIT_PATH, homeDir);
  writeFileAtomic(unitPath, renderSystemdUnit(daemonBinary), 0o644);
  const reload = spawnSync("systemctl", ["--user", "daemon-reload"], {
    encoding: "utf8",
    shell: false,
  });
  if (reload.status !== 0) {
    return { ok: false, hint: "systemctl --user daemon-reload failed" };
  }
  const enable = spawnSync("systemctl", ["--user", "enable", "--now", "terminal-commanderd.service"], {
    encoding: "utf8",
    shell: false,
  });
  if (enable.status !== 0) {
    return {
      ok: false,
      hint: `systemctl --user enable --now failed: ${(enable.stderr || "").trim()}`,
    };
  }
  return { ok: true, unitPath };
}

function runAutostartOnce(homeDir) {
  const script = expandHome(AUTOSTART_SH, homeDir);
  if (!fs.existsSync(script)) return { ok: false };
  const r = spawnSync("bash", [script], { encoding: "utf8", shell: false });
  return { ok: r.status === 0, exit_code: r.status };
}

/**
 * Install autostart artifacts on the current Linux/WSL host.
 *
 * @param {Object} [opts]
 * @param {NodeJS.ProcessEnv} [opts.env]
 * @param {string} [opts.homeDir]
 * @param {boolean} [opts.dry_run]
 */
function installDaemonAutostart(opts) {
  const o = opts || {};
  const env = o.env || process.env;
  const platform = o.platform || process.platform;

  if (platform !== "linux") {
    return {
      status: AUTOSTART_STATUSES.UNSUPPORTED_HOST,
      hint: "daemon autostart installs only on Linux / WSL",
    };
  }

  if (!shouldInstallDaemonAutostart(env)) {
    return {
      status: AUTOSTART_STATUSES.SKIPPED,
      hint: "daemon autostart skipped (TC_SKIP_DAEMON_AUTOSTART=1 or TC_BOOTSTRAP_START_DAEMON=0)",
    };
  }

  const homeDir = o.homeDir || os.homedir();
  const daemonBinary = o.daemonBinary || resolveDaemonBinary(env);
  if (!daemonBinary && o.dry_run !== true) {
    return {
      status: AUTOSTART_STATUSES.BINARY_MISSING,
      hint: "terminal-commanderd not on PATH inside this environment",
    };
  }

  if (o.dry_run === true) {
    return {
      status: AUTOSTART_STATUSES.OK,
      mode: systemdUserAvailable(env) ? "systemd" : "profile",
      hint: "dry-run: would install daemon autostart",
    };
  }

  const cfgDir = expandHome(CONFIG_DIR, homeDir);
  const autostartPath = expandHome(AUTOSTART_SH, homeDir);
  const snippetPath = expandHome(PROFILE_SNIPPET, homeDir);

  writeFileAtomic(autostartPath, renderAutostartScript(), 0o755);
  writeFileAtomic(snippetPath, renderProfileSnippet(), 0o644);

  if (systemdUserAvailable(env) && daemonBinary) {
    const systemd = installSystemdUserUnit(daemonBinary, homeDir);
    if (systemd.ok) {
      return {
        status: AUTOSTART_STATUSES.SYSTEMD_ENABLED,
        hint: `systemd user service enabled (${systemd.unitPath})`,
        mode: "systemd",
      };
    }
  }

  const targets = [".profile", ".bashrc", ".zshrc"];
  const patched = [];
  for (const rel of targets) {
    try {
      const r = patchProfileFile(path.join(homeDir, rel), homeDir);
      if (r.changed) patched.push(r.path);
    } catch (_e) {
      /* ignore missing permission */
    }
  }

  runAutostartOnce(homeDir);

  return {
    status: AUTOSTART_STATUSES.PROFILE_HOOK,
    hint:
      patched.length > 0
        ? `profile hook installed (${patched.join(", ")})`
        : "autostart script installed; profile hook already present",
    mode: "profile",
    patched,
  };
}

function renderInstallBash() {
  const autostart = renderAutostartScript();
  const profile = renderProfileSnippet();
  return `#!/usr/bin/env bash
set -eu
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$HOME/.local/bin:$HOME/.npm-global/bin"
TC_HOME="$HOME"
TC_CFG="$TC_HOME/.config/terminal-commander"
mkdir -p "$TC_CFG/profile.d"
cat > "$TC_CFG/autostart.sh" <<'TC_AUTOSTART'
${autostart}TC_AUTOSTART
chmod 755 "$TC_CFG/autostart.sh"
cat > "$TC_CFG/profile.d/terminal-commander.sh" <<'TC_PROFILE'
${profile}TC_PROFILE
chmod 644 "$TC_CFG/profile.d/terminal-commander.sh"
DAEMON_BIN=""
if command -v terminal-commanderd >/dev/null 2>&1; then
  DAEMON_BIN="$(command -v terminal-commanderd)"
fi
USE_SYSTEMD=0
if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] && systemctl is-system-running --quiet 2>/dev/null; then
  USE_SYSTEMD=1
fi
if [ "$USE_SYSTEMD" = "1" ] && [ -n "$DAEMON_BIN" ]; then
  mkdir -p "$TC_HOME/.config/systemd/user"
  cat > "$TC_HOME/.config/systemd/user/terminal-commanderd.service" <<TC_UNIT
[Unit]
Description=Terminal Commander daemon (user)
After=default.target

[Service]
Type=simple
ExecStart=$DAEMON_BIN --data-dir $TC_HOME/.local/share/terminal-commanderd start --mode ipc-server
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
TC_UNIT
  systemctl --user daemon-reload
  systemctl --user enable --now terminal-commanderd.service
else
  for f in .profile .bashrc .zshrc; do
    PF="$TC_HOME/$f"
    touch "$PF"
    if ! grep -q 'terminal-commander autostart BEGIN' "$PF" 2>/dev/null; then
      cat >> "$PF" <<'TC_MARK'

# terminal-commander autostart BEGIN
[ -f "$TC_HOME/.config/terminal-commander/profile.d/terminal-commander.sh" ] && . "$TC_HOME/.config/terminal-commander/profile.d/terminal-commander.sh"
# terminal-commander autostart END
TC_MARK
    fi
  done
  . "$TC_CFG/autostart.sh" || true
fi
`;
}

function buildWslInstallCommand() {
  const b64 = Buffer.from(renderInstallBash(), "utf8").toString("base64");
  return `${LINUX_PATH_PREFIX}command -v base64 >/dev/null 2>&1 && echo ${b64} | base64 -d | bash`;
}

function doctorDaemonAutostart(opts) {
  const o = opts || {};
  const homeDir = o.homeDir || os.homedir();
  const sock = path.join(
    expandHome(DEFAULT_DATA_DIR.replace("$HOME", homeDir), homeDir),
    "terminal-commanderd.sock",
  );
  const running = fs.existsSync(sock);
  const autostartPath = expandHome(AUTOSTART_SH, homeDir);
  const installed = fs.existsSync(autostartPath);
  let systemd = "n/a";
  if (systemdUserAvailable(o.env || process.env)) {
    const st = spawnSync(
      "systemctl",
      ["--user", "is-active", "terminal-commanderd.service"],
      { encoding: "utf8", shell: false },
    );
    systemd = st.status === 0 ? "active" : (st.stdout || st.stderr || "").trim() || "inactive";
  }
  return {
    socket_path: sock,
    daemon_running: running,
    autostart_installed: installed,
    systemd_user: systemd,
  };
}

module.exports = {
  AUTOSTART_STATUSES,
  shouldInstallDaemonAutostart,
  renderAutostartScript,
  renderProfileSnippet,
  renderSystemdUnit,
  installDaemonAutostart,
  buildWslInstallCommand,
  renderInstallBash,
  doctorDaemonAutostart,
  resolveDaemonBinary,
  systemdUserAvailable,
};
