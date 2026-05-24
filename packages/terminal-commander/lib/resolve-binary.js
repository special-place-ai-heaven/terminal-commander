// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const fs = require("fs");
const path = require("path");

const SUPPORTED_TARGETS = Object.freeze([
  Object.freeze({ platform: "linux", arch: "x64", pkg: "@terminal-commander/linux-x64" }),
  Object.freeze({ platform: "linux", arch: "arm64", pkg: "@terminal-commander/linux-arm64" }),
  Object.freeze({ platform: "win32", arch: "x64", pkg: "@terminal-commander/windows-x64" }),
]);

const ALLOWED_BINARIES = Object.freeze([
  "terminal-commanderd",
  "terminal-commander-mcp",
  "terminal-commander",
]);

const MONOREPO_PLATFORM_DIRS = Object.freeze({
  "@terminal-commander/linux-x64": "terminal-commander-linux-x64",
  "@terminal-commander/linux-arm64": "terminal-commander-linux-arm64",
  "@terminal-commander/windows-x64": "terminal-commander-windows-x64",
});

function findPlatformPackageJson(target, requireResolve) {
  try {
    return requireResolve(target.pkg + "/package.json");
  } catch (_err) {
    /* fall through */
  }

  let dir = path.join(__dirname, "..");
  for (let i = 0; i < 6; i++) {
    const nested = path.join(dir, "node_modules", target.pkg, "package.json");
    if (fs.existsSync(nested)) {
      return nested;
    }
    const siblingDir = MONOREPO_PLATFORM_DIRS[target.pkg];
    if (siblingDir) {
      const sibling = path.join(dir, "..", siblingDir, "package.json");
      if (fs.existsSync(sibling)) {
        return sibling;
      }
      const sibling2 = path.join(dir, siblingDir, "package.json");
      if (fs.existsSync(sibling2)) {
        return sibling2;
      }
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return null;
}

function resolveBinary(opts) {
  const platform = (opts && opts.platform) || process.platform;
  const arch = (opts && opts.arch) || process.arch;
  const requireResolve = (opts && opts.requireResolve) || require.resolve;
  const binary = opts && opts.binary;

  if (!ALLOWED_BINARIES.includes(binary)) {
    return {
      platformPackage: null,
      binaryPath: null,
      reason: "invalid_binary",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  const target = SUPPORTED_TARGETS.find((t) => t.platform === platform && t.arch === arch);
  if (!target) {
    return {
      platformPackage: null,
      binaryPath: null,
      reason: "unsupported_platform",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  const pkgJsonPath = findPlatformPackageJson(target, requireResolve);
  if (!pkgJsonPath) {
    return {
      platformPackage: target.pkg,
      binaryPath: null,
      reason: "platform_package_missing",
      supportedTargets: SUPPORTED_TARGETS,
    };
  }

  const pkgRoot = path.dirname(pkgJsonPath);
  let binaryPath = path.join(pkgRoot, "bin", binary);
  if (platform === "win32") {
    const withExe = `${binaryPath}.exe`;
    if (fs.existsSync(withExe)) {
      binaryPath = withExe;
    } else if (!fs.existsSync(binaryPath)) {
      return {
        platformPackage: target.pkg,
        binaryPath: null,
        reason: "platform_package_missing",
        supportedTargets: SUPPORTED_TARGETS,
      };
    }
  }

  return {
    platformPackage: target.pkg,
    binaryPath,
    reason: "ok",
    supportedTargets: SUPPORTED_TARGETS,
  };
}

function formatResolveError(result, opts) {
  const platform = (opts && opts.platform) || process.platform;
  const arch = (opts && opts.arch) || process.arch;
  if (result.reason === "ok") return null;
  const targets = result.supportedTargets.map((t) => `${t.platform}-${t.arch}`).join(", ");
  if (result.reason === "unsupported_platform") {
    return `terminal-commander: unsupported platform ${platform}-${arch}; supported: ${targets}`;
  }
  if (result.reason === "platform_package_missing") {
    return (
      `terminal-commander: platform package ${result.platformPackage} not installed. ` +
      `From the terminal-commander package directory run: npm install ${result.platformPackage}@0.1.4 ` +
      `(or for local dev: npm install file:../terminal-commander-windows-x64), then reinstall globally.`
    );
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
