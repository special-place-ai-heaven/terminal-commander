#!/usr/bin/env node
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
/**
 * Turn the README's "not yet in a tagged release" notice into a released one.
 *
 * A README line that starts with the marker
 *   <!-- release-status -->Landed 2026-10-05, not yet in a tagged release.
 * becomes
 *   <!-- release-status -->Landed 2026-10-05, released in v0.3.11.
 *
 * Run by release-pr-sync.yml (and the ensure-release self-heal) next to the
 * version syncs. validate-release-inputs.js --require-readme-stamped fails the
 * prepublish gate if any marker line still says "not yet in a tagged release".
 *
 * Several marker lines are supported; each is stamped independently. Only the
 * ", not yet in a tagged release." fragment is replaced, so the landing date,
 * the rest of the file, and the file's line endings are preserved byte for
 * byte. No marker, or an already-stamped marker, is a no-op.
 */
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const MARKER = "<!-- release-status -->";
const UNRELEASED = ", not yet in a tagged release.";
const SEMVER = /^\d+\.\d+\.\d+$/;

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// Marker, then anything but a line break, then the unreleased fragment. The
// lazy middle keeps the date; [^\r\n] keeps a match inside one line.
const UNRELEASED_LINE = new RegExp(
  `(${escapeRegExp(MARKER)}[^\\r\\n]*?)${escapeRegExp(UNRELEASED)}`,
  "g",
);

function stampReleaseStatus(text, version) {
  if (!SEMVER.test(version || "")) {
    throw new Error(`release version is invalid: '${version || ""}'`);
  }
  return text.replace(UNRELEASED_LINE, (_, head) => `${head}, released in v${version}.`);
}

function findUnreleasedMarkers(text) {
  return text.match(UNRELEASED_LINE) || [];
}

function stampReadmeFile(file, version) {
  // latin1 round-trips every byte, so nothing outside the marker line can change.
  const before = fs.readFileSync(file, "latin1");
  const after = stampReleaseStatus(before, version);
  if (after === before) return false;
  fs.writeFileSync(file, after, "latin1");
  return true;
}

if (require.main === module) {
  const version = process.argv[2];
  const file = path.resolve(__dirname, "../..", process.argv[3] || "README.md");
  try {
    const changed = stampReadmeFile(file, version);
    process.stdout.write(
      changed
        ? `readme release-status: stamped as released in v${version}\n`
        : "readme release-status: nothing to stamp\n",
    );
  } catch (err) {
    process.stderr.write(`readme release-status: ${err.message}\n`);
    process.exit(1);
  }
}

module.exports = {
  MARKER,
  UNRELEASED,
  stampReleaseStatus,
  findUnreleasedMarkers,
  stampReadmeFile,
};
