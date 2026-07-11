// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const {
  resolveReleaseContext,
} = require("../../../scripts/release/resolve-release-context.js");
const {
  validateReleaseInputs,
} = require("../../../scripts/release/validate-release-inputs.js");
const {
  verifyReleaseResults,
  EXPECTED_RELEASE_JOBS,
} = require("../../../scripts/release/verify-release-results.js");

const repoRoot = path.resolve(__dirname, "../../..");

function parseWorkflowJobs(workflow) {
  const starts = [...workflow.matchAll(/^  ([a-z0-9-]+):\s*$/gm)];
  const jobs = new Map();
  for (let index = 0; index < starts.length; index += 1) {
    const name = starts[index][1];
    const start = starts[index].index;
    const end = index + 1 < starts.length ? starts[index + 1].index : workflow.length;
    const block = workflow.slice(start, end);
    const inline = block.match(/^    needs:[ \t]*\[([^\]]+)\]/m);
    const scalar = block.match(/^    needs:[ \t]*([a-z0-9-]+)[ \t]*$/m);
    const expanded = block.match(
      /^    needs:[ \t]*\r?\n((?:      - [a-z0-9-]+[ \t]*\r?\n?)+)/m,
    );
    const needs = inline
      ? inline[1].split(",").map((value) => value.trim())
      : scalar
        ? [scalar[1]]
        : expanded
          ? [...expanded[1].matchAll(/- ([a-z0-9-]+)/g)].map((match) => match[1])
          : [];
    const references = [...block.matchAll(/needs\.([a-z0-9-]+)/g)].map((match) => match[1]);
    jobs.set(name, { needs: new Set(needs), references: new Set(references) });
  }
  return jobs;
}

test("release context selects each supported publish source deterministically", () => {
  assert.deepEqual(
    resolveReleaseContext({
      releasePleaseResult: "success",
      ensureResult: "skipped",
      publishVersionResult: "skipped",
    }),
    { publish: false, version: "", source: "none" },
  );
  assert.deepEqual(
    resolveReleaseContext({
      releasePleaseResult: "success",
      releaseCreated: "true",
      releaseVersion: "1.2.3",
      ensureResult: "skipped",
      publishVersionResult: "skipped",
    }),
    { publish: true, version: "1.2.3", source: "release-please" },
  );
  assert.deepEqual(
    resolveReleaseContext({
      releasePleaseResult: "success",
      ensureResult: "success",
      ensurePublish: "true",
      ensureVersion: "1.2.3",
      publishVersionResult: "skipped",
    }),
    { publish: true, version: "1.2.3", source: "ensure-release" },
  );
  assert.deepEqual(
    resolveReleaseContext({
      releasePleaseResult: "success",
      ensureResult: "skipped",
      publishVersionResult: "success",
      forcePublish: "true",
      forceVersion: "1.2.3",
    }),
    { publish: true, version: "1.2.3", source: "force-publish" },
  );
});

test("wrapper test entrypoint is cross-platform and does not rely on shell globs", () => {
  const command = require("../package.json").scripts.test;
  assert.equal(command, "node scripts/run-tests.js");
  assert.doesNotMatch(command, /[*?\[\]]/);
});

test("release context refuses blank, conflicting, or failed publish sources", () => {
  assert.throws(
    () => resolveReleaseContext({ releaseCreated: "true", releaseVersion: "" }),
    /invalid version/,
  );
  assert.throws(
    () =>
      resolveReleaseContext({
        releaseCreated: "true",
        releaseVersion: "1.2.3",
        ensurePublish: "true",
        ensureVersion: "1.2.4",
      }),
    /version disagreement/,
  );
  assert.throws(
    () => resolveReleaseContext({ releasePleaseResult: "failure" }),
    /upstream failed/,
  );
});

test("release verdict distinguishes no-release, complete, and silently skipped runs", () => {
  assert.deepEqual(
    verifyReleaseResults({
      "release-context": { result: "success", outputs: { publish: "false" } },
    }),
    { status: "no_release", checkedJobs: 0 },
  );

  const complete = {
    "release-context": { result: "success", outputs: { publish: "true" } },
  };
  for (const name of EXPECTED_RELEASE_JOBS) complete[name] = { result: "success" };
  assert.deepEqual(verifyReleaseResults(complete), {
    status: "complete",
    checkedJobs: EXPECTED_RELEASE_JOBS.length,
  });

  complete["prepublish-gate"] = { result: "skipped" };
  assert.throws(
    () => verifyReleaseResults(complete),
    /prepublish-gate=skipped/,
  );
});

test("all committed release version anchors agree", () => {
  const version = require("../package.json").version;
  const result = validateReleaseInputs(repoRoot, version);
  assert.ok(result.checkedVersions >= 15);
  assert.throws(
    () => validateReleaseInputs(repoRoot, "9.9.9"),
    /release version mismatch/,
  );
});

test("release failure reporting is repository-explicit and uses canonical context", () => {
  const workflow = fs.readFileSync(
    path.join(repoRoot, ".github", "workflows", "release-please.yml"),
    "utf8",
  );
  assert.match(workflow, /^  release-context:/m);
  assert.match(workflow, /^  release-verdict:/m);
  assert.match(
    workflow,
    /prepublish-gate:[\s\S]*?if: >-\r?\n\s+always\(\) && !cancelled\(\)/,
  );
  assert.match(workflow, /VER: \$\{\{ needs\.release-context\.outputs\.version \}\}/);
  assert.match(workflow, /gh issue create --repo "\$GITHUB_REPOSITORY"/);
  assert.doesNotMatch(workflow, /gh issue list[^\n]+--label "release-broken"/);
  assert.match(workflow, /filing issue without it/);
});

test("all Cargo release jobs use the checksum-verifying retry state machine", () => {
  const workflow = fs.readFileSync(
    path.join(repoRoot, ".github", "workflows", "release-please.yml"),
    "utf8",
  );
  const invocations = [
    ...workflow.matchAll(/node scripts\/release\/publish-cargo-crate\.js --crate ([a-z0-9_-]+)/g),
  ];
  assert.deepEqual(
    invocations.map((match) => match[1]),
    [
      "terminal-commander-core",
      "terminal-commander-sifters",
      "terminal-commander-probes",
      "terminal-commander-store",
      "terminal-commander-supervisor",
      "terminal-commander-ipc",
      "terminal-commanderd",
      "terminal-commander-mcp",
    ],
  );
  assert.doesNotMatch(workflow, /if cargo publish -p/);
  assert.equal((workflow.match(/Setup Node for release state machine/g) || []).length, 8);
  assert.equal((workflow.match(/cargo package and verify release archive/g) || []).length, 8);
  assert.doesNotMatch(workflow, /^\s+run: cargo publish --dry-run/m);
  assert.doesNotMatch(workflow, /Poll crates\.io until/);
});

test("release workflow needs graph is closed and registry publishes are prevalidated", () => {
  const workflow = fs.readFileSync(
    path.join(repoRoot, ".github", "workflows", "release-please.yml"),
    "utf8",
  );
  const jobs = parseWorkflowJobs(workflow);

  for (const [name, job] of jobs) {
    assert.ok(!job.needs.has(name), `${name} must not depend on itself`);
    for (const dependency of job.needs) {
      assert.ok(jobs.has(dependency), `${name} needs unknown job ${dependency}`);
    }
    for (const reference of job.references) {
      assert.ok(job.needs.has(reference), `${name} references needs.${reference} without declaring it`);
    }
  }

  const reachesPrepublishGate = (name, seen = new Set()) => {
    if (name === "prepublish-gate") return true;
    if (seen.has(name)) return false;
    seen.add(name);
    const job = jobs.get(name);
    return job && [...job.needs].some((dependency) => reachesPrepublishGate(dependency, seen));
  };
  const registryPublishes = [...jobs.keys()].filter(
    (name) => name === "publish-root" || name.startsWith("publish-linux-") ||
      name.startsWith("publish-windows-") || name.startsWith("publish-mac-") ||
      name.startsWith("publish-cargo-"),
  );
  for (const name of registryPublishes) {
    assert.ok(reachesPrepublishGate(name), `${name} can publish without prepublish-gate`);
  }
});
