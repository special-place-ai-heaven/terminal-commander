// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const path = require("node:path");
const { stableBinDir } = require("../harness/stable_bin.js");

const SUPPORTED_HOSTS = new Set(["win32", "linux", "darwin"]);

function detectRuntimeEnvironment(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  const flags = o.flags || {};

  if (!SUPPORTED_HOSTS.has(platform)) {
    return {
      status: "unsupported",
      host: platform,
      runtime: null,
      evidence: `unsupported_platform:${platform}`,
    };
  }

  if (platform !== "win32") {
    return {
      status: "ok",
      host: platform === "darwin" ? "macos" : "linux",
      runtime: "native",
      evidence: `${platform}_native`,
    };
  }

  if (typeof flags.distro === "string" && flags.distro.length > 0) {
    return { status: "ok", host: "windows", runtime: "wsl", evidence: "flag:distro" };
  }
  if (typeof env.TC_WSL_DISTRO === "string" && env.TC_WSL_DISTRO.length > 0) {
    return { status: "ok", host: "windows", runtime: "wsl", evidence: "env:TC_WSL_DISTRO" };
  }
  if (env.TC_USE_LEGACY_WSL_BRIDGE === "1") {
    return {
      status: "ok",
      host: "windows",
      runtime: "wsl",
      evidence: "env:TC_USE_LEGACY_WSL_BRIDGE",
    };
  }

  return { status: "ok", host: "windows", runtime: "native", evidence: "win32_default" };
}

function windowsUpdateScopes(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  if (platform !== "win32") return [];
  if (!o.packageRoot) {
    throw new Error("terminal-commander: package root missing during update preflight");
  }

  const scopes = [
    path.dirname(o.packageRoot),
    stableBinDir({ platform, env: o.env || process.env }),
  ];
  const seen = new Set();
  return scopes.filter((scope) => {
    const key = path.resolve(scope).toLowerCase();
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function describeError(value) {
  if (value && typeof value.code === "string" && value.code.length > 0) {
    return value.code;
  }
  if (value && typeof value.message === "string" && value.message.length > 0) {
    return value.message;
  }
  if (typeof value === "string" && value.length > 0) return value;
  const type = value === null ? "null" : typeof value;
  return `non-Error rejection (${type})`;
}

module.exports = {
  detectRuntimeEnvironment,
  windowsUpdateScopes,
  describeError,
};
