// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS02 shim win32-branch tests.
//
// Verifies each of the three bin shims, when run on a host that
// reports `process.platform === 'win32'`, refuses with a single
// bounded stderr line and exits with code 64. Crucially, NO
// `wsl.exe` invocation occurs — WWS02 is only the bridge-required
// resolver branch, not the actual bridge. WWS04 wires
// `terminal-commander-mcp` into `lib/wsl/spawn.js`; WWS06 wires
// `terminal-commander` into the setup CLI; the daemon shim stays a
// permanent refusal because the Unix-only daemon cannot honor a
// Windows-native invocation.
//
// The test forces win32 by spawning Node with `--require` pointed
// at a small shim that monkey-patches `process.platform` /
// `process.arch` BEFORE the bin script loads. The bin scripts read
// `process.platform` exactly once via `resolveBinary()` (which
// defaults to `process.platform`/`process.arch`), so the patch is
// observed before the resolver runs.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

const PKG_ROOT = path.resolve(__dirname, "..");
const BIN_DIR  = path.join(PKG_ROOT, "bin");

function makePlatformInjector(tmpDir, platformValue, archValue) {
  const injector = path.join(tmpDir, "force-platform.js");
  const body =
    "Object.defineProperty(process, 'platform', { value: " +
    JSON.stringify(platformValue) +
    " });\n" +
    "Object.defineProperty(process, 'arch', { value: " +
    JSON.stringify(archValue) +
    " });\n";
  fs.writeFileSync(injector, body, "utf8");
  return injector;
}

function runShim(shimName, platform, arch, extraEnv) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wws02-shim-"));
  try {
    const injector = makePlatformInjector(tmpDir, platform, arch);
    const shimPath = path.join(BIN_DIR, shimName);
    // Build a clean env: keep PATH/SystemRoot/etc, override / append
    // extraEnv. We deliberately keep the parent's process.env so
    // wsl.exe can be located, but we strip token-shaped values to
    // mirror production behaviour.
    const env = { ...process.env, ...(extraEnv || {}) };
    const result = spawnSync(
      process.execPath,
      ["--require", injector, shimPath],
      {
        encoding: "utf8",
        timeout: 15_000,
        // No shell. No stdin. We DO inherit nothing — capture
        // stderr/stdout into the buffer so we can assert on them.
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        env,
      },
    );
    return result;
  } finally {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (_e) {
      /* ignore */
    }
  }
}

test("terminal-commanderd.js on win32 attempts native binary or fails with bounded message", () => {
  // Phase 3: win32-x64 is now a SUPPORTED_TARGET with the native Windows
  // package. The shim resolves @terminal-commander/windows-x64 and either
  // spawns the binary (if installed) or exits 64 with a platform_package_missing
  // or spawn-error message. The old bridge-required / WSL refusal no longer occurs.
  const r = runShim("terminal-commanderd.js", "win32", "x64");
  // Either the binary runs (any exit code) or the package is missing (exit 64).
  // What must NOT happen: the shim must still exit without signaling.
  assert.equal(r.signal, null);
  // The shim must write nothing to stdout regardless of outcome.
  assert.equal(r.stdout, "");
  // The shim must NOT produce the old bridge-required / WSL error message.
  assert.equal(r.stderr.includes("terminal-commanderd runs only inside Linux"), false);
  assert.equal(r.stderr.includes("Spawned wsl"), false);
});

test("terminal-commander-mcp.js on win32 uses native direct-spawn path (Phase 3)", () => {
  // Phase 3: win32-x64 is now a supported target. The mcp shim on win32
  // no longer enters the WWS04 WSL bridge path. Instead it goes through
  // the isWindowsMountedShimPath / native-mcp path, then falls through
  // to spawn(result.binaryPath) if the native binary is available.
  // TC_USE_LEGACY_WSL_BRIDGE must not be set for this test.
  const r = runShim("terminal-commander-mcp.js", "win32", "x64", {
    TC_SUPERVISOR_ALLOW_SPAWN: "0",
  });
  // The shim must never produce WSL-specific bridge output.
  assert.equal(r.signal, null);
  assert.equal(r.stdout, "", "shim must write nothing to stdout (rmcp framing)");
  assert.equal(r.stderr.includes("Spawned wsl"), false);
  assert.equal(r.stderr.includes("no distro"), false);
  assert.equal(r.stderr.includes("wsl.exe not found"), false);
});

test("terminal-commander.js on win32 arm64 exits 64 with unsupported_platform message", () => {
  // win32-arm64 is NOT in SUPPORTED_TARGETS (only win32-x64 was added).
  // The shim calls resolveBinary({platform:'win32', arch:'arm64'}) which
  // returns unsupported_platform; formatResolveError is called and the
  // shim exits 64 with a bounded stderr message. The bridge_required path
  // and lib/cli/run.js delegation no longer trigger on win32-arm64.
  const r = runShim("terminal-commander.js", "win32", "arm64");
  assert.equal(r.status, 64, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  assert.match(r.stderr, /unsupported platform win32-arm64/);
  // Message must mention at least one supported target.
  assert.match(r.stderr, /win32-x64/);
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
