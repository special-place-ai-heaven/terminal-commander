// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { isWindowsMountedShimPath } = require("../lib/wsl/native-mcp.js");

test("isWindowsMountedShimPath is true only for /mnt/ paths on linux", () => {
  const orig = process.platform;
  try {
    Object.defineProperty(process, "platform", { value: "linux", configurable: true });
    assert.equal(isWindowsMountedShimPath("/mnt/c/Program Files/nodejs/foo"), true);
    assert.equal(isWindowsMountedShimPath("/home/user/.npm-global/bin/foo"), false);
    assert.equal(isWindowsMountedShimPath("C:\\nodejs\\foo"), false);
  } finally {
    Object.defineProperty(process, "platform", { value: orig, configurable: true });
  }
  Object.defineProperty(process, "platform", { value: "win32", configurable: true });
  assert.equal(isWindowsMountedShimPath("/mnt/c/foo"), false);
});
