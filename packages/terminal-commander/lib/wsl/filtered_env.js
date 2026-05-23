// SPDX-License-Identifier: Apache-2.0
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

module.exports = {
  buildFilteredEnv,
  isSecretEnvKey,
  EXPLICIT_SECRET_KEYS,
  SECRET_ENV_PATTERNS,
};
