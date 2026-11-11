#!/usr/bin/env node
"use strict";

const pkg = require("../../packages/terminal-commander/package.json");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");

const version = pkg.version;
const deps = pkg.optionalDependencies || {};
const extras = Object.keys(deps).filter((name) => !PLATFORM_PACKAGES.includes(name));
if (extras.length > 0) {
  console.error(`unexpected optionalDependencies: ${extras.join(", ")}`);
  process.exit(1);
}

for (const name of PLATFORM_PACKAGES) {
  if (deps[name] !== version) {
    console.error(
      `${name} optionalDependency = ${deps[name] ?? "(missing)"} != ${version}`
    );
    process.exit(1);
  }
}

console.log(`optionalDependencies pinned to ${version} (${PLATFORM_PACKAGES.length} platforms)`);
