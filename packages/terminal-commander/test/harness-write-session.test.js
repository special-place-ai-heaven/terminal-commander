// SPDX-License-Identifier: Apache-2.0
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
