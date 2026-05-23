// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 `terminal-commander pair create`.
//
// Generates a 6-digit code via crypto.randomInt, persists a bounded
// pair.json under %LOCALAPPDATA%\terminal-commander\, and prints the
// code to the operator. Pair codes are operator confirmation, NOT a
// security secret.
//
// NO credentials. NO tokens. NO passwords. NO env values. NO command
// history. The on-disk record contains only:
//   schema_version, pair_id (uuid v7), code, created_at,
//   accepted_at (initially null), distro (optional safe name | null).

"use strict";

const crypto = require("node:crypto");
const { writePairJson, SCHEMA_VERSION } = require("./setup_state.js");
const {
  isSafeDistroName,
  assertSafeDistroName,
} = require("../wsl/distro-name.js");

const PAIR_CREATE_STATUSES = Object.freeze({
  PAIR_CREATED: "pair_created",
  UNSAFE_DISTRO_NAME: "unsafe_distro_name",
  WRITE_FAILED: "write_failed",
  UNSUPPORTED_HOST: "unsupported_host",
});

// UUID v7 generator (without depending on node:crypto.randomUUID
// behavior — randomUUID is v4 only at most Node versions). Falls
// back to v4 if v7 cannot be generated.
function uuidV7(now, randomBytes) {
  const nowMs = typeof now === "function" ? now() : Date.now();
  const rnd =
    typeof randomBytes === "function"
      ? randomBytes(10)
      : crypto.randomBytes(10);
  const bytes = Buffer.alloc(16);
  // 48-bit big-endian timestamp (ms).
  bytes[0] = (nowMs / 2 ** 40) & 0xff;
  bytes[1] = (nowMs / 2 ** 32) & 0xff;
  bytes[2] = (nowMs / 2 ** 24) & 0xff;
  bytes[3] = (nowMs / 2 ** 16) & 0xff;
  bytes[4] = (nowMs / 2 ** 8) & 0xff;
  bytes[5] = nowMs & 0xff;
  // 12 random bits + version 7 in the high nibble of byte 6.
  bytes[6] = 0x70 | (rnd[0] & 0x0f);
  bytes[7] = rnd[1];
  // 62 random bits + variant 10 in the high two bits of byte 8.
  bytes[8] = 0x80 | (rnd[2] & 0x3f);
  bytes[9] = rnd[3];
  bytes[10] = rnd[4];
  bytes[11] = rnd[5];
  bytes[12] = rnd[6];
  bytes[13] = rnd[7];
  bytes[14] = rnd[8];
  bytes[15] = rnd[9];
  const hex = bytes.toString("hex");
  return (
    hex.slice(0, 8) +
    "-" +
    hex.slice(8, 12) +
    "-" +
    hex.slice(12, 16) +
    "-" +
    hex.slice(16, 20) +
    "-" +
    hex.slice(20, 32)
  );
}

function generateSixDigitCode(randomInt) {
  if (typeof randomInt === "function") {
    return String(randomInt(100000, 1000000)).padStart(6, "0");
  }
  return String(crypto.randomInt(100000, 1000000)).padStart(6, "0");
}

function buildResult(partial) {
  return {
    status: partial.status,
    exit_code: typeof partial.exit_code === "number" ? partial.exit_code : 64,
    pair_id: partial.pair_id || null,
    code: partial.code || null,
    path: partial.path || null,
    distro: partial.distro || null,
    output: partial.output || "",
    hint: partial.hint || "",
  };
}

async function runPairCreate(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const flags = o.flags || {};
  const writeState = o.writePairJson || writePairJson;
  const randomInt = o.randomInt;
  const randomBytes = o.randomBytes;
  const now = o.now;

  let distro = null;
  if (flags.distro != null && flags.distro !== "") {
    if (!isSafeDistroName(flags.distro)) {
      return buildResult({
        status: PAIR_CREATE_STATUSES.UNSAFE_DISTRO_NAME,
        distro: flags.distro,
        output: "distro name failed safety whitelist; only ASCII letters, digits, '.', '_' and '-' are allowed (length 1..64).",
        hint: "pass --distro <safe-name> or omit --distro to leave pair.json unannotated.",
      });
    }
    distro = flags.distro;
  }

  const pair_id = uuidV7(now, randomBytes);
  const code = generateSixDigitCode(randomInt);
  const created_at = (typeof now === "function" ? now() : new Date()).toISOString
    ? (typeof now === "function" ? now() : new Date()).toISOString()
    : new Date().toISOString();
  const payload = {
    schema_version: SCHEMA_VERSION,
    pair_id,
    code,
    created_at,
    accepted_at: null,
    distro,
  };

  const w = writeState({
    platform,
    env,
    payload,
    stateDir: o.stateDir,
    randomSuffix: o.randomSuffix,
  });
  if (w.status !== "ok") {
    return buildResult({
      status: PAIR_CREATE_STATUSES.WRITE_FAILED,
      pair_id,
      code,
      path: w.path || null,
      distro,
      output: "failed to write pair.json.",
      hint: "ensure %LOCALAPPDATA% is set and writable.",
    });
  }
  return buildResult({
    status: PAIR_CREATE_STATUSES.PAIR_CREATED,
    exit_code: 0,
    pair_id,
    code,
    path: w.path,
    distro,
    output: `pair_created\npair_id=${pair_id}\ncode=${code}\npath=${w.path}` + (distro ? `\ndistro=${distro}` : ""),
    hint: "the code is operator confirmation, not a cryptographic secret; share via a side-channel only.",
  });
}

module.exports = {
  runPairCreate,
  generateSixDigitCode,
  uuidV7,
  PAIR_CREATE_STATUSES,
};
