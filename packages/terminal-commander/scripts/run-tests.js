#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const packageRoot = path.resolve(__dirname, "..");
const testDir = path.join(packageRoot, "test");
const testFiles = fs
  .readdirSync(testDir, { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".test.js"))
  .map((entry) => path.join(testDir, entry.name))
  .sort();

if (testFiles.length === 0) {
  process.stderr.write(`terminal-commander tests: no *.test.js files found in ${testDir}\n`);
  process.exit(1);
}

const result = spawnSync(process.execPath, ["--test", ...testFiles], {
  cwd: packageRoot,
  stdio: "inherit",
  shell: false,
});

if (result.error) {
  process.stderr.write(`terminal-commander tests: failed to start Node: ${result.error.message}\n`);
  process.exit(126);
}
if (result.signal) {
  process.stderr.write(`terminal-commander tests: Node terminated by ${result.signal}\n`);
  process.exit(1);
}
process.exit(result.status == null ? 1 : result.status);
