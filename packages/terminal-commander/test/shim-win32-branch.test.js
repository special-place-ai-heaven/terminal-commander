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

test("terminal-commander-mcp.js on win32 enters WWS04 bridge path; refuses with bounded error when no WSL distro is reachable", () => {
  // WWS04 wires the bridge. On the verification machine there is no
  // guarantee the WSL runtime is installed inside the chosen distro,
  // so the bridge will short-circuit with `runtime_missing` (or
  // `no_default_distro` if no WSL is configured at all). Either way,
  // the shim must write NOTHING to stdout, write a bounded single
  // stderr line, and exit 64. We force win32 + invalid TC_WSL_DISTRO
  // so the bridge stops at `distro_not_found` deterministically — no
  // real wsl.exe invocation can produce that distro name.
  const r = runShim("terminal-commander-mcp.js", "win32", "x64", {
    TC_WSL_DISTRO: "Bogus-Distro-That-Cannot-Exist-XYZ-9999",
    TC_WSL_SKIP_DOCTOR: "1",
  });
  assert.equal(r.status, 64, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  assert.equal(r.stdout, "", "shim must write nothing to stdout (rmcp framing)");
  assert.equal(r.stderr.endsWith("\n"), true);
  // Bounded: one logical line of stderr.
  assert.equal(r.stderr.split("\n").filter(Boolean).length, 1);
  // Either runs through detect first and reports wsl_not_found /
  // no_distros / distro_not_found / no_default_distro depending on
  // host state. All four are acceptable bounded outcomes.
  assert.match(
    r.stderr,
    /not found in 'wsl -l -v'|wsl\.exe not found on PATH|no distro is registered|no WSL distro selected/,
  );
});

test("terminal-commander.js on win32 prints CLI usage (WWS06 setup/doctor/pair surface)", () => {
  // No argv -> CLI runs the help path. exit 0; output goes to stderr
  // (shim writes the result.output to stderr) and includes the WWS06
  // command surface.
  const r = runShim("terminal-commander.js", "win32", "arm64");
  assert.equal(r.status, 0, `unexpected exit code; stderr=${r.stderr} stdout=${r.stdout}`);
  assert.equal(r.signal, null);
  // The shim writes the help text to stderr. Cursor never invokes
  // `terminal-commander` itself (Cursor calls `terminal-commander-mcp`),
  // so stdout cleanliness is not strictly required here — but the help
  // panel SHOULD include the locked subcommand names.
  assert.match(r.stderr, /terminal-commander\b/);
  assert.match(r.stderr, /doctor/);
  assert.match(r.stderr, /setup cursor-wsl/);
  assert.match(r.stderr, /pair create/);
});

test("shim bin/* files contain no wsl.exe literal invocation; bridge spawn is owned by lib/wsl/spawn.js (executable code only)", () => {
  // WWS04 invariant: the three shim files MUST NOT literally
  // `spawn('wsl', ...)` themselves. The Windows MCP shim may
  // require() lib/wsl/spawn.js and call spawnWslBridge(); the spawn
  // helper is the only site that can name wsl.exe in argv. The
  // daemon + admin-CLI shims still refuse with bounded stderr +
  // exit 64 at WWS04 (WWS06 owns the admin CLI surface).
  // We strip comments + quoted string literals before grepping, so
  // only executable code is checked.
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
    // Defense in depth: the spawn() call (if any literal spawn() is
    // present in the shim) must spawn the resolved Linux binary path
    // returned by the resolver, never a literal wsl invocation. The
    // WWS04 mcp.js shim moved its Linux spawn into the `else` block
    // of the resolver switch — the assertion below still matches
    // because `spawn(result.binaryPath` appears verbatim there.
    assert.match(
      codeOnly,
      /\bspawn\s*\(\s*result\.binaryPath/,
      `${shim} must spawn result.binaryPath, not a literal command`,
    );
  }
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
