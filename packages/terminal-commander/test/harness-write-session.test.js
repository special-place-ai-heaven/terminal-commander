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

// Providers whose written stanza carries an env block (Item 1 surface-flag scope).
const ENV_WRITERS = ["cursor", "claude-code", "claude-desktop"];

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

test("codex-cli is scoped OUT of TC_SURFACE: its TOML block emits no env even if surface passed", () => {
  // Item 1 decision (b): Codex ships includeEnv:false (TC_SESSION already does
  // not reach it; pre-existing B1 follow-up). Confirm threading surface does NOT
  // silently half-wire a TC_SURFACE into the Codex block.
  const block = buildCodexTomlBlock({ surface: "compact" });
  assert.equal(block.includes("TC_SURFACE"), false);
  assert.equal(block.includes("[mcp_servers.terminal_commander.env]"), false);
});
