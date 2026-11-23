// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// doctor harness: agent tools: lines must name tools the active surface exposes.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { agentSurfaceWarnings } = require("../lib/cli/agent_surface.js");
const { runDoctorHarness } = require("../lib/cli/doctor_harness.js");

function fixture(surface, agents) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tc-agents-"));
  const claudeJsonPath = path.join(dir, "claude.json");
  const env = surface === undefined ? {} : { TC_SURFACE: surface };
  fs.writeFileSync(claudeJsonPath, JSON.stringify({ mcpServers: { terminal_commander: { command: "tc", env } } }));
  const agentsDir = path.join(dir, "agents");
  fs.mkdirSync(agentsDir);
  for (const [name, body] of Object.entries(agents)) fs.writeFileSync(path.join(agentsDir, name), body);
  return { claudeJsonPath, agentsDir };
}

const GRANULAR = "---\r\nname: a\r\ntools: Read, mcp__terminal_commander__run_and_watch, mcp__terminal_commander__command_status\r\n---\r\nbody mcp__terminal_commander__file_write\r\n";
const CODE = "---\nname: b\ntools:\n  - Read\n  - mcp__terminal_commander__code_execute\n  - mcp__terminal_commander__status\n---\n";

test("unset surface is code: granular agent tools warn, code tools do not", () => {
  const w = agentSurfaceWarnings(fixture(undefined, { "a.md": GRANULAR, "b.md": CODE }));
  assert.equal(w.length, 1);
  assert.match(w[0], /agent a\.md .* code surface .*\(run_and_watch, command_status\)/);
  assert.doesNotMatch(w[0], /file_write/, "names outside the frontmatter are ignored");
  assert.match(w[0], /mcp__terminal_commander__code_execute/);
});

test("full surface flags only the code-only tools", () => {
  const w = agentSurfaceWarnings(fixture("full", { "a.md": GRANULAR, "b.md": CODE }));
  assert.equal(w.length, 1);
  assert.match(w[0], /agent b\.md .* full surface .*\(code_execute\)/);
});

test("missing agents dir yields no warnings and doctor output carries them", () => {
  assert.deepEqual(agentSurfaceWarnings({ claudeJsonPath: "/nonexistent.json", agentsDir: "/nonexistent-agents" }), []);
  const r = runDoctorHarness({ platform: "linux", detections: [], configuredFn: () => false, agentWarnings: ["WARNING: agent x.md test"] });
  assert.match(r.output, /WARNING: agent x\.md test/);
  assert.equal(r.exit_code, 0);
});
