// SPDX-License-Identifier: Apache-2.0

"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const {
  runBootstrap,
  shouldSkipBootstrap,
  isGlobalNpmInstall,
} = require("../lib/bootstrap/orchestrator.js");

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
