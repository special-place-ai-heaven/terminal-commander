#!/usr/bin/env node
"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const zlib = require("node:zlib");
const { spawn } = require("node:child_process");

const DEFAULT_API_BASE = "https://crates.io/api/v1/crates";
const USER_AGENT =
  "terminal-commander-release (https://github.com/special-place-ai-heaven/terminal-commander)";

function positiveInteger(value, fallback, name) {
  if (value === undefined || value === "") return fallback;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive integer`);
  }
  return parsed;
}

function validateIdentity(crate, version) {
  if (!/^[a-z0-9][a-z0-9_-]*$/.test(crate || "")) {
    throw new Error(`invalid crate name: ${JSON.stringify(crate)}`);
  }
  if (!/^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/.test(version || "")) {
    throw new Error(`invalid crate version: ${JSON.stringify(version)}`);
  }
}

function verifyPublishedChecksum(crate, version, archive, publishedChecksum) {
  if (!/^[0-9a-f]{64}$/i.test(publishedChecksum || "")) {
    throw new Error(`${crate}@${version} returned an invalid registry checksum`);
  }
  const actual = crypto.createHash("sha256").update(archive).digest("hex");
  if (actual !== publishedChecksum.toLowerCase()) {
    throw new Error(
      `${crate}@${version} registry archive checksum mismatch: expected ` +
        `${publishedChecksum}, downloaded ${actual}`,
    );
  }
}

function tarString(buffer, offset, length) {
  const end = buffer.indexOf(0, offset);
  return buffer.subarray(offset, end >= offset && end < offset + length ? end : offset + length)
    .toString("utf8")
    .trim();
}

function tarNumber(buffer, offset, length) {
  const raw = tarString(buffer, offset, length).replace(/^0+/, "");
  if (raw === "") return 0;
  if (!/^[0-7]+$/.test(raw)) throw new Error(`unsupported tar number: ${JSON.stringify(raw)}`);
  return Number.parseInt(raw, 8);
}

function parsePax(content) {
  const values = {};
  let offset = 0;
  while (offset < content.length) {
    const space = content.indexOf(0x20, offset);
    if (space < 0) throw new Error("invalid PAX header length");
    const length = Number(content.subarray(offset, space).toString("ascii"));
    if (!Number.isSafeInteger(length) || length <= 0 || offset + length > content.length) {
      throw new Error("invalid PAX record length");
    }
    const record = content.subarray(space + 1, offset + length - 1).toString("utf8");
    const equals = record.indexOf("=");
    if (equals > 0) values[record.slice(0, equals)] = record.slice(equals + 1);
    offset += length;
  }
  return values;
}

function canonicalCrateContentDigest(archive) {
  const tar = zlib.gunzipSync(archive, { maxOutputLength: 64 * 1024 * 1024 });
  const entries = new Map();
  let offset = 0;
  let nextPath = "";
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const name = tarString(header, 0, 100);
    const prefix = tarString(header, 345, 155);
    const size = tarNumber(header, 124, 12);
    const type = String.fromCharCode(header[156] || 0x30);
    const contentStart = offset + 512;
    const contentEnd = contentStart + size;
    if (contentEnd > tar.length) throw new Error("truncated tar entry");
    const content = tar.subarray(contentStart, contentEnd);

    if (type === "x" || type === "g") {
      const pax = parsePax(content);
      if (type === "x" && pax.path) nextPath = pax.path;
    } else if (type === "L") {
      nextPath = tarString(content, 0, content.length);
    } else if (type === "0" || type === "\0" || type === "2" || type === "1") {
      const archivePath = nextPath || (prefix ? `${prefix}/${name}` : name);
      nextPath = "";
      if (archivePath.startsWith("/") || archivePath.split("/").includes("..")) {
        throw new Error(`unsafe crate archive path: ${archivePath}`);
      }
      const slash = archivePath.indexOf("/");
      if (slash < 1) throw new Error(`crate archive entry lacks package root: ${archivePath}`);
      const relativePath = archivePath.slice(slash + 1);
      if (relativePath !== ".cargo_vcs_info.json") {
        if (entries.has(relativePath)) throw new Error(`duplicate crate archive path: ${relativePath}`);
        const value = type === "2" || type === "1"
          ? Buffer.from(tarString(header, 157, 100), "utf8")
          : content;
        const canonicalType = type === "2" ? "symlink" : type === "1" ? "hardlink" : "file";
        entries.set(relativePath, { type: canonicalType, value });
      }
    }
    offset = contentStart + Math.ceil(size / 512) * 512;
  }
  if (entries.size === 0) throw new Error("crate archive contains no package files");

  const digest = crypto.createHash("sha256");
  for (const [entryPath, entry] of [...entries].sort(([left], [right]) => left.localeCompare(right))) {
    digest.update(entry.type);
    digest.update("\0");
    digest.update(entryPath);
    digest.update("\0");
    digest.update(String(entry.value.length));
    digest.update("\0");
    digest.update(entry.value);
  }
  return digest.digest("hex");
}

async function verifyRegistryArchive(crate, version, registryChecksum, localContentDigest, options = {}) {
  const fetchImpl = options.fetchImpl || globalThis.fetch;
  const timeoutMs = positiveInteger(options.timeoutMs, 20_000, "archive timeout");
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  let response;
  let archive;
  try {
    response = await fetchImpl(
      `https://static.crates.io/crates/${encodeURIComponent(crate)}/${encodeURIComponent(crate)}-${encodeURIComponent(version)}.crate`,
      { headers: { "User-Agent": USER_AGENT }, signal: controller.signal },
    );
    if (response.status !== 200) return { verified: false, status: response.status };
    const declaredLength = Number(response.headers.get("content-length") || 0);
    if (declaredLength > 10 * 1024 * 1024) throw new Error("registry crate archive is unexpectedly large");
    archive = Buffer.from(await response.arrayBuffer());
  } catch (error) {
    if (error.message === "registry crate archive is unexpectedly large") throw error;
    return { verified: false, status: 0, error: error.message };
  } finally {
    clearTimeout(timeout);
  }
  if (archive.length > 10 * 1024 * 1024) throw new Error("registry crate archive is unexpectedly large");
  verifyPublishedChecksum(crate, version, archive, registryChecksum);
  const remoteContentDigest = canonicalCrateContentDigest(archive);
  if (remoteContentDigest !== localContentDigest) {
    throw new Error(
      `${crate}@${version} exists on crates.io but its packaged contents differ from this release`,
    );
  }
  return { verified: true };
}

async function lookupVersion(crate, version, options = {}) {
  const fetchImpl = options.fetchImpl || globalThis.fetch;
  if (typeof fetchImpl !== "function") throw new Error("fetch is unavailable");

  const apiBase = (options.apiBase || DEFAULT_API_BASE).replace(/\/$/, "");
  const timeoutMs = positiveInteger(options.timeoutMs, 15_000, "lookup timeout");
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetchImpl(
      `${apiBase}/${encodeURIComponent(crate)}/${encodeURIComponent(version)}`,
      { headers: { "User-Agent": USER_AGENT }, signal: controller.signal },
    );
    if (response.status === 200) {
      const payload = await response.json();
      return { kind: "present", checksum: payload?.version?.checksum || "" };
    }
    if (response.status === 404) return { kind: "missing", status: 404 };
    if (response.status === 408 || response.status === 425 || response.status === 429 || response.status >= 500) {
      return { kind: "transient", status: response.status };
    }
    return { kind: "error", status: response.status };
  } catch (error) {
    return { kind: "transient", status: 0, error: error.message };
  } finally {
    clearTimeout(timeout);
  }
}

function runCargoPublish(crate, options = {}) {
  const cargo = options.cargo || "cargo";
  return new Promise((resolve, reject) => {
    const child = spawn(cargo, ["publish", "-p", crate], {
      cwd: options.cwd,
      env: options.env || process.env,
      stdio: "inherit",
      shell: false,
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
}

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function reconcile(crate, version, localContentDigest, polls, delayMs, deps) {
  let last = { kind: "missing", status: 404 };
  for (let poll = 1; poll <= polls; poll += 1) {
    last = await deps.lookup(crate, version);
    if (last.kind === "present") {
      const archive = await deps.verifyPresent(crate, version, last.checksum, localContentDigest);
      if (archive.verified) return { present: true, poll };
      last = { kind: "transient", status: archive.status, error: archive.error };
    }
    deps.log(
      `registry reconciliation ${poll}/${polls}: ${last.kind}` +
        (last.status === undefined ? "" : ` (HTTP ${last.status || "network"})`),
    );
    if (poll < polls) await deps.sleep(delayMs);
  }
  return { present: false, last };
}

async function publishCrate(config, dependencies = {}) {
  const { crate, version, localContentDigest } = config;
  validateIdentity(crate, version);
  if (!/^[0-9a-f]{64}$/i.test(localContentDigest || "")) {
    throw new Error("local release content digest is invalid");
  }

  const attempts = positiveInteger(config.attempts, 3, "publish attempts");
  const failurePolls = positiveInteger(config.failurePolls, 6, "failure polls");
  const visibilityPolls = positiveInteger(config.visibilityPolls, 30, "visibility polls");
  const pollDelayMs = positiveInteger(config.pollDelayMs, 10_000, "poll delay");
  const retryBaseMs = positiveInteger(config.retryBaseMs, 15_000, "retry delay");
  const deps = {
    lookup: dependencies.lookup || lookupVersion,
    verifyPresent: dependencies.verifyPresent || verifyRegistryArchive,
    publish: dependencies.publish || runCargoPublish,
    sleep: dependencies.sleep || sleep,
    log: dependencies.log || console.log,
    warn: dependencies.warn || console.warn,
  };

  const existing = await reconcile(crate, version, localContentDigest, 1, pollDelayMs, deps);
  if (existing.present) {
    deps.log(`${crate}@${version} already exists with the expected checksum`);
    return { status: "already-present", publishAttempts: 0 };
  }

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    deps.log(`cargo publish attempt ${attempt}/${attempts}: ${crate}@${version}`);
    const result = await deps.publish(crate);
    const publishSucceeded = result && result.code === 0;
    if (!publishSucceeded) {
      deps.warn(
        `cargo publish attempt ${attempt} failed` +
          (result?.signal ? ` with signal ${result.signal}` : ` with exit ${result?.code ?? "unknown"}`),
      );
    }

    const reconciled = await reconcile(
      crate,
      version,
      localContentDigest,
      publishSucceeded ? visibilityPolls : failurePolls,
      pollDelayMs,
      deps,
    );
    if (reconciled.present) {
      return {
        status: publishSucceeded ? "published" : "published-after-ambiguous-failure",
        publishAttempts: attempt,
      };
    }

    if (publishSucceeded) {
      throw new Error(
        `${crate}@${version} upload returned success but the exact checksum did not become queryable`,
      );
    }
    if (attempt < attempts) await deps.sleep(retryBaseMs * 2 ** (attempt - 1));
  }

  throw new Error(`${crate}@${version} was not published after ${attempts} bounded attempts`);
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if ((key !== "--crate" && key !== "--version") || value === undefined) {
      throw new Error("usage: publish-cargo-crate.js --crate <name> --version <semver>");
    }
    args[key.slice(2)] = value;
  }
  validateIdentity(args.crate, args.version);
  return args;
}

async function main() {
  const { crate, version } = parseArgs(process.argv.slice(2));
  const targetDirectory = path.resolve(process.env.CARGO_TARGET_DIR || "target");
  const archive = path.join(targetDirectory, "package", `${crate}-${version}.crate`);
  if (!fs.existsSync(archive)) {
    throw new Error(`release archive not found: ${archive}; run cargo package first`);
  }
  const localContentDigest = canonicalCrateContentDigest(fs.readFileSync(archive));
  const config = {
    crate,
    version,
    localContentDigest,
    attempts: process.env.TC_CARGO_PUBLISH_ATTEMPTS,
    failurePolls: process.env.TC_CARGO_FAILURE_POLLS,
    visibilityPolls: process.env.TC_CARGO_VISIBILITY_POLLS,
    pollDelayMs: process.env.TC_CARGO_POLL_DELAY_MS,
    retryBaseMs: process.env.TC_CARGO_RETRY_BASE_MS,
  };
  const result = await publishCrate(config, {
    lookup: (name, release) =>
      lookupVersion(name, release, { apiBase: process.env.TC_CRATES_IO_API_BASE }),
    verifyPresent: (name, release, registryChecksum, digest) =>
      verifyRegistryArchive(name, release, registryChecksum, digest),
    publish: (name) => runCargoPublish(name, { cwd: process.cwd() }),
  });
  process.stdout.write(
    `${crate}@${version}: ${result.status}; publish_attempts=${result.publishAttempts}\n`,
  );
}

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`cargo-release: ${error.message}\n`);
    process.exitCode = 1;
  });
}

module.exports = {
  canonicalCrateContentDigest,
  lookupVersion,
  parseArgs,
  publishCrate,
  validateIdentity,
  verifyRegistryArchive,
  verifyPublishedChecksum,
};
