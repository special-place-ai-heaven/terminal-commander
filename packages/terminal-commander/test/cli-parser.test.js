// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 CLI parser tests.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { parseArgv, USAGE, DOCTOR_USAGE, SETUP_USAGE, PAIR_USAGE } = require("../lib/cli/parser.js");

test("USAGE / DOCTOR_USAGE / SETUP_USAGE / PAIR_USAGE are non-empty strings", () => {
  for (const t of [USAGE, DOCTOR_USAGE, SETUP_USAGE, PAIR_USAGE]) {
    assert.equal(typeof t, "string");
    assert.ok(t.length > 50, "usage panel must be non-trivial");
  }
});

test("empty argv routes to top-level help", () => {
  const r = parseArgv([]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "help");
  assert.equal(r.help, true);
});

test("--help routes through the parsed command", () => {
  assert.equal(parseArgv(["--help"]).help, true);
  assert.equal(parseArgv(["doctor", "--help"]).help, true);
  assert.equal(parseArgv(["setup", "--help"]).help, true);
  assert.equal(parseArgv(["pair", "--help"]).help, true);
});

test("unknown flag is rejected with a usage error", () => {
  const r = parseArgv(["doctor", "--frobnicate"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /unknown flag/);
});

test("doctor parses with no subcommand", () => {
  const r = parseArgv(["doctor"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "doctor");
  assert.equal(r.subcommand, null);
});

test("doctor wsl parses subcommand + --distro + --probe-runtime", () => {
  const r = parseArgv(["doctor", "wsl", "--distro", "Ubuntu", "--probe-runtime"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "doctor");
  assert.equal(r.subcommand, "wsl");
  assert.equal(r.flags.distro, "Ubuntu");
  assert.equal(r.flags["probe-runtime"], true);
});

test("doctor rejects unknown subcommand", () => {
  const r = parseArgv(["doctor", "bogus"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /unknown doctor subcommand/);
});

test("doctor rejects flags that belong to other commands", () => {
  const r = parseArgv(["doctor", "wsl", "--force"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /not valid for 'doctor'/);
});

test("setup cursor-wsl parses all locked flags", () => {
  const r = parseArgv([
    "setup",
    "cursor-wsl",
    "--distro",
    "Ubuntu-24.04",
    "--global",
    "--force",
    "--clobber-backup",
    "--print-config",
    "--dry-run",
    "--install-wsl-runtime",
  ]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "setup");
  assert.equal(r.subcommand, "cursor-wsl");
  assert.equal(r.flags.distro, "Ubuntu-24.04");
  assert.equal(r.flags.global, true);
  assert.equal(r.flags.force, true);
  assert.equal(r.flags["clobber-backup"], true);
  assert.equal(r.flags["print-config"], true);
  assert.equal(r.flags["dry-run"], true);
  assert.equal(r.flags["install-wsl-runtime"], true);
});

test("setup cursor-wsl --global and --project are mutually exclusive", () => {
  const r = parseArgv(["setup", "cursor-wsl", "--global", "--project", "/repo"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /mutually exclusive/);
});

test("setup cursor-wsl --project requires a value", () => {
  const r = parseArgv(["setup", "cursor-wsl", "--project"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /flag --project requires a value/);
});

test("setup with no subcommand defaults to harness", () => {
  const r = parseArgv(["setup"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "setup");
  assert.equal(r.subcommand, "harness");
  assert.equal(r.help, false);
});

test("setup rejects unknown subcommand", () => {
  const r = parseArgv(["setup", "claude"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /unknown setup subcommand/);
});

test("pair create parses optional --distro", () => {
  const r = parseArgv(["pair", "create", "--distro", "Ubuntu"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "pair");
  assert.equal(r.subcommand, "create");
  assert.equal(r.flags.distro, "Ubuntu");
});

test("pair create rejects unrelated flags", () => {
  const r = parseArgv(["pair", "create", "--global"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /not valid for 'pair create'/);
});

test("pair accept requires a positional code", () => {
  const r = parseArgv(["pair", "accept"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /requires a <code>/);
});

test("pair accept parses a 6-digit code positional", () => {
  const r = parseArgv(["pair", "accept", "123456"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "pair");
  assert.equal(r.subcommand, "accept");
  assert.deepEqual(r.positional, ["123456"]);
});

test("pair with no subcommand routes to help", () => {
  const r = parseArgv(["pair"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "pair");
  assert.equal(r.subcommand, null);
  assert.equal(r.help, true);
});

test("pair rejects unknown subcommand", () => {
  const r = parseArgv(["pair", "bogus"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /unknown pair subcommand/);
});

test("unknown top-level command is rejected", () => {
  const r = parseArgv(["frobnicate"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /unknown command/);
});

test("restart parses with no flags", () => {
  const r = parseArgv(["restart"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "restart");
  assert.equal(r.help, false);
});

test("restart parses --distro and --force", () => {
  const r = parseArgv(["restart", "--distro", "Ubuntu-24.04", "--force"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "restart");
  assert.equal(r.flags.distro, "Ubuntu-24.04");
  assert.equal(r.flags.force, true);
});

test("restart --help routes to help", () => {
  const r = parseArgv(["restart", "--help"]);
  assert.equal(r.ok, true);
  assert.equal(r.command, "restart");
  assert.equal(r.help, true);
});

test("restart rejects flags that belong to other commands", () => {
  const r = parseArgv(["restart", "--global"]);
  assert.equal(r.ok, false);
  assert.match(r.error, /not valid for 'restart'/);
});
