// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 platform resolver.
//
// Maps `process.platform` + `process.arch` to the matching Terminal
// Commander platform npm package and returns the absolute path to a
// requested Rust binary. Bounded behavior:
//
//   - Only `linux/x64` and `linux/arm64` are supported.
//   - Returns `{ binaryPath: null, ... }` on unsupported platform OR
//     when the platform package is not installed.
//   - Never reads files. Never opens sockets. Never spawns a child.
//     (The bin shims own the spawn step.)
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
 *   reason: "ok"|"unsupported_platform"|"platform_package_missing"|"invalid_binary",
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
