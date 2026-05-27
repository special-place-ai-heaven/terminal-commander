// npm postinstall hook: after the package is (re)installed with new
// binaries, replace any stale running daemon with the freshly-installed
// one by invoking the daemon's `update` run-mode (single source of
// truth -- all kill/version/respawn logic lives in the Rust supervisor).
//
// MUST be best-effort and never fail the install: a stale daemon left
// running is recovered by the MCP adapter's own auto-replace on its
// next launch (the backstop). package.json calls this with `|| exit 0`
// and this script also swallows all errors and exits 0.

"use strict";

const os = require("node:os");
const path = require("node:path");
const fs = require("node:fs");
const { spawnSync } = require("node:child_process");

function stateDir(env, platform) {
  const e = env || process.env;
  if (e.TC_DATA && e.TC_DATA.length > 0) return e.TC_DATA;
  if (platform === "win32" && e.LOCALAPPDATA) {
    return path.join(e.LOCALAPPDATA, "terminal-commanderd", "state");
  }
  const home = e.HOME || os.homedir();
  return path.join(home, ".local", "share", "terminal-commanderd");
}

// Resolve the daemon binary to invoke `update` on.
function daemonBinary(env, platform) {
  if (platform === "win32") {
    const staged = path.resolve(
      __dirname,
      "..",
      "..",
      "..",
      "terminal-commander-windows-x64",
      "bin",
      "terminal-commanderd.exe",
    );
    if (fs.existsSync(staged)) return { cmd: staged, wsl: false };
    const w = spawnSync("where", ["terminal-commanderd.exe"], {
      encoding: "utf8",
      env,
    });
    if (w.status === 0) {
      const line = (w.stdout || "").trim().split(/\r?\n/)[0];
      if (line) return { cmd: line, wsl: false };
    }
    return null;
  }
  // unix / WSL: resolve via a login-shell PATH (mirrors autostart).
  const e = env || process.env;
  const home = e.HOME || os.homedir();
  const pathPrefix =
    `${home}/.npm-global/bin:${home}/.local/bin:${home}/.cargo/bin:` +
    "/usr/local/bin:/usr/bin:/bin";
  const r = spawnSync(
    "bash",
    ["-lc", `export PATH="${pathPrefix}"; command -v terminal-commanderd`],
    { encoding: "utf8", env: e },
  );
  if (r.status !== 0) return null;
  const line = (r.stdout || "").trim().split(/\r?\n/)[0];
  if (!line) return null;
  return { cmd: line, wsl: false };
}

function main() {
  try {
    const platform = process.platform;
    const env = process.env;
    const sd = stateDir(env, platform);
    const bin = daemonBinary(env, platform);
    if (!bin) {
      process.stderr.write(
        "terminal-commander: postinstall could not locate the daemon binary; " +
          "the MCP adapter will auto-replace a stale daemon on next launch.\n",
      );
      return;
    }
    const r = spawnSync(bin.cmd, ["--data-dir", sd, "update"], {
      encoding: "utf8",
      env,
      timeout: 30000,
    });
    const out = `${r.stdout || ""}${r.stderr || ""}`.trim();
    if (out) process.stderr.write(`terminal-commander: ${out}\n`);
  } catch (err) {
    process.stderr.write(
      `terminal-commander: postinstall daemon-replace skipped (${
        err && err.message ? err.message : "error"
      }); adapter auto-replace is the backstop.\n`,
    );
  }
  // Always succeed: never fail the install.
  process.exit(0);
}

main();
