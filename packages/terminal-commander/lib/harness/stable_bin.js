// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Stable per-user binary directory for AV-safe direct-exe launches.
//
// PROBLEM: pointing an MCP client config at the resolved native exe inside
//   node_modules/@terminal-commander/<plat>/bin/...
// is fragile — npm hoisting moves that path on update, silently breaking the
// config. And pointing at the bare npm name (`terminal-commander-mcp`) makes
// the client launch the npm script-launcher shim -> node -> JS shim -> spawn
// native exe, a script-interpreter-then-spawn chain that heuristic AV reads as a
// loader.
//
// SOLUTION: copy the resolved native exe(s) into a FIXED per-user directory the
// package owns and point the config there. A user copying a tool into their own
// AppData (or ~/.local/share) and running it is completely normal, user-space,
// no-admin behavior. We re-copy whenever the installed package version changes
// so the stable path always matches the installed build (this also fixes the
// adapter/daemon version-skew that Cursor flagged).
//
// USER-SPACE ONLY: plain fs.copyFileSync (no spawn, no shell, no hidden
// window). The target dir is under %LOCALAPPDATA% (Windows) or
// $XDG_DATA_HOME / ~/.local/share (Unix) — both writable by the normal user
// with no elevation. Copy failure (e.g. locked file mid-update) is non-fatal:
// the caller falls back to the bare-name / JS-shim command so setup never
// hard-fails.

"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { resolveBinary } = require("../resolve-binary.js");

// Binaries we mirror into the stable dir. The MCP server exe is the one the
// client config points at; the daemon exe is mirrored so the logon Scheduled
// Task (Part B) can target a stable path too.
const STABLE_BINARIES = Object.freeze(["terminal-commander-mcp", "terminal-commanderd"]);

const STABLE_SUBDIR = Object.freeze(["terminal-commander", "bin"]);
const VERSION_STAMP = ".version";

/**
 * Resolve the fixed per-user directory the package owns for stable binary
 * copies. Pure: derives only from platform + env, never hardcodes a username.
 *
 * Windows: %LOCALAPPDATA%\terminal-commander\bin
 * Unix:    $XDG_DATA_HOME/terminal-commander/bin or ~/.local/share/...
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @returns {string} absolute path
 * @throws {Error} when the required base env var is missing.
 */
function stableBinDir(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;

  let base;
  if (platform === "win32") {
    base = env.LOCALAPPDATA;
    if (!base || base.length === 0) {
      throw new Error(
        "terminal-commander: LOCALAPPDATA not set; cannot derive stable binary directory",
      );
    }
  } else {
    base = env.XDG_DATA_HOME;
    if (!base || base.length === 0) {
      const home = env.HOME;
      if (!home || home.length === 0) {
        throw new Error(
          "terminal-commander: HOME not set; cannot derive stable binary directory",
        );
      }
      base = path.join(home, ".local", "share");
    }
  }
  return path.join(base, ...STABLE_SUBDIR);
}

/**
 * Absolute path of a binary inside the stable dir, with the platform's
 * executable suffix (`.exe` on Windows).
 *
 * @param {string} binary  One of STABLE_BINARIES.
 * @param {Object} [opts]   Same shape as stableBinDir + {platform}.
 * @returns {string}
 */
function stableBinPath(binary, opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const name = platform === "win32" ? `${binary}.exe` : binary;
  return path.join(stableBinDir(o), name);
}

function copyFileSync(src, dest) {
  // Plain user-space copy. No spawn, no shell, no hidden window.
  fs.copyFileSync(src, dest);
}

/**
 * Ensure the resolved native exe(s) are mirrored into the stable per-user dir,
 * re-copying on version change. Returns the stable path of the requested MCP
 * binary (the one the client config points at), or `null` if the copy could
 * not be completed so the caller falls back to the bare-name / JS-shim command.
 *
 * Side effects are confined to the stable dir. All FS ops are guarded; any
 * failure (resolver miss, locked file, mkdir EACCES) yields `null`, never a
 * throw, so `setup` / `update` / `restart` never hard-fail on this step.
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {string} [opts.arch=process.arch]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @param {boolean} [opts.dry_run=false]  Resolve + report the stable path WITHOUT
 *     mkdir / copy / stamp. Used by `--dry-run` / `--print-config` so the planned
 *     direct-exe stanza can be shown without mutating the filesystem.
 * @param {string} [opts.version]  Installed package version; stamped so a
 *     version change forces a re-copy. Defaults to package.json version.
 * @param {string} [opts.primary="terminal-commander-mcp"]  Which binary's
 *     stable path to return.
 * @param {(o:object)=>{reason:string,binaryPath:string|null}} [opts.resolveBinary]
 *     Test seam; defaults to the real resolver.
 * @param {(src:string,dest:string)=>void} [opts.copyFile]  Test seam.
 * @param {(dir:string)=>void} [opts.mkdirp]  Test seam.
 * @param {(p:string)=>boolean} [opts.existsSync]  Test seam.
 * @returns {{ exePath: string|null, copied: string[], reason: string }}
 */
function ensureStableBinaries(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const arch = o.arch || process.arch;
  const primary = o.primary || "terminal-commander-mcp";
  const dryRun = o.dry_run === true;
  const resolve = o.resolveBinary || resolveBinary;
  const copy = o.copyFile || copyFileSync;
  const exists = o.existsSync || fs.existsSync;
  const mkdirp =
    o.mkdirp || ((dir) => fs.mkdirSync(dir, { recursive: true }));
  const version = o.version || readPackageVersion();

  let dir;
  try {
    dir = stableBinDir({ platform, env: o.env });
  } catch (_e) {
    return { exePath: null, copied: [], reason: "no_stable_dir" };
  }

  // Dry-run: report the planned stable path of the primary binary iff it
  // currently resolves, without touching the filesystem.
  if (dryRun) {
    const resolved = resolve({ binary: primary, platform, arch });
    if (resolved.reason !== "ok" || !resolved.binaryPath) {
      return { exePath: null, copied: [], reason: "resolve_failed" };
    }
    return {
      exePath: stableBinPath(primary, { platform, env: o.env }),
      copied: [],
      reason: "dry_run",
    };
  }

  try {
    mkdirp(dir);
  } catch (_e) {
    return { exePath: null, copied: [], reason: "mkdir_failed" };
  }

  // A version stamp lets us skip re-copying unchanged builds while guaranteeing
  // a re-copy after an `npm update` bumps the version (kills version skew).
  const stampPath = path.join(dir, VERSION_STAMP);
  let stamped = null;
  try {
    stamped = fs.readFileSync(stampPath, "utf8").trim();
  } catch (_e) {
    stamped = null;
  }

  const copied = [];
  for (const binary of STABLE_BINARIES) {
    const resolved = resolve({ binary, platform, arch });
    if (resolved.reason !== "ok" || !resolved.binaryPath) {
      // The daemon exe may legitimately be absent on some hosts; only a missing
      // PRIMARY binary forces fallback.
      if (binary === primary) {
        return { exePath: null, copied, reason: "resolve_failed" };
      }
      continue;
    }
    const dest = stableBinPath(binary, { platform, env: o.env });
    const fresh = stamped === version && exists(dest);
    if (fresh) continue;
    try {
      copy(resolved.binaryPath, dest);
      copied.push(dest);
    } catch (_e) {
      if (binary === primary) {
        return { exePath: null, copied, reason: "copy_failed" };
      }
    }
  }

  if (copied.length > 0) {
    try {
      fs.writeFileSync(stampPath, `${version}\n`, { mode: 0o644 });
    } catch (_e) {
      /* stamp is an optimization; a write miss only forces a future re-copy */
    }
  }

  const exePath = stableBinPath(primary, { platform, env: o.env });
  if (!exists(exePath)) {
    return { exePath: null, copied, reason: "missing_after_copy" };
  }
  return { exePath, copied, reason: "ok" };
}

function readPackageVersion() {
  try {
    return require("../../package.json").version || "0.0.0";
  } catch (_e) {
    return "0.0.0";
  }
}

/**
 * True when `child` resolves to a path under the OS temp dir or an npm/npx
 * cache. A binary living there is TRANSIENT — npx unpacks one run's package
 * into a cache dir that is GC'd, so registering it bakes a path that vanishes.
 * Mirrors SymForge's `path_is_inside(temp_dir, ...)` + `_npx`/`npx-cache`
 * refusal (src/cli/init.rs).
 *
 * @param {string} child
 * @param {Object} [opts]
 * @param {string} [opts.tmpDir]  Defaults to os.tmpdir().
 * @returns {boolean}
 */
function isTransientBinaryPath(child, opts) {
  const o = opts || {};
  if (typeof child !== "string" || child.length === 0) return false;
  const lower = child.toLowerCase();
  if (lower.includes("_npx") || lower.includes("npx-cache")) return true;
  const tmp = o.tmpDir || os.tmpdir();
  if (typeof tmp === "string" && tmp.length > 0) {
    const sep = path.sep;
    const absTmp = path.resolve(tmp);
    const absChild = path.resolve(child);
    if (absChild === absTmp || absChild.startsWith(absTmp + sep)) return true;
  }
  return false;
}

/**
 * Resolve a real ABSOLUTE path to the currently-running platform MCP binary
 * inside node_modules (the same exe `resolveBinary` returns) for use as a
 * harness `command` when the stable per-user copy could not be made. This is
 * the known-good resolution the live codex entry already uses.
 *
 * SymForge guard mirrored: a binary under the OS temp dir / npx cache is
 * transient, so we REFUSE to return it (the caller warns loudly and falls back
 * to the bare name only as an absolute last resort).
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {string} [opts.arch=process.arch]
 * @param {string} [opts.binary="terminal-commander-mcp"]
 * @param {(o:object)=>{reason:string,binaryPath:string|null}} [opts.resolveBinary]  Test seam.
 * @param {string} [opts.tmpDir]  Test seam for the temp-dir guard.
 * @returns {{ exePath: string|null, reason: string }}
 *   reason: "ok" | "resolve_failed" | "transient_path"
 */
function resolveDirectExePath(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const arch = o.arch || process.arch;
  const binary = o.binary || "terminal-commander-mcp";
  const resolve = o.resolveBinary || resolveBinary;
  const resolved = resolve({ binary, platform, arch });
  if (resolved.reason !== "ok" || !resolved.binaryPath) {
    return { exePath: null, reason: "resolve_failed" };
  }
  if (isTransientBinaryPath(resolved.binaryPath, { tmpDir: o.tmpDir })) {
    return { exePath: null, reason: "transient_path" };
  }
  return { exePath: resolved.binaryPath, reason: "ok" };
}

module.exports = {
  STABLE_BINARIES,
  VERSION_STAMP,
  stableBinDir,
  stableBinPath,
  ensureStableBinaries,
  resolveDirectExePath,
  isTransientBinaryPath,
};
