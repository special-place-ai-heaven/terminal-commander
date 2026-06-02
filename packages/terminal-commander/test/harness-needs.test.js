// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { harnessNeedsConfiguration } = require("../lib/harness/needs.js");

test("harnessNeedsConfiguration returns boolean", () => {
  const r = harnessNeedsConfiguration({ platform: process.platform, env: process.env });
  assert.equal(typeof r, "boolean");
});
