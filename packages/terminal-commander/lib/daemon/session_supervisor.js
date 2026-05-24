// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

const CREATE_NO_WINDOW = 0x08000000;
const SESSIONS_ROOT = path.join(os.homedir(), ".config", "terminal-commander", "sessions");
const STARTUP_TIMEOUT_MS = 20_000;
const IPC_PROBE_INTERVAL_MS = 100;

function newSessionId() {
  return `h${process.pid.toString(36)}${crypto.randomBytes(4).toString("hex")}`;
}

function sanitizeSessionId(sessionId) {
  return String(sessionId).replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 48);
}

function resolveSessionPaths(sessionId, env) {
  const id = sanitizeSessionId(sessionId);
  const base = path.join(SESSIONS_ROOT, id);
  const dataDir = path.join(base, "data");
  const manifestPath = path.join(base, "manifest.json");
  let ipcEndpoint;
  if (process.platform === "win32") {
    const user = (env && env.USERNAME) || process.env.USERNAME || "user";
    ipcEndpoint = `\\\\.\\pipe\\terminal-commander-${user}-${id}`;
  } else {
    ipcEndpoint = path.join(dataDir, "terminal-commanderd.sock");
  }
  return { sessionId: id, base, dataDir, ipcEndpoint, manifestPath };
}

function writeManifest(manifestPath, manifest) {
  fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
  const tmp = `${manifestPath}.tmp.${process.pid}`;
  fs.writeFileSync(tmp, JSON.stringify(manifest, null, 2), { encoding: "utf8", mode: 0o600 });
  fs.renameSync(tmp, manifestPath);
}

function readManifest(manifestPath) {
  try {
    return JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  } catch (_e) {
    return null;
  }
}

function isProcessAlive(pid) {
  if (!pid || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e && e.code === "EPERM";
  }
}

function probeIpcReachable(ipcEndpoint) {
  return new Promise((resolve) => {
    if (process.platform === "win32") {
      const client = net.connect(ipcEndpoint);
      const done = (ok) => {
        try {
          client.destroy();
        } catch (_e) {
          /* ignore */
        }
        resolve(ok);
      };
      client.setTimeout(500, () => done(false));
      client.on("connect", () => done(true));
      client.on("error", () => done(false));
      return;
    }
    try {
      resolve(fs.existsSync(ipcEndpoint));
    } catch (_e) {
      resolve(false);
    }
  });
}

async function waitForIpc(ipcEndpoint, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await probeIpcReachable(ipcEndpoint)) return true;
    await new Promise((r) => setTimeout(r, IPC_PROBE_INTERVAL_MS));
  }
  return false;
}

function spawnDaemonHidden(daemonBinary, dataDir, ipcEndpoint, env) {
  const args = ["--data-dir", dataDir, "start", "--mode", "ipc-server"];
  const childEnv = { ...(env || process.env), TC_SOCKET: ipcEndpoint };
  const logPath = path.join(dataDir, "terminal-commanderd.log");
  try { fs.mkdirSync(dataDir, { recursive: true }); } catch (_e) {}
  const logFd = fs.openSync(logPath, "a");
  const opts = {
    env: childEnv,
    stdio: ["ignore", logFd, logFd],
    shell: false,
    detached: process.platform !== "win32",
    windowsHide: true,
  };
  if (process.platform === "win32") {
    opts.creationFlags = CREATE_NO_WINDOW;
  }
  const child = spawn(daemonBinary, args, opts);
  // The child inherits a duplicate of logFd; release the parent's
  // copy so one fd per session does not leak in long-lived supervisor
  // processes. Log rotation TODO: deferred to Phase 4.
  try { fs.closeSync(logFd); } catch (_e) {}
  return child;
}

function killProcessTree(child) {
  if (!child || child.killed || child.exitCode != null) return;
  const pid = child.pid;
  if (!pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(pid), "/t", "/f"], {
      stdio: "ignore",
      windowsHide: true,
    });
  } else {
    try {
      process.kill(-pid, "SIGTERM");
    } catch (_e) {
      try {
        process.kill(pid, "SIGTERM");
      } catch (_ee) {
        /* ignore */
      }
    }
  }
}

function removeSessionDir(base) {
  try {
    fs.rmSync(base, { recursive: true, force: true });
  } catch (_e) {
    /* ignore */
  }
}

function cleanupStaleSessions(env) {
  let entries;
  try {
    entries = fs.readdirSync(SESSIONS_ROOT, { withFileTypes: true });
  } catch (_e) {
    return;
  }
  for (const ent of entries) {
    if (!ent.isDirectory()) continue;
    const manifestPath = path.join(SESSIONS_ROOT, ent.name, "manifest.json");
    const manifest = readManifest(manifestPath);
    if (!manifest) {
      removeSessionDir(path.join(SESSIONS_ROOT, ent.name));
      continue;
    }
    if (!isProcessAlive(manifest.supervisor_pid) && !isProcessAlive(manifest.daemon_pid)) {
      removeSessionDir(path.join(SESSIONS_ROOT, ent.name));
    }
  }
}

async function runHarnessMcpSession(opts) {
  const o = opts || {};
  const env = o.env || process.env;

  const paths = resolveSessionPaths(newSessionId(), env);
  fs.mkdirSync(paths.dataDir, { recursive: true });

  const manifest = {
    session_id: paths.sessionId,
    supervisor_pid: process.pid,
    daemon_pid: null,
    ipc_endpoint: paths.ipcEndpoint,
    data_dir: paths.dataDir,
    started_at: new Date().toISOString(),
  };
  writeManifest(paths.manifestPath, manifest);

  let daemon = null;
  let cleaned = false;
  const cleanup = () => {
    if (cleaned) return;
    cleaned = true;
    if (daemon) killProcessTree(daemon);
    // NOTE: session base directory is intentionally preserved across
    // MCP exits so doctor/debugging can inspect it. Cleanup is now
    // explicit via `terminal-commander maintenance cleanup --older-than 7d`.
  };

  let cancelSignal = null;
  const onSignal = (sig) => {
    cancelSignal = sig || "SIGINT";
    cleanup();
  };
  const onSigInt = () => onSignal("SIGINT");
  const onSigTerm = () => onSignal("SIGTERM");
  process.on("SIGINT", onSigInt);
  process.on("SIGTERM", onSigTerm);

  daemon = spawnDaemonHidden(o.daemonBinary, paths.dataDir, paths.ipcEndpoint, env);

  // Await daemon readiness BEFORE spawning the MCP child. Otherwise the
  // MCP child's Rust supervisor (with TC_SUPERVISOR_ALLOW_SPAWN=0) would
  // probe-and-find-nothing on cold start, freeze its DaemonStatusHandle
  // to Unavailable, and return daemon_unavailable for every tool call
  // for the rest of the session.
  //
  // A short total budget (5s) is enough on typical cold starts:
  // named-pipe + UDS bind under 200ms. If the daemon really cannot
  // come up, fall through and let MCP serve in degraded mode anyway —
  // better than no session at all.
  const COLD_START_BUDGET_MS = 5_000;
  const ready = await waitForIpc(paths.ipcEndpoint, COLD_START_BUDGET_MS);
  if (ready && daemon && daemon.pid) {
    manifest.daemon_pid = daemon.pid;
    writeManifest(paths.manifestPath, manifest);
  }
  // If !ready: continue anyway. The MCP child will still serve
  // tools/list and daemon-free tools; daemon-requiring tools will
  // surface daemon_unavailable, which is the honest status.

  // Guard: if a signal arrived during the readiness wait, cleanup() has
  // already run (daemon killed, cleaned=true). Do NOT proceed to spawn MCP —
  // that would leave an orphan child after the user cancelled.
  if (cleaned) {
    process.off("SIGINT", onSigInt);
    process.off("SIGTERM", onSigTerm);
    return {
      code: cancelSignal === "SIGTERM" ? 143 : 130,
      signal: cancelSignal || "SIGINT",
    };
  }

  const mcpEnv = {
    ...env,
    TC_SOCKET: paths.ipcEndpoint,
    TC_SESSION_ID: paths.sessionId,
    TC_DATA: paths.dataDir,
    // Prevent the Rust MCP supervisor from spawning a second daemon.
    // The JS wrapper already owns the daemon; the MCP child must probe-only.
    TC_SUPERVISOR_ALLOW_SPAWN: "0",
  };

  return new Promise((resolve) => {
    const mcp = spawn(o.mcpBinary, o.argv || [], {
      stdio: "inherit",
      shell: false,
      windowsHide: true,
      env: mcpEnv,
    });

    mcp.on("exit", (code, signal) => {
      if (daemon) killProcessTree(daemon);
      cleaned = true;
      process.off("SIGINT", onSigInt);
      process.off("SIGTERM", onSigTerm);
      resolve({ code: code == null ? 1 : code, signal: signal || null });
    });

    mcp.on("error", () => {
      cleanup();
      process.off("SIGINT", onSigInt);
      process.off("SIGTERM", onSigTerm);
      resolve({ code: 126, signal: null });
    });
  });
}

module.exports = {
  runHarnessMcpSession,
  resolveSessionPaths,
  cleanupStaleSessions,
  probeIpcReachable,
  SESSIONS_ROOT,
};
