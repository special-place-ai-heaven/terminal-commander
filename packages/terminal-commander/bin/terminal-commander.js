#!/usr/bin/env node
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Shim for the admin CLI `terminal-commander`. Version and update are
// handled here so users can inspect or refresh the npm-managed install
// even if the native binary is missing.
//
// Native inspection commands spawn the resolved platform binary with
// shell:false and stdio:inherit. JS-only setup/pair/update commands
// stay in this wrapper so they work even when the native CLI is present.

"use strict";

const { spawn } = require("child_process");
const fs = require("fs");
const https = require("https");
const path = require("path");
const pkg = require("../package.json");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");
const { stableBinPath } = require("../lib/harness/stable_bin.js");
const {
  detectRuntimeEnvironment,
  windowsUpdateScopes,
  describeError,
} = require("../lib/cli/runtime_environment.js");

const args = process.argv.slice(2);

function isVersionRequest(argv) {
  return argv.length === 1 && (argv[0] === "--version" || argv[0] === "-V");
}

function isUpdateRequest(argv) {
  return argv.length === 1 && argv[0] === "update";
}

function isJsCliRequest(argv) {
  const command = argv[0];
  if (command === "setup" || command === "pair" || command === "restart") {
    return true;
  }
  if (command === "doctor") {
    return argv[1] === "wsl" || argv[1] === "harness" || argv[1] === "daemon";
  }
  return false;
}

function npmInvocation() {
  const args = ["install", "-g", "terminal-commander@latest"];
  if (process.platform !== "win32") {
    return { command: "npm", args };
  }

  const candidates = [];
  if (process.env.npm_execpath && path.extname(process.env.npm_execpath).toLowerCase() === ".js") {
    candidates.push(process.env.npm_execpath);
  }
  candidates.push(path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"));

  const npmCli = candidates.find((candidate) => candidate && fs.existsSync(candidate));
  if (npmCli) {
    return { command: process.execPath, args: [npmCli, ...args] };
  }

  return null;
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
  const invocation = npmInvocation();
  if (!invocation) {
    process.stderr.write(
      "terminal-commander: npm CLI entrypoint not found; reinstall Node/npm and retry `terminal-commander update`.\n",
    );
    process.exit(126);
  }

  runUpdatePreflight((preflightCode) => {
    if (preflightCode !== 0) {
      process.stderr.write(
        `terminal-commander: update preflight failed with exit code ${preflightCode}; close Terminal Commander processes and retry.\n`,
      );
      process.exit(preflightCode || 1);
      return;
    }
    const child = spawn(invocation.command, invocation.args, {
      stdio: "inherit",
      shell: false,
      env: process.env,
    });

    child.on("exit", (code, signal) => {
      if (signal) {
        process.kill(process.pid, signal);
        return;
      }
      if (code === 0) {
        // The npm update landed a NEW package version on disk. This process is
        // still the OLD launcher, so re-run harness setup by spawning the
        // freshly-installed launcher in a NEW process (mirrors SymForge
        // update.rs re-spawning the new binary) — never in-process, which would
        // re-register the OLD paths. Best-effort: never turn a successful update
        // into a failure.
        reregisterHarnesses(() => process.exit(0));
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
  });
}

// Locate the freshly-installed launcher's JS entry under the npm GLOBAL root so
// we can re-run setup with the NEW version via `node <new>/bin/terminal-commander.js`.
// Spawning `process.execPath` + the resolved JS entry (no shell, no Windows
// shim) keeps the AV-safe contract. Returns null when the global path is
// unresolvable.
function globalLauncherEntry() {
  const prefix = process.env.npm_config_prefix || process.env.PREFIX;
  const candidates = [];
  if (prefix) {
    // Unix global: <prefix>/lib/node_modules; Windows global: <prefix>/node_modules.
    candidates.push(
      path.join(prefix, "lib", "node_modules", "terminal-commander", "bin", "terminal-commander.js"),
      path.join(prefix, "node_modules", "terminal-commander", "bin", "terminal-commander.js"),
    );
  }
  // Fallback: the global root that hosts node itself (npm's default prefix).
  candidates.push(
    path.join(path.dirname(process.execPath), "node_modules", "terminal-commander", "bin", "terminal-commander.js"),
  );
  return candidates.find((c) => {
    try {
      return fs.existsSync(c);
    } catch (_e) {
      return false;
    }
  }) || null;
}

// Re-run `setup harness` with the freshly-installed launcher. Always invokes
// `done` (success or not) so a setup hiccup never fails the update.
function reregisterHarnesses(done) {
  const entry = globalLauncherEntry();
  if (!entry) {
    process.stderr.write(
      "terminal-commander: update installed; could not locate the new launcher to refresh harness configs. Run 'terminal-commander setup harness'.\n",
    );
    done();
    return;
  }
  const child = spawn(process.execPath, [entry, "setup", "harness"], {
    stdio: "inherit",
    shell: false,
    env: process.env,
  });
  child.on("exit", () => done());
  child.on("error", () => {
    process.stderr.write(
      "terminal-commander: update installed; harness refresh did not start. Run 'terminal-commander setup harness'.\n",
    );
    done();
  });
}

function runUpdatePreflight(done) {
  const environment = detectRuntimeEnvironment({
    platform: process.platform,
    env: process.env,
    flags: {},
  });
  if (environment.status !== "ok") {
    process.stderr.write(
      `terminal-commander: update preflight unsupported environment (${environment.evidence}).\n`,
    );
    done(64);
    return;
  }
  if (process.platform !== "win32") {
    done(0);
    return;
  }

  const result = resolveBinary({ binary: "terminal-commander" });
  let helperPath = result.reason === "ok" ? result.binaryPath : null;
  if (!helperPath) {
    const stableHelper = stableBinPath("terminal-commander", {
      platform: process.platform,
      env: process.env,
    });
    if (fs.existsSync(stableHelper)) {
      helperPath = stableHelper;
      process.stderr.write(
        `${formatResolveError(result)}; using stable update helper ${stableHelper}.\n`,
      );
    } else {
      process.stderr.write(
        `${formatResolveError(result)}; no update helper is available, continuing with npm repair.\n`,
      );
      done(0);
      return;
    }
  }

  let scopes;
  try {
    scopes = windowsUpdateScopes({
      platform: process.platform,
      env: process.env,
      packageRoot: path.dirname(__dirname),
    });
  } catch (err) {
    process.stderr.write(
      `terminal-commander: update preflight environment error: ${describeError(err)}\n`,
    );
    done(64);
    return;
  }

  const runScope = (index) => {
    if (index >= scopes.length) {
      done(0);
      return;
    }
    const scopeDir = scopes[index];
    const child = spawn(
      helperPath,
      ["update-locks", "--scope-dir", scopeDir],
      {
        stdio: "inherit",
        shell: false,
        env: process.env,
      },
    );

    child.on("exit", (code, signal) => {
      if (signal) {
        done(1);
        return;
      }
      const exitCode = code == null ? 1 : code;
      if (exitCode !== 0) {
        done(exitCode);
        return;
      }
      runScope(index + 1);
    });

    child.on("error", (err) => {
      process.stderr.write(
        `terminal-commander: failed to start update preflight for ${scopeDir}: ${describeError(err)}\n`,
      );
      done(126);
    });
  };

  runScope(0);
}

function writeCliResult(result) {
  if (result.output) {
    const stream = result.exit_code === 0 ? process.stdout : process.stderr;
    stream.write(result.output);
    if (!result.output.endsWith("\n")) stream.write("\n");
  }
  process.exit(typeof result.exit_code === "number" ? result.exit_code : 64);
}

function runJsCli() {
  const { run } = require("../lib/cli/run.js");
  run({ argv: args })
    .then(writeCliResult)
    .catch((err) => {
      process.stderr.write(
        `terminal-commander: CLI internal error: ${describeError(err)}\n`,
      );
      process.exit(64);
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
} else if (isJsCliRequest(args)) {
  runJsCli();
} else {
  const result = resolveBinary({ binary: "terminal-commander" });

  if (result.reason === "bridge_required") {
    runJsCli();
  } else if (result.reason !== "ok") {
    process.stderr.write(formatResolveError(result) + "\n");
    process.exit(64);
  } else {
    // M7 (decided): this native passthrough runs the local platform binary as
    // the SAME user on the SAME host with inherited stdio — a normal CLI exec.
    // Inheriting the full parent env is intentional and correct here: stripping
    // vars would break commands that legitimately read the operator's env. This
    // is deliberately UNLIKE the WSL bridge (lib/wsl/spawn.js) and the MCP shim,
    // which cross a process/trust boundary and therefore filter secret-shaped
    // vars via buildFilteredEnv. No filtering on the same-host native path.
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
