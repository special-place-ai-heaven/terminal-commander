// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// `terminal-commander restart` handler tests. Mocks detect + exec so
// the suite is portable to any host and never spawns a real daemon.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");
const { spawnSync } = require("node:child_process");

const { runRestart, DAEMON_RESTART_CMD } = require("../lib/cli/restart.js");
const { DETECT_REASONS } = require("../lib/wsl/detect.js");
const {
  detectRuntimeEnvironment,
  windowsUpdateScopes,
  describeError,
} = require("../lib/cli/runtime_environment.js");

// A fake child that emits the given exit code on next tick.
function fakeChild(code, stdout) {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  setImmediate(() => {
    if (stdout) child.stdout.emit("data", Buffer.from(stdout, "utf8"));
    child.emit("close", code);
  });
  return child;
}

function okDetect(defaultName) {
  return {
    host_platform: "win32",
    wsl_callable: true,
    default_distro: defaultName,
    distros: [{ name: defaultName, state: "Running", wsl_version: 2, is_default: true }],
    reason: DETECT_REASONS.OK,
  };
}

test("DAEMON_RESTART_CMD always forces (--force)", () => {
  assert.match(DAEMON_RESTART_CMD, /terminal-commanderd update --force/);
});

test("restart on Windows selects the native daemon by default", async () => {
  let seen = null;
  const r = await runRestart({
    platform: "win32",
    env: {},
    flags: {},
    detect: async () => {
      throw new Error("native Windows restart must not probe WSL");
    },
    resolveBinary: () => ({
      reason: "ok",
      binaryPath: "C:\\tc\\terminal-commanderd.exe",
    }),
    exec: ({ file, argv }) => {
      seen = { file, argv };
      return fakeChild(0, "terminal-commanderd: daemon running");
    },
  });

  assert.equal(r.status, "ok");
  assert.deepEqual(seen, {
    file: "C:\\tc\\terminal-commanderd.exe",
    argv: ["update", "--force"],
  });
  assert.match(r.output, /native Windows daemon restarted/);
});

test("restart on Windows dispatches daemon update --force through WSL", async () => {
  let seenArgv = null;
  const r = await runRestart({
    platform: "win32",
    env: { TC_WSL_DISTRO: "Ubuntu-24.04" },
    flags: {},
    detect: async () => okDetect("Ubuntu-24.04"),
    exec: ({ file, argv }) => {
      seenArgv = { file, argv };
      return fakeChild(0, "terminal-commanderd: replaced 0.1.17 -> 0.1.18");
    },
  });
  assert.equal(r.status, "ok");
  assert.equal(r.exit_code, 0);
  assert.equal(seenArgv.file, "wsl.exe");
  assert.deepEqual(seenArgv.argv.slice(0, 3), ["-d", "Ubuntu-24.04", "--"]);
  // The bash -lc payload must invoke the forced daemon update.
  const payload = seenArgv.argv[seenArgv.argv.length - 1];
  assert.match(payload, /terminal-commanderd update --force/);
  assert.match(r.output, /restarted in 'Ubuntu-24\.04'/);
});

test("restart on Windows neutralizes ambient WSLENV (no credential crosses into WSL)", async () => {
  // With a session token, WSLENV must be reduced to the TC-only
  // allowlist so wsl.exe forwards ONLY TC_SESSION -- never an operator's
  // ambient WSL_SUDO_CREDENTIAL carried by an ambient WSLENV entry.
  let withSession = null;
  await runRestart({
    platform: "win32",
    env: {
      TC_WSL_DISTRO: "Ubuntu-24.04",
      TC_SESSION: "sess-abc",
      WSLENV: "WSL_SUDO_CREDENTIAL/u",
      WSL_SUDO_CREDENTIAL: "super-secret",
    },
    flags: {},
    detect: async () => okDetect("Ubuntu-24.04"),
    exec: ({ env }) => {
      withSession = env;
      return fakeChild(0, "ok");
    },
  });
  assert.equal(withSession.WSLENV, "TC_SESSION/u");

  // Without a session token, the ambient WSLENV is dropped entirely so
  // nothing crosses the boundary.
  let noSession = null;
  await runRestart({
    platform: "win32",
    env: {
      TC_WSL_DISTRO: "Ubuntu-24.04",
      WSLENV: "WSL_SUDO_CREDENTIAL/u",
      WSL_SUDO_CREDENTIAL: "super-secret",
    },
    flags: {},
    detect: async () => okDetect("Ubuntu-24.04"),
    exec: ({ env }) => {
      noSession = env;
      return fakeChild(0, "ok");
    },
  });
  assert.equal(noSession.WSLENV, undefined);
});

test("restart on Linux invokes the daemon binary directly", async () => {
  let seenArgv = null;
  const r = await runRestart({
    platform: "linux",
    env: {},
    flags: {},
    exec: ({ file, argv }) => {
      seenArgv = { file, argv };
      return fakeChild(0, "");
    },
  });
  assert.equal(r.status, "ok");
  assert.equal(seenArgv.file, "terminal-commanderd");
  assert.deepEqual(seenArgv.argv, ["update", "--force"]);
});

test("restart surfaces a non-zero daemon exit as restart_failed", async () => {
  const r = await runRestart({
    platform: "linux",
    env: {},
    flags: {},
    exec: () => fakeChild(2, "boom"),
  });
  assert.equal(r.status, "restart_failed");
  assert.equal(r.exit_code, 2);
  assert.match(r.output, /restart failed/);
});

test("restart on Windows refuses when no distro resolves", async () => {
  const r = await runRestart({
    platform: "win32",
    env: { TC_USE_LEGACY_WSL_BRIDGE: "1" },
    flags: {},
    detect: async () => ({
      host_platform: "win32",
      wsl_callable: true,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.OK,
    }),
    exec: () => {
      throw new Error("must not spawn when distro unresolved");
    },
  });
  assert.notEqual(r.status, "ok");
  assert.equal(r.exit_code, 64);
});

test("Windows selects the native runtime unless WSL is explicitly requested", () => {
  assert.deepEqual(
    detectRuntimeEnvironment({ platform: "win32", env: {}, flags: {} }),
    { status: "ok", host: "windows", runtime: "native", evidence: "win32_default" },
  );
  assert.equal(
    detectRuntimeEnvironment({
      platform: "win32",
      env: { TC_USE_LEGACY_WSL_BRIDGE: "1" },
      flags: {},
    }).runtime,
    "wsl",
  );
  assert.equal(
    detectRuntimeEnvironment({
      platform: "win32",
      env: { TC_WSL_DISTRO: "Ubuntu-24.04" },
      flags: {},
    }).runtime,
    "wsl",
  );
  assert.equal(
    detectRuntimeEnvironment({
      platform: "win32",
      env: {},
      flags: { distro: "Ubuntu-24.04" },
    }).runtime,
    "wsl",
  );
});

test("unsupported hosts are explicit and never classified as unknown", () => {
  assert.deepEqual(
    detectRuntimeEnvironment({ platform: "plan9", env: {}, flags: {} }),
    {
      status: "unsupported",
      host: "plan9",
      runtime: null,
      evidence: "unsupported_platform:plan9",
    },
  );
});

test("Windows update owns both npm staging and stable native binary scopes", () => {
  const packageRoot = path.join("C:\\", "npm", "node_modules", "terminal-commander");
  const scopes = windowsUpdateScopes({
    platform: "win32",
    env: { LOCALAPPDATA: path.join("C:\\", "Users", "me", "AppData", "Local") },
    packageRoot,
  });
  assert.deepEqual(scopes, [
    path.dirname(packageRoot),
    path.join("C:\\", "Users", "me", "AppData", "Local", "terminal-commander", "bin"),
  ]);
});

test("error diagnostics preserve messages instead of collapsing to unknown", () => {
  assert.equal(describeError(new Error("LOCALAPPDATA is missing")), "LOCALAPPDATA is missing");
  assert.equal(describeError({ code: "ELOCKED" }), "ELOCKED");
  assert.match(describeError(null), /non-Error rejection/);
});

test("Windows update launcher reaps npm and stable scopes before npm install", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "tc-update-scopes-"));
  try {
    const recordPath = path.join(tmp, "spawns.json");
    const preload = path.join(tmp, "preload.js");
    const npmCli = path.join(tmp, "npm-cli.js");
    const prefix = path.join(tmp, "prefix");
    const freshLauncher = path.join(
      prefix,
      "node_modules",
      "terminal-commander",
      "bin",
      "terminal-commander.js",
    );
    fs.mkdirSync(path.dirname(freshLauncher), { recursive: true });
    fs.writeFileSync(freshLauncher, "// mocked fresh launcher\n", "utf8");
    fs.writeFileSync(npmCli, "// mocked npm cli\n", "utf8");
    fs.writeFileSync(recordPath, "[]", "utf8");
    fs.writeFileSync(
      preload,
      `
"use strict";
Object.defineProperty(process, "platform", { value: "win32" });
const cp = require("node:child_process");
const fs = require("node:fs");
const { EventEmitter } = require("node:events");
const recordPath = ${JSON.stringify(recordPath)};
cp.spawn = function mockedSpawn(command, args) {
  const records = JSON.parse(fs.readFileSync(recordPath, "utf8"));
  records.push({ command: String(command), args: (args || []).map(String) });
  fs.writeFileSync(recordPath, JSON.stringify(records), "utf8");
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  setImmediate(() => child.emit("exit", 0, null));
  return child;
};
`,
      "utf8",
    );

    const localAppData = path.join(tmp, "local-app-data");
    const launcher = path.resolve(__dirname, "../bin/terminal-commander.js");
    const result = spawnSync(
      process.execPath,
      ["--require", preload, launcher, "update"],
      {
        encoding: "utf8",
        timeout: 20_000,
        shell: false,
        env: {
          ...process.env,
          LOCALAPPDATA: localAppData,
          npm_execpath: npmCli,
          npm_config_prefix: prefix,
        },
      },
    );
    assert.equal(result.status, 0, result.stderr);

    const spawns = JSON.parse(fs.readFileSync(recordPath, "utf8"));
    const preflights = spawns.filter((spawn) => spawn.args[0] === "update-locks");
    assert.equal(preflights.length, 2, JSON.stringify(spawns));
    assert.equal(preflights[0].args[1], "--scope-dir");
    assert.equal(preflights[0].args[2], path.resolve(__dirname, "../.."));
    assert.equal(preflights[1].args[1], "--scope-dir");
    assert.equal(
      preflights[1].args[2],
      path.join(localAppData, "terminal-commander", "bin"),
    );
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test("Windows update uses the stable helper when the platform package is missing", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "tc-update-stable-fallback-"));
  try {
    const recordPath = path.join(tmp, "spawns.json");
    const preload = path.join(tmp, "preload.js");
    const npmCli = path.join(tmp, "npm-cli.js");
    const prefix = path.join(tmp, "prefix");
    const localAppData = path.join(tmp, "local-app-data");
    const stableHelper = path.join(
      localAppData,
      "terminal-commander",
      "bin",
      "terminal-commander.exe",
    );
    const freshLauncher = path.join(
      prefix,
      "node_modules",
      "terminal-commander",
      "bin",
      "terminal-commander.js",
    );
    fs.mkdirSync(path.dirname(stableHelper), { recursive: true });
    fs.mkdirSync(path.dirname(freshLauncher), { recursive: true });
    fs.writeFileSync(stableHelper, "stable helper", "utf8");
    fs.writeFileSync(freshLauncher, "// mocked fresh launcher\n", "utf8");
    fs.writeFileSync(npmCli, "// mocked npm cli\n", "utf8");
    fs.writeFileSync(recordPath, "[]", "utf8");
    fs.writeFileSync(
      preload,
      `
"use strict";
Object.defineProperty(process, "platform", { value: "win32" });
const Module = require("node:module");
const originalLoad = Module._load;
Module._load = function mockedLoad(request, parent, isMain) {
  if (String(request).endsWith("lib/resolve-binary.js")) {
    return {
      resolveBinary: () => ({ reason: "missing_platform_package" }),
      formatResolveError: () => "terminal-commander: platform package missing",
    };
  }
  return originalLoad.call(this, request, parent, isMain);
};
const cp = require("node:child_process");
const fs = require("node:fs");
const { EventEmitter } = require("node:events");
const recordPath = ${JSON.stringify(recordPath)};
cp.spawn = function mockedSpawn(command, args) {
  const records = JSON.parse(fs.readFileSync(recordPath, "utf8"));
  records.push({ command: String(command), args: (args || []).map(String) });
  fs.writeFileSync(recordPath, JSON.stringify(records), "utf8");
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  setImmediate(() => child.emit("exit", 0, null));
  return child;
};
`,
      "utf8",
    );

    const launcher = path.resolve(__dirname, "../bin/terminal-commander.js");
    const result = spawnSync(process.execPath, ["--require", preload, launcher, "update"], {
      encoding: "utf8",
      timeout: 20_000,
      shell: false,
      env: {
        ...process.env,
        LOCALAPPDATA: localAppData,
        npm_execpath: npmCli,
        npm_config_prefix: prefix,
      },
    });
    assert.equal(result.status, 0, result.stderr);

    const spawns = JSON.parse(fs.readFileSync(recordPath, "utf8"));
    const preflights = spawns.filter((spawn) => spawn.args[0] === "update-locks");
    assert.equal(preflights.length, 2, JSON.stringify(spawns));
    assert.equal(preflights[0].command, stableHelper);
    assert.equal(preflights[1].command, stableHelper);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
