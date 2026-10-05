// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  stampReleaseStatus,
  findUnreleasedMarkers,
  stampReadmeFile,
} = require("../../../scripts/release/stamp-readme-release-status.js");
const {
  validateReleaseInputs,
} = require("../../../scripts/release/validate-release-inputs.js");

const repoRoot = path.resolve(__dirname, "../../..");
const UNRELEASED = "<!-- release-status -->Landed 2026-10-05, not yet in a tagged release.";
const RELEASED = "<!-- release-status -->Landed 2026-10-05, released in v0.3.11.";

function readme(eol, line) {
  return ["# T", "", "## Recent improvements", "", line, "", "- item", ""].join(eol);
}

test("stamps an unreleased marker line and keeps the landing date", () => {
  assert.equal(stampReleaseStatus(readme("\n", UNRELEASED), "0.3.11"), readme("\n", RELEASED));
});

test("stamping is idempotent", () => {
  const once = stampReleaseStatus(readme("\n", UNRELEASED), "0.3.11");
  assert.equal(stampReleaseStatus(once, "0.3.12"), once);
});

test("no marker and already-released marker are no-ops", () => {
  const plain = "# T\n\nLanded 2026-10-05, not yet in a tagged release.\n";
  assert.equal(stampReleaseStatus(plain, "0.3.11"), plain);
  assert.equal(stampReleaseStatus(readme("\n", RELEASED), "0.3.12"), readme("\n", RELEASED));
});

test("LF and CRLF round-trip byte-exactly outside the marker line", () => {
  for (const eol of ["\n", "\r\n"]) {
    const out = stampReleaseStatus(readme(eol, UNRELEASED), "0.3.11");
    assert.equal(out, readme(eol, RELEASED), JSON.stringify(eol));
  }
  const noTrailing = readme("\n", UNRELEASED).trimEnd();
  assert.equal(stampReleaseStatus(noTrailing, "0.3.11"), readme("\n", RELEASED).trimEnd());
});

test("several marker lines are each stamped independently", () => {
  const older = "<!-- release-status -->Landed 2026-09-01, released in v0.3.9.";
  const newer = "<!-- release-status -->Landed 2026-11-02, not yet in a tagged release.";
  const text = [older, UNRELEASED, newer].join("\n");
  assert.equal(
    stampReleaseStatus(text, "0.3.11"),
    [older, RELEASED, "<!-- release-status -->Landed 2026-11-02, released in v0.3.11."].join("\n"),
  );
});

test("rejects a non-semver version", () => {
  assert.throws(() => stampReleaseStatus(UNRELEASED, "v0.3.11"), /invalid/);
});

test("stampReadmeFile rewrites in place only when needed", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tc-readme-"));
  const file = path.join(dir, "README.md");
  try {
    fs.writeFileSync(file, readme("\r\n", UNRELEASED));
    assert.equal(stampReadmeFile(file, "0.3.11"), true);
    assert.equal(fs.readFileSync(file, "utf8"), readme("\r\n", RELEASED));
    assert.equal(stampReadmeFile(file, "0.3.11"), false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("guard detects an unstamped marker but not a stamped one", () => {
  assert.equal(findUnreleasedMarkers(readme("\n", UNRELEASED)).length, 1);
  assert.equal(findUnreleasedMarkers(readme("\n", RELEASED)).length, 0);
});

test("prepublish guard: README must be stamped only when requireReadmeStamped is set", () => {
  const version = require("../package.json").version;
  const readmeText = fs.readFileSync(path.join(repoRoot, "README.md"), "utf8");
  // Ordinary branches may legitimately say "not yet in a tagged release".
  assert.ok(validateReleaseInputs(repoRoot, version));
  if (findUnreleasedMarkers(readmeText).length > 0) {
    assert.throws(
      () => validateReleaseInputs(repoRoot, version, { requireReadmeStamped: true }),
      /not yet in a tagged release/,
    );
  } else {
    assert.ok(validateReleaseInputs(repoRoot, version, { requireReadmeStamped: true }));
  }
});
