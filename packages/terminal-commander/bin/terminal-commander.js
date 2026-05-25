#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Shim for the admin CLI `terminal-commander`. Version and update are
// handled here so users can inspect or refresh the npm-managed install
// even if the native binary is missing.
//
// Normal commands spawn the resolved platform binary with shell:false
// and stdio:inherit, mirroring the native exit code or signal.

"use strict";

const { spawn } = require("child_process");
const https = require("https");
const pkg = require("../package.json");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const args = process.argv.slice(2);

function isVersionRequest(argv) {
  return argv.length === 1 && (argv[0] === "--version" || argv[0] === "-V");
}

function isUpdateRequest(argv) {
  return argv.length === 1 && argv[0] === "update";
}

function npmProgram() {
  return process.platform === "win32" ? "npm.cmd" : "npm";
}

function numericVersionParts(version) {
  const core = String(version || "").split(/[+-]/, 1)[0];
  if (!core) return null;
  const parts = core.split(".").map((part) => {
    if (!/^\d+$/.test(part)) return null;
    return Number.parseInt(part, 10);
  });
  return parts.some((part) => part == null) ? null : parts;
}

function isNewerVersion(latest, current) {
  const l = numericVersionParts(latest);
  const c = numericVersionParts(current);
  if (!l || !c) return false;
  const len = Math.max(l.length, c.length);
  for (let i = 0; i < len; i += 1) {
    const lp = l[i] || 0;
    const cp = c[i] || 0;
    if (lp > cp) return true;
    if (lp < cp) return false;
  }
  return false;
}

function parseLatestVersion(body) {
  try {
    const parsed = JSON.parse(body);
    const version = parsed && typeof parsed.version === "string" ? parsed.version.trim() : "";
    return /^[0-9A-Za-z.+-]+$/.test(version) ? version : null;
  } catch (_err) {
    return null;
  }
}

function latestNpmVersion(timeoutMs) {
  return new Promise((resolve) => {
    let settled = false;
    const done = (value) => {
      if (settled) return;
      settled = true;
      resolve(value || null);
    };

    const req = https.get(
      "https://registry.npmjs.org/terminal-commander/latest",
      {
        timeout: timeoutMs,
        headers: {
          Accept: "application/json",
          "User-Agent": `terminal-commander/${pkg.version}`,
        },
      },
      (res) => {
        if (!res.statusCode || res.statusCode < 200 || res.statusCode >= 300) {
          res.resume();
          done(null);
          return;
        }

        let body = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          body += chunk;
          if (body.length > 8192) {
            req.destroy();
            done(null);
          }
        });
        res.on("end", () => done(parseLatestVersion(body)));
      },
    );

    req.on("timeout", () => {
      req.destroy();
      done(null);
    });
    req.on("error", () => done(null));
  });
}

async function runVersion() {
  process.stdout.write(`terminal-commander ${pkg.version}\n`);
  const latest = await latestNpmVersion(1500);
  if (latest && isNewerVersion(latest, pkg.version)) {
    process.stdout.write(`Update available: ${latest} (run \`terminal-commander update\`)\n`);
  }
}

function runUpdate() {
  const child = spawn(npmProgram(), ["install", "-g", "terminal-commander@latest"], {
    stdio: "inherit",
    shell: false,
    env: process.env,
  });

  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code == null ? 1 : code);
  });

  child.on("error", (err) => {
    process.stderr.write(
      `terminal-commander: failed to start npm update: ${err.code || err.message}\n`,
    );
    process.exit(126);
  });
}

if (isVersionRequest(args)) {
  runVersion()
    .then(() => process.exit(0))
    .catch(() => {
      process.stdout.write(`terminal-commander ${pkg.version}\n`);
      process.exit(0);
    });
} else if (isUpdateRequest(args)) {
  runUpdate();
} else {
  const result = resolveBinary({ binary: "terminal-commander" });

  if (result.reason === "bridge_required") {
    const { run } = require("../lib/cli/run.js");
    (async () => {
      const r = await run();
      if (r.output) {
        process.stderr.write(r.output);
        if (!r.output.endsWith("\n")) process.stderr.write("\n");
      }
      process.exit(typeof r.exit_code === "number" ? r.exit_code : 64);
    })().catch((err) => {
      process.stderr.write(
        `terminal-commander: CLI internal error: ${err && err.code ? err.code : "unknown"}\n`,
      );
      process.exit(64);
    });
  } else if (result.reason !== "ok") {
    process.stderr.write(formatResolveError(result) + "\n");
    process.exit(64);
  } else {
    const child = spawn(result.binaryPath, args, {
      stdio: "inherit",
      shell: false,
    });

    child.on("exit", (code, signal) => {
      if (signal) {
        process.kill(process.pid, signal);
      }
      process.exit(code == null ? 1 : code);
    });

    child.on("error", (err) => {
      process.stderr.write(
        `terminal-commander: failed to spawn ${result.binaryPath}: ${err.code || err.message}\n`,
      );
      process.exit(126);
    });
  }
}
