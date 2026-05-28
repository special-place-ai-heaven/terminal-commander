// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { detectAllHarnesses } = require("../harness/detect.js");
const { listProviders } = require("../harness/registry.js");
const { entryConfigured } = require("../harness/needs.js");

function runDoctorHarness(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  // Injectable seams keep this testable without touching the filesystem.
  const detections = o.detections || detectAllHarnesses({ platform, env });
  const configuredFn =
    o.configuredFn || ((id) => entryConfigured(id, { platform, env }));
  const lines = ["terminal-commander harness doctor:", ""];
  lines.push("ID                  DETECTED  CONFIGURED  NOTE");
  let detectedCount = 0;
  let unconfiguredCount = 0;
  for (const p of listProviders({ includeStubs: true })) {
    const d = detections.find((x) => x.id === p.id) || { detected: false };
    const isConfigured = !d.stub && configuredFn(p.id);
    const configured = d.stub ? "n/a" : isConfigured ? "yes" : "no";
    const det = d.detected ? "yes" : "no";
    if (d.detected && !d.stub) {
      detectedCount += 1;
      if (!isConfigured) unconfiguredCount += 1;
    }
    const note = d.note || (p.stub ? "stub" : "");
    lines.push(
      `${p.id.padEnd(20)}${det.padEnd(10)}${String(configured).padEnd(12)}${note}`,
    );
  }
  // F1: warn about shared-daemon mode only when there are >=2 harnesses AND at
  // least one is NOT yet configured by `setup harness` (which mints TC_SESSION).
  // A fully-configured multi-harness install already has per-harness tokens, so
  // it must not get the nag — that was a false positive.
  if (detectedCount >= 2 && unconfiguredCount >= 1) {
    lines.push("");
    lines.push(
      "WARNING: shared daemon mode — multiple harnesses detected and at least " +
        "one is not configured. Run `terminal-commander setup harness` so each " +
        "agent gets a per-harness TC_SESSION and its own daemon endpoint.",
    );
  }
  return {
    status: "ok",
    exit_code: 0,
    output: lines.join("\n"),
  };
}

module.exports = {
  runDoctorHarness,
};
