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
    detached: false,
    windowsHide: true,
  };
  if (process.platform === "win32") {
    opts.creationFlags = CREATE_NO_WINDOW;
  }
  return spawn(daemonBinary, args, opts);
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

  const onSignal = () => cleanup();
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);

  daemon = spawnDaemonHidden(o.daemonBinary, paths.dataDir, paths.ipcEndpoint, env);

  void waitForIpc(paths.ipcEndpoint, STARTUP_TIMEOUT_MS).then((ready) => {
    if (ready && daemon && daemon.pid) {
      manifest.daemon_pid = daemon.pid;
      writeManifest(paths.manifestPath, manifest);
    }
  });

  const mcpEnv = {
    ...env,
    TC_SOCKET: paths.ipcEndpoint,
    TC_SESSION_ID: paths.sessionId,
    TC_DATA: paths.dataDir,
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
      process.off("SIGINT", onSignal);
      process.off("SIGTERM", onSignal);
      resolve({ code: code == null ? 1 : code, signal: signal || null });
    });

    mcp.on("error", () => {
      cleanup();
      process.off("SIGINT", onSignal);
      process.off("SIGTERM", onSignal);
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
