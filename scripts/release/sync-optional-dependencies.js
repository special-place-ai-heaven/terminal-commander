#!/usr/bin/env node
/**
 * Keep root wrapper optionalDependencies pinned to package.json version.
 * Used in CI before npm publish; release-please extra-files should do this
 * on the release PR, but we enforce at publish time as a safety net.
 */
"use strict";

const fs = require("fs");
const path = require("path");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");

const pkgPath = path.join(
  __dirname,
  "..",
  "..",
  "packages",
  "terminal-commander",
  "package.json"
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
