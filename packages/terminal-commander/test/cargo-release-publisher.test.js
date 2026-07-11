// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const zlib = require("node:zlib");
const {
  canonicalCrateContentDigest,
  parseArgs,
  publishCrate,
  verifyPublishedChecksum,
} = require("../../../scripts/release/publish-cargo-crate.js");

const CHECKSUM = "a".repeat(64);

function harness(lookups, publishes = [], verifyPresent = async () => ({ verified: true })) {
  const calls = { lookup: 0, publish: 0, sleeps: [] };
  return {
    calls,
    deps: {
      lookup: async () => lookups[Math.min(calls.lookup++, lookups.length - 1)],
      verifyPresent,
      publish: async () => publishes[Math.min(calls.publish++, publishes.length - 1)],
      sleep: async (milliseconds) => calls.sleeps.push(milliseconds),
      log: () => {},
      warn: () => {},
    },
  };
}

const config = (overrides = {}) => ({
  crate: "terminal-commander-ipc",
  version: "0.1.76",
  localContentDigest: CHECKSUM,
  attempts: 3,
  failurePolls: 2,
  visibilityPolls: 3,
  pollDelayMs: 1,
  retryBaseMs: 2,
  ...overrides,
});

test("cargo publisher treats an exact existing archive as the achieved postcondition", async () => {
  const h = harness([{ kind: "present", checksum: CHECKSUM }]);
  assert.deepEqual(await publishCrate(config(), h.deps), {
    status: "already-present",
    publishAttempts: 0,
  });
  assert.equal(h.calls.publish, 0);
});

test("cargo publisher reconciles an ambiguous upload failure before retrying", async () => {
  const h = harness(
    [
      { kind: "missing", status: 404 },
      { kind: "transient", status: 500 },
      { kind: "present", checksum: CHECKSUM },
    ],
    [{ code: 1 }],
  );
  assert.deepEqual(await publishCrate(config(), h.deps), {
    status: "published-after-ambiguous-failure",
    publishAttempts: 1,
  });
  assert.equal(h.calls.publish, 1);
});

test("cargo publisher retries a confirmed-missing upload with bounded backoff", async () => {
  const h = harness(
    [
      { kind: "missing", status: 404 },
      { kind: "missing", status: 404 },
      { kind: "missing", status: 404 },
      { kind: "present", checksum: CHECKSUM },
    ],
    [{ code: 1 }, { code: 0 }],
  );
  assert.deepEqual(await publishCrate(config(), h.deps), {
    status: "published",
    publishAttempts: 2,
  });
  assert.equal(h.calls.publish, 2);
  assert.ok(h.calls.sleeps.includes(2));
});

test("cargo publisher fails closed when the registry version has different contents", async () => {
  const h = harness(
    [{ kind: "present", checksum: "b".repeat(64) }],
    [],
    async () => {
      throw new Error("packaged contents differ from this release");
    },
  );
  await assert.rejects(() => publishCrate(config(), h.deps), /packaged contents differ/);
  assert.equal(h.calls.publish, 0);
});

test("cargo publisher stops after its configured attempt budget", async () => {
  const h = harness(
    Array.from({ length: 5 }, () => ({ kind: "missing", status: 404 })),
    [{ code: 1 }, { code: 1 }],
  );
  await assert.rejects(
    () => publishCrate(config({ attempts: 2 }), h.deps),
    /after 2 bounded attempts/,
  );
  assert.equal(h.calls.publish, 2);
});

test("cargo publisher validates CLI identities and checksum shape", () => {
  assert.deepEqual(
    parseArgs(["--crate", "terminal-commander-ipc", "--version", "0.1.76"]),
    { crate: "terminal-commander-ipc", version: "0.1.76" },
  );
  assert.throws(() => parseArgs(["--crate", "../ipc", "--version", "0.1.76"]), /invalid crate/);
  assert.throws(
    () => verifyPublishedChecksum("crate", "1.0.0", Buffer.from("archive"), "not-a-checksum"),
    /invalid registry checksum/,
  );
});

function tarArchive(files, options = {}) {
  const blocks = [];
  for (const [name, value] of Object.entries(files)) {
    const content = Buffer.from(value);
    const header = Buffer.alloc(512);
    header.write(`crate-1.0.0/${name}`, 0, 100, "utf8");
    header.write("0000644\0", 100, 8, "ascii");
    header.write("0000000\0", 108, 8, "ascii");
    header.write("0000000\0", 116, 8, "ascii");
    header.write(`${content.length.toString(8).padStart(11, "0")}\0`, 124, 12, "ascii");
    header.write(`${String(options.mtime || 0).padStart(11, "0")}\0`, 136, 12, "ascii");
    header.fill(0x20, 148, 156);
    header[156] = options.nulType ? 0 : "0".charCodeAt(0);
    header.write("ustar\0", 257, 6, "ascii");
    const checksum = [...header].reduce((sum, byte) => sum + byte, 0);
    header.write(`${checksum.toString(8).padStart(6, "0")}\0 `, 148, 8, "ascii");
    blocks.push(header, content, Buffer.alloc((512 - (content.length % 512)) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  return zlib.gzipSync(Buffer.concat(blocks), { mtime: options.gzipMtime || 0 });
}

test("canonical crate digest ignores archive metadata and generated VCS identity only", () => {
  const left = tarArchive(
    { "src/lib.rs": "pub fn value() -> u8 { 1 }\n", ".cargo_vcs_info.json": "commit-a" },
    { mtime: 1, gzipMtime: 1 },
  );
  const equivalent = tarArchive(
    { ".cargo_vcs_info.json": "commit-b", "src/lib.rs": "pub fn value() -> u8 { 1 }\n" },
    { mtime: 2, gzipMtime: 2, nulType: true },
  );
  const changed = tarArchive(
    { "src/lib.rs": "pub fn value() -> u8 { 2 }\n", ".cargo_vcs_info.json": "commit-a" },
  );
  assert.equal(canonicalCrateContentDigest(left), canonicalCrateContentDigest(equivalent));
  assert.notEqual(canonicalCrateContentDigest(left), canonicalCrateContentDigest(changed));
});
