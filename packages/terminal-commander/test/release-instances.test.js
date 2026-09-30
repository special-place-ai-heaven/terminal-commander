// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// An upgrade must never be blocked or left half-replaced by running Terminal
// Commander processes. npm has no hook before it swaps package files, so the
// install lifecycle stops every instance of this install and removes npm's
// leftover folders, which otherwise hold the old binaries and make the NEXT
// upgrade collide with them (EBUSY, then a rollback that restores old files).

"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  releaseRunningInstances,
  removeNpmLeftovers,
} = require("../lib/bootstrap/release_instances.js");
const { runBootstrap } = require("../lib/bootstrap/orchestrator.js");

function fakeGlobalRoot() {
  const scope = fs.mkdtempSync(path.join(os.tmpdir(), "tc-scope-"));
  const pkgRoot = path.join(scope, "terminal-commander");
  fs.mkdirSync(pkgRoot);
  fs.writeFileSync(path.join(pkgRoot, "package.json"), JSON.stringify({ name: "terminal-commander" }));
  const mk = (name, pkgName) => {
    const d = path.join(scope, name);
    fs.mkdirSync(path.join(d, "bin"), { recursive: true });
    if (pkgName) fs.writeFileSync(path.join(d, "package.json"), JSON.stringify({ name: pkgName }));
    return d;
  };
  return {
    scope,
    pkgRoot,
    leftover: mk(".terminal-commander-gmG5XvLR", "terminal-commander"),
    noManifest: mk(".terminal-commander-Abc12345", null),
    otherPackage: mk(".other-tool-Zz9Yy8Xx", "other-tool"),
    lookalike: mk(".terminal-commander-Q1w2E3r4", "not-terminal-commander"),
  };
}

test("removeNpmLeftovers deletes only terminal-commander leftovers next to the package", () => {
  const f = fakeGlobalRoot();
  const r = removeNpmLeftovers(f.scope);
  assert.deepEqual(r.removed, [f.leftover]);
  assert.deepEqual(r.failed, []);
  assert.equal(fs.existsSync(f.leftover), false);
  assert.equal(fs.existsSync(f.pkgRoot), true, "the installed package is kept");
  assert.equal(fs.existsSync(f.noManifest), true, "a folder without a manifest is not ours to delete");
  assert.equal(fs.existsSync(f.otherPackage), true);
  assert.equal(fs.existsSync(f.lookalike), true);
  fs.rmSync(f.scope, { recursive: true, force: true });
});

test("releaseRunningInstances on Windows stops this install's processes, then clears leftovers", async () => {
  const f = fakeGlobalRoot();
  const calls = [];
  const r = await releaseRunningInstances({
    platform: "win32",
    env: { LOCALAPPDATA: "C:\\Users\\op\\AppData\\Local" },
    packageRoot: f.pkgRoot,
    runUpdatePreflight: async (opts) => {
      calls.push(opts);
      // Processes are stopped BEFORE the leftover is removed: a running old
      // binary is exactly what keeps a leftover folder from being deleted.
      assert.equal(fs.existsSync(f.leftover), true);
      return 0;
    },
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].platform, "win32");
  assert.equal(calls[0].packageRoot, f.pkgRoot);
  assert.equal(r.code, 0);
  assert.equal(fs.existsSync(f.leftover), false);
  assert.ok(r.lines.some((l) => /stopped running Terminal Commander processes/.test(l)), r.lines.join("\n"));
  assert.ok(r.lines.some((l) => l.includes(f.leftover)), r.lines.join("\n"));
  fs.rmSync(f.scope, { recursive: true, force: true });
});

test("releaseRunningInstances keeps going when stopping processes fails", async () => {
  const f = fakeGlobalRoot();
  const r = await releaseRunningInstances({
    platform: "win32",
    env: {},
    packageRoot: f.pkgRoot,
    runUpdatePreflight: async () => 1,
  });
  assert.equal(r.code, 1);
  assert.ok(r.lines.some((l) => /WARNING/.test(l)), r.lines.join("\n"));
  fs.rmSync(f.scope, { recursive: true, force: true });
});

test("releaseRunningInstances does nothing off Windows (replacing files there never blocks)", async () => {
  let called = false;
  const r = await releaseRunningInstances({
    platform: "linux",
    env: {},
    runUpdatePreflight: async () => {
      called = true;
      return 0;
    },
  });
  assert.equal(called, false);
  assert.deepEqual(r.lines, []);
});

test("install bootstrap on Windows releases running instances before refreshing the stable copy", async () => {
  const order = [];
  const r = await runBootstrap({
    mode: "install",
    platform: "win32",
    env: {
      npm_lifecycle_event: "postinstall",
      npm_lifecycle_script: "node scripts/postinstall.js",
      USERPROFILE: "C:\\Users\\example",
      LOCALAPPDATA: "C:\\Users\\example\\AppData\\Local",
    },
    acquireLock: false,
    releaseRunningInstances: async () => {
      order.push("release");
      return { lines: ["terminal-commander: stopped running Terminal Commander processes before replacing binaries"] };
    },
    ensureStableBinaries: () => {
      order.push("stable-copy");
      return { exePath: "C:\\stable\\terminal-commander-mcp.exe", copied: [], reason: "ok" };
    },
    writeAllHarnesses: () => {
      order.push("harness");
      return [];
    },
    writeState: () => ({ status: "ok" }),
    skipDaemonAutostart: true,
  });
  assert.equal(r.exit_code, 0);
  assert.deepEqual(order, ["release", "stable-copy", "harness"]);
});

test("setup (cli mode) never stops running instances", async () => {
  let released = false;
  await runBootstrap({
    mode: "cli",
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\example", LOCALAPPDATA: "C:\\Users\\example\\AppData\\Local" },
    force: true,
    acquireLock: false,
    releaseRunningInstances: async () => {
      released = true;
      return { lines: [] };
    },
    ensureStableBinaries: () => ({ exePath: null, copied: [], reason: "skip" }),
    writeAllHarnesses: () => [],
    writeState: () => ({ status: "ok" }),
  });
  assert.equal(released, false);
});
