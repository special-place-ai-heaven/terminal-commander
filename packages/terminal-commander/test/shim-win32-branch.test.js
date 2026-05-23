// SPDX-License-Identifier: Apache-2.0
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

function runShim(shimName, platform, arch) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wws02-shim-"));
  try {
    const injector = makePlatformInjector(tmpDir, platform, arch);
    const shimPath = path.join(BIN_DIR, shimName);
    const result = spawnSync(
      process.execPath,
      ["--require", injector, shimPath],
      {
        encoding: "utf8",
        timeout: 10_000,
        // No shell. No stdin. We DO inherit nothing — capture
        // stderr/stdout into the buffer so we can assert on them.
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
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

test("terminal-commanderd.js on win32 refuses with exit 64 and bounded message", () => {
  const r = runShim("terminal-commanderd.js", "win32", "x64");
  assert.equal(r.status, 64, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  assert.equal(r.stdout, "");
  // Single stderr line + a trailing newline.
  assert.equal(r.stderr.endsWith("\n"), true);
  assert.equal(r.stderr.split("\n").filter(Boolean).length, 1);
  assert.match(r.stderr, /terminal-commanderd runs only inside Linux \/ WSL/);
  assert.match(r.stderr, /WSL distro/);
  // Must NOT mention wsl.exe execution by THIS process.
  // (The hint may name a wsl command for the operator to copy, but
  // the shim itself ran nothing.)
  assert.equal(r.stderr.includes("Spawned wsl"), false);
});

test("terminal-commander-mcp.js on win32 enters bridge-required stub with exit 64 (pending WWS04)", () => {
  const r = runShim("terminal-commander-mcp.js", "win32", "x64");
  assert.equal(r.status, 64, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  assert.equal(r.stdout, "");
  assert.equal(r.stderr.endsWith("\n"), true);
  assert.equal(r.stderr.split("\n").filter(Boolean).length, 1);
  assert.match(r.stderr, /Windows host bridge mode is pending WWS04/);
});

test("terminal-commander.js on win32 prints bounded WWS06 hint with exit 64", () => {
  const r = runShim("terminal-commander.js", "win32", "arm64");
  assert.equal(r.status, 64, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  assert.equal(r.stdout, "");
  assert.equal(r.stderr.endsWith("\n"), true);
  assert.equal(r.stderr.split("\n").filter(Boolean).length, 1);
  assert.match(r.stderr, /setup \/ doctor \/ pair subcommands are pending WWS06/);
});

test("shim bin/* files contain no wsl.exe invocation in WWS02 (executable code only)", () => {
  // Static check: the three shim files must not invoke the
  // `wsl.exe` bridge in executable code. WWS04 introduces
  // lib/wsl/spawn.js; until then, the shims MUST exit-64 instead
  // of invoking the bridge. References to `wsl.exe` inside
  // // comments or inside bounded stderr-hint strings are allowed
  // (the hints tell the operator the exact WSL command to run by
  // hand). We strip comments and quoted string literals before
  // grepping, so only executable code is checked.
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
    // Defense in depth: the spawn() call in each shim must spawn
    // the resolved Linux binary path returned by the resolver,
    // never a literal wsl invocation.
    assert.match(
      codeOnly,
      /\bspawn\s*\(\s*result\.binaryPath/,
      `${shim} must spawn result.binaryPath, not a literal command`,
    );
  }
});
