// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 resolver tests. Use the Node test runner (built-in) so no
// extra npm dev-dependencies ship. The resolver is pure; we inject a
// fake `requireResolve` to simulate platform-package presence.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("path");

const {
  resolveBinary,
  formatResolveError,
  SUPPORTED_TARGETS,
  ALLOWED_BINARIES,
} = require("../lib/resolve-binary.js");

function fakeRequireResolveOk(stubRoot) {
  return (id /*, opts */) => {
    // Map `<pkg>/package.json` -> `<stubRoot>/<pkg>/package.json`
    return path.join(stubRoot, id);
  };
}

function fakeRequireResolveMissing() {
  return (id /*, opts */) => {
    const err = new Error("Cannot find module " + id);
    err.code = "MODULE_NOT_FOUND";
    throw err;
  };
}

test("ALLOWED_BINARIES is exactly the three TC commands", () => {
  assert.deepEqual(
    [...ALLOWED_BINARIES],
    ["terminal-commanderd", "terminal-commander-mcp", "terminal-commander"],
  );
});

test("SUPPORTED_TARGETS is exactly linux-x64 + linux-arm64", () => {
  const summary = SUPPORTED_TARGETS.map((t) => `${t.platform}-${t.arch}`);
  assert.deepEqual(summary, ["linux-x64", "linux-arm64"]);
});

test("invalid binary name returns invalid_binary", () => {
  const r = resolveBinary({
    platform: "linux",
    arch: "x64",
    binary: "rm-rf-slash",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "invalid_binary");
  assert.equal(r.binaryPath, null);
});

test("linux x64 with platform package installed resolves to bin path", () => {
  const r = resolveBinary({
    platform: "linux",
    arch: "x64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub/node_modules"),
  });
  assert.equal(r.reason, "ok");
  assert.equal(r.platformPackage, "@terminal-commander/linux-x64");
  assert.equal(
    r.binaryPath,
    path.join(
      "/stub/node_modules",
      "@terminal-commander/linux-x64",
      "bin",
      "terminal-commanderd",
    ),
  );
});

test("linux arm64 with platform package installed resolves to bin path", () => {
  const r = resolveBinary({
    platform: "linux",
    arch: "arm64",
    binary: "terminal-commander-mcp",
    requireResolve: fakeRequireResolveOk("/stub/node_modules"),
  });
  assert.equal(r.reason, "ok");
  assert.equal(r.platformPackage, "@terminal-commander/linux-arm64");
  assert.equal(
    r.binaryPath,
    path.join(
      "/stub/node_modules",
      "@terminal-commander/linux-arm64",
      "bin",
      "terminal-commander-mcp",
    ),
  );
});

test("darwin x64 is rejected as unsupported_platform", () => {
  const r = resolveBinary({
    platform: "darwin",
    arch: "x64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "unsupported_platform");
  assert.equal(r.binaryPath, null);
  const msg = formatResolveError(r, { platform: "darwin", arch: "x64" });
  assert.match(msg, /unsupported platform darwin-x64/);
  assert.match(msg, /linux-x64/);
  assert.match(msg, /linux-arm64/);
});

test("win32 x64 returns bridge_required (WWS02)", () => {
  const r = resolveBinary({
    platform: "win32",
    arch: "x64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "bridge_required");
  assert.equal(r.platformPackage, null);
  assert.equal(r.binaryPath, null);
  const msg = formatResolveError(r, { platform: "win32", arch: "x64" });
  assert.match(msg, /Windows host/);
  assert.match(msg, /bridge\/setup only/);
  assert.match(msg, /linux-x64/);
  assert.match(msg, /linux-arm64/);
  // Bridge_required is NOT the same as unsupported_platform — the
  // message must not falsely report the host as fully unsupported.
  assert.equal(msg.includes("unsupported platform"), false);
});

test("win32 arm64 returns bridge_required (WWS02)", () => {
  const r = resolveBinary({
    platform: "win32",
    arch: "arm64",
    binary: "terminal-commander-mcp",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "bridge_required");
  assert.equal(r.binaryPath, null);
  const msg = formatResolveError(r, { platform: "win32", arch: "arm64" });
  assert.match(msg, /Windows host/);
  assert.match(msg, /win32-arm64/);
});

test("win32 with invalid binary name still rejects invalid_binary before bridge", () => {
  const r = resolveBinary({
    platform: "win32",
    arch: "x64",
    binary: "rm-rf-slash",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  // Binary-name validation happens BEFORE platform branching, so an
  // unknown name must not slip past the resolver guard on Windows.
  assert.equal(r.reason, "invalid_binary");
});

test("freebsd x64 is still rejected as unsupported_platform (regression guard)", () => {
  const r = resolveBinary({
    platform: "freebsd",
    arch: "x64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "unsupported_platform");
  const msg = formatResolveError(r, { platform: "freebsd", arch: "x64" });
  assert.match(msg, /unsupported platform freebsd-x64/);
});

test("formatResolveError bridge_required message is bounded ASCII single-line", () => {
  const r = resolveBinary({
    platform: "win32",
    arch: "x64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  const msg = formatResolveError(r, { platform: "win32", arch: "x64" });
  assert.equal(msg.includes("\n"), false);
  assert.equal(msg.startsWith("terminal-commander:"), true);
  // ASCII-only.
  assert.ok(
    /^[\x20-\x7e]+$/.test(msg),
    "bridge_required message must be ASCII-only: " + msg,
  );
  // Mentions the word "bridge" so the operator can map the error to
  // the WWS01 contract.
  assert.match(msg, /bridge/);
  assert.ok(msg.length < 300, `msg length ${msg.length} >= 300`);
});

test("linux mips is rejected as unsupported_platform", () => {
  const r = resolveBinary({
    platform: "linux",
    arch: "mips",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "unsupported_platform");
});

test("linux x64 without platform package returns platform_package_missing", () => {
  const r = resolveBinary({
    platform: "linux",
    arch: "x64",
    binary: "terminal-commander",
    requireResolve: fakeRequireResolveMissing(),
  });
  assert.equal(r.reason, "platform_package_missing");
  assert.equal(r.platformPackage, "@terminal-commander/linux-x64");
  assert.equal(r.binaryPath, null);
  const msg = formatResolveError(r, { platform: "linux", arch: "x64" });
  assert.match(msg, /platform package @terminal-commander\/linux-x64 not installed/);
  assert.match(msg, /npm may have skipped optionalDependencies/);
});

test("formatResolveError returns null on ok result", () => {
  const r = resolveBinary({
    platform: "linux",
    arch: "x64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "ok");
  assert.equal(formatResolveError(r), null);
});

test("formatResolveError unsupported_platform message is single-line and bounded", () => {
  // darwin/arm64 still resolves to unsupported_platform — bridge mode
  // only triggers on win32, so this test exercises the original
  // bounded-format invariant on an unsupported host.
  const r = resolveBinary({
    platform: "darwin",
    arch: "arm64",
    binary: "terminal-commanderd",
    requireResolve: fakeRequireResolveOk("/stub"),
  });
  assert.equal(r.reason, "unsupported_platform");
  const msg = formatResolveError(r, { platform: "darwin", arch: "arm64" });
  assert.equal(msg.includes("\n"), false);
  assert.equal(msg.startsWith("terminal-commander:"), true);
  // Bounded: keep the message under ~200 chars so it does not blow up
  // stderr in the harness.
  assert.ok(msg.length < 200, `msg length ${msg.length} >= 200`);
});

test("SUPPORTED_TARGETS is frozen and immutable", () => {
  assert.equal(Object.isFrozen(SUPPORTED_TARGETS), true);
  assert.throws(() => {
    SUPPORTED_TARGETS.push({ platform: "darwin", arch: "arm64", pkg: "x" });
  });
  assert.equal(SUPPORTED_TARGETS.length, 2);
});
