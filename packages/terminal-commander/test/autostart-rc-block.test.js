// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// The managed autostart block in ~/.profile / ~/.bashrc / ~/.zshrc is brought
// up to date IN PLACE by both installers (Linux: installDaemonAutostart in JS;
// Windows-side WSL installer: tc_rc_sync in bash). Up to 0.3.11 an existing
// block was never rewritten, so the Windows installer's inert `$TC_HOME` block
// stayed forever. Every byte outside the markers, the trailing-newline state,
// the mode and a symlinked rc file must survive; an up-to-date file must not
// be rewritten; malformed markers are left alone and reported.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const {
  installDaemonAutostart,
  patchProfileFile,
  renderRcSyncBash,
  PROFILE_BLOCK_BODY,
} = require("../lib/daemon/autostart.js");

const POSIX = process.platform !== "win32";
const BASH_SKIP =
  POSIX && spawnSync("bash", ["-c", "true"]).status === 0
    ? false
    : "needs a POSIX bash (Linux / WSL / macOS)";

const BEGIN = "# terminal-commander autostart BEGIN";
const END = "# terminal-commander autostart END";
// What the <= 0.3.11 Windows-side installer appended: `$TC_HOME` is unset in
// the user's shell, so this block never did anything.
const TC_HOME_BODY =
  '[ -f "$TC_HOME/.config/terminal-commander/profile.d/terminal-commander.sh" ] && . "$TC_HOME/.config/terminal-commander/profile.d/terminal-commander.sh"';
// Bytes the update must not touch: a latin1 byte (not valid UTF-8) before the
// block, and content after it without a trailing newline.
const BEFORE = Buffer.from("# user rc \xe9\nexport FOO=1\n\n", "latin1");
const AFTER = Buffer.from("alias ll='ls -l'", "latin1");

function block(body, eol = "\n") {
  return Buffer.from(`${BEGIN}${eol}${body}${eol}${END}${eol}`, "latin1");
}

function tmpHome() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "tc-rc-"));
}

const PAST = new Date("2020-01-01T00:00:00Z");

function writeRc(file, bytes, mode) {
  fs.writeFileSync(file, bytes);
  if (POSIX && mode) fs.chmodSync(file, mode);
  fs.utimesSync(file, PAST, PAST);
}

function assertUntouched(file, bytes) {
  assert.deepEqual(fs.readFileSync(file), bytes, `${file} bytes changed`);
  assert.equal(fs.statSync(file).mtimeMs, PAST.getTime(), `${file} was rewritten`);
}

function install(home) {
  return installDaemonAutostart({
    platform: "linux",
    homeDir: home,
    env: { HOME: home },
    daemonBinary: "/fake/terminal-commanderd",
    systemdUserAvailable: () => false,
    runAutostartOnce: () => ({ ok: true, exit_code: 0 }),
  });
}

// ---- Linux installer (JS) ------------------------------------------------

test("JS: a stale $TC_HOME block is replaced in place; outside bytes, no-trailing-newline and mode kept", () => {
  const home = tmpHome();
  const rc = path.join(home, ".profile");
  writeRc(rc, Buffer.concat([BEFORE, block(TC_HOME_BODY), AFTER]), 0o600);
  const r = install(home);
  assert.deepEqual(
    fs.readFileSync(rc),
    Buffer.concat([BEFORE, block(PROFILE_BLOCK_BODY), AFTER]),
  );
  if (POSIX) assert.equal(fs.statSync(rc).mode & 0o777, 0o600);
  assert.ok(r.patched.includes(rc));
});

test("JS: CRLF rc file keeps CRLF on the replaced body line", () => {
  const home = tmpHome();
  const rc = path.join(home, ".bashrc");
  writeRc(rc, Buffer.concat([Buffer.from("x=1\r\n"), block(TC_HOME_BODY, "\r\n"), Buffer.from("y=2\r\n")]));
  install(home);
  assert.deepEqual(
    fs.readFileSync(rc),
    Buffer.concat([Buffer.from("x=1\r\n"), block(PROFILE_BLOCK_BODY, "\r\n"), Buffer.from("y=2\r\n")]),
  );
});

test("JS: an up-to-date block leaves the file byte-identical and not rewritten", () => {
  const home = tmpHome();
  const rc = path.join(home, ".zshrc");
  const bytes = Buffer.concat([BEFORE, block(PROFILE_BLOCK_BODY), AFTER]);
  writeRc(rc, bytes);
  const r = install(home);
  assertUntouched(rc, bytes);
  assert.ok(!r.patched.includes(rc));
});

test("JS: malformed markers (BEGIN without END, duplicated block) are left untouched and reported", () => {
  const home = tmpHome();
  const noEnd = Buffer.concat([BEFORE, Buffer.from(`${BEGIN}\n${TC_HOME_BODY}\n`)]);
  const dup = Buffer.concat([block(TC_HOME_BODY), BEFORE, block(TC_HOME_BODY)]);
  writeRc(path.join(home, ".profile"), noEnd);
  writeRc(path.join(home, ".bashrc"), dup);
  const r = install(home);
  assertUntouched(path.join(home, ".profile"), noEnd);
  assertUntouched(path.join(home, ".bashrc"), dup);
  assert.deepEqual(r.malformed_rc_files.sort(), [path.join(home, ".bashrc"), path.join(home, ".profile")].sort());
  assert.match(r.hint, /malformed terminal-commander autostart markers left unchanged/);
});

test("JS: without add (systemd mode) a missing block is not created, a stale one is still updated", () => {
  const home = tmpHome();
  const none = path.join(home, ".profile");
  const stale = path.join(home, ".bashrc");
  writeRc(none, BEFORE);
  writeRc(stale, Buffer.concat([BEFORE, block(TC_HOME_BODY)]));
  assert.equal(patchProfileFile(none, home, false).changed, false);
  assertUntouched(none, BEFORE);
  assert.equal(patchProfileFile(stale, home, false).changed, true);
  assert.deepEqual(fs.readFileSync(stale), Buffer.concat([BEFORE, block(PROFILE_BLOCK_BODY)]));
});

test("JS: a symlinked rc file stays a symlink; its target is updated", { skip: POSIX ? false : "symlinks need POSIX" }, () => {
  const home = tmpHome();
  const real = path.join(home, "dotfiles-profile");
  writeRc(real, Buffer.concat([BEFORE, block(TC_HOME_BODY)]));
  fs.symlinkSync(real, path.join(home, ".profile"));
  install(home);
  assert.ok(fs.lstatSync(path.join(home, ".profile")).isSymbolicLink());
  assert.deepEqual(fs.readFileSync(real), Buffer.concat([BEFORE, block(PROFILE_BLOCK_BODY)]));
});

// ---- Windows-side WSL installer (bash tc_rc_sync) ------------------------

function rcSync(home, file, add) {
  const script = path.join(home, "rc-sync.sh");
  fs.writeFileSync(script, `set -eu\n${renderRcSyncBash()}tc_rc_sync "$1" "$2"\n`);
  return spawnSync("bash", [script, file, add ? "1" : "0"], {
    encoding: "utf8",
    env: { HOME: home, PATH: "/usr/bin:/bin" },
    stdio: ["ignore", "pipe", "pipe"],
  });
}

test("bash: a stale $TC_HOME block is replaced in place; outside bytes, no-trailing-newline, mode and symlink kept", { skip: BASH_SKIP }, () => {
  const home = tmpHome();
  const real = path.join(home, "dotfiles-profile");
  writeRc(real, Buffer.concat([BEFORE, block(TC_HOME_BODY), AFTER]), 0o600);
  const rc = path.join(home, ".profile");
  fs.symlinkSync(real, rc);
  const r = rcSync(home, rc, true);
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(fs.readFileSync(real), Buffer.concat([BEFORE, block(PROFILE_BLOCK_BODY), AFTER]));
  assert.equal(fs.statSync(real).mode & 0o777, 0o600);
  assert.ok(fs.lstatSync(rc).isSymbolicLink());
  assert.deepEqual(fs.readdirSync(home).filter((n) => n.includes("tc-sync")), [], "temp file left behind");
});

test("bash: CRLF rc file keeps CRLF and its trailing newline", { skip: BASH_SKIP }, () => {
  const home = tmpHome();
  const rc = path.join(home, ".bashrc");
  writeRc(rc, Buffer.concat([Buffer.from("x=1\r\n"), block(TC_HOME_BODY, "\r\n")]));
  assert.equal(rcSync(home, rc, true).status, 0);
  assert.deepEqual(fs.readFileSync(rc), Buffer.concat([Buffer.from("x=1\r\n"), block(PROFILE_BLOCK_BODY, "\r\n")]));
});

test("bash: an up-to-date block leaves the file byte-identical and not rewritten", { skip: BASH_SKIP }, () => {
  const home = tmpHome();
  const rc = path.join(home, ".zshrc");
  const bytes = Buffer.concat([BEFORE, block(PROFILE_BLOCK_BODY), AFTER]);
  writeRc(rc, bytes);
  const r = rcSync(home, rc, true);
  assert.equal(r.status, 0, r.stderr);
  assertUntouched(rc, bytes);
  assert.equal(r.stdout, "");
});

test("bash: malformed markers are left untouched and reported", { skip: BASH_SKIP }, () => {
  const home = tmpHome();
  for (const bytes of [
    Buffer.concat([BEFORE, Buffer.from(`${BEGIN}\n${TC_HOME_BODY}\n`)]),
    Buffer.concat([block(TC_HOME_BODY), BEFORE, block(TC_HOME_BODY)]),
    Buffer.from(`${END}\n${TC_HOME_BODY}\n${BEGIN}\n`),
  ]) {
    const rc = path.join(home, ".profile");
    writeRc(rc, bytes);
    const r = rcSync(home, rc, true);
    assert.equal(r.status, 0, r.stderr);
    assertUntouched(rc, bytes);
    assert.match(r.stdout, /^terminal-commander: malformed terminal-commander autostart markers left unchanged in /m);
  }
});

test("bash: add=1 appends a missing block, add=0 leaves the file alone", { skip: BASH_SKIP }, () => {
  const home = tmpHome();
  const rc = path.join(home, ".profile");
  writeRc(rc, BEFORE);
  assert.equal(rcSync(home, rc, false).status, 0);
  assertUntouched(rc, BEFORE);
  assert.equal(rcSync(home, rc, true).status, 0);
  assert.deepEqual(fs.readFileSync(rc), Buffer.concat([BEFORE, Buffer.from("\n"), block(PROFILE_BLOCK_BODY)]));
});
