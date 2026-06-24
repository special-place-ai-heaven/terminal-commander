// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// F1 launcher wiring: every harness stanza must carry a per-harness
// env.TC_SESSION so each agent gets its own daemon endpoint. Dry-run is used
// so these assertions touch no filesystem.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { writeProvider } = require("../lib/harness/write_all.js");
const { isValidSessionToken } = require("../lib/session/mint.js");
const { buildCodexTomlBlock } = require("../lib/harness/io/toml_mcp.js");

// Providers whose written stanza carries an env block (surface-flag scope).
// codex-cli is now wired too (its TOML gets a per-server [.env] table).
const ENV_WRITERS = ["cursor", "claude-code", "claude-desktop", "codex-cli"];

function dryRun(id, extra) {
  return writeProvider(id, {
    dry_run: true,
    detection: { detected: true },
    machineKey: "test-machine",
    ...(extra || {}),
  });
}

test("cursor dry-run stanza carries a valid env.TC_SESSION", () => {
  const r = dryRun("cursor");
  assert.equal(r.status, "ok");
  assert.ok(r.stanza, "expected a stanza in dry-run");
  assert.ok(r.stanza.env, "expected env block");
  assert.equal(
    isValidSessionToken(r.stanza.env.TC_SESSION),
    true,
    `cursor TC_SESSION must be valid: ${r.stanza.env.TC_SESSION}`,
  );
});

test("claude-code dry-run stanza carries a valid env.TC_SESSION", () => {
  const r = dryRun("claude-code");
  assert.equal(r.status, "ok");
  assert.ok(r.stanza && r.stanza.env, "expected env block");
  assert.equal(isValidSessionToken(r.stanza.env.TC_SESSION), true);
});

test("distinct providers get distinct TC_SESSION tokens", () => {
  const cursor = dryRun("cursor");
  const claude = dryRun("claude-code");
  assert.notEqual(
    cursor.stanza.env.TC_SESSION,
    claude.stanza.env.TC_SESSION,
    "two providers on one machine must not share a session token",
  );
});

test("same provider mints a stable token across runs", () => {
  const a = dryRun("cursor");
  const b = dryRun("cursor");
  assert.equal(
    a.stanza.env.TC_SESSION,
    b.stanza.env.TC_SESSION,
    "re-running setup must not churn the token",
  );
});

test("cursor on Windows merges TC_SESSION with TC_WSL_DISTRO", () => {
  const r = dryRun("cursor", { platform: "win32", distro: "Ubuntu-24.04" });
  assert.equal(r.stanza.env.TC_WSL_DISTRO, "Ubuntu-24.04");
  assert.equal(isValidSessionToken(r.stanza.env.TC_SESSION), true);
});

// --- Item 1: TC_SURFACE flag threading (S2/S3) ---

for (const id of ENV_WRITERS) {
  test(`${id} dry-run stanza carries env.TC_SURFACE=compact when surface set`, () => {
    const r = dryRun(id, { surface: "compact" });
    assert.equal(r.status, "ok");
    assert.ok(r.stanza && r.stanza.env, `${id} expected env block`);
    assert.equal(r.stanza.env.TC_SURFACE, "compact");
    // surface must not disturb the existing per-harness session token.
    assert.equal(isValidSessionToken(r.stanza.env.TC_SESSION), true);
  });

  test(`${id} dry-run stanza carries env.TC_SURFACE=full when surface set`, () => {
    const r = dryRun(id, { surface: "full" });
    assert.equal(r.stanza.env.TC_SURFACE, "full");
  });

  test(`${id} dry-run stanza OMITS TC_SURFACE when surface absent (S3 default)`, () => {
    const r = dryRun(id);
    assert.ok(r.stanza && r.stanza.env, `${id} expected env block`);
    assert.equal(
      Object.prototype.hasOwnProperty.call(r.stanza.env, "TC_SURFACE"),
      false,
      `${id} must NOT emit TC_SURFACE when --surface is absent`,
    );
  });
}

// --- Codex env wiring: TC_SESSION + TC_SURFACE + TC_WSL_DISTRO reach Codex ---

test("codex-cli TOML block emits [.env] with TC_SESSION + TC_SURFACE (literal values)", () => {
  const block = buildCodexTomlBlock({ sessionToken: "tc-abc123", surface: "compact" });
  assert.match(block, /\[mcp_servers\.terminal_commander\.env\]/);
  assert.match(block, /TC_SESSION = "tc-abc123"/);
  assert.match(block, /TC_SURFACE = "compact"/);
});

test("codex-cli TOML block emits TC_WSL_DISTRO only on win32", () => {
  const win = buildCodexTomlBlock({
    sessionToken: "tc-abc123",
    distro: "Ubuntu-24.04",
    platform: "win32",
  });
  assert.match(win, /TC_WSL_DISTRO = "Ubuntu-24.04"/);
  const lin = buildCodexTomlBlock({
    sessionToken: "tc-abc123",
    distro: "Ubuntu-24.04",
    platform: "linux",
  });
  assert.equal(lin.includes("TC_WSL_DISTRO"), false);
});

test("codex-cli TOML block omits the env sub-table when there are no env values", () => {
  const block = buildCodexTomlBlock({ exePath: "x" });
  assert.equal(block.includes("[mcp_servers.terminal_commander.env]"), false);
});

test("codex-cli TOML block rejects an unsafe TC_SESSION token", () => {
  assert.throws(
    () => buildCodexTomlBlock({ sessionToken: "bad token!" }),
    (e) => e && e.code === "UNSAFE_SESSION_TOKEN",
  );
});

test("codex-cli TOML block rejects an unsafe TC_WSL_DISTRO on win32", () => {
  assert.throws(
    () =>
      buildCodexTomlBlock({
        sessionToken: "tc-abc123",
        distro: "bad; rm -rf /",
        platform: "win32",
      }),
    (e) => e && (e.code === "UNSAFE_DISTRO_NAME" || /distro/i.test(e.message)),
  );
});
