// SPDX-License-Identifier: Apache-2.0
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { runHarnessMcpSession, SESSIONS_ROOT } = require('../lib/daemon/session_supervisor.js');

test('SIGINT during cold-start wait returns early without spawning MCP', async () => {
  const node = process.execPath;
  const tmpDir = os.tmpdir();

  // Fake daemon: stays alive but never binds the IPC pipe (so waitForIpc blocks).
  const fakeDaemon = path.join(tmpDir, `fake-daemon-noop-${process.pid}.js`);
  fs.writeFileSync(fakeDaemon, 'setInterval(()=>{},1000);');

  // Fake MCP: writes a marker file if it ever runs, then EXITS so the
  // regression case fails fast (clean assertion failure) instead of
  // hanging on setInterval until an external timeout.
  const markerFile = path.join(tmpDir, `mcp-ran-${process.pid}.marker`);
  const fakeMcp = path.join(tmpDir, `fake-mcp-noop-${process.pid}.js`);
  fs.writeFileSync(
    fakeMcp,
    `require('fs').writeFileSync(${JSON.stringify(markerFile)}, 'ran'); process.exit(0);`,
  );
  // Remove any stale marker from a previous run.
  try { fs.unlinkSync(markerFile); } catch (_e) {}

  const sessionPromise = runHarnessMcpSession({
    daemonBinary: node,
    mcpBinary: node,
    argv: [fakeMcp],
    env: { ...process.env },
  });

  // Wait briefly so we're inside the cold-start waitForIpc window.
  await new Promise((r) => setTimeout(r, 500));

  // Emit SIGINT — triggers the registered SIGINT handler synchronously.
  process.emit('SIGINT');

  // Defensive timeout: even with the fake MCP exiting fast, if the
  // supervisor itself wedged we should fail loud rather than hang CI.
  const TIMEOUT_MS = 15_000;
  const outcome = await Promise.race([
    sessionPromise,
    new Promise((_, reject) =>
      setTimeout(
        () => reject(new Error(`runHarnessMcpSession did not resolve within ${TIMEOUT_MS}ms`)),
        TIMEOUT_MS,
      ),
    ),
  ]);

  assert.equal(outcome.code, 130, `expected exit 130 on SIGINT, got ${outcome.code}`);
  assert.equal(outcome.signal, 'SIGINT', `expected signal SIGINT, got ${outcome.signal}`);
  assert.ok(
    !fs.existsSync(markerFile),
    'MCP must NOT have spawned — marker file exists, meaning the orphan-MCP bug regressed',
  );

  // Cleanup
  try { fs.unlinkSync(fakeDaemon); } catch (_e) {}
  try { fs.unlinkSync(fakeMcp); } catch (_e) {}
  try { fs.unlinkSync(markerFile); } catch (_e) {}
});

test('session base directory survives MCP child exit', async () => {
  const node = process.execPath;
  const tmpDir = os.tmpdir();

  // Fake daemon: stays alive (ignores the args node would reject)
  const fakeDaemon = path.join(tmpDir, `fake-daemon-${process.pid}.js`);
  fs.writeFileSync(fakeDaemon, 'setInterval(()=>{},1000);');

  // Fake MCP: exits immediately with code 0
  const fakeMcp = path.join(tmpDir, `fake-mcp-${process.pid}.js`);
  fs.writeFileSync(fakeMcp, 'process.exit(0);');

  // Snapshot session dirs before the run
  fs.mkdirSync(SESSIONS_ROOT, { recursive: true });
  const before = new Set(
    fs.readdirSync(SESSIONS_ROOT, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name),
  );

  const outcome = await runHarnessMcpSession({
    daemonBinary: node,
    mcpBinary: node,
    argv: [fakeMcp],
    env: { ...process.env },
    // Override daemon args to be the fakeDaemon script so node can handle them:
    // We patch via env — but spawnDaemonHidden always passes fixed args.
    // Instead we rely on the daemon child exiting quickly (node rejects unknown
    // flags) while the session dir must still survive after MCP exits.
  });

  // Clean up temp scripts
  try { fs.unlinkSync(fakeDaemon); } catch (_e) {}
  try { fs.unlinkSync(fakeMcp); } catch (_e) {}

  assert.equal(outcome.code, 0, `MCP exit code should be 0, got ${outcome.code}`);

  // Find new session directories created during this run
  const after = fs
    .readdirSync(SESSIONS_ROOT, { withFileTypes: true })
    .filter((e) => e.isDirectory() && !before.has(e.name))
    .map((e) => e.name);

  assert.ok(
    after.length > 0,
    `Expected at least one new session directory to remain under ${SESSIONS_ROOT}, but none found. ` +
    `The supervisor must NOT delete the session base on MCP exit.`,
  );
});
