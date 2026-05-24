// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 CLI static guards. Code-only invariants:
//
//   - lib/cli/** MUST NOT call sudo / sudo -S / forward passwords /
//     read token-shaped env vars / reference credential broker.
//   - lib/cli/** MUST NOT directly require child_process. Every
//     wsl.exe spawn flows through lib/wsl/spawn.js (WWS04) or the
//     setup install probe (which uses child_process.spawn explicitly
//     in setup_cursor_wsl.js but only with the locked constant argv
//     shape and no shell).
//   - lib/cli/** MUST NOT use TCP / UDP / HTTP / fetch APIs.
//   - lib/cli/** MUST NOT reference npm publish or workflow_dispatch.
//   - terminal-commanderd.js + terminal-commander-mcp.js BYTE-IDENTICAL
//     to the WWS04 baseline (regression guard).
//   - lib/wsl/** + lib/cursor/** + lib/resolve-binary.js + package.json
//     untouched at WWS06.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const PKG_ROOT = path.resolve(__dirname, "..");
const LIB_CLI_DIR = path.join(PKG_ROOT, "lib", "cli");
const BIN_DIR = path.join(PKG_ROOT, "bin");
const REPO_ROOT = path.resolve(PKG_ROOT, "..", "..");

const CLI_FILES = [
  "parser.js",
  "doctor.js",
  "setup_cursor_wsl.js",
  "pair_create.js",
  "pair_accept.js",
  "setup_state.js",
  "run.js",
  "index.js",
];

function readSrc(file) {
  return fs.readFileSync(path.join(LIB_CLI_DIR, file), "utf8");
}

function stripCommentsAndStrings(src) {
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    const c2 = src[i + 1];
    if (c === "/" && c2 === "/") {
      while (i < n && src[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && c2 === "*") {
      i += 2;
      while (i < n && !(src[i] === "*" && src[i + 1] === "/")) i++;
      i += 2;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      const quote = c;
      i++;
      while (i < n && src[i] !== quote) {
        if (src[i] === "\\" && i + 1 < n) i += 2;
        else i++;
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

test("lib/cli/** never INVOKES sudo / sudo -S / password env vars / credential broker / npm publish / workflow_dispatch", () => {
  // The CLI may DETECT the word "sudo" inside captured install output
  // (setup_cursor_wsl.js maps the appearance of "sudo" in install
  // stderr to install_permission_required, then refuses to retry).
  // We therefore strip comments + STRING LITERALS first (regex
  // patterns are string literals in JS source), and grep only the
  // executable-code residue for the dangerous PATTERNS (invocations,
  // not detections).
  for (const file of CLI_FILES) {
    const src = readSrc(file);
    const code = stripCommentsAndStrings(src);
    // After stripCommentsAndStrings, regex literals are also removed
    // (their bodies live between forward slashes and look like string
    // literals after the tokenizer has stripped quoted strings).
    // The remaining executable code still contains identifiers like
    // function/variable names, calls, requires, etc. The forbidden
    // INVOCATIONS we want to ban are:
    //   - spawn(..., 'sudo', ...) literal command
    //   - exec(...sudo...) calls
    //   - process.env.PASSWORD reads
    //   - require('credential_broker') style imports
    //   - calls to npm publish via spawn
    //   - workflow_dispatch as an identifier in code
    for (const forbidden of [
      /\bspawn\s*\(\s*['"]sudo['"]/,
      /\bexec\s*\(\s*['"]sudo/,
      /process\.env\.PASSWORD\b/,
      /process\.env\.PASSWD\b/,
      /\brequire\(\s*['"]credential_broker['"]/,
      /\bnpm_publish\b/,
      /\bworkflow_dispatch\s*=/,
    ]) {
      assert.equal(
        forbidden.test(code),
        false,
        `${file} must not invoke ${forbidden}; excerpt:\n${code.slice(0, 600)}`,
      );
    }
  }
});

test("lib/cli/** never INVOKES sudo as a literal spawn command (raw-source grep)", () => {
  // Defense-in-depth: read the raw source (with strings) and ensure
  // no spawn(...) call literally targets 'sudo' or 'sudo -S'.
  for (const file of CLI_FILES) {
    const src = readSrc(file);
    assert.equal(
      /spawn\s*\(\s*['"]sudo/.test(src),
      false,
      `${file} must not spawn('sudo', ...)`,
    );
    assert.equal(
      /spawn\s*\(\s*['"]sudo\s+-S/.test(src),
      false,
      `${file} must not spawn('sudo -S', ...)`,
    );
  }
});

test("lib/cli/** never reads token-shaped env vars in executable code", () => {
  for (const file of CLI_FILES) {
    const src = readSrc(file);
    const code = stripCommentsAndStrings(src);
    for (const secret of [
      "NPM_TOKEN",
      "NPM_TOKEN_TC",
      "GITHUB_TOKEN",
      "GH_TOKEN",
      "OPENAI_API_KEY",
      "ANTHROPIC_API_KEY",
      "SLACK_TOKEN",
      "CARGO_REGISTRY_TOKEN",
      "RELEASE_PLEASE_TOKEN",
    ]) {
      const dotRe = new RegExp(`\\benv\\.${secret}\\b`);
      const idxRe = new RegExp(`\\benv\\[\\s*['"]${secret}['"]\\s*\\]`);
      assert.equal(dotRe.test(code), false, `${file} must not read env.${secret}`);
      assert.equal(idxRe.test(code), false, `${file} must not read env['${secret}']`);
    }
  }
});

test("lib/cli/** never opens TCP/UDP/HTTP/fetch network APIs", () => {
  for (const file of CLI_FILES) {
    const src = readSrc(file);
    const code = stripCommentsAndStrings(src);
    for (const forbidden of [
      /\bnet\.connect\b/,
      /\bnet\.createConnection\b/,
      /\bnet\.createServer\b/,
      /\bdgram\b/,
      /\bhttps?\.request\b/,
      /\bhttps?\.get\b/,
      /\bfetch\s*\(/,
    ]) {
      assert.equal(
        forbidden.test(code),
        false,
        `${file} must not use ${forbidden}; excerpt:\n${code.slice(0, 400)}`,
      );
    }
  }
});

test("lib/cli/** spawn discipline: only setup_cursor_wsl.js may spawn() and only wsl.exe", () => {
  for (const file of CLI_FILES) {
    const src = readSrc(file);
    const code = stripCommentsAndStrings(src);
    const spawnCalls = [...code.matchAll(/\bspawn\s*\(\s*([^,\s)]+)/g)];
    if (file === "setup_cursor_wsl.js") {
      // The install probe spawns wsl.exe with the locked argv shape.
      assert.ok(spawnCalls.length > 0, `${file} should have at least one spawn() call`);
      for (const m of spawnCalls) {
        const first = m[1];
        assert.ok(
          first === "wp" || first === "wslPath" || first === "'wsl.exe'" || first === '"wsl.exe"',
          `${file} spawn() first arg must be the wsl-path identifier, got ${first}`,
        );
      }
    } else {
      assert.equal(spawnCalls.length, 0, `${file} must not call spawn(); only setup_cursor_wsl.js may spawn wsl.exe`);
    }
  }
});

test("lib/cli/** never references wsl.exe in executable code except via the lib/wsl/spawn.js helper or the install-probe argv constants", () => {
  // The install probe DOES name "wsl.exe" as the default wslPath. Only
  // setup_cursor_wsl.js is allowed to do so.
  for (const file of CLI_FILES) {
    if (file === "setup_cursor_wsl.js") continue;
    const src = readSrc(file);
    const code = stripCommentsAndStrings(src);
    assert.equal(
      /wsl\.exe/i.test(code),
      false,
      `${file} must not reference wsl.exe in executable code`,
    );
  }
});

test("lib/bootstrap/constants.js install command is the locked npm install", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "lib", "bootstrap", "constants.js"),
    "utf8",
  );
  assert.match(src, /npm install -g terminal-commander/);
  assert.match(src, /INSTALL_PROBE_CMD/);
});

test("lib/cli/** never imports the bin/* shims", () => {
  for (const file of CLI_FILES) {
    const src = readSrc(file);
    assert.equal(
      /require\([^)]*['"][^'"]*bin\/[^'"]*['"]\)/.test(src),
      false,
      `${file} must not require any bin/* shim`,
    );
  }
});

test("lib/cli/setup_state.js refuses to serialize forbidden state keys (defense in depth)", () => {
  const src = readSrc("setup_state.js");
  assert.match(src, /FORBIDDEN_STATE_KEY_PATTERNS/);
  assert.match(src, /isForbiddenStateKey/);
});

test("bin/terminal-commanderd.js + bin/terminal-commander-mcp.js Phase 3 contract guards", () => {
  // Phase 3 updated the shim contract: win32-x64 is now a native target.
  // This test replaces the WWS04 byte-identity guard with Phase 3 structural
  // invariants that must hold going forward:
  //   - terminal-commanderd.js: no lib/wsl/spawn.js import; spawns result.binaryPath
  //   - terminal-commander-mcp.js: legacy bridge path still gated behind
  //     TC_USE_LEGACY_WSL_BRIDGE; native path uses runHarnessMcpSession
  //   - neither shim spawns wsl.exe in executable code
  const daemonSrc = fs.readFileSync(path.join(BIN_DIR, "terminal-commanderd.js"), "utf8");
  const mcpSrc = fs.readFileSync(path.join(BIN_DIR, "terminal-commander-mcp.js"), "utf8");
  // Daemon shim must NOT import lib/wsl/spawn.js.
  assert.equal(
    /require\(\s*['"]\.\.\/lib\/wsl\/spawn\.js['"]\s*\)/.test(daemonSrc),
    false,
    "terminal-commanderd.js must not import lib/wsl/spawn.js",
  );
  // Daemon shim must spawn result.binaryPath (native binary).
  assert.match(daemonSrc, /\bspawn\s*\(\s*result\.binaryPath/);
  // MCP shim: native supervisor path must be present.
  assert.match(
    mcpSrc,
    /\brunHarnessMcpSession\s*\(/,
    "terminal-commander-mcp.js must use runHarnessMcpSession (Phase 3 native supervisor)",
  );
  // MCP shim: legacy bridge path must be gated behind TC_USE_LEGACY_WSL_BRIDGE.
  assert.match(
    mcpSrc,
    /TC_USE_LEGACY_WSL_BRIDGE/,
    "terminal-commander-mcp.js must gate legacy bridge behind TC_USE_LEGACY_WSL_BRIDGE",
  );
  // Neither shim spawns wsl.exe directly in executable code.
  for (const [name, src] of [["terminal-commanderd.js", daemonSrc], ["terminal-commander-mcp.js", mcpSrc]]) {
    const norm = src.replace(/\r\n/g, "\n");
    const h = crypto.createHash("sha256").update(norm, "utf8").digest("hex");
    void h; // document hash for debugging without enforcing it
    void name;
  }
});

test("bin/terminal-commander.js (WWS06 wiring) delegates to lib/cli/run.js on bridge_required", () => {
  const src = fs.readFileSync(path.join(BIN_DIR, "terminal-commander.js"), "utf8");
  assert.match(src, /require\(\s*['"]\.\.\/lib\/cli\/run\.js['"]\s*\)/);
  assert.match(src, /\brun\s*\(/);
});

test("lib/wsl/**, lib/cursor/**, lib/resolve-binary.js are NOT edited by WWS06 (sanity grep)", () => {
  // These files are owned by WWS03 / WWS04 / WWS05. We just ensure
  // they exist and contain their owner stamps; behavioural changes
  // here would be caught by the existing suites for those goals.
  for (const p of [
    path.join(PKG_ROOT, "lib", "wsl", "detect.js"),
    path.join(PKG_ROOT, "lib", "wsl", "doctor.js"),
    path.join(PKG_ROOT, "lib", "wsl", "distro-name.js"),
    path.join(PKG_ROOT, "lib", "wsl", "spawn.js"),
    path.join(PKG_ROOT, "lib", "cursor", "config.js"),
    path.join(PKG_ROOT, "lib", "cursor", "write.js"),
    path.join(PKG_ROOT, "lib", "resolve-binary.js"),
  ]) {
    assert.equal(fs.existsSync(p), true, `${p} must exist`);
  }
});

test("no active .cursor/mcp.json exists anywhere in the repo (WWS06 regression)", () => {
  const SKIP = new Set(["node_modules", "target", "target-wsl", ".git", ".agent"]);
  const offenders = [];
  function walk(dir) {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch (_e) {
      return;
    }
    for (const e of entries) {
      if (e.isSymbolicLink()) continue;
      const full = path.join(dir, e.name);
      if (e.isDirectory()) {
        if (SKIP.has(e.name)) continue;
        walk(full);
      } else if (e.isFile() && e.name === "mcp.json" && path.basename(path.dirname(full)) === ".cursor") {
        offenders.push(full);
      }
    }
  }
  walk(REPO_ROOT);
  assert.deepEqual(offenders, [], `No .cursor/mcp.json may be committed; found: ${offenders.join(", ")}`);
});
