// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const os = require("node:os");
const fs = require("node:fs");
const path = require("node:path");
const {
  runBootstrap,
  shouldSkipBootstrap,
  isGlobalNpmInstall,
} = require("../lib/bootstrap/orchestrator.js");
const { runSetupHarness } = require("../lib/cli/setup_harness.js");

test("shouldSkipBootstrap respects TC_SKIP_BOOTSTRAP", () => {
  assert.equal(shouldSkipBootstrap({ TC_SKIP_BOOTSTRAP: "1" }), true);
  assert.equal(shouldSkipBootstrap({}), false);
});

test("isGlobalNpmInstall detects npm_config_global", () => {
  assert.equal(isGlobalNpmInstall({ npm_config_global: "true" }), true);
  assert.equal(isGlobalNpmInstall({}), false);
});

test("runBootstrap install mode auto-configures harnesses (force)", async () => {
  const wrote = [];
  const r = await runBootstrap({
    mode: "install",
    platform: "linux",
    env: {
      npm_lifecycle_event: "install",
      npm_lifecycle_script: "terminal-commander setup harness",
      HOME: process.env.HOME || process.env.USERPROFILE || "/tmp",
    },
    acquireLock: false,
    require_install_lifecycle: false,
    writeAllHarnesses: (opts) => {
      wrote.push(opts.force);
      return [];
    },
    skipDaemonAutostart: true,
  });
  assert.equal(r.exit_code, 0);
  assert.equal(wrote.length, 1);
  assert.equal(wrote[0], true);
});

test("runBootstrap skips on TC_SKIP_BOOTSTRAP", async () => {
  const r = await runBootstrap({
    mode: "install",
    platform: "win32",
    env: {
      TC_SKIP_BOOTSTRAP: "1",
      npm_lifecycle_event: "install",
    },
    acquireLock: false,
  });
  assert.equal(r.status, "bootstrap_skipped");
  assert.equal(r.exit_code, 0);
});

test("runBootstrap win32 defaults to native MCP path without WSL probe", async () => {
  let detectCalled = false;
  const writes = [];
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example" },
    dry_run: true,
    acquireLock: false,
    detect: async () => {
      detectCalled = true;
      throw new Error("WSL probe must not run for native default");
    },
    writeAllHarnesses: (opts) => {
      writes.push(opts);
      return [{ id: "cursor", status: "ok" }];
    },
  });
  assert.equal(r.exit_code, 0);
  assert.equal(detectCalled, false);
  assert.equal(writes.length, 1);
  assert.equal(writes[0].distro, null);
  assert.match(r.output, /native Windows MCP path selected/);
});

test("runBootstrap threads the stable exe path into writeAllHarnesses (direct-exe config)", async () => {
  const writes = [];
  let ensureOpts = null;
  const STABLE =
    "C:\\Users\\example\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe";
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\Users\\example\\AppData\\Local" },
    force: true,
    acquireLock: false,
    ensureStableBinaries: (opts) => {
      ensureOpts = opts;
      return { exePath: STABLE, copied: [STABLE], reason: "ok" };
    },
    writeAllHarnesses: (opts) => {
      writes.push(opts);
      return [{ id: "cursor", status: "ok" }];
    },
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(writes.length, 1);
  assert.equal(writes[0].exePath, STABLE, "harness writers receive the stable exe path");
  assert.equal(ensureOpts.platform, "win32");
  assert.equal(ensureOpts.dry_run, false, "a real bootstrap copies (not dry-run)");
  assert.match(r.output, /stable exe/);
});

test("runBootstrap dry-run resolves the stable exe path WITHOUT copying", async () => {
  let ensureOpts = null;
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\L" },
    dry_run: true,
    acquireLock: false,
    ensureStableBinaries: (opts) => {
      ensureOpts = opts;
      return { exePath: null, copied: [], reason: "dry_run" };
    },
    writeAllHarnesses: () => [{ id: "cursor", status: "ok" }],
  });
  assert.equal(r.exit_code, 0);
  assert.equal(ensureOpts.dry_run, true, "dry-run must not copy binaries");
});

test("runBootstrap resolves the absolute node_modules exe when the stable copy fails", async () => {
  // Fix 2: a failed stable copy must NOT fall through to the bare PATH-dependent
  // command. The orchestrator resolves the absolute node_modules exe instead
  // (the known-good path the live codex entry uses) and threads it to the writers.
  const DIRECT =
    "C:\\Users\\example\\node_modules\\@terminal-commander\\windows-x64\\bin\\terminal-commander-mcp.exe";
  const writes = [];
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\L" },
    force: true,
    acquireLock: false,
    ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "copy_failed" }),
    resolveDirectExePath: () => ({ exePath: DIRECT, reason: "ok" }),
    writeAllHarnesses: (opts) => {
      writes.push(opts);
      return [{ id: "cursor", status: "ok" }];
    },
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(writes.length, 1);
  assert.equal(writes[0].exePath, DIRECT, "writer receives the absolute node_modules exe, not bare");
  assert.match(r.output, /absolute node_modules exe/);
});

test("runBootstrap falls back to bare command only when NO absolute path resolves (loud warning)", async () => {
  // Last resort: stable copy failed AND the only resolvable binary is transient
  // (temp/npx cache). The writer's exePath is left unset (bare-name fallback),
  // and a loud warning is emitted naming the upgrade path.
  const writes = [];
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\L" },
    force: true,
    acquireLock: false,
    ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "copy_failed" }),
    resolveDirectExePath: () => ({ exePath: null, reason: "transient_path" }),
    writeAllHarnesses: (opts) => {
      writes.push(opts);
      return [{ id: "cursor", status: "ok" }];
    },
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(writes.length, 1);
  assert.equal(writes[0].exePath, undefined, "no absolute path -> writer uses the bare-name fallback");
  assert.match(r.output, /WARNING could not resolve an absolute MCP binary path/);
});

test("runBootstrap cli mode fails loudly when requested harness write fails", async () => {
  const r = await runBootstrap({
    mode: "cli",
    platform: "linux",
    env: { HOME: process.env.HOME || process.env.USERPROFILE || "/tmp" },
    force: true,
    acquireLock: false,
    skipDaemonAutostart: true,
    writeAllHarnesses: () => [
      {
        id: "codex-cli",
        status: "failed",
        harness_status: "backup_failed",
        path: "/tmp/config.toml",
        hint: "terminal-commander: codex config.toml backup failed",
      },
    ],
  });
  assert.equal(r.status, "harness_failed");
  assert.equal(r.exit_code, 64);
  assert.match(r.output, /codex config\.toml backup failed/);
  assert.equal(r.harness_results[0].harness_status, "backup_failed");
});

test("runBootstrap returns diagnostics without writing stderr by default", async () => {
  const writes = [];
  const originalWrite = process.stderr.write;
  process.stderr.write = function write(chunk, ...args) {
    writes.push(String(chunk));
    if (typeof args[args.length - 1] === "function") {
      args[args.length - 1]();
    }
    return true;
  };
  try {
    const r = await runBootstrap({
      mode: "cli",
      platform: "linux",
      env: { HOME: process.env.HOME || process.env.USERPROFILE || "/tmp" },
      force: true,
      acquireLock: false,
      skipDaemonAutostart: true,
      // Pin the exe resolution so the orchestrator emits exactly the harness
      // diagnostic under test (no environment-dependent stable/direct-exe line).
      ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "copy_failed" }),
      resolveDirectExePath: () => ({ exePath: null, reason: "resolve_failed" }),
      writeAllHarnesses: () => [
        {
          id: "codex-cli",
          status: "failed",
          harness_status: "backup_failed",
          hint: "terminal-commander: codex config.toml backup failed",
        },
      ],
    });
    assert.equal(r.status, "harness_failed");
    assert.match(r.output, /codex config\.toml backup failed/);
    assert.deepEqual(writes, []);
  } finally {
    process.stderr.write = originalWrite;
  }
});

test("runBootstrap writes stderr only when emitOutput is explicit", async () => {
  const writes = [];
  const originalWrite = process.stderr.write;
  process.stderr.write = function write(chunk, ...args) {
    writes.push(String(chunk));
    if (typeof args[args.length - 1] === "function") {
      args[args.length - 1]();
    }
    return true;
  };
  try {
    const DIRECT =
      "/home/example/node_modules/@terminal-commander/linux-x64/bin/terminal-commander-mcp";
    const r = await runBootstrap({
      mode: "cli",
      platform: "linux",
      env: { HOME: process.env.HOME || process.env.USERPROFILE || "/tmp" },
      force: true,
      acquireLock: false,
      skipDaemonAutostart: true,
      emitOutput: true,
      // Pin the exe resolution to the absolute-node_modules fallback so the
      // emitted lines are deterministic across environments.
      ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "copy_failed" }),
      resolveDirectExePath: () => ({ exePath: DIRECT, reason: "ok" }),
      writeAllHarnesses: () => [
        {
          id: "codex-cli",
          status: "failed",
          harness_status: "backup_failed",
          hint: "terminal-commander: codex config.toml backup failed",
        },
      ],
    });
    assert.equal(r.status, "harness_failed");
    assert.deepEqual(writes, [
      `terminal-commander: stable exe copy unavailable (copy_failed); harness configs point at the absolute node_modules exe ${DIRECT}\n`,
      "terminal-commander: codex config.toml backup failed\n",
    ]);
  } finally {
    process.stderr.write = originalWrite;
  }
});

test("runBootstrap install mode keeps provider write failures fail-soft", async () => {
  const r = await runBootstrap({
    mode: "install",
    platform: "linux",
    env: {
      npm_lifecycle_event: "install",
      npm_lifecycle_script: "terminal-commander setup harness",
      HOME: process.env.HOME || process.env.USERPROFILE || "/tmp",
    },
    acquireLock: false,
    require_install_lifecycle: false,
    skipDaemonAutostart: true,
    writeAllHarnesses: () => [
      {
        id: "codex-cli",
        status: "failed",
        harness_status: "backup_failed",
        hint: "terminal-commander: codex config.toml backup failed",
      },
    ],
  });
  assert.equal(r.status, "bootstrap_partial");
  assert.equal(r.exit_code, 0);
  assert.match(r.output, /codex config\.toml backup failed/);
  assert.equal(r.harness_results[0].status, "failed");
});

test("runBootstrap linux writes harness without WSL", async () => {
  if (process.platform === "win32") return;
  const r = await runBootstrap({
    mode: "cli",
    platform: "linux",
    env: process.env,
    dry_run: true,
    acquireLock: false,
  });
  assert.equal(r.exit_code, 0);
  assert.ok(Array.isArray(r.harness_results));
});

test("runBootstrap print_config runs harness no-write and does not persist state", async () => {
  let stateWrites = 0;
  let harnessDryRun = "unset";
  const r = await runBootstrap({
    mode: "cli",
    platform: "linux",
    env: {
      HOME: process.env.HOME || process.env.USERPROFILE || os.tmpdir(),
      TC_SKIP_DAEMON_AUTOSTART: "1",
    },
    print_config: true,
    acquireLock: false,
    writeAllHarnesses: (opts) => {
      harnessDryRun = opts.dry_run;
      return [
        { id: "cursor", status: "ok", dry_run: true, stanza: { type: "stdio", command: "node" } },
      ];
    },
    writeState: () => {
      stateWrites += 1;
      return { status: "ok" };
    },
  });
  assert.equal(r.exit_code, 0);
  assert.equal(stateWrites, 0, "print_config must not persist setup.json");
  assert.equal(harnessDryRun, true, "print_config must run harness writers in no-write mode");
});

// Minimal fake WSL child for runWslBashLc's injected exec ({wslPath,argv,env}).
function fakeWslChild(code, stdout) {
  const { EventEmitter } = require("node:events");
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.kill = () => {};
  process.nextTick(() => {
    if (stdout) child.stdout.emit("data", Buffer.from(String(stdout)));
    child.emit("close", code);
  });
  return child;
}

const HOST_VERSION = require("../package.json").version;

test("runBootstrap upgrades WSL runtime AND swaps live daemon on version skew", async () => {
  // Defect A: presence-only gate never upgrades a stale WSL runtime. With the
  // runtime present but at a different version than the host package, the gate
  // must (a) run ensureWslRuntime to upgrade and (b) dispatch the live-daemon
  // swap (`terminal-commanderd update --force`) exactly once.
  const execCmds = [];
  let ensureCalled = false;
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: os.tmpdir() },
    distro: "Ubuntu",
    acquireLock: false,
    skipDaemonAutostart: true,
    detect: async () => ({ reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" }),
    doctor: async () => ({ status: "runtime_present", distro: "Ubuntu", runtime_present: true }),
    probeRuntimeVersion: async () => "0.0.0-stale",
    ensureWslRuntime: async () => {
      ensureCalled = true;
      return { status: "ok", hint: "installed" };
    },
    exec: ({ argv }) => {
      execCmds.push(argv[argv.length - 1]);
      return fakeWslChild(0, "");
    },
    ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "skip" }),
    resolveDirectExePath: () => ({ exePath: null, reason: "skip" }),
    writeAllHarnesses: () => [{ id: "cursor", status: "ok" }],
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(ensureCalled, true, "stale runtime must trigger ensureWslRuntime (upgrade)");
  assert.ok(
    execCmds.some((c) => c.includes("terminal-commanderd update --force")),
    "live daemon swap must be dispatched on skew",
  );
  assert.match(r.output, /!= host/);
});

test("runBootstrap matched runtime skips install and daemon swap (fast path)", async () => {
  // Regression: when the WSL runtime version matches the host, the gate keeps
  // the "already present" optimization — no npm install, no daemon restart.
  const execCmds = [];
  let ensureCalled = false;
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: os.tmpdir() },
    distro: "Ubuntu",
    acquireLock: false,
    skipDaemonAutostart: true,
    detect: async () => ({ reason: "ok", distros: [{ name: "Ubuntu" }], default_distro: "Ubuntu" }),
    doctor: async () => ({ status: "runtime_present", distro: "Ubuntu", runtime_present: true }),
    probeRuntimeVersion: async () => HOST_VERSION,
    ensureWslRuntime: async () => {
      ensureCalled = true;
      return { status: "ok", hint: "installed" };
    },
    exec: ({ argv }) => {
      execCmds.push(argv[argv.length - 1]);
      return fakeWslChild(0, "");
    },
    ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "skip" }),
    resolveDirectExePath: () => ({ exePath: null, reason: "skip" }),
    writeAllHarnesses: () => [{ id: "cursor", status: "ok" }],
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(ensureCalled, false, "matched runtime must NOT reinstall");
  assert.equal(execCmds.length, 0, "matched runtime must NOT dispatch a daemon swap");
  assert.match(r.output, /already present/);
});

test("setup harness --print-config forwards the flag and does not persist state", async () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "tc-printcfg-"));
  let stateWrites = 0;
  try {
    await runSetupHarness({
      platform: "linux",
      env: { HOME: home, USERPROFILE: home, LOCALAPPDATA: home, TC_SKIP_DAEMON_AUTOSTART: "1" },
      flags: { "print-config": true },
      writeAllHarnesses: () => [
        { id: "cursor", status: "ok", dry_run: true, stanza: { type: "stdio" } },
      ],
      writeState: () => {
        stateWrites += 1;
        return { status: "ok" };
      },
    });
    assert.equal(stateWrites, 0, "setup harness --print-config must not write setup.json");
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});
