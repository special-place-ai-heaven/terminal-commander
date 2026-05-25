#!/usr/bin/env node
"use strict";

const pkg = require("../../packages/terminal-commander/package.json");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");

const version = pkg.version;
const deps = pkg.optionalDependencies || {};

for (const name of PLATFORM_PACKAGES) {
  if (deps[name] !== version) {
    console.error(
      `${name} optionalDependency = ${deps[name] ?? "(missing)"} != ${version}`
    );
    process.exit(1);
  }
}

console.log(`optionalDependencies pinned to ${version} (${PLATFORM_PACKAGES.length} platforms)`);
