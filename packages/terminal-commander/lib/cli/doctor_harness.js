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
  const detections = detectAllHarnesses({ platform, env });
  const lines = ["terminal-commander harness doctor:", ""];
  lines.push("ID                  DETECTED  CONFIGURED  NOTE");
  for (const p of listProviders({ includeStubs: true })) {
    const d = detections.find((x) => x.id === p.id) || { detected: false };
    const configured = d.stub ? "n/a" : entryConfigured(p.id, { platform, env }) ? "yes" : "no";
    const det = d.detected ? "yes" : "no";
    const note = d.note || (p.stub ? "stub" : "");
    lines.push(
      `${p.id.padEnd(20)}${det.padEnd(10)}${String(configured).padEnd(12)}${note}`,
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
