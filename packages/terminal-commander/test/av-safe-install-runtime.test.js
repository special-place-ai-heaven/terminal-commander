// SPDX-License-Identifier: Apache-2.0

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const PKG_ROOT = path.resolve(__dirname, "..");
const RUNTIME_JS_ROOTS = Object.freeze([
  path.join(PKG_ROOT, "bin"),
  path.join(PKG_ROOT, "lib"),
]);
const WSL_HELPERS = Object.freeze([
  path.join(PKG_ROOT, "lib", "wsl", "detect.js"),
  path.join(PKG_ROOT, "lib", "wsl", "doctor.js"),
  path.join(PKG_ROOT, "lib", "wsl", "spawn.js"),
]);
const HIDDEN_WINDOW_PATTERNS = Object.freeze([
  { label: "windowsHide", re: /\bwindowsHide\b/ },
  { label: "CREATE_NO_WINDOW", re: /\bCREATE_NO_WINDOW\b/ },
  { label: "SW_HIDE", re: /\bSW_HIDE\b/ },
]);
const WINDOWS_SHELL_PATTERNS = Object.freeze([
  { label: "PowerShell", re: /\bPowerShell\b|\bpowershell\b/ },
  { label: "cmd.exe", re: /\bcmd\.exe\b|\bcmd\s+\/c\b|\.cmd\b/ },
  { label: "ExecutionPolicy", re: /\bExecutionPolicy\b/ },
]);

function collectJsFiles(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...collectJsFiles(p));
    } else if (entry.isFile() && entry.name.endsWith(".js")) {
      out.push(p);
    }
  }
  return out;
}

test("npm install is passive: no lifecycle script starts bootstrap work", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(PKG_ROOT, "package.json"), "utf8"));
  const scripts = pkg.scripts || {};

  assert.equal(scripts.install, undefined);
  assert.equal(scripts.postinstall, undefined);
  assert.equal(scripts.preinstall, undefined);
  assert.equal((pkg.files || []).includes("scripts/"), false);
  assert.equal(fs.existsSync(path.join(PKG_ROOT, "scripts", "install.js")), false);
  assert.equal(
    fs.existsSync(path.join(PKG_ROOT, "lib", "daemon", "session_supervisor.js")),
    false,
  );
});

test("Cursor-facing MCP shim directly spawns the native MCP binary", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander-mcp.js"),
    "utf8",
  );

  assert.match(src, /require\(\s*["']child_process["']\s*\)/);
  assert.match(src, /\bspawn\s*\(\s*result\.binaryPath/);
  assert.doesNotMatch(src, /session_supervisor/);
  assert.doesNotMatch(src, /runHarnessMcpSession/);
  assert.doesNotMatch(src, /windowsHide/);
});

test("admin CLI update is explicit npm update with no shell wrapper", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander.js"),
    "utf8",
  );

  assert.match(src, /terminal-commander@latest/);
  assert.match(src, /update-locks/);
  assert.match(src, /npm-cli\.js/);
  assert.match(src, /process\.execPath/);
  assert.match(src, /shell:\s*false/);
  assert.doesNotMatch(src, /npm\.cmd|taskkill|cmd\.exe|cmd \/c|powershell|ExecutionPolicy|windowsHide/);
});

test("legacy WSL bridge helpers do not request hidden subprocess windows", () => {
  for (const file of WSL_HELPERS) {
    const src = fs.readFileSync(file, "utf8");
    assert.doesNotMatch(src, /windowsHide/);
  }
});

test("runtime JS never requests hidden subprocess windows", () => {
  const files = RUNTIME_JS_ROOTS.flatMap(collectJsFiles);
  assert.ok(files.length > 0, "expected runtime JS files to scan");
  for (const file of files) {
    const src = fs.readFileSync(file, "utf8");
    for (const pattern of HIDDEN_WINDOW_PATTERNS) {
      assert.doesNotMatch(src, pattern.re, `${file} must not contain ${pattern.label}`);
    }
  }
});

test("runtime JS does not mention Windows shell interpreters", () => {
  const files = RUNTIME_JS_ROOTS.flatMap(collectJsFiles);
  assert.ok(files.length > 0, "expected runtime JS files to scan");
  for (const file of files) {
    const src = fs.readFileSync(file, "utf8");
    for (const pattern of WINDOWS_SHELL_PATTERNS) {
      assert.doesNotMatch(src, pattern.re, `${file} must not contain ${pattern.label}`);
    }
  }
});

test("admin CLI version advisory checks npm registry without spawning npm", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander.js"),
    "utf8",
  );

  assert.match(src, /registry\.npmjs\.org\/terminal-commander\/latest/);
  assert.match(src, /Update available/);
  assert.doesNotMatch(src, /npm view/);
});

test("JS-only control-plane commands route before native binary spawn", () => {
  const shim = path.join(PKG_ROOT, "bin", "terminal-commander.js");
  for (const args of [
    ["setup", "--help"],
    ["pair", "--help"],
    ["doctor", "harness", "--help"],
  ]) {
    const r = spawnSync(process.execPath, [shim, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      shell: false,
    });
    assert.equal(r.status, 0, `${args.join(" ")} failed: ${r.stderr}`);
    assert.equal(r.stderr, "");
    assert.match(r.stdout, /terminal-commander/);
  }
});

test("setup harness provider failure emits each diagnostic once", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-setup-output-"));
  const codexDir = path.join(root, ".codex");
  fs.mkdirSync(codexDir, { recursive: true });
  fs.writeFileSync(
    path.join(codexDir, "config.toml"),
    '[mcp_servers.terminal_commander]\ncommand = "old"\nargs = []\n',
  );
  fs.writeFileSync(path.join(codexDir, "config.toml.bak"), "existing backup\n");

  const shim = path.join(PKG_ROOT, "bin", "terminal-commander.js");
  const r = spawnSync(
    process.execPath,
    [shim, "setup", "harness", "--provider", "codex-cli", "--force"],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        HOME: root,
        USERPROFILE: root,
        TC_SKIP_DAEMON_AUTOSTART: "1",
      },
      stdio: ["ignore", "pipe", "pipe"],
      shell: false,
    },
  );

  const stderrLines = r.stderr.trim().split(/\r?\n/).filter(Boolean);
  const expectedLines = [];
  if (process.platform === "win32") {
    expectedLines.push("terminal-commander: native Windows MCP path selected; WSL bootstrap skipped.");
  }
  expectedLines.push("codex-cli: backup_failed");

  assert.equal(r.status, 64, r.stderr);
  assert.equal(r.stdout, "");
  assert.deepEqual(stderrLines, expectedLines);
});
