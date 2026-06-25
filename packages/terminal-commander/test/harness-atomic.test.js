// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Shared atomic-write-with-backup helper (lib/harness/io/atomic.js) +
// parity proof that the Codex/TOML writer now fsyncs and scope-checks like the
// JSON and Cursor writers (it previously hand-rolled a weaker write).

"use strict";

const { test, mock } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  atomicWrite,
  backupExisting,
  atomicWriteWithBackup,
  ATOMIC_REASONS,
} = require("../lib/harness/io/atomic.js");
const { writeCodexTomlConfig } = require("../lib/harness/io/toml_mcp.js");

function tmpDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "tc-atomic-"));
}

test("atomicWriteWithBackup creates the file and removes the tmp sibling", () => {
  const target = path.join(tmpDir(), "out.json");
  const r = atomicWriteWithBackup(target, "hello", { randomSuffix: () => "fixed" });
  assert.equal(r.ok, true);
  assert.equal(r.path, target);
  assert.equal(r.backup_path, null); // nothing to back up
  assert.equal(fs.readFileSync(target, "utf8"), "hello");
  assert.equal(fs.existsSync(target + ".tmp.fixed"), false);
});

test("atomicWriteWithBackup backs up an existing target to a timestamped .bak", () => {
  const target = path.join(tmpDir(), "out.json");
  fs.writeFileSync(target, "old");
  const r = atomicWriteWithBackup(target, "new", { timestamp: () => "20260625T140530123Z" });
  assert.equal(r.ok, true);
  assert.equal(r.backup_path, `${target}.20260625T140530123Z.bak`);
  assert.equal(fs.readFileSync(r.backup_path, "utf8"), "old");
  assert.equal(fs.readFileSync(target, "utf8"), "new");
});

test("atomicWriteWithBackup never collides on re-run (timestamped backups, no backup_failed)", () => {
  // Timestamped backups are unique per call, so a re-run with an existing
  // backup beside the target no longer fails with backup_failed (the brokenness
  // the old non-timestamped .bak model caused). Two applies => two backups.
  const target = path.join(tmpDir(), "out.json");
  fs.writeFileSync(target, "v1");
  const first = atomicWriteWithBackup(target, "v2", { timestamp: () => "20260625T000000001Z" });
  assert.equal(first.ok, true);
  assert.equal(fs.readFileSync(first.backup_path, "utf8"), "v1");
  const second = atomicWriteWithBackup(target, "v3", { timestamp: () => "20260625T000000002Z" });
  assert.equal(second.ok, true, "re-run must NOT fail on a pre-existing backup");
  assert.equal(fs.readFileSync(second.backup_path, "utf8"), "v2");
  assert.equal(fs.readFileSync(target, "utf8"), "v3");
  assert.notEqual(first.backup_path, second.backup_path);
});

test("atomicWriteWithBackup refuses a target outside scopeDir (path_not_allowed)", () => {
  const scope = tmpDir();
  const other = tmpDir();
  const target = path.join(other, "evil.json");
  const r = atomicWriteWithBackup(target, "x", { scopeDir: scope });
  assert.equal(r.ok, false);
  assert.equal(r.reason, ATOMIC_REASONS.PATH_NOT_ALLOWED);
  assert.equal(fs.existsSync(target), false);
});

test("atomicWrite refuses a tmp path that would escape the scope dir", () => {
  // Direct primitive: a target inside scope is fine; outside is refused.
  const scope = tmpDir();
  const inside = atomicWrite(path.join(scope, "ok.json"), "y", {});
  assert.equal(inside.ok, true);
  const outside = atomicWrite(path.join(tmpDir(), "x.json"), "y", { scopeDir: scope });
  assert.equal(outside.ok, false);
  assert.equal(outside.reason, ATOMIC_REASONS.PATH_NOT_ALLOWED);
});

test("backupExisting is a no-op when the target does not exist", () => {
  const r = backupExisting(path.join(tmpDir(), "missing.json"));
  assert.equal(r.ok, true);
  assert.equal(r.backup_path, null);
});

// --- Parity proof: the Codex/TOML writer now fsyncs (it previously did NOT) ---
test("writeCodexTomlConfig fsyncs the tmp file before rename (durability parity)", () => {
  const target = path.join(tmpDir(), "config.toml");
  const spy = mock.method(fs, "fsyncSync");
  try {
    const r = writeCodexTomlConfig({
      path: target,
      sessionToken: "tc-abc123",
      surface: "compact",
    });
    assert.equal(r.status, "config_created");
    assert.ok(
      spy.mock.calls.length >= 1,
      "writeCodexTomlConfig must fsync the tmp file via the shared atomic helper",
    );
    // And it really wrote the config.
    assert.match(fs.readFileSync(target, "utf8"), /\[mcp_servers\.terminal_commander\]/);
  } finally {
    spy.mock.restore();
  }
});
