#!/usr/bin/env node
/**
 * Keep the npm package graph pinned to the root package.json version.
 *
 * release-please updates the root package first. This script is run by
 * release-pr-sync on the release PR branch before auto-merge so the five
 * platform package package.json files, root optionalDependencies, and
 * release-please manifest entries stay in lockstep.
 */
"use strict";

const fs = require("fs");
const path = require("path");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");

const ROOT = path.join(__dirname, "..", "..");
const pkgPath = path.join(
  ROOT,
  "packages",
  "terminal-commander",
  "package.json"
);
const manifestPath = path.join(ROOT, ".github", ".release-please-manifest.json");
const platformPackageDirs = new Map(
  PLATFORM_PACKAGES.map((name) => [
    name,
    path.join(ROOT, "packages", name.replace("@terminal-commander/", "terminal-commander-")),
  ]),
);

const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
const version = pkg.version;
if (!version) {
  console.error("package.json missing version");
  process.exit(1);
}

pkg.optionalDependencies = pkg.optionalDependencies || {};

let changed = false;
for (const name of PLATFORM_PACKAGES) {
  if (pkg.optionalDependencies[name] !== version) {
    pkg.optionalDependencies[name] = version;
    changed = true;
  }
}

if (changed) {
  fs.writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log(`synced optionalDependencies to ${version}`);
} else {
  console.log(`optionalDependencies already at ${version}`);
}

for (const [name, dir] of platformPackageDirs.entries()) {
  const platformPkgPath = path.join(dir, "package.json");
  const platformPkg = JSON.parse(fs.readFileSync(platformPkgPath, "utf8"));
  if (platformPkg.version !== version) {
    platformPkg.version = version;
    fs.writeFileSync(platformPkgPath, `${JSON.stringify(platformPkg, null, 2)}\n`);
    changed = true;
    console.log(`synced ${name} package.json to ${version}`);
  } else {
    console.log(`${name} package.json already at ${version}`);
  }
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
for (const manifestKey of Object.keys(manifest)) {
  if (manifest[manifestKey] !== version) {
    manifest[manifestKey] = version;
    changed = true;
  }
}

if (changed) {
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`synced release-please manifest to ${version}`);
} else {
  console.log(`release-please manifest already at ${version}`);
}
