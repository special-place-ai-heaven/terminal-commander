// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Part A: stable per-user binary directory + copy logic. No real filesystem
// writes outside an isolated tmp dir; the resolver + copy are injected.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  STABLE_BINARIES,
  stableBinDir,
  stableBinPath,
  ensureStableBinaries,
} = require("../lib/harness/stable_bin.js");

test("stableBinDir derives %LOCALAPPDATA%\\terminal-commander\\bin on Windows", () => {
  const dir = stableBinDir({
    platform: "win32",
    env: { LOCALAPPDATA: "C:\\Users\\op\\AppData\\Local" },
  });
  assert.equal(
    dir,
    path.join("C:\\Users\\op\\AppData\\Local", "terminal-commander", "bin"),
  );
});

test("stableBinDir derives ~/.local/share/terminal-commander/bin on Unix", () => {
  const dir = stableBinDir({ platform: "linux", env: { HOME: "/home/op" } });
  assert.equal(
    dir,
    path.join("/home/op", ".local", "share", "terminal-commander", "bin"),
  );
});

test("stableBinDir honors XDG_DATA_HOME on Unix", () => {
  const dir = stableBinDir({
    platform: "linux",
    env: { XDG_DATA_HOME: "/data", HOME: "/home/op" },
  });
  assert.equal(dir, path.join("/data", "terminal-commander", "bin"));
});

test("stableBinDir throws when the base env var is missing", () => {
  assert.throws(() => stableBinDir({ platform: "win32", env: {} }), /LOCALAPPDATA/);
  assert.throws(() => stableBinDir({ platform: "linux", env: {} }), /HOME/);
});

test("stableBinPath appends .exe on Windows only", () => {
  const win = stableBinPath("terminal-commander-mcp", {
    platform: "win32",
    env: { LOCALAPPDATA: "C:\\L" },
  });
  assert.equal(path.basename(win), "terminal-commander-mcp.exe");
  const nix = stableBinPath("terminal-commander-mcp", {
    platform: "linux",
    env: { HOME: "/home/op" },
  });
  assert.equal(path.basename(nix), "terminal-commander-mcp");
});

test("STABLE_BINARIES mirrors both the MCP server and the daemon exe", () => {
  assert.deepEqual([...STABLE_BINARIES].sort(), [
    "terminal-commander-mcp",
    "terminal-commanderd",
  ]);
});

test("ensureStableBinaries copies resolved exes into the stable dir and returns the primary path", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-stable-"));
  // Fake "node_modules" source binaries.
  const srcDir = path.join(root, "src");
  fs.mkdirSync(srcDir, { recursive: true });
  const srcMcp = path.join(srcDir, "terminal-commander-mcp.exe");
  const srcDaemon = path.join(srcDir, "terminal-commanderd.exe");
  fs.writeFileSync(srcMcp, "MCP");
  fs.writeFileSync(srcDaemon, "DAEMON");

  const resolveBinary = ({ binary }) => ({
    reason: "ok",
    binaryPath: binary === "terminal-commander-mcp" ? srcMcp : srcDaemon,
  });

  const r = ensureStableBinaries({
    platform: "win32",
    arch: "x64",
    env: { LOCALAPPDATA: root },
    version: "1.2.3",
    resolveBinary,
  });

  assert.equal(r.reason, "ok");
  const expectedDir = path.join(root, "terminal-commander", "bin");
  assert.equal(r.exePath, path.join(expectedDir, "terminal-commander-mcp.exe"));
  assert.equal(fs.existsSync(r.exePath), true);
  assert.equal(fs.readFileSync(r.exePath, "utf8"), "MCP");
  // Daemon exe mirrored too (for Part B logon task).
  assert.equal(
    fs.existsSync(path.join(expectedDir, "terminal-commanderd.exe")),
    true,
  );
  assert.equal(r.copied.length, 2);
  // Version stamp written.
  assert.equal(
    fs.readFileSync(path.join(expectedDir, ".version"), "utf8").trim(),
    "1.2.3",
  );
});

test("ensureStableBinaries skips re-copy when version stamp matches, re-copies on version change", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-stable-skew-"));
  const srcDir = path.join(root, "src");
  fs.mkdirSync(srcDir, { recursive: true });
  const srcMcp = path.join(srcDir, "terminal-commander-mcp.exe");
  const srcDaemon = path.join(srcDir, "terminal-commanderd.exe");
  fs.writeFileSync(srcMcp, "v1");
  fs.writeFileSync(srcDaemon, "v1");
  const resolveBinary = ({ binary }) => ({
    reason: "ok",
    binaryPath: binary === "terminal-commander-mcp" ? srcMcp : srcDaemon,
  });

  const first = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    version: "1.0.0",
    resolveBinary,
  });
  assert.equal(first.copied.length, 2);

  // Same version -> no copy.
  const second = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    version: "1.0.0",
    resolveBinary,
  });
  assert.equal(second.copied.length, 0, "unchanged version must not re-copy");
  assert.equal(second.reason, "ok");

  // Bump source + version -> re-copy (kills adapter/daemon version skew).
  fs.writeFileSync(srcMcp, "v2");
  fs.writeFileSync(srcDaemon, "v2");
  const third = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    version: "2.0.0",
    resolveBinary,
  });
  assert.equal(third.copied.length, 2, "version change must force re-copy");
  assert.equal(fs.readFileSync(third.exePath, "utf8"), "v2");
});

test("ensureStableBinaries falls back (exePath:null) when the copy fails (locked file)", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-stable-locked-"));
  const resolveBinary = () => ({ reason: "ok", binaryPath: "/nonexistent/src.exe" });
  const copyFile = () => {
    const err = new Error("EBUSY: resource busy or locked");
    err.code = "EBUSY";
    throw err;
  };
  const r = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    version: "1.0.0",
    resolveBinary,
    copyFile,
  });
  assert.equal(r.exePath, null, "locked copy must yield null so caller falls back");
  assert.equal(r.reason, "copy_failed");
});

test("ensureStableBinaries returns null when the primary binary does not resolve", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-stable-noresolve-"));
  const resolveBinary = () => ({ reason: "platform_package_missing", binaryPath: null });
  const r = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    resolveBinary,
  });
  assert.equal(r.exePath, null);
  assert.equal(r.reason, "resolve_failed");
});

test("ensureStableBinaries dry-run reports the planned path WITHOUT writing", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-stable-dry-"));
  const resolveBinary = () => ({ reason: "ok", binaryPath: "/some/src.exe" });
  let copied = false;
  const r = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    dry_run: true,
    resolveBinary,
    copyFile: () => {
      copied = true;
    },
  });
  assert.equal(r.reason, "dry_run");
  assert.equal(
    r.exePath,
    path.join(root, "terminal-commander", "bin", "terminal-commander-mcp.exe"),
  );
  assert.equal(copied, false, "dry-run must not copy");
  assert.equal(fs.existsSync(path.join(root, "terminal-commander")), false, "dry-run must not mkdir");
});

test("ensureStableBinaries tolerates a missing daemon exe (only primary forces fallback)", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-stable-nodaemon-"));
  const srcDir = path.join(root, "src");
  fs.mkdirSync(srcDir, { recursive: true });
  const srcMcp = path.join(srcDir, "terminal-commander-mcp.exe");
  fs.writeFileSync(srcMcp, "MCP");
  const resolveBinary = ({ binary }) =>
    binary === "terminal-commander-mcp"
      ? { reason: "ok", binaryPath: srcMcp }
      : { reason: "platform_package_missing", binaryPath: null };
  const r = ensureStableBinaries({
    platform: "win32",
    env: { LOCALAPPDATA: root },
    version: "1.0.0",
    resolveBinary,
  });
  assert.equal(r.reason, "ok");
  assert.ok(r.exePath);
  assert.equal(r.copied.length, 1, "only the MCP exe was copied");
});
