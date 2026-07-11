#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");
const { SEMVER } = require("./resolve-release-context.js");

const PACKAGE_DIRS = [
  "packages/terminal-commander",
  "packages/terminal-commander-linux-x64",
  "packages/terminal-commander-linux-arm64",
  "packages/terminal-commander-windows-x64",
  "packages/terminal-commander-mac-x64",
  "packages/terminal-commander-mac-arm64",
];

function readJson(repoRoot, relativePath) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), "utf8"));
}

function validateReleaseInputs(repoRoot, expectedVersion) {
  if (!SEMVER.test(expectedVersion || "")) {
    throw new Error(`expected release version is invalid: '${expectedVersion || ""}'`);
  }

  const versions = [];
  for (const packageDir of PACKAGE_DIRS) {
    const pkg = readJson(repoRoot, `${packageDir}/package.json`);
    versions.push([`${packageDir}/package.json`, pkg.version]);
  }

  const manifest = readJson(repoRoot, ".github/.release-please-manifest.json");
  for (const packageDir of PACKAGE_DIRS) {
    versions.push([`.github/.release-please-manifest.json:${packageDir}`, manifest[packageDir]]);
  }

  const rootLock = readJson(repoRoot, "packages/terminal-commander/package-lock.json");
  versions.push(["packages/terminal-commander/package-lock.json", rootLock.version]);
  versions.push([
    "packages/terminal-commander/package-lock.json:packages['']",
    rootLock.packages && rootLock.packages[""] && rootLock.packages[""].version,
  ]);

  const cargoToml = fs.readFileSync(path.join(repoRoot, "Cargo.toml"), "utf8");
  const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
  versions.push(["Cargo.toml:[workspace.package].version", cargoVersion && cargoVersion[1]]);

  const mismatches = versions.filter(([, version]) => version !== expectedVersion);
  if (mismatches.length > 0) {
    throw new Error(
      `release version mismatch; expected ${expectedVersion}: ${mismatches
        .map(([source, version]) => `${source}=${version || "(missing)"}`)
        .join(", ")}`,
    );
  }

  const rootPackage = readJson(repoRoot, "packages/terminal-commander/package.json");
  for (const packageName of PLATFORM_PACKAGES) {
    const actual = rootPackage.optionalDependencies && rootPackage.optionalDependencies[packageName];
    if (actual !== expectedVersion) {
      throw new Error(
        `${packageName} optionalDependency=${actual || "(missing)"} != ${expectedVersion}`,
      );
    }
  }

  return { version: expectedVersion, checkedVersions: versions.length };
}

if (require.main === module) {
  try {
    const expectedVersion = process.argv[2] || process.env.EXPECTED_VERSION;
    const result = validateReleaseInputs(path.resolve(__dirname, "../.."), expectedVersion);
    process.stdout.write(
      `release-inputs: ${result.checkedVersions} version anchors agree on ${result.version}\n`,
    );
  } catch (err) {
    process.stderr.write(`release-inputs: ${err.message}\n`);
    process.exit(1);
  }
}

module.exports = { validateReleaseInputs, PACKAGE_DIRS };
