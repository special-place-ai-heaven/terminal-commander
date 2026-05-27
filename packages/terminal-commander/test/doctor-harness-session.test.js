// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// F1: doctor surfaces a "shared daemon mode" warning when multiple harnesses
// are present without per-harness TC_SESSION isolation.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { runDoctorHarness } = require("../lib/cli/doctor_harness.js");

const SHARED_WARN = /shared daemon mode/i;

function det(id, detected) {
  return { id, label: id, stub: false, detected };
}

test("warns about shared daemon mode when >=2 harnesses detected", () => {
  const r = runDoctorHarness({
    platform: "linux",
    detections: [det("cursor", true), det("codex-cli", true)],
    configuredFn: () => false,
  });
  assert.match(r.output, SHARED_WARN);
});

test("no shared-mode warning when only one harness detected", () => {
  const r = runDoctorHarness({
    platform: "linux",
    detections: [det("cursor", true), det("codex-cli", false)],
    configuredFn: () => false,
  });
  assert.doesNotMatch(r.output, SHARED_WARN);
});

test("no shared-mode warning when no harness detected", () => {
  const r = runDoctorHarness({
    platform: "linux",
    detections: [det("cursor", false), det("codex-cli", false)],
    configuredFn: () => false,
  });
  assert.doesNotMatch(r.output, SHARED_WARN);
});

test("runDoctorHarness still returns ok/exit 0 with the warning", () => {
  const r = runDoctorHarness({
    platform: "linux",
    detections: [det("cursor", true), det("claude-code", true)],
    configuredFn: () => true,
  });
  assert.equal(r.status, "ok");
  assert.equal(r.exit_code, 0);
});
