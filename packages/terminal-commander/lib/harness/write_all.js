// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Apply harness config writes for detected providers.

"use strict";

const { writeCursorMcpConfig } = require("../cursor/write.js");
const {
  buildTerminalCommanderCommandConfig,
  buildTerminalCommanderServerConfig,
} = require("../cursor/config.js");
const { writeJsonMcpConfig } = require("./io/json_mcp.js");
const { writeCodexTomlConfig } = require("./io/toml_mcp.js");
const {
  codexConfigPath,
  claudeCodeMcpConfigPath,
  claudeDesktopConfigPath,
} = require("./paths.js");
const { detectProvider } = require("./detect.js");
const { getProvider, listProviders } = require("./registry.js");

const HARNESS_WRITE_STATUSES = Object.freeze({
  SKIPPED: "skipped",
  STUB_UNVERIFIED: "stub_unverified",
  OK: "ok",
  FAILED: "failed",
});

function buildJsonMcpStanza(opts) {
  const o = opts || {};
  const commandConfig = buildTerminalCommanderCommandConfig(o);
  const stanza = {
    command: commandConfig.command,
    args: commandConfig.args,
  };
  if (o.distro && o.platform === "win32") {
    stanza.env = { TC_WSL_DISTRO: o.distro };
  }
  return stanza;
}

function writeProvider(id, opts) {
  const o = opts || {};
  const provider = getProvider(id);
  if (!provider) {
    return { id, status: HARNESS_WRITE_STATUSES.FAILED, hint: "unknown provider" };
  }
  const detection = o.detection || detectProvider(id, o);
  if (!detection.detected) {
    return { id, status: HARNESS_WRITE_STATUSES.SKIPPED, hint: `${id}: not detected` };
  }
  if (provider.stub) {
    return {
      id,
      status: HARNESS_WRITE_STATUSES.STUB_UNVERIFIED,
      hint: `${id}: config_path_unverified (see docs/integrations/${id}.md)`,
    };
  }
  const force = o.force === true;
  const clobber_backup = o.clobber_backup === true;
  const dryRun = o.dry_run === true;

  if (id === "cursor") {
    if (dryRun) {
      const stanza = buildTerminalCommanderServerConfig({
        distro: o.distro,
        knownDistros: o.knownDistros,
        requireKnownDistro: o.requireKnownDistro === true,
      });
      return { id, status: HARNESS_WRITE_STATUSES.OK, dry_run: true, stanza };
    }
    const r = writeCursorMcpConfig({
      scope: o.cursor_scope || "global",
      projectRoot: o.projectRoot,
      platform: o.platform,
      env: o.env,
      distro: o.distro,
      knownDistros: o.knownDistros,
      requireKnownDistro: o.requireKnownDistro === true,
      force,
      clobber_backup,
      randomSuffix: o.randomSuffix,
    });
    return {
      id,
      status: r.status.includes("created") || r.status.includes("updated") ? HARNESS_WRITE_STATUSES.OK : HARNESS_WRITE_STATUSES.FAILED,
      harness_status: r.status,
      path: r.path,
      hint: r.hint,
    };
  }

  if (id === "codex-cli") {
    const target = detection.config_path || codexConfigPath(o);
    if (dryRun) {
      return { id, status: HARNESS_WRITE_STATUSES.OK, dry_run: true, path: target };
    }
    const r = writeCodexTomlConfig({
      path: target,
      force,
      clobber_backup,
      includeEnv: false,
    });
    const ok =
      r.status === "config_created" || r.status === "config_updated";
    return {
      id,
      status: ok ? HARNESS_WRITE_STATUSES.OK : HARNESS_WRITE_STATUSES.FAILED,
      harness_status: r.status,
      path: r.path,
      hint: r.hint,
    };
  }

  if (id === "claude-code" || id === "claude-desktop") {
    const target =
      detection.config_path ||
      (id === "claude-code" ? claudeCodeMcpConfigPath(o) : claudeDesktopConfigPath(o));
    const stanza = buildJsonMcpStanza(o);
    if (dryRun) {
      return { id, status: HARNESS_WRITE_STATUSES.OK, dry_run: true, path: target, stanza };
    }
    const r = writeJsonMcpConfig({
      path: target,
      serverName: provider.serverName,
      serverConfig: stanza,
      force,
      clobber_backup,
      randomSuffix: o.randomSuffix,
    });
    const ok =
      r.status === "config_created" || r.status === "config_updated";
    return {
      id,
      status: ok ? HARNESS_WRITE_STATUSES.OK : HARNESS_WRITE_STATUSES.FAILED,
      harness_status: r.status,
      path: r.path,
      hint: r.hint,
    };
  }

  return { id, status: HARNESS_WRITE_STATUSES.SKIPPED, hint: `${id}: no writer` };
}

/**
 * @param {Object} opts
 * @param {string[]} [opts.providers]  Subset of provider ids; default all non-filtered.
 * @param {string} [opts.providerFilter]  Single provider id from CLI --provider.
 */
function writeAllHarnesses(opts) {
  const o = opts || {};
  let ids = listProviders({ includeStubs: true }).map((p) => p.id);
  if (o.providerFilter) {
    ids = ids.filter((id) => id === o.providerFilter);
  } else if (Array.isArray(o.providers) && o.providers.length > 0) {
    ids = o.providers;
  } else if (o.cursorOnly === true) {
    ids = ["cursor"];
  }
  const results = [];
  for (const id of ids) {
    results.push(writeProvider(id, o));
  }
  return results;
}

module.exports = {
  writeAllHarnesses,
  writeProvider,
  buildJsonMcpStanza,
  HARNESS_WRITE_STATUSES,
};
