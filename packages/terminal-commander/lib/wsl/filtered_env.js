// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const EXPLICIT_SECRET_KEYS = Object.freeze([
  "NPM_TOKEN",
  "NPM_TOKEN_TC",
  "CARGO_REGISTRY_TOKEN",
  "CARGO_REGISTRY_TOKEN_TC",
  "RELEASE_PLEASE_TOKEN",
  "RELEASE_PLEASE_TOKEN_TC",
  "GITHUB_TOKEN",
  "GH_TOKEN",
  "OPENAI_API_KEY",
  "ANTHROPIC_API_KEY",
  "SLACK_TOKEN",
]);

const SECRET_ENV_PATTERNS = Object.freeze([
  /_TOKEN$/i,
  /_SECRET$/i,
  /_PASSWORD$/i,
  /_PASS$/i,
  /_API_KEY$/i,
  /_APIKEY$/i,
  /^AWS_SESSION_TOKEN$/i,
  /^AWS_SECRET_ACCESS_KEY$/i,
]);

function isSecretEnvKey(key) {
  if (EXPLICIT_SECRET_KEYS.includes(key)) return true;
  for (const re of SECRET_ENV_PATTERNS) {
    if (re.test(key)) return true;
  }
  return false;
}

function buildFilteredEnv(parentEnv) {
  const out = {};
  for (const key of Object.keys(parentEnv)) {
    if (isSecretEnvKey(key)) continue;
    out[key] = parentEnv[key];
  }
  return out;
}

/**
 * Rebuild WSLENV from a TC-only allowlist so a wsl.exe spawn forwards exactly
 * `TC_SESSION/u` to the WSL child — nothing else.
 *
 * `wsl.exe` forwards every Windows env var listed in WSLENV into the Linux
 * process it launches. Preserving the operator's ambient WSLENV would leak
 * whatever they had named there (e.g. `WSL_SUDO_CREDENTIAL/u`) across the
 * boundary. `buildFilteredEnv` only strips secrets by variable *name*; it does
 * NOT rebuild WSLENV, so an ambient `WSLENV=SOME_SECRET/u` still forwards
 * SOME_SECRET into WSL even after name-based filtering. This function is the
 * required second step. Therefore:
 *
 *   - TC_SESSION present -> WSLENV = "TC_SESSION/u" (ambient dropped).
 *   - TC_SESSION absent  -> WSLENV deleted (ambient dropped to avoid
 *     forwarding credential-shaped vars even when we have nothing of our own
 *     to send).
 *
 * Flag `/u` = forward Windows->WSL only, no path translation (TC_SESSION is
 * an opaque token, not a path). Pure: returns a new object.
 *
 * Lives here (rather than spawn.js) so every wsl-spawn site can import it
 * alongside buildFilteredEnv without a require cycle through spawn.js. It is
 * re-exported from spawn.js for back-compat.
 *
 * @param {NodeJS.ProcessEnv} filteredEnv  Already secret-filtered env.
 * @returns {NodeJS.ProcessEnv}
 */
function ensureSessionInWslEnv(filteredEnv) {
  const out = { ...filteredEnv };
  if (out.TC_SESSION == null || out.TC_SESSION === "") {
    // No session token -> drop any ambient WSLENV. The spawn forwards nothing
    // the WSL runtime needs (distro is host-side), and ambient entries could
    // forward credentials (e.g. WSL_SUDO_CREDENTIAL).
    delete out.WSLENV;
    return out;
  }
  // TC-only allowlist: TC_SESSION/u and nothing else. Ambient WSLENV dropped.
  out.WSLENV = "TC_SESSION/u";
  return out;
}

module.exports = {
  buildFilteredEnv,
  ensureSessionInWslEnv,
  isSecretEnvKey,
  EXPLICIT_SECRET_KEYS,
  SECRET_ENV_PATTERNS,
};
