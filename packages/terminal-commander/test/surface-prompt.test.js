// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// update's tool-surface menu: flag wins, terminal gets a menu, otherwise keep.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { chooseSurface } = require("../lib/cli/surface_prompt.js");

function asker(answers) {
  const seen = [];
  return { seen, ask: async (q) => { seen.push(q); return answers.shift(); } };
}

test("an explicit flag wins without asking", async () => {
  const a = asker([]);
  assert.equal(await chooseSurface({ flag: "full", interactive: true, current: "code", ask: a.ask }), "full");
  assert.equal(a.seen.length, 0);
});

test("no terminal and no flag keeps the configured surface", async () => {
  assert.equal(await chooseSurface({ interactive: false, current: "full", ask: async () => "2" }), undefined);
});

test("Enter keeps the current surface and the menu marks it", async () => {
  const a = asker([""]);
  assert.equal(await chooseSurface({ interactive: true, current: "full", ask: a.ask }), "full");
  assert.match(a.seen[0], /\[2\] MCP full .*\(current\)/);
  assert.match(a.seen[0], /\[2\]: $/);
});

test("a number or a name picks; anything else asks again", async () => {
  assert.equal(await chooseSurface({ interactive: true, current: "code", ask: asker(["3"]).ask }), "compact");
  const a = asker(["9", "mcp-full"]);
  assert.equal(await chooseSurface({ interactive: true, current: "code", ask: a.ask }), "full");
  assert.equal(a.seen.length, 2);
});
