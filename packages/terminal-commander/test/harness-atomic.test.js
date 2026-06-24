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

test("atomicWriteWithBackup backs up an existing target to .bak", () => {
  const target = path.join(tmpDir(), "out.json");
  fs.writeFileSync(target, "old");
  const r = atomicWriteWithBackup(target, "new", {});
  assert.equal(r.ok, true);
  assert.equal(r.backup_path, target + ".bak");
  assert.equal(fs.readFileSync(target + ".bak", "utf8"), "old");
  assert.equal(fs.readFileSync(target, "utf8"), "new");
});

test("atomicWriteWithBackup refuses an existing .bak unless clobber_backup", () => {
  const target = path.join(tmpDir(), "out.json");
  fs.writeFileSync(target, "v2");
  fs.writeFileSync(target + ".bak", "old-bak");
  const refused = atomicWriteWithBackup(target, "v3", {});
  assert.equal(refused.ok, false);
  assert.equal(refused.reason, ATOMIC_REASONS.BACKUP_FAILED);
  assert.equal(fs.readFileSync(target + ".bak", "utf8"), "old-bak"); // untouched
  assert.equal(fs.readFileSync(target, "utf8"), "v2"); // target NOT overwritten on backup failure
  const ok = atomicWriteWithBackup(target, "v3", { clobber_backup: true });
  assert.equal(ok.ok, true);
  assert.equal(fs.readFileSync(target + ".bak", "utf8"), "v2");
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
