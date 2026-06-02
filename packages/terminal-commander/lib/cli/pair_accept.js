// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 `terminal-commander pair accept <code>`.
//
// Validates the code's 6-digit shape AND its match against the
// persisted pair.json. The full WSL-side handshake (session token
// exchange with the daemon over the bridge) is DEFERRED — at WWS06
// `pair accept` returns `pair_accepted` on a code match, or
// `pair_deferred` if no record exists yet (which is the expected
// state on machines that never ran `pair create`).
//
// NO network. NO credential. NO env var read. NO secret. NO LLM-
// supplied value forwarded anywhere. The on-disk record is updated
// only to set `accepted_at`.

"use strict";

const path = require("node:path");
const { readPairJson, writePairJson, getStateDir } = require("./setup_state.js");

const PAIR_ACCEPT_STATUSES = Object.freeze({
  PAIR_ACCEPTED: "pair_accepted",
  PAIR_DEFERRED: "pair_deferred",
  INVALID_CODE_SHAPE: "invalid_code_shape",
  WRITE_FAILED: "write_failed",
  UNSUPPORTED_HOST: "unsupported_host",
});

const SIX_DIGIT_RE = /^[0-9]{6}$/;

function buildResult(partial) {
  return {
    status: partial.status,
    exit_code: typeof partial.exit_code === "number" ? partial.exit_code : 64,
    pair_id: partial.pair_id || null,
    code: partial.code || null,
    accepted_at: partial.accepted_at || null,
    output: partial.output || "",
    hint: partial.hint || "",
  };
}

async function runPairAccept(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const code = o.code;
  const readState = o.readPairJson || readPairJson;
  const writeState = o.writePairJson || writePairJson;
  const now = o.now;

  if (typeof code !== "string" || !SIX_DIGIT_RE.test(code)) {
    return buildResult({
      status: PAIR_ACCEPT_STATUSES.INVALID_CODE_SHAPE,
      code: typeof code === "string" ? code : null,
      output: "pair accept expects a 6-digit code (exactly six ASCII digits).",
      hint: "run 'terminal-commander pair create' first to generate a code.",
    });
  }

  let stateDir;
  try {
    stateDir = o.stateDir || getStateDir({ platform, env });
  } catch (_e) {
    return buildResult({
      status: PAIR_ACCEPT_STATUSES.UNSUPPORTED_HOST,
      code,
      output: "cannot derive state directory on this host.",
      hint: "set LOCALAPPDATA on Windows or HOME on Linux.",
    });
  }

  const r = readState({ stateDir });
  if (!r.ok || r.value == null) {
    return buildResult({
      status: PAIR_ACCEPT_STATUSES.PAIR_DEFERRED,
      code,
      output: "no pair.json found; the WSL-side handshake is not yet implemented at WWS06.",
      hint: "future enhancement: implement the WSL-side daemon session token exchange.",
    });
  }
  const persisted = r.value;
  if (
    typeof persisted.code !== "string" ||
    persisted.code !== code
  ) {
    return buildResult({
      status: PAIR_ACCEPT_STATUSES.PAIR_DEFERRED,
      code,
      pair_id: persisted.pair_id || null,
      output: "supplied code does not match the persisted pair record.",
      hint: "re-run 'terminal-commander pair create' to generate a fresh code.",
    });
  }
  const accepted_at = (typeof now === "function" ? now() : new Date()).toISOString();
  const updated = {
    ...persisted,
    accepted_at,
  };
  const w = writeState({
    platform,
    env,
    payload: updated,
    stateDir,
    randomSuffix: o.randomSuffix,
  });
  if (w.status !== "ok") {
    return buildResult({
      status: PAIR_ACCEPT_STATUSES.WRITE_FAILED,
      code,
      pair_id: persisted.pair_id || null,
      output: "failed to update pair.json with accepted_at timestamp.",
      hint: "the code matched, but the on-disk update failed; re-run after fixing %LOCALAPPDATA% permissions.",
    });
  }
  return buildResult({
    status: PAIR_ACCEPT_STATUSES.PAIR_ACCEPTED,
    exit_code: 0,
    code,
    pair_id: persisted.pair_id || null,
    accepted_at,
    output: `pair_accepted\npair_id=${persisted.pair_id}\naccepted_at=${accepted_at}`,
    hint: "the full WSL-side handshake is deferred to a future enhancement; this command only validates the code shape and persisted match.",
  });
}

module.exports = {
  runPairAccept,
  PAIR_ACCEPT_STATUSES,
  SIX_DIGIT_RE,
};
