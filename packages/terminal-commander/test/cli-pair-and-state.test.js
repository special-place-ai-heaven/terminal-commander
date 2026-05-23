// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 pair + setup_state tests. File I/O is confined to per-test
// temp directories under os.tmpdir().

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

const {
  getStateDir,
  isForbiddenStateKey,
  atomicWriteJson,
  readJsonIfExists,
  writeSetupJson,
  readSetupJson,
  writePairJson,
  readPairJson,
  SCHEMA_VERSION,
  MAX_STATE_BYTES,
  SETUP_FILENAME,
  PAIR_FILENAME,
  STATE_STATUSES,
} = require("../lib/cli/setup_state.js");
const { runPairCreate, generateSixDigitCode, PAIR_CREATE_STATUSES } = require("../lib/cli/pair_create.js");
const { runPairAccept, PAIR_ACCEPT_STATUSES, SIX_DIGIT_RE } = require("../lib/cli/pair_accept.js");

function mkScope() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "wws06-state-"));
}

function rmScope(p) {
  try {
    fs.rmSync(p, { recursive: true, force: true });
  } catch (_e) {
    /* ignore */
  }
}

test("getStateDir derives Windows path from LOCALAPPDATA", () => {
  const p = getStateDir({ platform: "win32", env: { LOCALAPPDATA: "C:\\Users\\op\\AppData\\Local" } });
  assert.equal(p, path.join("C:\\Users\\op\\AppData\\Local", "terminal-commander"));
});

test("getStateDir derives Linux path from XDG_STATE_HOME, falls back to HOME/.local/state", () => {
  const a = getStateDir({ platform: "linux", env: { XDG_STATE_HOME: "/var/state" } });
  assert.equal(a, path.join("/var/state", "terminal-commander"));
  const b = getStateDir({ platform: "linux", env: { HOME: "/home/op" } });
  assert.equal(b, path.join("/home/op", ".local", "state", "terminal-commander"));
});

test("getStateDir throws when env vars are missing", () => {
  assert.throws(() => getStateDir({ platform: "win32", env: {} }), (e) => {
    assert.equal(e.code, "UNSUPPORTED_HOST");
    return true;
  });
  assert.throws(() => getStateDir({ platform: "linux", env: {} }));
});

test("isForbiddenStateKey rejects token-shaped keys and bare credential/password", () => {
  for (const k of [
    "NPM_TOKEN",
    "FOO_SECRET",
    "X_PASSWORD",
    "Y_PASS",
    "Z_API_KEY",
    "AA_APIKEY",
    "password",
    "PASSWORD",
    "credential",
    "credentials",
  ]) {
    assert.equal(isForbiddenStateKey(k), true, `${k} must be forbidden`);
  }
  for (const k of ["distro", "schema_version", "cursor_scope", "created_at", "pair_id", "code"]) {
    assert.equal(isForbiddenStateKey(k), false, `${k} must be allowed`);
  }
});

test("atomicWriteJson refuses forbidden state keys", () => {
  const root = mkScope();
  try {
    const r = atomicWriteJson(path.join(root, "x.json"), { NPM_TOKEN: "x", schema_version: 1 });
    assert.equal(r.ok, false);
    assert.equal(r.reason, "write_failed");
    assert.equal(fs.existsSync(path.join(root, "x.json")), false);
  } finally {
    rmScope(root);
  }
});

test("atomicWriteJson writes JSON inside scope dir with same-dir tmp file", () => {
  const root = mkScope();
  try {
    const target = path.join(root, "x.json");
    const r = atomicWriteJson(target, { schema_version: 1, distro: "Ubuntu" });
    assert.equal(r.ok, true);
    const parsed = JSON.parse(fs.readFileSync(target, "utf8"));
    assert.deepEqual(parsed, { schema_version: 1, distro: "Ubuntu" });
    // tmp file removed after rename.
    const leftovers = fs.readdirSync(root).filter((n) => n.includes(".tmp."));
    assert.equal(leftovers.length, 0);
  } finally {
    rmScope(root);
  }
});

test("atomicWriteJson refuses targets outside scope dir (path_not_allowed)", () => {
  const root = mkScope();
  const other = mkScope();
  try {
    const target = path.join(other, "evil.json");
    const r = atomicWriteJson(target, { schema_version: 1 }, { scopeDir: root });
    assert.equal(r.ok, false);
    assert.equal(r.reason, "path_not_allowed");
    assert.equal(fs.existsSync(target), false);
  } finally {
    rmScope(root);
    rmScope(other);
  }
});

test("readJsonIfExists rejects over-size + invalid JSON; returns null for missing", () => {
  const root = mkScope();
  try {
    const missing = path.join(root, "missing.json");
    const a = readJsonIfExists(missing);
    assert.deepEqual(a, { ok: true, value: null });

    const big = path.join(root, "big.json");
    fs.writeFileSync(big, "x".repeat(MAX_STATE_BYTES + 1));
    assert.deepEqual(readJsonIfExists(big), { ok: false, reason: "config_too_large" });

    const bad = path.join(root, "bad.json");
    fs.writeFileSync(bad, "{not json");
    assert.deepEqual(readJsonIfExists(bad), { ok: false, reason: "invalid_json" });
  } finally {
    rmScope(root);
  }
});

test("writeSetupJson + readSetupJson round-trip; created_at preserved across updates", () => {
  const root = mkScope();
  try {
    const r1 = writeSetupJson({
      stateDir: root,
      distro: "Ubuntu",
      cursor_scope: "global",
      now: () => new Date("2026-05-23T00:00:00Z"),
    });
    assert.equal(r1.status, "ok");
    assert.equal(r1.value.distro, "Ubuntu");
    assert.equal(r1.value.cursor_scope, "global");
    assert.equal(r1.value.created_at, "2026-05-23T00:00:00.000Z");
    const r2 = writeSetupJson({
      stateDir: root,
      distro: "Debian",
      cursor_scope: "project",
      now: () => new Date("2026-05-23T01:00:00Z"),
    });
    assert.equal(r2.status, "ok");
    assert.equal(r2.value.distro, "Debian");
    // created_at preserved from the first write.
    assert.equal(r2.value.created_at, "2026-05-23T00:00:00.000Z");
    assert.equal(r2.value.updated_at, "2026-05-23T01:00:00.000Z");
    const reloaded = readSetupJson({ stateDir: root });
    assert.equal(reloaded.ok, true);
    assert.equal(reloaded.value.distro, "Debian");
  } finally {
    rmScope(root);
  }
});

test("generateSixDigitCode returns a 6-digit ASCII string padded with leading zeros", () => {
  // Force the boundary values.
  const lo = generateSixDigitCode(() => 100000);
  const hi = generateSixDigitCode(() => 999999);
  assert.equal(lo, "100000");
  assert.equal(hi, "999999");
  // Pad short value with leading zeros.
  const pad = generateSixDigitCode(() => 42);
  assert.equal(pad, "000042");
  assert.match(pad, /^[0-9]{6}$/);
});

test("runPairCreate writes a bounded pair.json with no forbidden keys", async () => {
  const root = mkScope();
  try {
    const r = await runPairCreate({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      flags: { distro: "Ubuntu" },
      randomInt: () => 123456,
      randomBytes: (n) => Buffer.alloc(n, 0x01),
      now: () => new Date("2026-05-23T00:00:00Z"),
    });
    assert.equal(r.status, "pair_created");
    assert.equal(r.code, "123456");
    assert.match(r.pair_id, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    const cfg = JSON.parse(fs.readFileSync(r.path, "utf8"));
    assert.deepEqual(Object.keys(cfg).sort(), ["accepted_at", "code", "created_at", "distro", "pair_id", "schema_version"]);
    assert.equal(cfg.distro, "Ubuntu");
    assert.equal(cfg.accepted_at, null);
    // No forbidden fields leaked through.
    for (const k of Object.keys(cfg)) {
      assert.equal(isForbiddenStateKey(k), false, `pair.json must not contain forbidden key ${k}`);
    }
  } finally {
    rmScope(root);
  }
});

test("runPairCreate rejects unsafe --distro without writing pair.json", async () => {
  const root = mkScope();
  try {
    const r = await runPairCreate({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      flags: { distro: "Bad; rm" },
      randomInt: () => 100000,
    });
    assert.equal(r.status, "unsafe_distro_name");
    assert.equal(fs.existsSync(path.join(root, "terminal-commander", "pair.json")), false);
  } finally {
    rmScope(root);
  }
});

test("runPairAccept rejects non-6-digit input with invalid_code_shape", async () => {
  for (const code of ["", "12345", "1234567", "abcdef", " 12345", "1234 5", "12 3456", null, undefined]) {
    const r = await runPairAccept({ platform: "linux", env: { HOME: "/tmp" }, code });
    assert.equal(r.status, "invalid_code_shape", `code=${JSON.stringify(code)} should be rejected`);
  }
});

test("runPairAccept returns pair_deferred when no pair.json exists", async () => {
  const root = mkScope();
  try {
    const r = await runPairAccept({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      code: "123456",
    });
    assert.equal(r.status, "pair_deferred");
  } finally {
    rmScope(root);
  }
});

test("runPairAccept matches persisted code -> pair_accepted; updates accepted_at", async () => {
  const root = mkScope();
  try {
    const created = await runPairCreate({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      flags: {},
      randomInt: () => 654321,
    });
    assert.equal(created.status, "pair_created");
    const r = await runPairAccept({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      code: "654321",
      now: () => new Date("2026-05-23T02:00:00Z"),
    });
    assert.equal(r.status, "pair_accepted");
    assert.equal(r.accepted_at, "2026-05-23T02:00:00.000Z");
    // Persisted record updated.
    const reloaded = JSON.parse(fs.readFileSync(created.path, "utf8"));
    assert.equal(reloaded.accepted_at, "2026-05-23T02:00:00.000Z");
    assert.equal(reloaded.code, "654321");
  } finally {
    rmScope(root);
  }
});

test("runPairAccept mismatched code returns pair_deferred + preserves persisted record", async () => {
  const root = mkScope();
  try {
    await runPairCreate({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      flags: {},
      randomInt: () => 111111,
    });
    const before = fs.readFileSync(path.join(root, "terminal-commander", "pair.json"), "utf8");
    const r = await runPairAccept({
      platform: "win32",
      env: { LOCALAPPDATA: root },
      code: "222222",
    });
    assert.equal(r.status, "pair_deferred");
    const after = fs.readFileSync(path.join(root, "terminal-commander", "pair.json"), "utf8");
    assert.equal(after, before, "mismatched code must not modify pair.json");
  } finally {
    rmScope(root);
  }
});

test("SIX_DIGIT_RE matches exactly 6 ASCII digits", () => {
  assert.equal(SIX_DIGIT_RE.test("123456"), true);
  assert.equal(SIX_DIGIT_RE.test("000000"), true);
  assert.equal(SIX_DIGIT_RE.test("12345"), false);
  assert.equal(SIX_DIGIT_RE.test("1234567"), false);
  assert.equal(SIX_DIGIT_RE.test("abcdef"), false);
  assert.equal(SIX_DIGIT_RE.test(" 12345"), false);
});

test("PAIR_CREATE_STATUSES + PAIR_ACCEPT_STATUSES + STATE_STATUSES expose the locked sets", () => {
  assert.ok(Object.values(PAIR_CREATE_STATUSES).includes("pair_created"));
  assert.ok(Object.values(PAIR_CREATE_STATUSES).includes("unsafe_distro_name"));
  assert.ok(Object.values(PAIR_ACCEPT_STATUSES).includes("pair_accepted"));
  assert.ok(Object.values(PAIR_ACCEPT_STATUSES).includes("pair_deferred"));
  assert.ok(Object.values(PAIR_ACCEPT_STATUSES).includes("invalid_code_shape"));
  assert.ok(Object.values(STATE_STATUSES).includes("ok"));
  assert.ok(Object.values(STATE_STATUSES).includes("path_not_allowed"));
});

test("SCHEMA_VERSION / MAX_STATE_BYTES / file names are locked", () => {
  assert.equal(SCHEMA_VERSION, 1);
  assert.equal(MAX_STATE_BYTES, 64 * 1024);
  assert.equal(SETUP_FILENAME, "setup.json");
  assert.equal(PAIR_FILENAME, "pair.json");
});
