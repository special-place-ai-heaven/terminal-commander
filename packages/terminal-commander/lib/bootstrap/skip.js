// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const path = require("node:path");

function shouldSkipBootstrap(env) {
  const e = env || process.env;
  // TC_SKIP_BOOTSTRAP is the long-standing opt-out; TC_NO_AUTO_SETUP is the
  // documented postinstall opt-out (Fix 4). Either one disables auto-setup.
  return e.TC_SKIP_BOOTSTRAP === "1" || e.TC_NO_AUTO_SETUP === "1";
}

/**
 * Heuristic CI / non-interactive / headless detection. A postinstall must be a
 * SAFE no-op in CI and in any non-interactive install so it never blocks or
 * surprises an automated pipeline (Fix 4). Honors the de-facto `CI` standard
 * plus the common provider flags, and treats a non-TTY stdout as headless.
 *
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {boolean}
 */
function isCiOrNonInteractive(env) {
  const e = env || process.env;
  if (e.CI === "true" || e.CI === "1") return true;
  for (const key of [
    "CONTINUOUS_INTEGRATION",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "BUILDKITE",
    "CIRCLECI",
    "TRAVIS",
    "TEAMCITY_VERSION",
    "JENKINS_URL",
  ]) {
    if (e[key]) return true;
  }
  return false;
}

function isGlobalNpmInstall(env) {
  const e = env || process.env;
  if (e.npm_config_global === "true" || e.NPM_CONFIG_GLOBAL === "true") return true;
  if (e.npm_command === "install" && (e.npm_config_global === "true" || e.npm_config_location === "global")) {
    return true;
  }
  if (e.INIT_CWD && e.PREFIX && e.INIT_CWD.startsWith(e.PREFIX)) return true;
  const prefix = e.npm_config_prefix || e.PREFIX;
  if (prefix) {
    try {
      const pkgRoot = path.dirname(path.dirname(__dirname));
      const normalizedPrefix = path.resolve(prefix);
      if (pkgRoot.startsWith(normalizedPrefix)) return true;
    } catch (_e) {
      /* ignore */
    }
  }
  return false;
}

/** True when npm is running this package's install lifecycle script. */
function isPackageInstallLifecycle(env) {
  const e = env || process.env;
  return e.npm_lifecycle_event === "install" && e.npm_lifecycle_script != null;
}

module.exports = {
  shouldSkipBootstrap,
  isCiOrNonInteractive,
  isGlobalNpmInstall,
  isPackageInstallLifecycle,
};
