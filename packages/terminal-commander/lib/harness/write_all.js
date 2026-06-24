// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Apply harness config writes for detected providers.

"use strict";

const { writeCursorMcpConfig } = require("../cursor/write.js");
const {
  buildTerminalCommanderCommandConfig,
  buildTerminalCommanderServerConfig,
  assertSafeDistroName,
} = require("../cursor/config.js");
const { writeJsonMcpConfig } = require("./io/json_mcp.js");
const { writeCodexTomlConfig, buildCodexEnv } = require("./io/toml_mcp.js");
const {
  codexConfigPath,
  claudeCodeMcpConfigPath,
  claudeDesktopConfigPath,
} = require("./paths.js");
const { detectProvider } = require("./detect.js");
const { getProvider, listProviders } = require("./registry.js");
const { mintSessionToken, isValidSessionToken } = require("../session/mint.js");

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
  const env = {};
  if (o.sessionToken != null && o.sessionToken !== "") {
    // Defense in depth: validate before the token can name a kernel object /
    // socket path, symmetric with the Cursor path (config.js). Today callers
    // pass minted (valid-by-construction) tokens, but this is an exported fn.
    if (!isValidSessionToken(o.sessionToken)) {
      const err = new Error(
        "terminal-commander: TC_SESSION token failed safety whitelist; only [A-Za-z0-9._-] (1..64, at least one alphanumeric, not dot-only) is allowed",
      );
      err.code = "UNSAFE_SESSION_TOKEN";
      throw err;
    }
    env.TC_SESSION = o.sessionToken;
  }
  if (o.distro && o.platform === "win32") {
    // Defense in depth: the distro is interpolated into a `wsl -d <distro>`
    // command downstream, so validate the charset before it lands in env —
    // symmetric with the Cursor path (buildTerminalCommanderServerConfig).
    assertSafeDistroName(o.distro);
    env.TC_WSL_DISTRO = o.distro;
  }
  // TC_SURFACE selects the MCP tool surface (compact 5-facade vs full). Value is
  // validated as compact|full at the CLI parser boundary; never security-sensitive
  // (unlike the token/distro above, it names no kernel object), so no extra guard.
  if (o.surface) {
    env.TC_SURFACE = o.surface;
  }
  if (Object.keys(env).length > 0) {
    stanza.env = env;
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

  // F1: mint a stable, per-harness session token so each provider gets its
  // own daemon endpoint. The provider id is the stable harness id; machineKey
  // defaults to the hostname inside mintSessionToken.
  const sessionToken = mintSessionToken({
    harnessId: id,
    machineKey: o.machineKey,
  });

  if (id === "cursor") {
    if (dryRun) {
      const stanza = buildTerminalCommanderServerConfig({
        exePath: o.exePath,
        sessionToken,
        distro: o.distro,
        knownDistros: o.knownDistros,
        requireKnownDistro: o.requireKnownDistro === true,
        surface: o.surface,
      });
      return { id, status: HARNESS_WRITE_STATUSES.OK, dry_run: true, stanza };
    }
    const r = writeCursorMcpConfig({
      scope: o.cursor_scope || "global",
      projectRoot: o.projectRoot,
      platform: o.platform,
      env: o.env,
      exePath: o.exePath,
      sessionToken,
      distro: o.distro,
      knownDistros: o.knownDistros,
      requireKnownDistro: o.requireKnownDistro === true,
      surface: o.surface,
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
    // Codex now gets the same per-harness env as the JSON harnesses: its own
    // TC_SESSION daemon endpoint, the TC_SURFACE tool surface, and TC_WSL_DISTRO
    // on Windows. Codex applies these literally to the spawned MCP server's
    // process env (verified against openai/codex codex-rs mcp_types.rs).
    const codexEnvOpts = {
      sessionToken,
      surface: o.surface,
      distro: o.distro,
      platform: o.platform,
    };
    if (dryRun) {
      const cmd = buildTerminalCommanderCommandConfig(o);
      const env = buildCodexEnv(codexEnvOpts);
      const stanza = { command: cmd.command, args: cmd.args };
      if (Object.keys(env).length > 0) stanza.env = env;
      return { id, status: HARNESS_WRITE_STATUSES.OK, dry_run: true, path: target, stanza };
    }
    const r = writeCodexTomlConfig({
      path: target,
      exePath: o.exePath,
      force,
      clobber_backup,
      ...codexEnvOpts,
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
    const stanza = buildJsonMcpStanza({ ...o, sessionToken });
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
