#!/usr/bin/env node
"use strict";

const fs = require("node:fs");

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function requested(value) {
  return String(value || "").toLowerCase() === "true";
}

function resolveReleaseContext(input) {
  const sources = [
    {
      name: "release-please",
      result: input.releasePleaseResult,
      publish: requested(input.releaseCreated),
      version: input.releaseVersion || "",
    },
    {
      name: "ensure-release",
      result: input.ensureResult,
      publish: requested(input.ensurePublish),
      version: input.ensureVersion || "",
    },
    {
      name: "force-publish",
      result: input.publishVersionResult,
      publish: requested(input.forcePublish),
      version: input.forceVersion || "",
    },
  ];

  const failed = sources.filter((source) => source.result === "failure");
  if (failed.length > 0) {
    throw new Error(`release context upstream failed: ${failed.map((s) => s.name).join(", ")}`);
  }

  const active = sources.filter((source) => source.publish);
  if (active.length === 0) return { publish: false, version: "", source: "none" };

  for (const source of active) {
    if (!SEMVER.test(source.version)) {
      throw new Error(`${source.name} requested publish with invalid version '${source.version}'`);
    }
  }
  const versions = new Set(active.map((source) => source.version));
  if (versions.size !== 1) {
    throw new Error(
      `release context version disagreement: ${active
        .map((source) => `${source.name}=${source.version}`)
        .join(", ")}`,
    );
  }

  return {
    publish: true,
    version: active[0].version,
    source: active.map((source) => source.name).join("+"),
  };
}

function resolveFromEnvironment(env) {
  return resolveReleaseContext({
    releasePleaseResult: env.RELEASE_PLEASE_RESULT,
    releaseCreated: env.RELEASE_CREATED,
    releaseVersion: env.RELEASE_VERSION,
    ensureResult: env.ENSURE_RESULT,
    ensurePublish: env.ENSURE_PUBLISH,
    ensureVersion: env.ENSURE_VERSION,
    publishVersionResult: env.PUBLISH_VERSION_RESULT,
    forcePublish: env.FORCE_PUBLISH,
    forceVersion: env.FORCE_VERSION,
  });
}

if (require.main === module) {
  try {
    const context = resolveFromEnvironment(process.env);
    const lines = [
      `publish=${context.publish}`,
      `version=${context.version}`,
      `source=${context.source}`,
    ];
    if (!process.env.GITHUB_OUTPUT) {
      throw new Error("GITHUB_OUTPUT is not set");
    }
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `${lines.join("\n")}\n`, "utf8");
    process.stdout.write(`release-context: ${JSON.stringify(context)}\n`);
  } catch (err) {
    process.stderr.write(`release-context: ${err.message}\n`);
    process.exit(1);
  }
}

module.exports = { resolveReleaseContext, resolveFromEnvironment, SEMVER };
