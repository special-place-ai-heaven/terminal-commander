// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const { parseArgv } = require("../lib/cli/parser.js");

test("setup defaults to harness subcommand", () => {
  const r = parseArgv(["setup"]);
  assert.equal(r.ok, true);
  assert.equal(r.subcommand, "harness");
});

test("setup harness accepts --provider", () => {
  const r = parseArgv(["setup", "harness", "--provider", "cursor", "--dry-run"]);
  assert.equal(r.ok, true);
  assert.equal(r.flags.provider, "cursor");
  assert.equal(r.flags["dry-run"], true);
});

test("doctor harness subcommand", () => {
  const r = parseArgv(["doctor", "harness"]);
  assert.equal(r.ok, true);
  assert.equal(r.subcommand, "harness");
});

test("setup harness accepts --surface compact", () => {
  const r = parseArgv(["setup", "harness", "--surface", "compact", "--dry-run"]);
  assert.equal(r.ok, true);
  assert.equal(r.flags.surface, "compact");
});

test("setup harness accepts --surface full", () => {
  const r = parseArgv(["setup", "harness", "--surface", "full"]);
  assert.equal(r.ok, true);
  assert.equal(r.flags.surface, "full");
});

test("setup (default subcommand) accepts --surface", () => {
  const r = parseArgv(["setup", "--surface", "compact"]);
  assert.equal(r.ok, true);
  assert.equal(r.subcommand, "harness");
  assert.equal(r.flags.surface, "compact");
});

test("setup harness rejects --surface bogus with a bounded error", () => {
  const r = parseArgv(["setup", "harness", "--surface", "bogus"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /--surface must be 'compact' or 'full'/);
});

test("setup harness --surface requires a value", () => {
  const r = parseArgv(["setup", "harness", "--surface"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /flag --surface requires a value/);
});

test("--surface followed by another flag is not swallowed as its value", () => {
  const r = parseArgv(["setup", "harness", "--surface", "--dry-run"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /flag --surface requires a value/);
});
