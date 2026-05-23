// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const path = require("node:path");

function shouldSkipBootstrap(env) {
  const e = env || process.env;
  return e.TC_SKIP_BOOTSTRAP === "1";
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
  isGlobalNpmInstall,
  isPackageInstallLifecycle,
};
