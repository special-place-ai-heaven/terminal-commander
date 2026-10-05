// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Install idempotent daemon autostart for Linux / WSL: systemd user
// unit when available, otherwise a profile hook + background start.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const net = require("node:net");
const { spawnSync } = require("node:child_process");
const {
  applyManagedBlock,
  managedBlockState,
  replaceManagedBlockBody,
} = require("./managed_block.js");
const {
  LINUX_PATH_PREFIX,
  AUTOSTART_SKIPPED_LINE,
  tcSessionVar,
  daemonStartSkippedReason,
} = require("../bootstrap/constants.js");

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
const STALE_SOCKET_LINE =
  "socket file present but no daemon answering; it will be replaced on next start";
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

// Shell condition: the daemon whose pidfile is in `dataDir` is alive. A live
// daemon is known by its pidfile, not by a socket file: a daemon that died
// leaves its socket behind. Read-only (no lock taken); the cmdline check
// rejects a reused pid. Linux only (/proc).
function renderDaemonAliveTest(dataDir) {
  return `TC_PID=$(sed -n 's/.*"pid":[[:space:]]*\\([0-9][0-9]*\\).*/\\1/p' "${dataDir}/terminal-commanderd.pid" 2>/dev/null || true); [ -n "$TC_PID" ] && grep -qa terminal-commanderd "/proc/$TC_PID/cmdline" 2>/dev/null`;
}

// The body runs in a subshell so `exit`, `set -eu` and the PATH export stay
// inside it even when a stale (<= 0.3.11) profile snippet still SOURCES this
// file. Callers must run it as a process: `bash .../autostart.sh`. POSIX sh
// only: a stale snippet may source it into dash or zsh.
function renderAutostartScript() {
  return `#!/usr/bin/env bash
# terminal-commander autostart — managed by terminal-commander; do not edit.
(
# First: a terminal that closes now must not end this before setsid below.
# Inside the subshell, so a stale snippet that sources this file does not
# change the user's shell.
trap '' HUP
# Never from inside a Terminal Commander process tree: the daemon marks its
# children (TC_DAEMON_CHILD), and TC_SOCKET / TC_SESSION select an endpoint
# this script does not serve. The MCP adapter starts those daemons itself.
# Only a caller that asks (TC_AUTOSTART_REPORT) is told.
if [ -n "\${TC_DAEMON_CHILD:-}" ] || [ -n "\${TC_SOCKET:-}" ] || [ -n "\${TC_SESSION:-}" ]; then
  if [ -n "\${TC_AUTOSTART_REPORT:-}" ]; then
    echo "${AUTOSTART_SKIPPED_LINE}"
  fi
  exit 0
fi
unset TC_AUTOSTART_REPORT
set -eu
# TC_DATA is honoured on purpose: a user may export it in their own profile
# to relocate the default daemon.
TC_DATA="\${TC_DATA:-$HOME/.local/share/terminal-commanderd}"
export PATH="$HOME/.npm-global/bin:$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
# Already running (by its pidfile)? A start that loses a race to another one
# exits on the daemon's data-dir lock, into daemon.log.
if ${renderDaemonAliveTest("$TC_DATA")}; then
  exit 0
fi
if ! command -v terminal-commanderd >/dev/null 2>&1; then
  exit 0
fi
mkdir -p "\$TC_DATA" "$HOME/.local/state/terminal-commander"
# Its own session (setsid), so closing the terminal that ran this hangs up
# nothing it started: nohup alone does not survive the npm shim, whose child
# gets SIGHUP's default action back.
TC_SETSID=
if command -v setsid >/dev/null 2>&1; then
  TC_SETSID=setsid
fi
$TC_SETSID nohup terminal-commanderd --data-dir "\$TC_DATA" start --mode ipc-server \\
  </dev/null >>"$HOME/.local/state/terminal-commander/daemon.log" 2>&1 &
)
`;
}

// Sourced from the user's ~/.profile, ~/.bashrc and ~/.zshrc on every shell
// start, so it must not be able to exit that shell, change its options or
// environment, block it, or print anything: run the script as a detached
// background process from a subshell (no job-control noise, no `$!`).
// Interactive shells only: tools run `bash -lc` constantly, and every step
// that needs the daemon (bridge, setup, restart) starts it explicitly.
// `trap '' HUP` before the launch: a terminal that closes at once hangs up
// the launcher before it reaches setsid; an ignored SIGHUP is inherited.
function renderProfileSnippet() {
  return `case $- in *i*) ( trap '' HUP; bash "$HOME/.config/terminal-commander/autostart.sh" </dev/null >/dev/null 2>&1 & ) ;; esac
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
  // No startup files on purpose: PATH is set explicitly, and the user's
  // ~/.profile / ~/.bashrc may hold a <= 0.3.11 snippet that exits the shell
  // once the daemon socket exists -- hiding the binary and blocking the
  // upgrade repair. Non-login `-c` skips ~/.profile; stdin "ignore" stops
  // bash's rshd heuristic (stdin is a socketpair under node) from reading
  // ~/.bashrc.
  const r = spawnSync("bash", ["-c", `export PATH="${pathPrefix}"; command -v terminal-commanderd`], {
    encoding: "utf8",
    shell: false,
    stdio: ["ignore", "pipe", "pipe"],
    env: e,
  });
  if (r.status !== 0) return null;
  const line = (r.stdout || "").trim().split(/\r?\n/)[0];
  if (!line || line.startsWith("/mnt/")) return null;
  return line;
}

// Same no-startup-files rule as resolveDaemonBinary: a login shell killed by a
// startup-file `exit 0` would read as "systemd available".
function systemdUserAvailable(env) {
  const r = spawnSync(
    "bash",
    [
      "-c",
      'command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] && systemctl is-system-running --quiet 2>/dev/null',
    ],
    { encoding: "utf8", shell: false, stdio: ["ignore", "pipe", "pipe"], env: env || process.env },
  );
  return r.status === 0;
}

function writeFileAtomic(filePath, content, mode) {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(dir, `.${path.basename(filePath)}.tmp-${process.pid}`);
  fs.writeFileSync(tmp, content, { encoding: "utf8", mode: mode || 0o644 });
  // The create mode is umask-filtered; set it exactly (rc files keep theirs).
  fs.chmodSync(tmp, mode || 0o644);
  fs.renameSync(tmp, filePath);
}

// Add the managed block (when `add`), or bring an existing one up to date in
// place. Only the lines between the markers change; a file with malformed
// markers is never touched, only reported.
function patchProfileFile(profilePath, homeDir, add) {
  const p = expandHome(profilePath, homeDir);
  const exists = fs.existsSync(p);
  // Write through a symlinked rc file (dotfile managers) instead of replacing it.
  const target = exists ? fs.realpathSync(p) : p;
  // latin1 round-trips every byte, so non-UTF-8 content outside the block survives.
  const content = exists ? fs.readFileSync(target, "latin1") : "";
  const block = managedBlockState(content, "autostart");
  if (block.state === "malformed") return { path: p, changed: false, malformed: true };
  if (block.state === "single" && block.body === PROFILE_BLOCK_BODY) return { path: p, changed: false };
  if (block.state === "absent" && !add) return { path: p, changed: false };
  const next =
    block.state === "single"
      ? replaceManagedBlockBody(content, "autostart", PROFILE_BLOCK_BODY)
      : applyManagedBlock(content, "autostart", PROFILE_BLOCK_BODY);
  writeFileAtomic(target, Buffer.from(next, "latin1"), exists ? fs.statSync(target).mode & 0o7777 : 0o644);
  return { path: p, changed: true };
}

function patchProfileFiles(homeDir, add) {
  const patched = [];
  const malformed = [];
  for (const rel of [".profile", ".bashrc", ".zshrc"]) {
    try {
      const r = patchProfileFile(path.join(homeDir, rel), homeDir, add);
      if (r.changed) patched.push(r.path);
      if (r.malformed) malformed.push(r.path);
    } catch (_e) {
      /* ignore missing permission */
    }
  }
  return { patched, malformed };
}

function malformedHint(malformed) {
  return malformed.length > 0
    ? `; WARNING: malformed terminal-commander autostart markers left unchanged in ${malformed.join(", ")}`
    : "";
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
 * NOTE (F1): the autostart daemon serves the LEGACY DEFAULT endpoint, by
 * design. It is a convenience pre-warm, not a per-harness daemon — each MCP
 * process spawns its own per-session daemon (TC_SESSION) on demand via
 * supervisor::ensure_daemon, and ignores this one. So the unit deliberately
 * carries no TC_SESSION.
 *
 * @param {Object} [opts]
 * @param {NodeJS.ProcessEnv} [opts.env]
 * @param {string} [opts.homeDir]
 * @param {boolean} [opts.dry_run]
 * @param {() => boolean} [opts.systemdUserAvailable]  Test seam.
 * @param {(homeDir:string) => {ok:boolean, exit_code?:number}} [opts.runAutostartOnce]  Test seam.
 */
function installDaemonAutostart(opts) {
  const o = opts || {};
  const env = o.env || process.env;
  const platform = o.platform || process.platform;
  const systemdAvailable = o.systemdUserAvailable || systemdUserAvailable;
  const runOnce = o.runAutostartOnce || runAutostartOnce;

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
      mode: systemdAvailable(env) ? "systemd" : "profile",
      hint: "dry-run: would install daemon autostart",
    };
  }

  const autostartPath = expandHome(AUTOSTART_SH, homeDir);
  const snippetPath = expandHome(PROFILE_SNIPPET, homeDir);

  writeFileAtomic(autostartPath, renderAutostartScript(), 0o755);
  writeFileAtomic(snippetPath, renderProfileSnippet(), 0o644);

  if (systemdAvailable(env) && daemonBinary) {
    const systemd = installSystemdUserUnit(daemonBinary, homeDir);
    if (systemd.ok) {
      // No new rc blocks under systemd, but an existing stale one is updated.
      const rc = patchProfileFiles(homeDir, false);
      return {
        status: AUTOSTART_STATUSES.SYSTEMD_ENABLED,
        hint: `systemd user service enabled (${systemd.unitPath})${malformedHint(rc.malformed)}`,
        mode: "systemd",
        patched: rc.patched,
        malformed_rc_files: rc.malformed,
      };
    }
  }

  const { patched, malformed } = patchProfileFiles(homeDir, true);

  const baseHint =
    patched.length > 0
      ? `profile hook installed (${patched.join(", ")})`
      : "autostart script installed; profile hook already present";

  // autostart.sh would decline anyway; say so instead of reporting a run.
  const sessionVar = tcSessionVar(env);
  if (sessionVar) {
    const reason = daemonStartSkippedReason(sessionVar);
    return {
      status: AUTOSTART_STATUSES.PROFILE_HOOK,
      hint: `${baseHint}; daemon not started: ${reason}${malformedHint(malformed)}`,
      mode: "profile",
      patched,
      malformed_rc_files: malformed,
      daemon_start: { status: "skipped", reason },
    };
  }

  // Run the autostart script once. Do NOT swallow a non-zero exit (was a
  // silent failure): surface it in the result so operators/doctor can see it.
  const ran = runOnce(homeDir);
  const exitCode = typeof ran.exit_code === "number" ? ran.exit_code : null;

  const hint =
    (ran.ok === false && exitCode !== 0
      ? `${baseHint}; WARNING: autostart run exited ${exitCode} (daemon may not have started — check ~/.local/state/terminal-commander/daemon.log)`
      : baseHint) + malformedHint(malformed);

  return {
    status: AUTOSTART_STATUSES.PROFILE_HOOK,
    hint,
    mode: "profile",
    patched,
    malformed_rc_files: malformed,
    autostart_run_exit_code: exitCode,
  };
}

function renderWriteFilesBash() {
  return `mkdir -p "$TC_CFG/profile.d"
cat > "$TC_CFG/autostart.sh" <<'TC_AUTOSTART'
${renderAutostartScript()}TC_AUTOSTART
chmod 755 "$TC_CFG/autostart.sh"
cat > "$TC_CFG/profile.d/terminal-commander.sh" <<'TC_PROFILE'
${renderProfileSnippet()}TC_PROFILE
chmod 644 "$TC_CFG/profile.d/terminal-commander.sh"
`;
}

// Bash twin of patchProfileFile for the WSL installer (POSIX awk/tail/cmp, no
// node yet). tc_rc_sync FILE ADD: update an existing managed block in place
// (bytes outside it, trailing newline, mode and symlinks kept; an up-to-date
// file is not rewritten), append one when ADD=1, and leave malformed markers
// alone with a "terminal-commander:" line the caller surfaces.
function renderRcSyncBash() {
  return String.raw`TC_BEGIN='# terminal-commander autostart BEGIN'
TC_END='# terminal-commander autostart END'
TC_BODY="$(cat <<'TC_BODY_EOF'
${PROFILE_BLOCK_BODY}
TC_BODY_EOF
)"
export TC_BODY
tc_rc_sync() {
  PF="$1"
  STATE=absent
  if [ -f "$PF" ]; then
    STATE="$(awk -v b="$TC_BEGIN" -v e="$TC_END" '
      { sub(/\r$/, "") }
      $0 == b { nb++; if (!bl) bl = NR }
      $0 == e { ne++; if (!el) el = NR }
      END {
        if (nb + ne == 0) print "absent"
        else if (nb != 1 || ne != 1 || bl > el) print "malformed"
        else print "single"
      }' "$PF" 2>/dev/null)" || STATE=unreadable
  fi
  case "$STATE" in
    absent)
      if [ "$2" = "1" ]; then
        touch "$PF"
        printf '\n%s\n%s\n%s\n' "$TC_BEGIN" "$TC_BODY" "$TC_END" >> "$PF"
      fi
      ;;
    single)
      if [ -z "$(tail -c 1 "$PF")" ]; then TNL=1; else TNL=0; fi
      TMP="$PF.tc-sync.$$"
      awk -v b="$TC_BEGIN" -v e="$TC_END" -v tnl="$TNL" '
        function out(s) { printf "%s%s", (n++ ? "\n" : ""), s }
        { bare = $0; sub(/\r$/, "", bare) }
        bare == b {
          out($0); cr = ($0 == bare) ? "" : "\r"
          k = split(ENVIRON["TC_BODY"], bl, "\n")
          for (i = 1; i <= k; i++) out(bl[i] cr)
          skip = 1; next
        }
        bare == e { skip = 0; out($0); next }
        !skip { out($0) }
        END { if (tnl) printf "\n" }' "$PF" > "$TMP"
      if cmp -s "$TMP" "$PF"; then rm -f "$TMP"; else cat "$TMP" > "$PF"; rm -f "$TMP"; fi
      ;;
    *)
      echo "terminal-commander: malformed terminal-commander autostart markers left unchanged in $PF"
      ;;
  esac
}
`;
}

function renderInstallBash() {
  return `#!/usr/bin/env bash
set -eu
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$HOME/.local/bin:$HOME/.npm-global/bin"
TC_HOME="$HOME"
TC_CFG="$TC_HOME/.config/terminal-commander"
${renderWriteFilesBash()}${renderRcSyncBash()}DAEMON_BIN=""
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
  # No new rc blocks under systemd, but an existing stale one is updated.
  for f in .profile .bashrc .zshrc; do
    tc_rc_sync "$TC_HOME/$f" 0
  done
  systemctl --user daemon-reload
  systemctl --user enable --now terminal-commanderd.service
else
  for f in .profile .bashrc .zshrc; do
    tc_rc_sync "$TC_HOME/$f" 1
  done
  bash "$TC_CFG/autostart.sh" || true
fi
`;
}

// Upgrade repair only: rewrite autostart.sh and the profile snippet when an
// earlier install left them on disk, and touch nothing else. Runs before any
// `bash -lc` step of the Windows-side bootstrap, because a <= 0.3.11 snippet
// kills those login shells once the daemon socket exists.
function renderRepairBash() {
  return `#!/usr/bin/env bash
set -eu
TC_CFG="$HOME/.config/terminal-commander"
if [ ! -e "$TC_CFG/autostart.sh" ] && [ ! -e "$TC_CFG/profile.d/terminal-commander.sh" ]; then
  exit 0
fi
${renderWriteFilesBash()}`;
}

function wslPipeCommand(script) {
  const b64 = Buffer.from(script, "utf8").toString("base64");
  return `${LINUX_PATH_PREFIX}command -v base64 >/dev/null 2>&1 && echo ${b64} | base64 -d | bash`;
}

function buildWslInstallCommand() {
  return wslPipeCommand(renderInstallBash());
}

function buildWslRepairCommand() {
  return wslPipeCommand(renderRepairBash());
}

// True when a daemon accepts a connection on `sock` within `timeoutMs`. A
// socket file alone proves nothing: a daemon that died leaves it behind.
function socketAnswers(sock, timeoutMs = 1000) {
  return new Promise((resolve) => {
    const conn = net.createConnection(sock);
    const done = (answered) => {
      conn.destroy();
      resolve(answered);
    };
    conn.setTimeout(timeoutMs, () => done(false));
    conn.once("connect", () => done(true));
    // EAGAIN: the listener is alive, its backlog is full.
    conn.once("error", (err) => done(err.code === "EAGAIN"));
  });
}

async function doctorDaemonAutostart(opts) {
  const o = opts || {};
  const homeDir = o.homeDir || os.homedir();
  const sock = path.join(
    expandHome(DEFAULT_DATA_DIR.replace("$HOME", homeDir), homeDir),
    "terminal-commanderd.sock",
  );
  const running = await socketAnswers(sock);
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
    stale_socket: !running && fs.existsSync(sock),
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
  buildWslRepairCommand,
  renderInstallBash,
  renderRepairBash,
  renderRcSyncBash,
  patchProfileFile,
  PROFILE_BLOCK_BODY,
  doctorDaemonAutostart,
  renderDaemonAliveTest,
  STALE_SOCKET_LINE,
  resolveDaemonBinary,
  systemdUserAvailable,
};
