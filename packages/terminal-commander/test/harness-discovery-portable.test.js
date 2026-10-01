// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { detectProvider, detectAllHarnesses } = require("../lib/harness/detect.js");

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-discovery-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

const globalPaths = {
  cursor: ".cursor/mcp.json",
  "codex-cli": ".codex/config.toml",
  "claude-code": ".claude.json",
  gemini: ".gemini/settings.json",
  grok: ".grok/config.toml",
  "kilo-code": ".config/kilo/kilo.jsonc",
  omp: ".omp/agent/mcp.json",
};

for (const host of ["win32", "linux", "wsl", "cloud"]) {
  test(`${host}: discover installed harness markers without GUI or TTY`, (t) => {
    const root = fixture(t);
    const options = { platform: host === "win32" ? "win32" : "linux", env: { HOME: root, USERPROFILE: root } };
    assert.ok(detectAllHarnesses(options).every((d) => !d.detected));
    for (const [id, relative] of Object.entries(globalPaths)) {
      const target = path.join(root, relative);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, "");
      const detection = detectProvider(id, options);
      assert.equal(detection.detected, true, id);
      assert.equal(detection.config_path, target, id);
    }
    assert.equal(detectProvider("kilo-code", options).config_format, "json-kilo");
  });
}

test("respect Codex home and Kilo XDG config home without searching another user", (t) => {
  const root = fixture(t);
  const env = { HOME: root, USERPROFILE: root, CODEX_HOME: path.join(root, "codex-custom"), XDG_CONFIG_HOME: path.join(root, "xdg-custom") };
  for (const [id, target] of [["codex-cli", path.join(env.CODEX_HOME, "config.toml")], ["kilo-code", path.join(env.XDG_CONFIG_HOME, "kilo/kilo.json")]]) {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "");
    assert.equal(detectProvider(id, { env }).config_path, target);
  }
});

test("project scope uses only explicit project targets, including Kilo legacy files", (t) => {
  const root = fixture(t);
  const projectRoot = path.join(root, "repo");
  fs.mkdirSync(projectRoot);
  const options = { env: { HOME: root, USERPROFILE: root }, scope: "project", projectRoot };
  const targets = { ...globalPaths, "claude-code": ".mcp.json", omp: ".omp/mcp.json", "kilo-code": ".kilo/kilo.jsonc" };
  for (const [id, relative] of Object.entries(targets)) {
    assert.equal(detectProvider(id, options).detected, false, id);
    const target = path.join(projectRoot, relative);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "");
    assert.equal(detectProvider(id, options).config_path, target, id);
  }
  fs.unlinkSync(path.join(projectRoot, ".kilo/kilo.jsonc"));
  const legacy = path.join(projectRoot, ".kilocode/mcp.json");
  fs.mkdirSync(path.dirname(legacy));
  fs.writeFileSync(legacy, "{}");
  assert.equal(detectProvider("kilo-code", options).config_path, legacy);
  assert.equal(detectProvider("kilo-code", options).config_format, "json-mcp");
  assert.equal(detectProvider("gemini", { scope: "project" }).reason, "project_root_required");
});

test("missing provider markers still yield a target for explicit first-time setup", (t) => {
  const root = fixture(t);
  for (const [id, relative] of Object.entries(globalPaths)) {
    const result = detectProvider(id, { env: { HOME: root, USERPROFILE: root } });
    assert.equal(result.detected, false);
    assert.equal(result.config_path, path.join(root, relative));
  }
});

test("missing injected home never falls back to the process user's home", () => {
  for (const platform of ["win32", "linux"]) {
    for (const id of Object.keys(globalPaths)) {
      const result = detectProvider(id, { platform, env: {} });
      assert.equal(result.detected, false);
      assert.equal(result.config_path, undefined);
    }
  }
});

test("Kilo prefers existing project JSON[C] files over the legacy location", (t) => {
  const projectRoot = fixture(t);
  const legacy = path.join(projectRoot, ".kilocode/mcp.json");
  fs.mkdirSync(path.dirname(legacy));
  fs.writeFileSync(legacy, "{}");
  const modern = path.join(projectRoot, "kilo.json");
  fs.writeFileSync(modern, "{}");
  const options = { scope: "project", projectRoot };
  assert.equal(detectProvider("kilo-code", options).config_path, modern);
  fs.writeFileSync(path.join(projectRoot, "kilo.jsonc"), "{}");
  assert.equal(detectProvider("kilo-code", options).config_path, path.join(projectRoot, "kilo.jsonc"));
});
