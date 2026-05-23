// SPDX-License-Identifier: Apache-2.0

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
