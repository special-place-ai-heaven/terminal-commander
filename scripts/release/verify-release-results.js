#!/usr/bin/env node
"use strict";

const EXPECTED_RELEASE_JOBS = Object.freeze([
  "prepublish-gate",
  "build-linux-x64",
  "presmoke-linux-x64",
  "publish-linux-x64",
  "build-linux-arm64",
  "presmoke-linux-arm64",
  "publish-linux-arm64",
  "build-windows-x64",
  "publish-windows-x64",
  "build-mac-x64",
  "publish-mac-x64",
  "build-mac-arm64",
  "publish-mac-arm64",
  "publish-root",
  "publish-cargo-terminal-commander-core",
  "publish-cargo-terminal-commander-sifters",
  "publish-cargo-terminal-commander-probes",
  "publish-cargo-terminal-commander-store",
  "publish-cargo-terminal-commander-supervisor",
  "publish-cargo-terminal-commander-ipc",
  "publish-cargo-terminal-commanderd",
  "publish-cargo-terminal-commander-mcp",
  "verify-linux-x64",
  "verify-linux-arm64",
  "verify-windows-x64",
  "verify-mac-x64",
  "verify-mac-arm64",
]);

function verifyReleaseResults(needs) {
  const context = needs && needs["release-context"];
  if (!context || context.result !== "success") {
    throw new Error(`release-context result=${context ? context.result : "missing"}`);
  }

  if (String(context.outputs && context.outputs.publish) !== "true") {
    return { status: "no_release", checkedJobs: 0 };
  }

  const incomplete = EXPECTED_RELEASE_JOBS.filter((name) => {
    const job = needs[name];
    return !job || job.result !== "success";
  });
  if (incomplete.length > 0) {
    throw new Error(
      `publish was requested but required jobs were not successful: ${incomplete
        .map((name) => `${name}=${needs[name] ? needs[name].result : "missing"}`)
        .join(", ")}`,
    );
  }

  return { status: "complete", checkedJobs: EXPECTED_RELEASE_JOBS.length };
}

if (require.main === module) {
  try {
    if (!process.env.NEEDS_JSON) throw new Error("NEEDS_JSON is not set");
    const result = verifyReleaseResults(JSON.parse(process.env.NEEDS_JSON));
    process.stdout.write(
      `release-verdict: ${result.status}; checked_jobs=${result.checkedJobs}\n`,
    );
  } catch (err) {
    process.stderr.write(`release-verdict: ${err.message}\n`);
    process.exit(1);
  }
}

module.exports = { verifyReleaseResults, EXPECTED_RELEASE_JOBS };
