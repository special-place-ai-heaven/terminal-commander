#!/usr/bin/env node
"use strict";

const pkg = require("../../packages/terminal-commander/package.json");
const version = pkg.version;
const deps = pkg.optionalDependencies || {};

for (const name of [
  "@terminal-commander/linux-x64",
  "@terminal-commander/linux-arm64",
]) {
  if (deps[name] !== version) {
    console.error(
      `${name} optionalDependency = ${deps[name] ?? "(missing)"} != ${version}`
    );
    process.exit(1);
  }
}

console.log(`optionalDependencies pinned to ${version}`);
