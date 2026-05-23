// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const { detectAllHarnesses } = require("../harness/detect.js");
const { listProviders } = require("../harness/registry.js");
const fs = require("node:fs");

function entryConfigured(id, opts) {
  const o = opts || {};
  try {
    const d = require("../harness/detect.js").detectProvider(id, o);
    if (!d.detected || !d.config_path) return false;
    if (!fs.existsSync(d.config_path)) return false;
    const text = fs.readFileSync(d.config_path, "utf8");
    if (id === "cursor") return text.includes("terminal-commander-mcp");
    if (id === "codex-cli") return text.includes("[mcp_servers.terminal_commander]");
    if (id === "claude-code") {
      return (
        text.includes("terminal-commander-mcp") &&
        text.includes('"terminal_commander"')
      );
    }
    return text.includes("terminal-commander-mcp");
  } catch (_e) {
    return false;
  }
}

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
