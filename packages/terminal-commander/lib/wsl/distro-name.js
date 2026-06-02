// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS03 distro-name whitelist.
//
// A WSL distro name is the ONLY operator-supplied string that the WWS
// chain ever passes to `wsl.exe`. Future callers (WWS04 bridge spawn,
// WWS06 setup CLI) take a distro string from operator input
// (--distro flag, persisted setup.json, or live `wsl.exe -l -v`
// output) and pass it as a single argv element after `-d`. To avoid
// `; calc.exe`-style injection, that string MUST pass a conservative
// character whitelist before it is allowed anywhere near a spawn
// argv.
//
// Allowed: ASCII letters, digits, `.`, `_`, `-`. Length 1..64.
// Rejected: everything else, including whitespace, NUL, quote,
// semicolon, pipe, dollar, backtick, slash, backslash, ampersand,
// parens, redirect arrows, and any non-ASCII byte.
//
// `isSafeDistroName` is pure. `assertSafeDistroName` throws an Error
// with `.code = 'UNSAFE_DISTRO_NAME'` so callers can branch on the
// code field instead of message-text matching.

"use strict";

const UNSAFE_DISTRO_NAME = "UNSAFE_DISTRO_NAME";

const SAFE_DISTRO_NAME_RE = /^[A-Za-z0-9._-]{1,64}$/;

function isSafeDistroName(name) {
  if (typeof name !== "string") return false;
  return SAFE_DISTRO_NAME_RE.test(name);
}

function assertSafeDistroName(name) {
  if (!isSafeDistroName(name)) {
    const err = new Error(
      "terminal-commander: distro name failed safety whitelist",
    );
    err.code = UNSAFE_DISTRO_NAME;
    throw err;
  }
}

module.exports = {
  isSafeDistroName,
  assertSafeDistroName,
  UNSAFE_DISTRO_NAME,
  SAFE_DISTRO_NAME_RE,
};
