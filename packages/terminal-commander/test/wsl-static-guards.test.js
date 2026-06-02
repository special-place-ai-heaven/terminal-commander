// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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

const HELPER_FILES = ["distro-name.js", "detect.js", "doctor.js", "spawn.js", "index.js"];

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
  for (const file of ["detect.js", "doctor.js", "spawn.js"]) {
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

test("lib/wsl helpers use shell: false and no window-hiding option on every spawn", () => {
  for (const file of ["detect.js", "doctor.js", "spawn.js"]) {
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
      assert.doesNotMatch(
        window,
        /windowsHide/,
        `${file} spawn at index ${idx} must not request hidden windows`,
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

test("bin/* shims contain no wsl.exe literal invocation in executable code (Phase 3 regression guard)", () => {
  // Phase 3 renamed the shim contract: win32-x64 is now a native target.
  // The bridge_required reason no longer exists in the resolver (win32-x64
  // resolves via @terminal-commander/windows-x64). The shims may still
  // reference "bridge_required" as dead code / legacy guard, but they
  // MUST NOT spawn wsl.exe literally in executable code — that is still
  // owned by lib/wsl/spawn.js behind the TC_USE_LEGACY_WSL_BRIDGE gate.
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
      `${shim} must not invoke wsl.exe in executable code`,
    );
    assert.equal(
      /\bspawn\s*\(\s*['"`]wsl/i.test(code),
      false,
      `${shim} must not spawn('wsl', ...) in executable code`,
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

test("spawn.js does not invoke lazy bootstrap or runtime install helpers", () => {
  const src = readSource("spawn.js");
  const code = stripCommentsAndStrings(src);
  for (const forbidden of [
    /\brunBootstrap\b/,
    /\bensureWslRuntime\b/,
    /\bensureDaemonAutostartInWsl\b/,
    /\btryAcquireBootstrapLock\b/,
    /\breleaseBootstrapLock\b/,
    /\bharnessNeedsConfiguration\b/,
    /\bshouldSkipBootstrap\b/,
  ]) {
    assert.equal(
      forbidden.test(code),
      false,
      `spawn.js must not invoke ${forbidden}; excerpt:\n${code.slice(0, 900)}`,
    );
  }
  for (const forbiddenPath of [
    "../bootstrap/orchestrator.js",
    "../bootstrap/ensure_wsl_runtime.js",
    "../bootstrap/ensure_daemon_autostart.js",
    "../bootstrap/lock.js",
  ]) {
    assert.equal(
      src.includes(forbiddenPath),
      false,
      `spawn.js must not import ${forbiddenPath}`,
    );
  }
});

test("detect.js does not forward any env var to the spawn call", () => {
  // detect.js spawn() options must NOT include an `env:` key. Its only
  // spawn is `wsl.exe -l -v`, a host-side WSL management command that
  // launches NO Linux process, so WSLENV forwarding is moot there and
  // the default (inherit) env is acceptable. We strip comments + strings
  // first so doc examples don't trip the guard.
  const src = stripCommentsAndStrings(readSource("detect.js"));
  const spawnIdx = [...src.matchAll(/\bspawn\s*\(/g)].map((m) => m.index);
  for (const idx of spawnIdx) {
    const window = src.slice(idx, idx + 400);
    assert.equal(
      /\benv\s*:/.test(window),
      false,
      `detect.js spawn at index ${idx} must not set env:`,
    );
  }
});

test("doctor.js runtime-probe spawn sanitizes WSLENV (no raw process.env leak)", () => {
  // SECURITY: doctor.js's runtime probe runs `wsl -d <distro> -- bash -lc
  // ...`, which DOES launch a Linux process. Without an explicit env the
  // child inherits the full process.env, so an ambient WSLENV=SECRET/u
  // would forward SECRET across the boundary. doctor.js MUST therefore set
  // env on its spawn, and that env MUST flow through
  // `ensureSessionInWslEnv(buildFilteredEnv(...))` and NEVER pass a raw,
  // unsanitized process.env. We strip comments + strings first so doc
  // examples don't trip the guard.
  const code = stripCommentsAndStrings(readSource("doctor.js"));
  const spawnIdx = [...code.matchAll(/\bspawn\s*\(/g)].map((m) => m.index);
  assert.ok(spawnIdx.length > 0, "doctor.js should call spawn");
  for (const idx of spawnIdx) {
    const window = code.slice(idx, idx + 600);
    // The spawn MUST set env, and the value MUST be the sanitized chain.
    assert.match(
      window,
      /\benv\s*:\s*ensureSessionInWslEnv\s*\(\s*buildFilteredEnv\s*\(/,
      `doctor.js spawn at index ${idx} must set env: ensureSessionInWslEnv(buildFilteredEnv(...)); got:\n${window.slice(0, 400)}`,
    );
    // Never pass a bare process.env to env: (it must be wrapped).
    assert.equal(
      /\benv\s*:\s*process\.env\b/.test(window),
      false,
      `doctor.js spawn at index ${idx} must not pass raw process.env to env:; got:\n${window.slice(0, 400)}`,
    );
  }
});

test("spawn.js passes only the filteredEnv identifier to spawn's env option (no raw process.env)", () => {
  // WWS04 spawn.js DOES set env: on the bridge spawn call, but the
  // identifier passed MUST be `env` (the parameter that the
  // production-path wrapper already filtered via `buildFilteredEnv`).
  // It must NEVER pass `process.env` directly to spawn, and must
  // never pass an unfiltered parameter named anything other than
  // `env`. We strip comments + strings first so JSDoc examples
  // don't trip the guard.
  const src = readSource("spawn.js");
  const code = stripCommentsAndStrings(src);
  const spawnIdx = [...code.matchAll(/\bspawn\s*\(/g)].map((m) => m.index);
  assert.ok(spawnIdx.length > 0, "spawn.js should call spawn");
  for (const idx of spawnIdx) {
    const window = code.slice(idx, idx + 600);
    assert.match(
      window,
      /\benv\s*:\s*env\b/,
      `spawn.js spawn at index ${idx} must pass env: env (the already-filtered param), got:\n${window.slice(0, 400)}`,
    );
    assert.equal(
      /\benv\s*:\s*process\.env\b/.test(window),
      false,
      `spawn.js spawn at index ${idx} must not pass process.env directly, got:\n${window.slice(0, 400)}`,
    );
  }
});

test("spawn.js never concatenates operator input into the bash -lc string", () => {
  // BRIDGE_PROBE_CMD must be a constant literal; no template
  // interpolation or `+` concatenation with the distro / argv.
  const src = readSource("spawn.js");
  const code = stripCommentsAndStrings(src);
  assert.equal(
    /BRIDGE_PROBE_CMD\s*\+/.test(code),
    false,
    `spawn.js executable code must not concatenate onto BRIDGE_PROBE_CMD`,
  );
  assert.equal(
    /\+\s*BRIDGE_PROBE_CMD\b/.test(code),
    false,
    `spawn.js executable code must not concatenate before BRIDGE_PROBE_CMD`,
  );
  // No template literal in executable code (string literals are
  // stripped; any remaining backtick would be a syntax mark we did
  // not strip — fail loudly).
  assert.equal(
    /`/.test(code),
    false,
    `spawn.js executable code must contain no template literals; excerpt:\n${code.slice(0, 600)}`,
  );
  // No `distro +` / `+ distro` string concat anywhere in executable code.
  assert.equal(
    /\bdistro\s*\+\s*['"`]/.test(code),
    false,
    `spawn.js must not concatenate distro into a string literal`,
  );
});

test("spawn.js argv array literal includes exactly one BRIDGE_PROBE_CMD use", () => {
  // The argv build is the security-critical site. We assert the
  // executable code contains exactly one occurrence of the
  // BRIDGE_PROBE_CMD identifier inside an array literal that begins
  // with the constants '-d', distro, '--', 'bash', '-lc'.
  const src = readSource("spawn.js");
  // Find the argv = [ ... ] declaration; do a structural match.
  assert.match(
    src,
    /argv\s*=\s*\[\s*['"]-d['"]\s*,\s*distro\s*,\s*['"]--['"]\s*,\s*['"]bash['"]\s*,\s*['"]-lc['"]\s*,\s*BRIDGE_PROBE_CMD/,
    "spawn.js argv literal must match ['-d', distro, '--', 'bash', '-lc', BRIDGE_PROBE_CMD, ...]",
  );
});

test("spawn.js does not log token values; secret-key list is a STRIP list, not a forward list", () => {
  // The implementation may NAME the secret keys (they are stripped),
  // but it must never use their VALUES. We assert there is no
  // console.* / process.stdout.write / process.stderr.write call
  // whose argument expression references env[<SECRET_KEY>] or
  // parentEnv[<SECRET_KEY>].
  const src = readSource("spawn.js");
  const code = stripCommentsAndStrings(src);
  for (const secret of ["NPM_TOKEN", "GITHUB_TOKEN", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"]) {
    // Detect any read access pattern like env[secret] / env.SECRET.
    const dotRe = new RegExp(`\\benv\\.${secret}\\b`);
    const idxRe = new RegExp(`\\benv\\[\\s*['"]${secret}['"]\\s*\\]`);
    assert.equal(
      dotRe.test(code),
      false,
      `spawn.js must not read env.${secret}`,
    );
    assert.equal(
      idxRe.test(code),
      false,
      `spawn.js must not read env['${secret}']`,
    );
  }
});
