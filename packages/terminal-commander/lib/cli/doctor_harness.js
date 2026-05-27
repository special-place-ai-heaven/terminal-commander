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
  for (const p of listProviders({ includeStubs: true })) {
    const d = detections.find((x) => x.id === p.id) || { detected: false };
    const configured = d.stub ? "n/a" : configuredFn(p.id) ? "yes" : "no";
    const det = d.detected ? "yes" : "no";
    if (d.detected && !d.stub) detectedCount += 1;
    const note = d.note || (p.stub ? "stub" : "");
    lines.push(
      `${p.id.padEnd(20)}${det.padEnd(10)}${String(configured).padEnd(12)}${note}`,
    );
  }
  // F1: when more than one harness is on this machine, each one needs its own
  // TC_SESSION (minted by `setup harness`) or they share a single daemon.
  if (detectedCount >= 2) {
    lines.push("");
    lines.push(
      "WARNING: shared daemon mode — multiple harnesses detected. Re-run " +
        "`terminal-commander setup harness` so each agent gets a per-harness " +
        "TC_SESSION and its own daemon endpoint.",
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
