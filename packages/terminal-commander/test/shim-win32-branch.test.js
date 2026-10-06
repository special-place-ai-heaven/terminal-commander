// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Bin shim win32-branch tests.
//
// Verifies what each bin shim does on a host that reports
// `process.platform === 'win32'`: spawn the resolved native binary directly
// (argv, env, stdio, exit code), or exit 64 with a bounded message when the
// platform package is missing / the target is unsupported. No `wsl.exe`.
//
// The test forces win32 by spawning Node with `--require` pointed at an
// injector that patches `process.platform` / `process.arch` BEFORE the bin
// script loads, and replaces child_process.spawn / spawnSync with recorders.
// The shim runs from a temp copy of the wrapper with a fixture platform
// package, so it can never start a real binary or reach a live daemon.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

const PKG_ROOT = path.resolve(__dirname, "..");
const BIN_DIR  = path.join(PKG_ROOT, "bin");

const { SUPPORTED_TARGETS } = require("../lib/resolve-binary.js");

// Hermetic shim run. The shim executes from a temp copy of the wrapper (bin/,
// lib/, package.json) whose only platform package is a fixture this test
// writes, and with child_process.spawn / spawnSync replaced by recorders.
// So no real binary - not this checkout's platform-package links, not a
// developer's stale build - can start or reach a live daemon, and the result
// is identical on CI and on a dev machine.
function makeInjector(tmpDir, platformValue, archValue, recordPath) {
  const injector = path.join(tmpDir, "inject.js");
  const body = `
"use strict";
Object.defineProperty(process, "platform", { value: ${JSON.stringify(platformValue)} });
Object.defineProperty(process, "arch", { value: ${JSON.stringify(archValue)} });
const cp = require("child_process");
const fs = require("fs");
const { EventEmitter } = require("events");
function record(kind, command, args, opts) {
  const o = opts || {};
  const list = JSON.parse(fs.readFileSync(${JSON.stringify(recordPath)}, "utf8"));
  list.push({
    kind,
    command: String(command),
    args: (args || []).map(String),
    shell: o.shell,
    stdio: o.stdio,
    envProvided: o.env != null,
    TC_SUPERVISOR_ALLOW_SPAWN: o.env ? o.env.TC_SUPERVISOR_ALLOW_SPAWN : undefined,
  });
  fs.writeFileSync(${JSON.stringify(recordPath)}, JSON.stringify(list), "utf8");
}
cp.spawn = function stubSpawn(command, args, opts) {
  record("spawn", command, args, opts);
  const child = new EventEmitter();
  child.pid = 4242;
  child.kill = () => true;
  setImmediate(() => child.emit("exit", Number(process.env.__TEST_CHILD_EXIT__ || "0"), null));
  return child;
};
cp.spawnSync = function stubSpawnSync(command, args, opts) {
  record("spawnSync", command, args, opts);
  return { status: 1, signal: null, stdout: "", stderr: "", output: [] };
};
`;
  fs.writeFileSync(injector, body, "utf8");
  return injector;
}

function runShim(shimName, platform, arch, opts) {
  const o = opts || {};
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wws02-shim-"));
  try {
    const wrapper = path.join(tmpDir, "wrapper");
    for (const dir of ["bin", "lib"]) {
      fs.cpSync(path.join(PKG_ROOT, dir), path.join(wrapper, dir), { recursive: true });
    }
    fs.copyFileSync(path.join(PKG_ROOT, "package.json"), path.join(wrapper, "package.json"));
    let fixtureBin = null;
    if (o.fixture) {
      const target = SUPPORTED_TARGETS.find((t) => t.platform === platform && t.arch === arch);
      const pkgDir = path.join(wrapper, "node_modules", ...target.pkg.split("/"));
      fixtureBin = path.join(pkgDir, "bin");
      fs.mkdirSync(fixtureBin, { recursive: true });
      fs.writeFileSync(path.join(pkgDir, "package.json"), JSON.stringify({ name: target.pkg, version: "0.0.0" }));
      for (const binary of ["terminal-commanderd", "terminal-commander-mcp", "terminal-commander"]) {
        const name = platform === "win32" ? `${binary}.exe` : binary;
        fs.writeFileSync(path.join(fixtureBin, name), "fixture: never executed (spawn is stubbed)");
      }
    }
    const recordPath = path.join(tmpDir, "spawns.json");
    fs.writeFileSync(recordPath, "[]", "utf8");
    const injector = makeInjector(tmpDir, platform, arch, recordPath);
    const env = { ...process.env, ...(o.env || {}) };
    delete env.TC_USE_LEGACY_WSL_BRIDGE;
    const result = spawnSync(
      process.execPath,
      ["--require", injector, path.join(wrapper, "bin", shimName), ...(o.args || [])],
      {
        encoding: "utf8",
        timeout: 15_000,
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        env,
      },
    );
    return {
      ...result,
      spawns: JSON.parse(fs.readFileSync(recordPath, "utf8")),
      fixtureBin: fixtureBin && fs.realpathSync(fixtureBin),
    };
  } finally {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (_e) {
      /* ignore */
    }
  }
}

test("terminal-commanderd.js on win32 spawns the resolved native daemon and mirrors its exit code", () => {
  // win32-x64 resolves @terminal-commander/windows-x64; the shim must spawn
  // its terminal-commanderd.exe directly (no WSL refusal, no shell), forward
  // argv verbatim, and exit with the child's code.
  const r = runShim("terminal-commanderd.js", "win32", "x64", {
    fixture: true,
    args: ["start", "--mode", "ipc-server"],
    env: { __TEST_CHILD_EXIT__: "3" },
  });
  assert.equal(r.signal, null);
  assert.equal(r.status, 3, `child exit code must be mirrored; stderr=${r.stderr}`);
  assert.equal(r.stdout, "");
  assert.equal(r.stderr.includes("terminal-commanderd runs only inside Linux"), false);
  assert.equal(r.spawns.length, 1, JSON.stringify(r.spawns));
  const [call] = r.spawns;
  assert.equal(call.kind, "spawn");
  assert.equal(path.normalize(call.command), path.join(r.fixtureBin, "terminal-commanderd.exe"));
  assert.deepEqual(call.args, ["start", "--mode", "ipc-server"]);
  assert.equal(call.shell, false);
  assert.equal(call.stdio, "inherit");
});

test("terminal-commanderd.js on win32 without the platform package exits 64 and spawns nothing", () => {
  const r = runShim("terminal-commanderd.js", "win32", "x64");
  assert.equal(r.signal, null);
  assert.equal(r.status, 64, `stderr=${r.stderr}`);
  assert.equal(r.stdout, "");
  assert.match(r.stderr, /platform package @terminal-commander\/windows-x64 not installed/);
  assert.deepEqual(r.spawns, []);
});

test("terminal-commander-mcp.js on win32 uses native direct-spawn path (Phase 3)", () => {
  // Phase 3: win32-x64 is a supported target. The mcp shim never enters the
  // WWS04 WSL bridge path; it spawns the resolved native MCP binary with the
  // caller's argv and env, keeps stdout untouched (rmcp framing), and
  // mirrors the child's exit code.
  const r = runShim("terminal-commander-mcp.js", "win32", "x64", {
    fixture: true,
    args: ["--surface", "compact"],
    env: { TC_SUPERVISOR_ALLOW_SPAWN: "0", __TEST_CHILD_EXIT__: "0" },
  });
  assert.equal(r.signal, null);
  assert.equal(r.status, 0, `stderr=${r.stderr}`);
  assert.equal(r.stdout, "", "shim must write nothing to stdout (rmcp framing)");
  assert.equal(r.stderr.includes("Spawned wsl"), false);
  assert.equal(r.stderr.includes("no distro"), false);
  assert.equal(r.stderr.includes("wsl.exe not found"), false);
  assert.equal(r.spawns.length, 1, JSON.stringify(r.spawns));
  const [call] = r.spawns;
  assert.equal(path.normalize(call.command), path.join(r.fixtureBin, "terminal-commander-mcp.exe"));
  assert.deepEqual(call.args, ["--surface", "compact"]);
  assert.equal(call.shell, false);
  assert.equal(call.stdio, "inherit");
  assert.equal(call.envProvided, true);
  assert.equal(call.TC_SUPERVISOR_ALLOW_SPAWN, "0");
});

test("terminal-commander.js on win32 arm64 exits 64 with unsupported_platform message", () => {
  // win32-arm64 is NOT in SUPPORTED_TARGETS (only win32-x64 was added).
  // The shim calls resolveBinary({platform:'win32', arch:'arm64'}) which
  // returns unsupported_platform; formatResolveError is called and the
  // shim exits 64 with a bounded stderr message.
  const r = runShim("terminal-commander.js", "win32", "arm64");
  assert.equal(r.status, 64, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  assert.match(r.stderr, /unsupported platform win32-arm64/);
  // Message must mention at least one supported target.
  assert.match(r.stderr, /win32-x64/);
  assert.deepEqual(r.spawns, []);
});

test("shim bin/* files contain no wsl.exe literal invocation in executable code (Phase 3 contract)", () => {
  // Phase 3 contract: the three shim files MUST NOT literally spawn wsl.exe.
  // terminal-commanderd.js and terminal-commander.js still use
  // spawn(result.binaryPath, ...) for the native binary path.
  // terminal-commander-mcp.js also spawns result.binaryPath directly so
  // Cursor sees a plain native MCP child, not a hidden Node supervisor.
  function stripCommentsAndStrings(src) {
    let out = "";
    let i = 0;
    const n = src.length;
    while (i < n) {
      const c = src[i];
      const c2 = src[i + 1];
      // Line comment.
      if (c === "/" && c2 === "/") {
        while (i < n && src[i] !== "\n") i++;
        continue;
      }
      // Block comment.
      if (c === "/" && c2 === "*") {
        i += 2;
        while (i < n && !(src[i] === "*" && src[i + 1] === "/")) i++;
        i += 2;
        continue;
      }
      // String literals: ", ', `
      if (c === '"' || c === "'" || c === "`") {
        const quote = c;
        i++;
        while (i < n && src[i] !== quote) {
          if (src[i] === "\\" && i + 1 < n) {
            i += 2;
          } else {
            i++;
          }
        }
        i++;
        out += " ";
        continue;
      }
      out += c;
      i++;
    }
    return out;
  }

  for (const shim of [
    "terminal-commanderd.js",
    "terminal-commander-mcp.js",
    "terminal-commander.js",
  ]) {
    const body = fs.readFileSync(path.join(BIN_DIR, shim), "utf8");
    const codeOnly = stripCommentsAndStrings(body);
    assert.equal(
      /wsl\.exe/i.test(codeOnly),
      false,
      `${shim} must not reference wsl.exe in executable code (only comments / hint strings allowed): code-only excerpt = ${codeOnly.slice(0, 400)}`,
    );
    assert.equal(
      /\bspawn\s*\(\s*['"`]wsl/i.test(codeOnly),
      false,
      `${shim} must not spawn('wsl', ...) in executable code: code-only excerpt = ${codeOnly.slice(0, 400)}`,
    );
  }

  // All three shims spawn result.binaryPath.
  for (const shim of ["terminal-commanderd.js", "terminal-commander.js", "terminal-commander-mcp.js"]) {
    const body = fs.readFileSync(path.join(BIN_DIR, shim), "utf8");
    const codeOnly = stripCommentsAndStrings(body);
    assert.match(
      codeOnly,
      /\bspawn\s*\(\s*result\.binaryPath/,
      `${shim} must spawn result.binaryPath (not a literal command)`,
    );
  }

  // terminal-commander-mcp.js (Phase 3) must not route through the
  // session supervisor. The legacy WSL bridge path (spawnWslBridge) is
  // still present but gated behind TC_USE_LEGACY_WSL_BRIDGE=1.
  const mcpBody = fs.readFileSync(path.join(BIN_DIR, "terminal-commander-mcp.js"), "utf8");
  const mcpCode = stripCommentsAndStrings(mcpBody);
  assert.equal(/runHarnessMcpSession/.test(mcpCode), false);
  assert.equal(/session_supervisor/.test(mcpBody), false);
  assert.equal(/windowsHide/.test(mcpBody), false);
});

test("terminal-commander.js routes the `restart` verb into the JS CLI (F3 wiring)", () => {
  // The `restart` verb is implemented in lib/cli/run.js, NOT the native Rust
  // binary (whose clap enum has no `restart`). The bin shim's isJsCliRequest
  // gate MUST include `restart`, or `terminal-commander restart` falls through
  // to spawn(result.binaryPath, ["restart"]) and the native CLI errors.
  const src = fs.readFileSync(path.join(BIN_DIR, "terminal-commander.js"), "utf8");
  // isJsCliRequest must treat `restart` as a JS-CLI command alongside setup/pair.
  assert.match(
    src,
    /command === "setup" \|\| command === "pair" \|\| command === "restart"/,
    "isJsCliRequest must route `restart` into lib/cli/run.js (F3); otherwise the verb is unreachable from the installed package",
  );
});

test("terminal-commander-mcp.js delegates to lib/wsl/spawn.js on bridge_required (WWS04 wiring)", () => {
  // Static guard for the WWS04 wiring: the mcp shim must require()
  // ../lib/wsl/spawn.js and call spawnWslBridge() inside the
  // bridge_required branch. The daemon + admin-CLI shims MUST NOT
  // require lib/wsl/spawn.js (they stay byte-identical to the WWS02
  // contract).
  const mcpSrc = fs.readFileSync(path.join(BIN_DIR, "terminal-commander-mcp.js"), "utf8");
  assert.match(mcpSrc, /require\(\s*['"]\.\.\/lib\/wsl\/spawn\.js['"]\s*\)/);
  assert.match(mcpSrc, /\bspawnWslBridge\s*\(/);
  for (const shim of ["terminal-commanderd.js", "terminal-commander.js"]) {
    const src = fs.readFileSync(path.join(BIN_DIR, shim), "utf8");
    assert.equal(
      /require\(\s*['"]\.\.\/lib\/wsl\/spawn\.js['"]\s*\)/.test(src),
      false,
      `${shim} must NOT require lib/wsl/spawn.js (WWS04 keeps these shims byte-identical to WWS02)`,
    );
    assert.equal(
      /spawnWslBridge/.test(src),
      false,
      `${shim} must NOT call spawnWslBridge (WWS04 keeps these shims byte-identical to WWS02)`,
    );
  }
});
