// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

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

test("postinstall is the guarded auto-setup trigger (Fix 4): safe, fail-soft, no daemon", () => {
  // Fix 4 reverses the prior "npm install is passive" decision: a `postinstall`
  // now auto-configures detected harnesses. It must remain AV/CI-safe — the
  // remaining assertions encode that contract.
  const pkg = JSON.parse(fs.readFileSync(path.join(PKG_ROOT, "package.json"), "utf8"));
  const scripts = pkg.scripts || {};

  // The trigger exists and points at the small delegating entry (no install/preinstall).
  assert.equal(scripts.postinstall, "node scripts/postinstall.js");
  assert.equal(scripts.install, undefined);
  assert.equal(scripts.preinstall, undefined);
  // The script ships in the published package.
  assert.equal((pkg.files || []).includes("scripts/"), true);
  assert.equal(fs.existsSync(path.join(PKG_ROOT, "scripts", "postinstall.js")), true);
  // No daemon supervisor is started by install.
  assert.equal(
    fs.existsSync(path.join(PKG_ROOT, "lib", "daemon", "session_supervisor.js")),
    false,
  );

  // The postinstall must be a SAFE no-op shape: it delegates to runBootstrap in
  // install mode, honors the CI / opt-out guards, and is wrapped so it never
  // fails `npm install`.
  const src = fs.readFileSync(path.join(PKG_ROOT, "scripts", "postinstall.js"), "utf8");
  assert.match(src, /runBootstrap/);
  assert.match(src, /mode:\s*"install"/);
  assert.match(src, /shouldSkipBootstrap/);
  assert.match(src, /isCiOrNonInteractive/);
  assert.match(src, /try\s*\{/);
  assert.match(src, /process\.exitCode = 0/);
  // No daemon autostart / spawn / shell from the install trigger.
  assert.doesNotMatch(src, /child_process|spawn|exec(?:Sync)?\b/);
});

test("postinstall is a SAFE no-op under CI / TC_NO_AUTO_SETUP and never throws", () => {
  const script = path.join(PKG_ROOT, "scripts", "postinstall.js");
  for (const env of [
    { CI: "true" },
    { TC_NO_AUTO_SETUP: "1" },
    { TC_SKIP_BOOTSTRAP: "1" },
    { GITHUB_ACTIONS: "true" },
  ]) {
    const r = spawnSync(process.execPath, [script], {
      encoding: "utf8",
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
      shell: false,
    });
    // Always exits 0, never blows up npm install.
    assert.equal(r.status, 0, `postinstall must exit 0 under ${JSON.stringify(env)}: ${r.stderr}`);
  }
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
  const launcherSrc = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander.js"),
    "utf8",
  );
  const preflightSrc = fs.readFileSync(
    path.join(PKG_ROOT, "lib", "cli", "update_preflight.js"),
    "utf8",
  );
  const src = `${launcherSrc}\n${preflightSrc}`;

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

test("setup harness --force refreshes a stale codex entry without colliding on a pre-existing .bak", () => {
  // Fixes 1 + 3: `setup harness --force` REFRESHES a stale terminal_commander
  // entry (it must not skip with already_exists), and the timestamped backup
  // never collides with a pre-existing `config.toml.bak`, so a re-run succeeds
  // instead of the old broken `backup_failed`. The pre-existing .bak is left
  // untouched and a fresh `<config>.<UTC>.bak` is created alongside it.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-setup-output-"));
  const codexDir = path.join(root, ".codex");
  fs.mkdirSync(codexDir, { recursive: true });
  const configPath = path.join(codexDir, "config.toml");
  fs.writeFileSync(
    configPath,
    '[mcp_servers.terminal_commander]\ncommand = "old"\nargs = []\n',
  );
  fs.writeFileSync(path.join(codexDir, "config.toml.bak"), "existing backup\n");

  const shim = path.join(PKG_ROOT, "bin", "terminal-commander.js");
  // S9 host-env leak: TC_WSL_DISTRO / TC_USE_LEGACY_WSL_BRIDGE flip the
  // orchestrator into the legacy WSL bootstrap lane, which emits
  // "WSL runtime already present." instead of the native-path diagnostic.
  // Strip the selectors so the DEFAULT path is under test. HOME/USERPROFILE
  // point the codex config path (~/.codex/config.toml) at our isolated root.
  const cleanEnv = {
    ...process.env,
    HOME: root,
    USERPROFILE: root,
    TC_SKIP_DAEMON_AUTOSTART: "1",
  };
  delete cleanEnv.TC_WSL_DISTRO;
  delete cleanEnv.TC_USE_LEGACY_WSL_BRIDGE;
  const r = spawnSync(
    process.execPath,
    [shim, "setup", "harness", "--provider", "codex-cli", "--force"],
    {
      encoding: "utf8",
      env: cleanEnv,
      stdio: ["ignore", "pipe", "pipe"],
      shell: false,
    },
  );

  // The refresh succeeds (no backup_failed, no already_exists skip).
  assert.equal(r.status, 0, r.stderr);
  assert.doesNotMatch(r.stderr, /backup_failed/);
  // The stale entry was rewritten with a fresh, correct stanza.
  const after = fs.readFileSync(configPath, "utf8");
  assert.match(after, /\[mcp_servers\.terminal_commander\]/);
  assert.doesNotMatch(after, /command = "old"/);
  // The pre-existing .bak is untouched; a fresh timestamped backup was created.
  assert.equal(
    fs.readFileSync(path.join(codexDir, "config.toml.bak"), "utf8"),
    "existing backup\n",
  );
  const timestamped = fs
    .readdirSync(codexDir)
    .filter((n) => /^config\.toml\.\d{8}T\d{9}Z\.bak$/.test(n));
  assert.equal(timestamped.length, 1, "exactly one timestamped backup of the prior config");
});
