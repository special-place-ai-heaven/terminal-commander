// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 platform resolver (extended at WWS02 for win32 bridge mode).
//
// Maps `process.platform` + `process.arch` to one of:
//
//   - `ok`                       Linux + supported arch + platform
//                                package installed; shims spawn the
//                                resolved Rust binary.
//   - `bridge_required`          Windows host (any arch). The root
//                                package is bridge/setup only on
//                                Windows; no Rust binary ships. The
//                                shims branch to a bounded refusal
//                                (terminal-commanderd) or a
//                                "pending WWS04/WWS06" stub message
//                                (terminal-commander-mcp,
//                                terminal-commander). WWS04 owns the
//                                real wsl.exe bridge spawn; this
//                                resolver does NOT call wsl.exe.
//   - `platform_package_missing` Linux + supported arch but
//                                `optionalDependencies` was skipped.
//   - `unsupported_platform`     macOS / FreeBSD / unknown / Linux
//                                arches outside x64+arm64.
//   - `invalid_binary`           Resolver called with a binary name
//                                not in ALLOWED_BINARIES.
//
// Bounded behavior preserved:
//
//   - Never reads files outside `require.resolve` of the platform
//     package's own `package.json`.
//   - Never opens sockets.
//   - Never spawns a child.
//   - Never invokes wsl.exe (deferred to WWS04 lib/wsl/spawn.js).
//   - The shims own the spawn step on Linux; on Windows the shims
//     refuse with a single bounded stderr line + exit 64.
//
// The resolver is pure; it is exercised by `test/resolve-binary.test.js`.

"use strict";

const path = require("path");

const SUPPORTED_TARGETS = Object.freeze([
  Object.freeze({ platform: "linux", arch: "x64",   pkg: "@terminal-commander/linux-x64" }),
  Object.freeze({ platform: "linux", arch: "arm64", pkg: "@terminal-commander/linux-arm64" }),
]);

const ALLOWED_BINARIES = Object.freeze([
  "terminal-commanderd",
  "terminal-commander-mcp",
  "terminal-commander",
]);

/**
 * Resolve the platform package + binary path for the current host.
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {string} [opts.arch=process.arch]
 * @param {(id: string, opts: object) => string} [opts.requireResolve=require.resolve]
 *   Injected for test purposes. Must throw on unresolved IDs.
 * @param {string} opts.binary  Which command to resolve. Must be one of
 *                              ALLOWED_BINARIES.
 * @returns {{
 *   platformPackage: string|null,
 *   binaryPath: string|null,
 *   reason: "ok"|"bridge_required"|"unsupported_platform"|"platform_package_missing"|"invalid_binary",
 *   supportedTargets: ReadonlyArray<{platform:string,arch:string,pkg:string}>
 * }}
 */
function resolveBinary(opts) {
  const platform = (opts && opts.platform) || process.platform;
  const arch     = (opts && opts.arch)     || process.arch;
  const requireResolve = (opts && opts.requireResolve) || require.resolve;
  const binary   = opts && opts.binary;

  if (!ALLOWED_BINARIES.includes(binary)) {
    return {
      platformPackage: null,
      binaryPath: null,
      reason: "invalid_binary",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  if (platform === "win32") {
    return {
      platformPackage: null,
      binaryPath: null,
      reason: "bridge_required",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  const target = SUPPORTED_TARGETS.find(
    (t) => t.platform === platform && t.arch === arch,
  );
  if (!target) {
    return {
      platformPackage: null,
      binaryPath: null,
      reason: "unsupported_platform",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  let pkgJsonPath;
  try {
    pkgJsonPath = requireResolve(target.pkg + "/package.json");
  } catch (_err) {
    return {
      platformPackage: target.pkg,
      binaryPath: null,
      reason: "platform_package_missing",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  const pkgRoot = path.dirname(pkgJsonPath);
  const binaryPath = path.join(pkgRoot, "bin", binary);
  return {
    platformPackage: target.pkg,
    binaryPath,
    reason: "ok",
    supportedTargets: SUPPORTED_TARGETS,
  };
}

/**
 * Format a one-line bounded stderr message for a non-OK resolver
 * result. Returns null when the result is OK. The caller writes the
 * message to stderr and exits non-zero.
 *
 * @param {ReturnType<typeof resolveBinary>} result
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {string} [opts.arch=process.arch]
 * @returns {string|null}
 */
function formatResolveError(result, opts) {
  const platform = (opts && opts.platform) || process.platform;
  const arch     = (opts && opts.arch)     || process.arch;
  if (result.reason === "ok") return null;
  const targets = result.supportedTargets
    .map((t) => `${t.platform}-${t.arch}`)
    .join(", ");
  if (result.reason === "bridge_required") {
    return `terminal-commander: Windows host detected (${platform}-${arch}); root package is bridge/setup only, native runtime targets are ${targets}`;
  }
  if (result.reason === "unsupported_platform") {
    return `terminal-commander: unsupported platform ${platform}-${arch}; supported: ${targets}`;
  }
  if (result.reason === "platform_package_missing") {
    return `terminal-commander: platform package ${result.platformPackage} not installed; npm may have skipped optionalDependencies (need npm >=8)`;
  }
  if (result.reason === "invalid_binary") {
    return "terminal-commander: internal error: invalid binary name passed to resolver";
  }
  return `terminal-commander: unknown resolver state ${result.reason}`;
}

module.exports = {
  resolveBinary,
  formatResolveError,
  SUPPORTED_TARGETS,
  ALLOWED_BINARIES,
};
