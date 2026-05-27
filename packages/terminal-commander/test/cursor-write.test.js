// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS05 Cursor mcp.json writer tests. All file I/O is contained to
// per-test temp directories under `os.tmpdir()`; the operator's real
// Cursor config is never touched.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

const {
  writeCursorMcpConfig,
  backupCursorConfig,
  atomicWrite,
} = require("../lib/cursor/write.js");
const {
  parseExistingCursorConfig,
  serializeCursorMcpConfig,
  MAX_CONFIG_BYTES,
} = require("../lib/cursor/config.js");

function mkScope() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "wws05-cursor-"));
  // tmp is the "project root" for project-scoped writes.
  return tmp;
}

function rmScope(p) {
  try {
    fs.rmSync(p, { recursive: true, force: true });
  } catch (_e) {
    /* ignore */
  }
}

function assertTerminalCommanderStanza(stanza) {
  assert.equal(stanza.type, "stdio");
  // Portable default: the installed `terminal-commander-mcp` bin on
  // PATH, NOT this machine's absolute node.exe + checkout script path
  // (which would leak a private \Users\ path into the generated config;
  // see the no-private-path guard in cursor-config.test.js).
  assert.equal(stanza.command, "terminal-commander-mcp");
  assert.deepEqual(stanza.args, []);
}

test("project scope: creates config when file missing (config_created)", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
    assert.equal(r.status, "config_created");
    assert.equal(r.path, path.join(root, ".cursor", "mcp.json"));
    assert.equal(r.backup_path, null);
    assert.equal(r.was_present, false);
    // Verify file contents.
    const data = JSON.parse(fs.readFileSync(r.path, "utf8"));
    assertTerminalCommanderStanza(data.mcpServers["terminal-commander"]);
    assert.equal("env" in data.mcpServers["terminal-commander"], false);
  } finally {
    rmScope(root);
  }
});

test("project scope: creates .cursor/ parent directory if missing", () => {
  const root = mkScope();
  try {
    assert.equal(fs.existsSync(path.join(root, ".cursor")), false);
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
    assert.equal(r.status, "config_created");
    assert.equal(fs.existsSync(path.join(root, ".cursor")), true);
    assert.equal(fs.existsSync(r.path), true);
  } finally {
    rmScope(root);
  }
});

test("project scope with safe distro emits env.TC_WSL_DISTRO and only that env key", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({
      scope: "project",
      projectRoot: root,
      distro: "Ubuntu-24.04",
    });
    assert.equal(r.status, "config_created");
    const data = JSON.parse(fs.readFileSync(r.path, "utf8"));
    assert.deepEqual(data.mcpServers["terminal-commander"].env, {
      TC_WSL_DISTRO: "Ubuntu-24.04",
    });
    assert.equal(Object.keys(data.mcpServers["terminal-commander"].env).length, 1);
  } finally {
    rmScope(root);
  }
});

test("unsafe distro is rejected with unsafe_distro_name (no file written)", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({
      scope: "project",
      projectRoot: root,
      distro: "Ubuntu; rm -rf /",
    });
    assert.equal(r.status, "unsafe_distro_name");
    assert.equal(fs.existsSync(path.join(root, ".cursor", "mcp.json")), false);
  } finally {
    rmScope(root);
  }
});

test("requireKnownDistro rejects a distro absent from knownDistros (no file written)", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({
      scope: "project",
      projectRoot: root,
      distro: "Fedora",
      requireKnownDistro: true,
      knownDistros: [{ name: "Ubuntu" }],
    });
    assert.equal(r.status, "distro_not_found");
    assert.equal(fs.existsSync(path.join(root, ".cursor", "mcp.json")), false);
  } finally {
    rmScope(root);
  }
});

test("requireKnownDistro accepts a known distro", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({
      scope: "project",
      projectRoot: root,
      distro: "Ubuntu",
      requireKnownDistro: true,
      knownDistros: [{ name: "Ubuntu" }, { name: "Debian" }],
    });
    assert.equal(r.status, "config_created");
  } finally {
    rmScope(root);
  }
});

test("project scope: existing terminal-commander entry refuses without force", () => {
  const root = mkScope();
  try {
    const cfgPath = path.join(root, ".cursor", "mcp.json");
    fs.mkdirSync(path.dirname(cfgPath), { recursive: true });
    fs.writeFileSync(
      cfgPath,
      JSON.stringify(
        {
          mcpServers: {
            "terminal-commander": { type: "stdio", command: "old" },
          },
        },
        null,
        2,
      ),
    );
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
    assert.equal(r.status, "already_exists");
    assert.equal(r.was_present, true);
    // Existing file untouched.
    const data = JSON.parse(fs.readFileSync(cfgPath, "utf8"));
    assert.equal(data.mcpServers["terminal-commander"].command, "old");
    // No backup made.
    assert.equal(fs.existsSync(cfgPath + ".bak"), false);
  } finally {
    rmScope(root);
  }
});

test("project scope: existing terminal-commander entry overwritten with force; .bak created", () => {
  const root = mkScope();
  try {
    const cfgPath = path.join(root, ".cursor", "mcp.json");
    fs.mkdirSync(path.dirname(cfgPath), { recursive: true });
    fs.writeFileSync(
      cfgPath,
      JSON.stringify(
        {
          mcpServers: {
            "terminal-commander": { type: "stdio", command: "old" },
            "other-server": { type: "stdio", command: "other-cmd" },
          },
        },
        null,
        2,
      ),
    );
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root, force: true });
    assert.equal(r.status, "config_updated");
    assert.equal(r.was_present, true);
    assert.equal(r.backup_path, cfgPath + ".bak");
    const data = JSON.parse(fs.readFileSync(cfgPath, "utf8"));
    assertTerminalCommanderStanza(data.mcpServers["terminal-commander"]);
    // Unrelated entry preserved.
    assert.equal(data.mcpServers["other-server"].command, "other-cmd");
    // Backup contains the previous file.
    const bak = JSON.parse(fs.readFileSync(r.backup_path, "utf8"));
    assert.equal(bak.mcpServers["terminal-commander"].command, "old");
  } finally {
    rmScope(root);
  }
});

test("refuses overwrite when .bak already exists unless clobber_backup:true", () => {
  const root = mkScope();
  try {
    const cfgPath = path.join(root, ".cursor", "mcp.json");
    fs.mkdirSync(path.dirname(cfgPath), { recursive: true });
    fs.writeFileSync(
      cfgPath,
      JSON.stringify(
        {
          mcpServers: { "terminal-commander": { type: "stdio", command: "old" } },
        },
        null,
        2,
      ),
    );
    // Pre-existing backup.
    fs.writeFileSync(cfgPath + ".bak", "stale-backup");
    const refused = writeCursorMcpConfig({
      scope: "project",
      projectRoot: root,
      force: true,
    });
    assert.equal(refused.status, "backup_failed");
    // Original config untouched.
    const stillOld = JSON.parse(fs.readFileSync(cfgPath, "utf8"));
    assert.equal(stillOld.mcpServers["terminal-commander"].command, "old");
    // Stale backup untouched.
    assert.equal(fs.readFileSync(cfgPath + ".bak", "utf8"), "stale-backup");

    // With clobber_backup, write proceeds.
    const r = writeCursorMcpConfig({
      scope: "project",
      projectRoot: root,
      force: true,
      clobber_backup: true,
    });
    assert.equal(r.status, "config_updated");
    // Backup now contains the just-before state (the "old" config).
    const bak = JSON.parse(fs.readFileSync(cfgPath + ".bak", "utf8"));
    assert.equal(bak.mcpServers["terminal-commander"].command, "old");
  } finally {
    rmScope(root);
  }
});

test("invalid JSON is rejected with invalid_json; original file untouched", () => {
  const root = mkScope();
  try {
    const cfgPath = path.join(root, ".cursor", "mcp.json");
    fs.mkdirSync(path.dirname(cfgPath), { recursive: true });
    const before = "{ not valid json";
    fs.writeFileSync(cfgPath, before);
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
    assert.equal(r.status, "invalid_json");
    // File contents byte-identical to before.
    assert.equal(fs.readFileSync(cfgPath, "utf8"), before);
    // No backup made.
    assert.equal(fs.existsSync(cfgPath + ".bak"), false);
  } finally {
    rmScope(root);
  }
});

test("over-size config is rejected with config_too_large; file untouched", () => {
  const root = mkScope();
  try {
    const cfgPath = path.join(root, ".cursor", "mcp.json");
    fs.mkdirSync(path.dirname(cfgPath), { recursive: true });
    const filler = "x".repeat(MAX_CONFIG_BYTES + 1);
    fs.writeFileSync(cfgPath, filler);
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
    assert.equal(r.status, "config_too_large");
    assert.equal(fs.readFileSync(cfgPath, "utf8"), filler);
    assert.equal(fs.existsSync(cfgPath + ".bak"), false);
  } finally {
    rmScope(root);
  }
});

test("project scope requires explicit projectRoot", () => {
  const r1 = writeCursorMcpConfig({ scope: "project" });
  assert.equal(r1.status, "project_root_required");
  const r2 = writeCursorMcpConfig({ scope: "project", projectRoot: "" });
  assert.equal(r2.status, "project_root_required");
});

test("round-trip: load existing config with two unrelated servers; add terminal-commander; reload preserves all three stanzas", () => {
  const root = mkScope();
  try {
    const cfgPath = path.join(root, ".cursor", "mcp.json");
    fs.mkdirSync(path.dirname(cfgPath), { recursive: true });
    const before = {
      mcpServers: {
        "vendor-a": { type: "stdio", command: "a-cmd" },
        "vendor-b": { type: "stdio", command: "b-cmd", env: { A: "1" } },
      },
      anotherTopLevelKey: { keep: true },
    };
    fs.writeFileSync(cfgPath, JSON.stringify(before, null, 2));
    const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
    assert.equal(r.status, "config_updated");
    const reloaded = JSON.parse(fs.readFileSync(cfgPath, "utf8"));
    assert.deepEqual(reloaded.mcpServers["vendor-a"], before.mcpServers["vendor-a"]);
    assert.deepEqual(reloaded.mcpServers["vendor-b"], before.mcpServers["vendor-b"]);
    assertTerminalCommanderStanza(reloaded.mcpServers["terminal-commander"]);
    assert.deepEqual(reloaded.anotherTopLevelKey, { keep: true });
  } finally {
    rmScope(root);
  }
});

test("global scope on Linux uses HOME-derived path", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({
      scope: "global",
      platform: "linux",
      env: { HOME: root },
    });
    assert.equal(r.status, "config_created");
    assert.equal(r.path, path.join(root, ".cursor", "mcp.json"));
  } finally {
    rmScope(root);
  }
});

test("global scope on Windows uses USERPROFILE-derived path", () => {
  const root = mkScope();
  try {
    const r = writeCursorMcpConfig({
      scope: "global",
      platform: "win32",
      env: { USERPROFILE: root },
    });
    assert.equal(r.status, "config_created");
    assert.equal(r.path, path.join(root, ".cursor", "mcp.json"));
  } finally {
    rmScope(root);
  }
});

test("atomicWrite places tmp file inside the same directory as the target", () => {
  const root = mkScope();
  try {
    const target = path.join(root, "out.json");
    let observedTmp = null;
    const r = atomicWrite(target, "x", {
      randomSuffix: () => {
        observedTmp = "fixedsuffix";
        return observedTmp;
      },
    });
    assert.equal(r.ok, true);
    assert.equal(r.path, target);
    // After rename, the tmp file should not remain.
    assert.equal(fs.existsSync(target + ".tmp.fixedsuffix"), false);
    assert.equal(fs.readFileSync(target, "utf8"), "x");
  } finally {
    rmScope(root);
  }
});

test("atomicWrite refuses targets outside the scope dir (path_not_allowed)", () => {
  const root = mkScope();
  const other = mkScope();
  try {
    const target = path.join(other, "evil.json");
    const r = atomicWrite(target, "x", { scopeDir: root });
    assert.equal(r.ok, false);
    assert.equal(r.reason, "path_not_allowed");
    assert.equal(fs.existsSync(target), false);
  } finally {
    rmScope(root);
    rmScope(other);
  }
});

test("backupCursorConfig is a no-op when target does not exist", () => {
  const root = mkScope();
  try {
    const target = path.join(root, "missing.json");
    const r = backupCursorConfig(target);
    assert.equal(r.ok, true);
    assert.equal(r.backup_path, null);
  } finally {
    rmScope(root);
  }
});

test("backupCursorConfig copies existing target to .bak (one-shot)", () => {
  const root = mkScope();
  try {
    const target = path.join(root, "x.json");
    fs.writeFileSync(target, "payload");
    const r = backupCursorConfig(target);
    assert.equal(r.ok, true);
    assert.equal(r.backup_path, target + ".bak");
    assert.equal(fs.readFileSync(target + ".bak", "utf8"), "payload");
  } finally {
    rmScope(root);
  }
});

test("backupCursorConfig refuses existing .bak unless clobber_backup", () => {
  const root = mkScope();
  try {
    const target = path.join(root, "x.json");
    fs.writeFileSync(target, "payload-v2");
    fs.writeFileSync(target + ".bak", "old-bak");
    const refused = backupCursorConfig(target);
    assert.equal(refused.ok, false);
    assert.equal(refused.reason, "backup_failed");
    assert.equal(fs.readFileSync(target + ".bak", "utf8"), "old-bak");

    const ok = backupCursorConfig(target, { clobber_backup: true });
    assert.equal(ok.ok, true);
    assert.equal(fs.readFileSync(target + ".bak", "utf8"), "payload-v2");
  } finally {
    rmScope(root);
  }
});

test("writeCursorMcpConfig stdout silence + result.path is the absolute scope path", () => {
  const root = mkScope();
  try {
    // Capture stdout / stderr writes during writeCursorMcpConfig.
    const stdoutChunks = [];
    const origStdout = process.stdout.write.bind(process.stdout);
    process.stdout.write = (chunk, ...rest) => {
      stdoutChunks.push(chunk);
      return origStdout(chunk, ...rest);
    };
    try {
      const r = writeCursorMcpConfig({ scope: "project", projectRoot: root });
      assert.equal(r.status, "config_created");
      assert.equal(r.path, path.join(root, ".cursor", "mcp.json"));
    } finally {
      process.stdout.write = origStdout;
    }
    // The writer must not write anything to stdout. (Test wrapper itself
    // may have echoed nothing because we did not call console.log; this
    // chunk array confirms the writer is silent.)
    assert.equal(stdoutChunks.length, 0, `writer must be stdout-silent; got chunks: ${stdoutChunks.length}`);
  } finally {
    rmScope(root);
  }
});
