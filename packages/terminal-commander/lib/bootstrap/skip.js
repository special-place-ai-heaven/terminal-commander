// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

function shouldSkipBootstrap(env) {
  const e = env || process.env;
  return e.TC_SKIP_BOOTSTRAP === "1";
}

function isGlobalNpmInstall(env) {
  const e = env || process.env;
  if (e.npm_config_global === "true" || e.NPM_CONFIG_GLOBAL === "true") return true;
  if (e.INIT_CWD && e.PREFIX && e.INIT_CWD.startsWith(e.PREFIX)) return true;
  return false;
}

module.exports = {
  shouldSkipBootstrap,
  isGlobalNpmInstall,
};
