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

test("runBootstrap falls back gracefully when the stable copy fails (exePath undefined)", async () => {
  const writes = [];
  const r = await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\L" },
    force: true,
    acquireLock: false,
    ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "copy_failed" }),
    writeAllHarnesses: (opts) => {
      writes.push(opts);
      return [{ id: "cursor", status: "ok" }];
    },
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(r.exit_code, 0);
  assert.equal(writes.length, 1);
  assert.equal(writes[0].exePath, undefined, "a failed copy leaves exePath unset so the writer uses the bare-name fallback");
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
    const r = await runBootstrap({
      mode: "cli",
      platform: "linux",
      env: { HOME: process.env.HOME || process.env.USERPROFILE || "/tmp" },
      force: true,
      acquireLock: false,
      skipDaemonAutostart: true,
      emitOutput: true,
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
