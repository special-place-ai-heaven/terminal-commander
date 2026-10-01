// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
"use strict";

function mergeManagedServer(previous, launch, opts) {
  const o = opts || {};
  if (previous != null && (typeof previous !== "object" || Array.isArray(previous))) {
    return { ok: false, reason: "invalid_json" };
  }
  const old = previous || {};
  if (old.url || old.httpUrl || old.serverUrl || (old.type && !["stdio", "local"].includes(old.type))) {
    return { ok: false, reason: "unsupported" };
  }
  const envKey = o.envKey || "env";
  for (const env of [old[envKey], launch[envKey]]) {
    if (env != null && (typeof env !== "object" || Array.isArray(env))) {
      return { ok: false, reason: "invalid_json" };
    }
  }
  const env = { ...old[envKey], ...launch[envKey] };
  if (!Object.hasOwn(launch[envKey] || {}, "TC_WSL_DISTRO")) delete env.TC_WSL_DISTRO;
  if (!o.surfaceExplicit && old[envKey]?.TC_SURFACE !== undefined) env.TC_SURFACE = old[envKey].TC_SURFACE;
  const value = { ...old, ...launch };
  if (envKey === "environment") delete value.args;
  if (Object.keys(env).length) value[envKey] = env;
  else delete value[envKey];
  return { ok: true, value };
}

module.exports = { mergeManagedServer };
