// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Behavioural guard for the Linux/WSL autostart files: nothing the wrapper
// puts into a user's shell startup may exit that shell, flip its options
// (set -e / -u) or replace its PATH. Up to 0.3.11 the profile snippet SOURCED
// autostart.sh, whose top-level `exit 0` (socket present) ended every login
// and interactive shell, and whose `set -eu` + `export PATH=...` leaked into
// the user's shell when the socket was absent. These tests drive the rendered
// files with a real bash and a real AF_UNIX socket in a throwaway HOME.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

const {
  renderAutostartScript,
  renderProfileSnippet,
  installDaemonAutostart,
  resolveDaemonBinary,
} = require("../lib/daemon/autostart.js");
const {
  LINUX_PATH_PREFIX,
  BRIDGE_DAEMON_ENSURE,
  BRIDGE_PROBE_CMD,
  AUTOSTART_RUN,
  DAEMON_START_CMD,
} = require("../lib/bootstrap/constants.js");
const { ensureDaemonAutostartInWsl } = require("../lib/bootstrap/ensure_daemon_autostart.js");
const { ensureWslRuntime } = require("../lib/bootstrap/ensure_wsl_runtime.js");
const { runBootstrap } = require("../lib/bootstrap/orchestrator.js");
const { runRestart } = require("../lib/cli/restart.js");
const { runDoctorDaemon } = require("../lib/cli/doctor_daemon.js");
const { wslDoctor } = require("../lib/wsl/doctor.js");

const SKIP =
  process.platform === "win32"
    ? "needs a POSIX bash that sees a real AF_UNIX socket via [ -S ] (Linux / WSL / macOS); Git Bash on Windows cannot"
    : spawnSync("bash", ["-c", "true"]).status === 0
      ? false
      : "bash not available on this host";

const BASE_PATH = "/usr/bin:/bin";

// What 0.3.0-0.3.11 wrote to disk. Kept verbatim: installs in the field carry
// these files until an upgraded wrapper rewrites them.
const LEGACY_AUTOSTART = `#!/usr/bin/env bash
# terminal-commander autostart - managed by terminal-commander; do not edit.
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
const LEGACY_SNIPPET = `. "$HOME/.config/terminal-commander/autostart.sh" 2>/dev/null || true
`;

function cfg(home, rel) {
  return path.join(home, ".config", "terminal-commander", rel);
}

function shellEnv(home) {
  return { HOME: home, PATH: BASE_PATH };
}

// Lay down exactly what installDaemonAutostart writes on the profile-hook
// path (autostart.sh, profile snippet, rc-file managed blocks), optionally
// overwriting the two generated files with the legacy content.
function makeHome({ legacy = false, stubDaemon = false } = {}) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "tc-shell-"));
  const r = installDaemonAutostart({
    platform: "linux",
    homeDir: home,
    env: shellEnv(home),
    daemonBinary: "/fake/terminal-commanderd",
    systemdUserAvailable: () => false,
    runAutostartOnce: () => ({ ok: true, exit_code: 0 }),
  });
  assert.equal(r.status, "profile_hook");
  if (legacy) {
    fs.writeFileSync(cfg(home, "autostart.sh"), LEGACY_AUTOSTART, { mode: 0o755 });
    fs.writeFileSync(cfg(home, "profile.d/terminal-commander.sh"), LEGACY_SNIPPET);
  }
  if (stubDaemon) {
    const bin = path.join(home, ".local", "bin");
    fs.mkdirSync(bin, { recursive: true });
    fs.writeFileSync(
      path.join(bin, "terminal-commanderd"),
      '#!/bin/sh\necho "$*" >> "$HOME/daemon-starts.log"\n',
      { mode: 0o755 },
    );
  }
  return home;
}

async function withSocket(home, fn) {
  const dir = path.join(home, ".local", "share", "terminal-commanderd");
  fs.mkdirSync(dir, { recursive: true });
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(path.join(dir, "terminal-commanderd.sock"), resolve);
  });
  try {
    return await fn();
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

function bash(home, args) {
  return spawnSync("bash", args, { env: shellEnv(home), encoding: "utf8", input: "" });
}

// Source `file` into a clean non-login bash, then report whether the shell
// survived, its option flags, and whether PATH was touched.
function sourceAndProbe(home, file) {
  const script = [
    'before="$PATH"',
    `. "${file}"`,
    "echo NEXT",
    'echo "FLAGS=$-"',
    '[ "$PATH" = "$before" ] && echo PATH_SAME || echo "PATH_CHANGED=$PATH"',
  ].join("\n");
  return bash(home, ["--noprofile", "--norc", "-c", script]);
}

function assertShellUntouched(r, what) {
  assert.match(r.stdout, /^NEXT$/m, `${what}: the shell must keep running after it (stdout=${JSON.stringify(r.stdout)} stderr=${JSON.stringify(r.stderr)})`);
  const flags = (r.stdout.match(/^FLAGS=(.*)$/m) || [])[1];
  assert.ok(flags != null, `${what}: no FLAGS line`);
  assert.doesNotMatch(flags, /[eu]/, `${what}: set -e/-u leaked into the shell ($-=${flags})`);
  assert.match(r.stdout, /^PATH_SAME$/m, `${what}: PATH must be unchanged`);
}

async function waitForStarts(home) {
  const log = path.join(home, "daemon-starts.log");
  for (let i = 0; i < 100 && !fs.existsSync(log); i++) {
    await new Promise((r) => setTimeout(r, 50));
  }
  // Give a second (wrong) start a chance to show up before counting.
  await new Promise((r) => setTimeout(r, 300));
  return fs.existsSync(log) ? fs.readFileSync(log, "utf8").split("\n").filter(Boolean) : [];
}

test("socket present: sourcing the rc files / snippet never exits or reconfigures the shell", { skip: SKIP }, async () => {
  const home = makeHome();
  await withSocket(home, async () => {
    for (const rel of [".profile", ".bashrc", ".zshrc"]) {
      assertShellUntouched(sourceAndProbe(home, path.join(home, rel)), rel);
    }
    assertShellUntouched(sourceAndProbe(home, cfg(home, "profile.d/terminal-commander.sh")), "profile snippet");
    // The reported symptom, verbatim: a login shell printed nothing.
    const login = bash(home, ["-lc", "echo NEXT"]);
    assert.match(login.stdout, /^NEXT$/m, `bash -lc must run its command (stderr=${login.stderr})`);
    const interactive = bash(home, ["-i", "-c", 'echo NEXT; echo "FLAGS=$-"']);
    assert.match(interactive.stdout, /^NEXT$/m, "interactive bash must run its command");
    assert.doesNotMatch(interactive.stdout.match(/^FLAGS=(.*)$/m)[1], /[eu]/);
  });
});

test("socket absent: sourcing .profile starts the daemon exactly once and leaves the shell alone", { skip: SKIP }, async () => {
  const home = makeHome({ stubDaemon: true });
  assertShellUntouched(sourceAndProbe(home, path.join(home, ".profile")), ".profile");
  const starts = await waitForStarts(home);
  assert.equal(starts.length, 1, `daemon must be started exactly once, got ${JSON.stringify(starts)}`);
  assert.match(starts[0], /--data-dir .*\/\.local\/share\/terminal-commanderd start --mode ipc-server$/);
  assert.ok(fs.existsSync(path.join(home, ".local", "state", "terminal-commander", "daemon.log")));
});

test("socket present: a stale snippet that still sources the new autostart.sh is harmless", { skip: SKIP }, async () => {
  const home = makeHome();
  fs.writeFileSync(cfg(home, "profile.d/terminal-commander.sh"), LEGACY_SNIPPET);
  await withSocket(home, async () => {
    assertShellUntouched(sourceAndProbe(home, path.join(home, ".profile")), "legacy snippet + new autostart.sh");
  });
});

test("socket absent: a stale snippet sourcing the new autostart.sh still starts once, shell untouched", { skip: SKIP }, async () => {
  const home = makeHome({ stubDaemon: true });
  fs.writeFileSync(cfg(home, "profile.d/terminal-commander.sh"), LEGACY_SNIPPET);
  assertShellUntouched(sourceAndProbe(home, path.join(home, ".profile")), "legacy snippet + new autostart.sh");
  assert.equal((await waitForStarts(home)).length, 1);
});

const ZSH =
  process.platform === "win32"
    ? null
    : (spawnSync("sh", ["-c", "command -v zsh"], { encoding: "utf8" }).stdout || "").trim() || null;
const ZSH_SKIP = SKIP || (ZSH ? false : "zsh not installed on this host");

// zsh reads ~/.zshrc (never ~/.profile) and only when interactive: `-i -c` is that path.
function zshInteractive(home) {
  const probe = `echo NEXT; [[ -o errexit || -o nounset ]] && echo OPTIONS_CHANGED; [ "$PATH" = "${BASE_PATH}" ] && echo PATH_SAME`;
  return spawnSync(ZSH, ["-i", "-c", probe], { env: shellEnv(home), encoding: "utf8", input: "" });
}

function assertZshUntouched(r, what) {
  assert.match(r.stdout, /^NEXT$/m, `${what}: zsh must keep running (stdout=${JSON.stringify(r.stdout)} stderr=${JSON.stringify(r.stderr)})`);
  assert.doesNotMatch(r.stdout, /OPTIONS_CHANGED/, `${what}: errexit/nounset leaked into zsh`);
  assert.match(r.stdout, /^PATH_SAME$/m, `${what}: PATH must be unchanged`);
}

test("zsh: an interactive zsh reading ~/.zshrc keeps running, options and PATH untouched, daemon started once", { skip: ZSH_SKIP }, async () => {
  // Control: the 0.3.11 files do end an interactive zsh, so this probe can see the bug.
  const old = makeHome({ legacy: true });
  await withSocket(old, async () => {
    assert.doesNotMatch(zshInteractive(old).stdout, /NEXT/, "0.3.11 files must kill zsh, or the probe proves nothing");
  });
  const home = makeHome();
  await withSocket(home, async () => assertZshUntouched(zshInteractive(home), "socket present"));
  fs.writeFileSync(cfg(home, "profile.d/terminal-commander.sh"), LEGACY_SNIPPET);
  await withSocket(home, async () => assertZshUntouched(zshInteractive(home), "legacy snippet + new autostart.sh"));
  const cold = makeHome({ stubDaemon: true });
  assertZshUntouched(zshInteractive(cold), "socket absent");
  assert.equal((await waitForStarts(cold)).length, 1);
});

test("socket present: the daemon-ensure prefix lets the chained command run", { skip: SKIP }, async () => {
  // BRIDGE_PROBE_CMD is exactly this prefix + `exec terminal-commander-mcp`;
  // the orchestrator's post-install start is DAEMON_START_CMD.
  assert.equal(BRIDGE_PROBE_CMD, `${LINUX_PATH_PREFIX}${BRIDGE_DAEMON_ENSURE}exec terminal-commander-mcp`);
  const home = makeHome();
  await withSocket(home, async () => {
    for (const cmd of [
      `${LINUX_PATH_PREFIX}${BRIDGE_DAEMON_ENSURE}echo NEXT`,
      `${LINUX_PATH_PREFIX}${AUTOSTART_RUN}; echo NEXT`,
      `${DAEMON_START_CMD}; echo NEXT`,
    ]) {
      const r = bash(home, ["--noprofile", "--norc", "-c", cmd]);
      assert.equal(r.stdout, "NEXT\n", `chained command must run and the prefix must print nothing: ${cmd}`);
    }
    // Same through a login shell, the shape spawn.js / the bootstrap use.
    const r = bash(home, ["-lc", `${LINUX_PATH_PREFIX}${BRIDGE_DAEMON_ENSURE}echo NEXT`]);
    assert.match(r.stdout, /^NEXT$/m);
  });
});

test("upgrade on Linux: legacy files + live socket do not hide the daemon binary from the installer", { skip: SKIP }, async () => {
  const home = makeHome({ legacy: true, stubDaemon: true });
  await withSocket(home, async () => {
    const env = shellEnv(home);
    assert.equal(resolveDaemonBinary(env), path.join(home, ".local", "bin", "terminal-commanderd"));
    const r = installDaemonAutostart({
      platform: "linux",
      homeDir: home,
      env,
      systemdUserAvailable: () => false,
      runAutostartOnce: () => ({ ok: true, exit_code: 0 }),
    });
    assert.equal(r.status, "profile_hook", r.hint);
    assert.equal(fs.readFileSync(cfg(home, "profile.d/terminal-commander.sh"), "utf8"), renderProfileSnippet());
    assert.equal(fs.readFileSync(cfg(home, "autostart.sh"), "utf8"), renderAutostartScript());
    assert.match(bash(home, ["-lc", "echo NEXT"]).stdout, /^NEXT$/m);
  });
});

// The Windows-host installer would take its systemd branch (and talk to the
// REAL user manager) when systemd reports "running" and a daemon binary is on
// its PATH. Never let the test reach that.
function installerWouldUseSystemd(home) {
  const r = bash(home, [
    "--noprofile",
    "--norc",
    "-c",
    `${LINUX_PATH_PREFIX}command -v terminal-commanderd >/dev/null 2>&1 && command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] && systemctl is-system-running --quiet 2>/dev/null`,
  ]);
  return r.status === 0;
}

test("upgrade from Windows: the WSL installer rewrites legacy files even with a live socket", { skip: SKIP }, async (t) => {
  const home = makeHome({ legacy: true });
  if (installerWouldUseSystemd(home)) {
    t.skip("installer would enable a real systemd user unit on this host");
    return;
  }
  await withSocket(home, async () => {
    const r = await ensureDaemonAutostartInWsl({
      platform: "win32",
      distro: "Ubuntu",
      env: {},
      wslPath: "wsl.exe",
      // Run what would run inside WSL (argv after `--`) with a real bash.
      exec: ({ argv }) =>
        spawn(argv[3], argv.slice(4), { env: shellEnv(home), stdio: ["ignore", "pipe", "pipe"] }),
      timeoutMs: 20_000,
    });
    assert.equal(r.status, "ok", r.hint);
    assert.equal(fs.readFileSync(cfg(home, "profile.d/terminal-commander.sh"), "utf8"), renderProfileSnippet());
    assert.equal(fs.readFileSync(cfg(home, "autostart.sh"), "utf8"), renderAutostartScript());
  });
});

// Stand-in for wsl.exe: run what would run inside WSL (argv after `--`) with a
// real bash. stdin "ignore" matches wsl.exe's fifo stdin (no ~/.bashrc read).
function wslExec(home) {
  return ({ argv }) =>
    spawn(argv[3], argv.slice(4), { env: shellEnv(home), stdio: ["ignore", "pipe", "pipe"] });
}

// Shell functions shadow the real tools for every `bash -lc` step that gets
// past ~/.profile, and record that the step really ran. They come BEFORE the
// managed autostart block, exactly where a legacy snippet would kill the shell.
const STEP_STUBS = [
  'tc_step() { echo "$*" >> "$HOME/steps.log"; }',
  'npm() { tc_step npm "$@"; }',
  'node() { tc_step node "$@"; }',
  'terminal-commander-mcp() { tc_step terminal-commander-mcp "$@"; echo "terminal-commander-mcp 0.0.0-stale"; }',
  'terminal-commanderd() { tc_step terminal-commanderd "$@"; }',
  "",
].join("\n");

test("one Windows-side setup on an affected machine repairs the snippet AND really runs every login-shell step", { skip: SKIP }, async (t) => {
  const home = makeHome({ legacy: true });
  if (installerWouldUseSystemd(home)) {
    t.skip("installer would enable a real systemd user unit on this host");
    return;
  }
  const profile = path.join(home, ".profile");
  fs.writeFileSync(profile, STEP_STUBS + fs.readFileSync(profile, "utf8"));
  await withSocket(home, async () => {
    const r = await runBootstrap({
      mode: "cli",
      platform: "win32",
      env: { USERPROFILE: home, LOCALAPPDATA: home },
      distro: "Ubuntu",
      acquireLock: false,
      detect: async () => ({ reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" }),
      doctor: async () => ({ status: "runtime_present", distro: "Ubuntu", runtime_present: true }),
      exec: wslExec(home),
      timeoutMs: 30_000,
      ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "skip" }),
      resolveDirectExePath: () => ({ exePath: null, reason: "skip" }),
      writeAllHarnesses: () => [{ id: "cursor", status: "ok" }],
      writeState: () => ({ status: "ok" }),
    });
    const out = (r.lines || []).join("\n");
    assert.equal(r.exit_code, 0, out);
    assert.equal(fs.readFileSync(cfg(home, "profile.d/terminal-commander.sh"), "utf8"), renderProfileSnippet());
    assert.equal(fs.readFileSync(cfg(home, "autostart.sh"), "utf8"), renderAutostartScript());
    const log = path.join(home, "steps.log");
    const steps = fs.existsSync(log) ? fs.readFileSync(log, "utf8") : "";
    for (const step of [
      /^terminal-commander-mcp --version$/m, // version probe
      /^npm install -g terminal-commander$/m, // runtime upgrade
      /^node -e /m, // runtime verify
      /^terminal-commanderd update --force$/m, // live daemon swap
    ]) {
      assert.match(steps, step, `login-shell step did not run; steps.log=${JSON.stringify(steps)}\n${out}`);
    }
    assert.match(out, /WSL runtime installed and verified/);
    assert.match(out, /live WSL daemon swapped/);
  });
});

test("a startup file that exits 0 makes every login-shell step fail with the cause, not succeed", { skip: SKIP }, async () => {
  const home = makeHome();
  fs.writeFileSync(path.join(home, ".profile"), "exit 0\n");
  const exec = wslExec(home);

  // Shared helper behind the runtime install/verify/version/swap/start steps.
  const ensure = await ensureWslRuntime({ platform: "win32", distro: "Ubuntu", env: {}, exec, timeoutMs: 20_000 });
  assert.equal(ensure.status, "shell_exited_early");
  assert.match(ensure.hint, /startup file/);

  const restart = await runRestart({
    platform: "win32",
    env: { TC_WSL_DISTRO: "Ubuntu" },
    flags: {},
    detect: async () => ({ reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" }),
    exec: ({ argv }) => exec({ argv }),
  });
  assert.equal(restart.status, "shell_exited_early");
  assert.notEqual(restart.exit_code, 0);

  const doc = await wslDoctor({
    distro: "Ubuntu",
    platform: "win32",
    probeRuntime: true,
    detectResult: { reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" },
    exec: async ({ argv }) => {
      const p = spawnSync(argv[3], argv.slice(4), { env: shellEnv(home), stdio: ["ignore", "pipe", "pipe"] });
      return { status: p.status, signal: p.signal, stdout: p.stdout, stderr: p.stderr, error: null };
    },
  });
  assert.equal(doc.status, "shell_exited_early");

  // doctor daemon spawns wslPath directly: a fake wsl.exe that drops `-d X --`.
  const fakeWsl = path.join(home, "fake-wsl");
  fs.writeFileSync(fakeWsl, '#!/bin/sh\nshift 3\nexec "$@"\n', { mode: 0o755 });
  const daemon = await runDoctorDaemon({
    platform: "win32",
    env: shellEnv(home),
    flags: { distro: "Ubuntu" },
    detect: async () => ({ reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" }),
    wslPath: fakeWsl,
  });
  assert.equal(daemon.status, "probe_failed");
  assert.notEqual(daemon.exit_code, 0);
  assert.match(daemon.output, /daemon_running: unknown/);
  assert.match(daemon.output, /startup file/);
});
