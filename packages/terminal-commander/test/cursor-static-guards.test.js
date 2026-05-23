// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS05 Cursor config writer static guards. Code-only invariants:
//
//   - lib/cursor/** MUST NOT require child_process (the Cursor writer
//     is pure file I/O; the WSL bridge is owned by lib/wsl/spawn.js).
//   - lib/cursor/** MUST NOT call spawn / exec / execFile / wsl.exe.
//   - lib/cursor/** MUST NOT reference sudo, npm install, apt-get,
//     pacman, --install, pair, credential, password in executable code.
//   - lib/cursor/** MUST NOT write any env key other than TC_WSL_DISTRO.
//   - lib/cursor/** MUST NOT use TCP/UDP/HTTP/fetch APIs.
//   - lib/cursor/** MUST NOT read token-shaped env vars.
//   - No active .cursor/mcp.json may be committed anywhere in the repo.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const PKG_ROOT = path.resolve(__dirname, "..");
const REPO_ROOT = path.resolve(PKG_ROOT, "..", "..");
const LIB_CURSOR_DIR = path.join(PKG_ROOT, "lib", "cursor");

const HELPER_FILES = ["config.js", "write.js", "index.js"];

function readSource(file) {
  return fs.readFileSync(path.join(LIB_CURSOR_DIR, file), "utf8");
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
      // Regex literal.
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

test("lib/cursor helpers MUST NOT require child_process", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    assert.equal(
      /require\(\s*['"]child_process['"]\s*\)/.test(src),
      false,
      `${file} must not require child_process; bridge spawn is lib/wsl/spawn.js territory`,
    );
    assert.equal(
      /require\(\s*['"]node:child_process['"]\s*\)/.test(src),
      false,
      `${file} must not require node:child_process`,
    );
  }
});

test("lib/cursor helpers MUST NOT call spawn / exec / execFile in executable code", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    const code = stripCommentsAndStrings(src);
    for (const forbidden of [/\bspawn\s*\(/, /\bexec\s*\(/, /\bexecFile\s*\(/, /\bexecSync\s*\(/, /\bspawnSync\s*\(/]) {
      assert.equal(
        forbidden.test(code),
        false,
        `${file} must not match ${forbidden} in executable code; excerpt:\n${code.slice(0, 600)}`,
      );
    }
  }
});

test("lib/cursor helpers MUST NOT reference wsl.exe in executable code", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    const code = stripCommentsAndStrings(src);
    assert.equal(
      /wsl\.exe/i.test(code),
      false,
      `${file} must not reference wsl.exe in executable code (only comments / docs allowed)`,
    );
  }
});

test("lib/cursor helpers MUST NOT reference install/sudo/pair/credential/password in executable code", () => {
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
      /\bcredential\b/i,
      /\bpassword\b/i,
    ]) {
      assert.equal(
        forbidden.test(code),
        false,
        `${file} must not reference ${forbidden} in executable code; excerpt:\n${code.slice(0, 600)}`,
      );
    }
  }
});

test("lib/cursor helpers MUST NOT use TCP/UDP/HTTP/fetch APIs", () => {
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

test("lib/cursor helpers MUST NOT read token-shaped env vars", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
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
      assert.equal(
        dotRe.test(code),
        false,
        `${file} must not read env.${secret}`,
      );
      assert.equal(
        idxRe.test(code),
        false,
        `${file} must not read env['${secret}']`,
      );
    }
  }
});

test("config.js: buildTerminalCommanderServerConfig only ever writes the TC_WSL_DISTRO env key", () => {
  // Static scan: the only literal env key the writer mentions inside
  // a stanza-builder context is TC_WSL_DISTRO. We grep raw source
  // because the env key DOES appear in string literals; we accept
  // those matches but disallow any OTHER token-shaped env key
  // appearing as a quoted key.
  const src = readSource("config.js");
  // Allowed env keys named in the stanza-builder:
  assert.match(src, /"TC_WSL_DISTRO"/);
  // No other quoted env key matching common shapes:
  for (const forbidden of [
    /"NPM_TOKEN"/,
    /"GITHUB_TOKEN"/,
    /"OPENAI_API_KEY"/,
    /"ANTHROPIC_API_KEY"/,
    /"SLACK_TOKEN"/,
    /"PASSWORD"/,
    /"PASSWD"/,
    /"PWD"/,
  ]) {
    assert.equal(forbidden.test(src), false, `config.js must not name ${forbidden}`);
  }
});

test("lib/cursor helpers do not import lib/wsl/spawn.js (bridge spawn stays in lib/wsl)", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    assert.equal(
      /require\(\s*['"][^'"]*lib\/wsl\/spawn[^'"]*['"]\s*\)/.test(src),
      false,
      `${file} must not require lib/wsl/spawn.js`,
    );
    assert.equal(
      /require\(\s*['"]\.\.\/wsl\/spawn\.js['"]\s*\)/.test(src),
      false,
      `${file} must not require ../wsl/spawn.js`,
    );
  }
});

test("lib/cursor helpers do not import the bin/* shims", () => {
  for (const file of HELPER_FILES) {
    const src = readSource(file);
    assert.equal(
      /require\([^)]*['"][^'"]*bin\/[^'"]*['"]\)/.test(src),
      false,
      `${file} must not require any bin/* shim`,
    );
    assert.equal(
      /require\([^)]*['"][^'"]*resolve-binary[^'"]*['"]\)/.test(src),
      false,
      `${file} must not require resolve-binary`,
    );
  }
});

test("no active .cursor/mcp.json exists anywhere in the repo", () => {
  // Recursive scan from repo root. Skip node_modules + target/ +
  // target-wsl/ + .git for speed.
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
      } else if (e.isFile()) {
        if (
          e.name === "mcp.json" &&
          path.basename(path.dirname(full)) === ".cursor"
        ) {
          offenders.push(full);
        }
      }
    }
  }
  walk(REPO_ROOT);
  assert.deepEqual(
    offenders,
    [],
    `No .cursor/mcp.json may be committed; found: ${offenders.join(", ")}`,
  );
});

test("write.js: atomicWrite tmp path is a child of the target's directory", () => {
  // Structural assertion: the tmp path is built as
  // `target + ".tmp." + suffix`, which guarantees same-directory
  // tmp file. We grep for the exact concatenation pattern.
  const src = readSource("write.js");
  assert.match(
    src,
    /const\s+tmp\s*=\s*target\s*\+\s*['"]\.tmp\.['"]\s*\+/,
    "atomicWrite must construct tmp path as `target + '.tmp.' + suffix` (same directory)",
  );
});

test("write.js: refuses non-string projectRoot via resolveScope path", () => {
  // Defense-in-depth structural assertion: resolveScope returns
  // project_root_required when projectRoot is missing/empty.
  const src = readSource("write.js");
  assert.match(
    src,
    /PROJECT_ROOT_REQUIRED/,
    "write.js must contain PROJECT_ROOT_REQUIRED status reference",
  );
});
