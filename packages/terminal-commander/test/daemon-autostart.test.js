// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const {
  applyManagedBlock,
  hasManagedBlock,
  extractManagedBlock,
} = require("../lib/daemon/managed_block.js");
const {
  shouldInstallDaemonAutostart,
  renderAutostartScript,
  renderSystemdUnit,
  buildWslInstallCommand,
} = require("../lib/daemon/autostart.js");

test("shouldInstallDaemonAutostart defaults on", () => {
  assert.equal(shouldInstallDaemonAutostart({}), true);
  assert.equal(shouldInstallDaemonAutostart({ TC_SKIP_DAEMON_AUTOSTART: "1" }), false);
  assert.equal(shouldInstallDaemonAutostart({ TC_BOOTSTRAP_START_DAEMON: "0" }), false);
});

test("renderAutostartScript checks the pidfile, not a socket file, before start", () => {
  const s = renderAutostartScript();
  // A dead daemon leaves its socket file; only a live pid means "running".
  assert.match(s, /terminal-commanderd\.pid/);
  assert.match(s, /\/proc\/\$TC_PID\/cmdline/);
  assert.doesNotMatch(s, /\[ -S /);
  assert.match(s, /start --mode ipc-server/);
  assert.match(s, /nohup/);
});

test("renderAutostartScript starts the daemon in its own session when setsid exists", () => {
  const s = renderAutostartScript();
  // A terminal's hangup must not reach the daemon: nohup alone does not
  // survive the npm shim. nohup stays as the fallback.
  assert.match(s, /command -v setsid/);
  assert.match(s, /\$TC_SETSID nohup terminal-commanderd /);
  assert.match(s, /<\/dev\/null >>"\$HOME\/\.local\/state\/terminal-commander\/daemon\.log" 2>&1 &/);
});

test("renderSystemdUnit uses ipc-server mode", () => {
  const u = renderSystemdUnit("/usr/bin/terminal-commanderd");
  assert.match(u, /ExecStart=\/usr\/bin\/terminal-commanderd/);
  assert.match(u, /ipc-server/);
});

test("buildWslInstallCommand uses base64 pipe", () => {
  const cmd = buildWslInstallCommand();
  assert.match(cmd, /base64/);
  assert.match(cmd, /npm-global\/bin/);
});

test("applyManagedBlock is idempotent", () => {
  const first = applyManagedBlock("", "autostart", "echo hi");
  assert.ok(hasManagedBlock(first, "autostart"));
  const second = applyManagedBlock(first, "autostart", "echo hi");
  assert.equal(first, second);
  assert.equal(extractManagedBlock(second, "autostart"), "echo hi");
});

test("Linux install inside a Terminal Commander session installs the hook but does not start the daemon, and says so", () => {
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const { installDaemonAutostart } = require("../lib/daemon/autostart.js");
  for (const extra of [{}, { TC_SESSION: "harness-a" }, { TC_SOCKET: "/tmp/x.sock" }, { TC_DAEMON_CHILD: "1" }]) {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "tc-install-"));
    let ran = 0;
    const r = installDaemonAutostart({
      platform: "linux",
      homeDir: home,
      env: { HOME: home, ...extra },
      daemonBinary: "/fake/terminal-commanderd",
      systemdUserAvailable: () => false,
      runAutostartOnce: () => {
        ran += 1;
        return { ok: true, exit_code: 0 };
      },
    });
    assert.equal(r.status, "profile_hook");
    const name = Object.keys(extra)[0];
    if (!name) {
      assert.equal(ran, 1);
      assert.equal(r.daemon_start, undefined);
      continue;
    }
    assert.equal(ran, 0, `${name}: must not run autostart.sh`);
    assert.equal(r.daemon_start.status, "skipped");
    assert.match(r.daemon_start.reason, new RegExp(`inside a Terminal Commander session \\(${name} is set\\)`));
    assert.match(r.hint, /daemon not started: running inside a Terminal Commander session/);
    assert.doesNotMatch(r.hint, /WARNING/);
  }
});

test("Windows setup daemon-autostart inside a Terminal Commander session says the daemon was not started", async () => {
  const { runSetupDaemonAutostart } = require("../lib/cli/setup_daemon_autostart.js");
  const run = (env) =>
    runSetupDaemonAutostart({
      platform: "win32",
      env,
      flags: { distro: "Ubuntu" },
      detect: async () => ({ reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" }),
      ensureDaemonAutostartInWsl: async () => ({ status: "ok", hint: "daemon autostart installed in WSL", warnings: [] }),
    });
  const plain = await run({});
  assert.equal(plain.status, "ok");
  assert.doesNotMatch(plain.output, /not started/);
  const inside = await run({ TC_SESSION: "harness-a" });
  assert.equal(inside.status, "ok");
  assert.equal(inside.exit_code, 0);
  assert.equal(inside.daemon_start.status, "skipped");
  assert.match(inside.output, /daemon not started: running inside a Terminal Commander session \(TC_SESSION is set\)/);
});
