// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS03 static guards for the WSL helper layer.
//
// These tests are CI guard-rails: they enforce that the WSL helper
// code (lib/wsl/**) never references install / sudo / pair /
// mcp.json / file-write APIs, never spawns anything other than
// `wsl.exe`, and never imports any bin/* shim. They also enforce
// that the three bin/* shims from WWS02 stay byte-identical to the
// WWS02 baseline (no bridge launch added at WWS03).
//
// To keep operator-hint strings (e.g. inside doctor.js) from
// tripping the install/sudo greps, we strip comments and string
// literals before grepping; only executable code is checked.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const PKG_ROOT = path.resolve(__dirname, "..");
const LIB_WSL_DIR = path.join(PKG_ROOT, "lib", "wsl");
const BIN_DIR = path.join(PKG_ROOT, "bin");

const HELPER_FILES = ["distro-name.js", "detect.js", "doctor.js", "index.js"];

function readSource(file) {
  return fs.readFileSync(path.join(LIB_WSL_DIR, file), "utf8");
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
    if (c === "/") {
      // Regex literal. Skip until unescaped closing slash. We don't
      // need flag handling here.
      i++;
      while (i < n && src[i] !== "/") {
        if (src[i] === "\\" && i + 1 < n) {
          i += 2;
        } else if (src[i] === "\n") {
          break;
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

test("lib/wsl helpers do not reference install/sudo/pair/mcp.json in executable code", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    const code = stripCommentsAndStrings(src);
    for (const forbidden of [
      /\bsudo\b/i,
      /npm\s+install/i,
      /apt[- ]get/i,
      /\bpacman\b/i,
      /--install\b/i,
      /\bpairing\b/i,
      /mcp\.json/i,
    ]) {
      assert.equal(
        forbidden.test(code),
        false,
        `${file} must not reference ${forbidden} in executable code; excerpt:\n${code.slice(0, 600)}`,
      );
    }
  }
});

test("lib/wsl helpers do not call any file-write API in executable code", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    const code = stripCommentsAndStrings(src);
    for (const forbidden of [
      /\bfs\.writeFile\b/,
      /\bfs\.writeFileSync\b/,
      /\bfs\.appendFile\b/,
      /\bfs\.appendFileSync\b/,
      /\bcreateWriteStream\b/,
      /\bfs\.mkdir\b/,
      /\bfs\.mkdirSync\b/,
      /\bfs\.rm\b/,
      /\bfs\.unlink\b/,
    ]) {
      assert.equal(
        forbidden.test(code),
        false,
        `${file} must not call ${forbidden} in executable code; excerpt:\n${code.slice(0, 600)}`,
      );
    }
  }
});

test("lib/wsl helpers spawn only wsl.exe (first spawn argument is wslPath / 'wsl.exe')", () => {
  // Inspect detect.js + doctor.js raw source: every spawn(...) call's
  // first argument MUST be the identifier `wslPath` (an injected
  // parameter). No literal command name should appear as the first
  // argument.
  for (const file of ["detect.js", "doctor.js"]) {
    const src = readSource(file);
    const spawnCalls = [...src.matchAll(/\bspawn\s*\(\s*([^,\s)]+)/g)];
    assert.ok(
      spawnCalls.length > 0,
      `${file} should contain at least one spawn() call`,
    );
    for (const m of spawnCalls) {
      const firstArg = m[1];
      assert.ok(
        firstArg === "wslPath" || firstArg === "'wsl.exe'" || firstArg === '"wsl.exe"',
        `${file} spawn() first arg must be wslPath / 'wsl.exe' (got ${firstArg})`,
      );
    }
  }
  // distro-name.js + index.js MUST NOT call spawn at all.
  for (const file of ["distro-name.js", "index.js"]) {
    const src = readSource(file);
    const code = stripCommentsAndStrings(src);
    assert.equal(
      /\bspawn\s*\(/.test(code),
      false,
      `${file} must not call spawn(); excerpt:\n${code.slice(0, 400)}`,
    );
  }
});

test("lib/wsl helpers do not import or invoke the bin/* shims", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    for (const forbidden of [
      /require\([^)]*['"][^'"]*bin\/[^'"]*['"]\)/,
      /require\([^)]*['"][^'"]*resolve-binary[^'"]*['"]\)/,
      /terminal-commanderd\.js/,
      /terminal-commander-mcp\.js/,
    ]) {
      assert.equal(
        forbidden.test(src),
        false,
        `${file} must not reference the bin/* shims; matched ${forbidden}`,
      );
    }
  }
});

test("lib/wsl helpers use shell: false and windowsHide: true on every spawn", () => {
  for (const file of ["detect.js", "doctor.js"]) {
    const src = readSource(file);
    // Every spawn block must include shell: false within ~400 chars.
    const spawnIdx = [...src.matchAll(/\bspawn\s*\(/g)].map((m) => m.index);
    assert.ok(spawnIdx.length > 0, `${file} should call spawn`);
    for (const idx of spawnIdx) {
      const window = src.slice(idx, idx + 400);
      assert.match(
        window,
        /shell\s*:\s*false/,
        `${file} spawn at index ${idx} must use shell: false`,
      );
      assert.match(
        window,
        /windowsHide\s*:\s*true/,
        `${file} spawn at index ${idx} must use windowsHide: true`,
      );
    }
  }
});

test("doctor.js never concatenates operator input into a bash -lc string", () => {
  // The probe command string MUST be a single literal token and the
  // identifier `RUNTIME_PROBE_CMD` MUST be passed straight into argv
  // without `+` concatenation or template interpolation that
  // references the distro. We strip comments + string literals
  // first so JSDoc examples (which reference `distro` for human
  // readability) don't trip the guard.
  const src = readSource("doctor.js");
  const code = stripCommentsAndStrings(src);
  // No template literal in executable code (all string literals are
  // stripped by `stripCommentsAndStrings`, so any remaining backtick
  // would be a syntax mark we did not strip — fail loudly).
  assert.equal(
    /`/.test(code),
    false,
    `doctor.js executable code must contain no template literals; excerpt:\n${code.slice(0, 600)}`,
  );
  // No `+` concatenation of RUNTIME_PROBE_CMD with anything.
  assert.equal(/RUNTIME_PROBE_CMD\s*\+/.test(code), false);
  assert.equal(/\+\s*RUNTIME_PROBE_CMD\b/.test(code), false);
  // And no `distro +` / `+ distro` concat anywhere in executable code.
  assert.equal(/\bdistro\s*\+\s*['"`]/.test(code), false);
});

test("bin/* shims are UNCHANGED at WWS03 (no terminal-commander-mcp wsl.exe bridge launched yet)", () => {
  // The shim files MUST still match the WWS02 contract: bridge_required
  // branch with exit 64, no wsl.exe invocation in executable code.
  // This is the same static guard as shim-win32-branch.test.js, kept
  // here as a WWS03-level regression so any accidental WWS03-time
  // edit to a shim fails CI loudly.
  for (const shim of [
    "terminal-commanderd.js",
    "terminal-commander-mcp.js",
    "terminal-commander.js",
  ]) {
    const src = fs.readFileSync(path.join(BIN_DIR, shim), "utf8");
    const code = stripCommentsAndStrings(src);
    assert.equal(
      /wsl\.exe/i.test(code),
      false,
      `${shim} must not invoke wsl.exe in executable code at WWS03`,
    );
    assert.equal(
      /\bspawn\s*\(\s*['"`]wsl/i.test(code),
      false,
      `${shim} must not spawn('wsl', ...) at WWS03`,
    );
    // And the WWS02 bridge_required branch must still be present.
    assert.match(
      src,
      /bridge_required/,
      `${shim} must keep the bridge_required branch from WWS02`,
    );
  }
});

test("lib/wsl helpers do not open any TCP/UDP socket or HTTP client", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
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

test("lib/wsl helpers do not forward any env var to the spawn call", () => {
  // The spawn() options object inside detect.js / doctor.js must
  // NOT include an `env:` key. Default behaviour inherits PATH only;
  // explicit env passthrough is forbidden at WWS03.
  for (const file of ["detect.js", "doctor.js"]) {
    const src = readSource(file);
    const spawnIdx = [...src.matchAll(/\bspawn\s*\(/g)].map((m) => m.index);
    for (const idx of spawnIdx) {
      const window = src.slice(idx, idx + 400);
      assert.equal(
        /\benv\s*:/.test(window),
        false,
        `${file} spawn at index ${idx} must not set env:`,
      );
    }
  }
});
