// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// fix/harness-windows-shell-false (FIX 1): `terminal-commander update` MUST
// re-register harnesses after a SUCCESSFUL npm upgrade, and MUST NOT do so when
// the npm upgrade fails.
//
// WHY: the npm update lands a NEW package version on disk; the running process is
// still the OLD launcher. Harness configs may still point at a stale / bare
// command (the bare `terminal-commander-mcp` is ENOENT-fatal on Windows under MCP
// shell:false). So after a clean upgrade `runUpdate` must spawn the FRESHLY
// INSTALLED launcher's `setup harness` (in a NEW process) to rewrite every
// harness config to the new absolute exe path. On npm failure it must NOT
// re-register (nothing new was installed) and must propagate the npm exit code.
//
// HOW: we run `bin/terminal-commander.js update` in a subprocess under a
// `--require` preload that (a) forces a non-win32 platform so the win32 preflight
// is skipped and the flow is deterministic, and (b) monkey-patches
// child_process.spawn to RECORD every spawn argv and synthesize a fake child
// whose exit code we control via env (no real npm, no real setup). The preload
// writes the recorded spawns to a JSON file we then assert on.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

const PKG_ROOT = path.resolve(__dirname, "..");
const BIN_DIR = path.join(PKG_ROOT, "bin");
const UPDATE_SHIM = path.join(BIN_DIR, "terminal-commander.js");

// Build a --require preload that forces platform=linux, fakes child_process
// .spawn, and records each spawn's {command,args} to recordPath. The first
// spawn (npm) exits with NPM_EXIT_CODE; any later spawn (setup harness) exits 0.
function makeSpawnRecorder(tmpDir, recordPath) {
  const injector = path.join(tmpDir, "spawn-recorder.js");
  const body = `
"use strict";
Object.defineProperty(process, "platform", { value: "linux" });
const cp = require("child_process");
const fs = require("fs");
const recordPath = ${JSON.stringify(recordPath)};
const npmExit = Number(process.env.__TEST_NPM_EXIT__ || "0");
let spawnCount = 0;
const { EventEmitter } = require("events");

function record(command, args) {
  let list = [];
  try { list = JSON.parse(fs.readFileSync(recordPath, "utf8")); } catch (_e) { list = []; }
  list.push({ command: String(command), args: (args || []).map(String) });
  fs.writeFileSync(recordPath, JSON.stringify(list), "utf8");
}

const realSpawn = cp.spawn;
cp.spawn = function patchedSpawn(command, args, _opts) {
  record(command, args);
  spawnCount += 1;
  const isFirst = spawnCount === 1;
  const child = new EventEmitter();
  child.pid = 4242;
  child.kill = () => {};
  // First spawn is the npm upgrade -> use the controlled npm exit code.
  // Any later spawn (the setup-harness re-register) -> exit 0.
  const code = isFirst ? npmExit : 0;
  setImmediate(() => child.emit("exit", code, null));
  return child;
};
`;
  fs.writeFileSync(injector, body, "utf8");
  return injector;
}

// Create a fake freshly-installed global launcher entry so globalLauncherEntry()
// resolves (<prefix>/lib/node_modules/terminal-commander/bin/terminal-commander.js
// on non-win32). The file content is irrelevant: spawn is mocked, it never runs.
function makeFakeGlobalLauncher(prefixDir) {
  const entry = path.join(
    prefixDir,
    "lib",
    "node_modules",
    "terminal-commander",
    "bin",
    "terminal-commander.js",
  );
  fs.mkdirSync(path.dirname(entry), { recursive: true });
  fs.writeFileSync(entry, "// fake launcher (never executed; spawn is mocked)\n", "utf8");
  return entry;
}

function runUpdate(npmExitCode) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "tc-update-"));
  try {
    const recordPath = path.join(tmpDir, "spawns.json");
    fs.writeFileSync(recordPath, "[]", "utf8");
    const injector = makeSpawnRecorder(tmpDir, recordPath);
    const prefixDir = path.join(tmpDir, "global-prefix");
    const fakeEntry = makeFakeGlobalLauncher(prefixDir);

    const env = {
      ...process.env,
      __TEST_NPM_EXIT__: String(npmExitCode),
      npm_config_prefix: prefixDir,
    };

    const result = spawnSync(
      process.execPath,
      ["--require", injector, UPDATE_SHIM, "update"],
      {
        encoding: "utf8",
        timeout: 20_000,
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        env,
      },
    );

    let spawns = [];
    try {
      spawns = JSON.parse(fs.readFileSync(recordPath, "utf8"));
    } catch (_e) {
      spawns = [];
    }
    return { result, spawns, fakeEntry };
  } finally {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (_e) {
      /* ignore */
    }
  }
}

function isSetupHarnessSpawn(s) {
  // The re-register spawn is `node <entry> setup harness`. We detect it by the
  // ["setup","harness"] argv suffix (the command is process.execPath; the entry
  // is the fake global launcher path).
  const a = s.args || [];
  for (let i = 0; i + 1 < a.length; i += 1) {
    if (a[i] === "setup" && a[i + 1] === "harness") return true;
  }
  return false;
}

test("update re-registers harnesses (spawns `setup harness`) after a SUCCESSFUL npm upgrade", () => {
  const { result, spawns, fakeEntry } = runUpdate(0);
  assert.equal(result.signal, null, `update must not be killed; stderr=${result.stderr}`);
  assert.equal(result.status, 0, `successful update must exit 0; stderr=${result.stderr}`);

  const setupSpawn = spawns.find(isSetupHarnessSpawn);
  assert.ok(
    setupSpawn,
    `expected a 'setup harness' re-register spawn after a clean upgrade; recorded spawns=${JSON.stringify(spawns)}`,
  );
  // It must spawn node + the freshly-installed global launcher entry, not in-process.
  assert.equal(setupSpawn.command, process.execPath);
  assert.ok(
    setupSpawn.args.includes(fakeEntry),
    `re-register must target the freshly-installed launcher entry ${fakeEntry}; args=${JSON.stringify(setupSpawn.args)}`,
  );
});

test("update does NOT re-register harnesses when the npm upgrade FAILS, and propagates the failure code", () => {
  const { result, spawns } = runUpdate(7);
  // The npm failure code must propagate; the update must be marked failed.
  assert.equal(result.signal, null);
  assert.equal(result.status, 7, `failed npm upgrade must propagate its exit code; stderr=${result.stderr}`);

  const setupSpawn = spawns.find(isSetupHarnessSpawn);
  assert.equal(
    setupSpawn,
    undefined,
    `a failed upgrade must NOT spawn 'setup harness'; recorded spawns=${JSON.stringify(spawns)}`,
  );
});

test("bin/terminal-commander.js wires the re-register into the npm-success branch (static guard)", () => {
  // Structural backstop for the behavioral tests above: the code==0 branch of the
  // npm child's exit handler must invoke reregisterHarnesses, which must spawn
  // `setup harness` with the freshly-installed launcher (never an in-process call).
  const src = fs.readFileSync(UPDATE_SHIM, "utf8");
  assert.match(
    src,
    /if\s*\(\s*code === 0\s*\)\s*\{[\s\S]*?reregisterHarnesses\s*\(/,
    "the npm-success (code===0) branch must call reregisterHarnesses",
  );
  assert.match(
    src,
    /spawn\(\s*process\.execPath\s*,\s*\[\s*entry\s*,\s*"setup"\s*,\s*"harness"\s*\]/,
    "reregisterHarnesses must spawn `node <entry> setup harness` (new process, new binary)",
  );
});
