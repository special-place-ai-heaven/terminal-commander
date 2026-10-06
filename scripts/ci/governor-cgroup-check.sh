#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Linux cgroup lane of the resource governor, for real (CI job
# `governor-cgroup-linux` in .github/workflows/npm-binary-build.yml).
#
# On a stock GitHub runner the test process sits in a cgroup whose PARENT it
# cannot write, so every governor test takes the rlimit lane and the cgroup
# paths (per-job cgroup, host ceiling, outliving-grandchild kill, boot sweep)
# never run. The governor needs the process in a LEAF cgroup whose parent
# directory is writable by the same user and delegates the `memory` and `pids`
# controllers; it then creates tc-jobs-<pid>/ and tc-job-<pid>-<id> as
# siblings of the leaf. That is the systemd layout (app.slice/<unit>). This
# script builds it, runs the governor tests in it and refuses to pass unless
# they report cgroup mode.
#
# Usage: scripts/ci/governor-cgroup-check.sh [BASE]
#   BASE  cgroup dir that becomes the parent (default /sys/fs/cgroup/tc-ci).
#         If its parent is writable by the current user nothing needs sudo
#         (a systemd user session); otherwise sudo is used for the one-time
#         setup: mkdir, controller delegation, chown, and the process move
#         (the kernel demands write access to the COMMON ANCESTOR's
#         cgroup.procs, the cgroup root on a runner, which a user lacks).
#         Everything the tests do runs as the current user, without sudo.
#
# Every assertion prints PASS or FAIL with the decisive text. Exit 0 only when
# nothing failed; 2 for an environment problem.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CG=/sys/fs/cgroup
BASE="${1:-$CG/tc-ci}"
PARENT="$(dirname "$BASE")"
LEAF="$BASE/leaf"
LOG="$(mktemp "${TMPDIR:-/tmp}/tc-governor-cgroup.XXXXXX")"
FAILS=0
CREATED=0
MOVED=0

envfail() { echo "governor-cgroup-check: $*" >&2; exit 2; }
pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*"; FAILS=$((FAILS + 1)); }

[ "$(stat -f -c %T "$CG" 2>/dev/null)" = cgroup2fs ] || envfail "$CG is not a cgroup v2 mount"
command -v cargo-nextest >/dev/null 2>&1 || envfail "cargo-nextest not installed"
[ ! -e "$BASE" ] || envfail "$BASE already exists (stale run?); rmdir it first"

SUDO=""
if [ ! -w "$PARENT" ]; then
  SUDO=sudo
  sudo -n true 2>/dev/null || envfail "$PARENT is not writable and passwordless sudo is unavailable"
fi
ORIG="$(sed -n 's/^0:://p' /proc/self/cgroup)"
[ -n "$ORIG" ] || envfail "no cgroup v2 line in /proc/self/cgroup"

# cgw <value> <cgroup file>: write a cgroup control file (sudo when needed).
cgw() {
  if [ -n "$SUDO" ]; then
    printf '%s\n' "$1" | $SUDO tee "$2" >/dev/null
  else
    printf '%s\n' "$1" > "$2"
  fi
}

# mv_self <cgroup dir>: move this shell (and so every child) into the dir.
# Plain write first; the sudo write only when the kernel refuses a user.
mv_self() {
  { echo "$$" > "$1/cgroup.procs"; } 2>/dev/null && return 0
  [ -n "$SUDO" ] && echo "$$" | $SUDO tee "$1/cgroup.procs" >/dev/null
}

cleanup() {
  [ "$MOVED" = 1 ] && mv_self "$CG$ORIG"
  [ "$CREATED" = 1 ] || return 0
  find "$BASE" -mindepth 1 -depth -type d -exec rmdir {} + 2>/dev/null
  $SUDO rmdir "$BASE" 2>/dev/null
  rm -f "$LOG"
}
trap cleanup EXIT

echo "== governor-cgroup-check: base=$BASE sudo=${SUDO:-no} kernel=$(uname -r)"

# ---- 1. build the layout ---------------------------------------------------

for c in memory pids; do
  grep -qw "$c" "$PARENT/cgroup.subtree_control" \
    || cgw "+$c" "$PARENT/cgroup.subtree_control" \
    || envfail "cannot delegate $c in $PARENT"
done
$SUDO mkdir "$BASE" || envfail "mkdir $BASE failed"
CREATED=1
cgw "+memory +pids" "$BASE/cgroup.subtree_control" || envfail "cannot delegate memory and pids in $BASE"
[ -z "$SUDO" ] || sudo chown -R "$(id -u):$(id -g)" "$BASE" || envfail "chown $BASE failed"
mkdir "$LEAF" || envfail "mkdir $LEAF failed as $(id -un): $BASE is not owned by us"
mv_self "$LEAF" || envfail "cannot move into $LEAF"
MOVED=1

own="$(sed -n 's/^0:://p' /proc/self/cgroup)"
if [ "$own" = "${LEAF#"$CG"}" ]; then
  pass "process cgroup is $own (parent $BASE)"
else
  fail "process cgroup is '$own', expected ${LEAF#"$CG"}"
fi
for c in memory pids; do
  if grep -qw "$c" "$BASE/cgroup.controllers"; then
    pass "$c listed in $BASE/cgroup.controllers"
  else
    fail "$c missing from $BASE/cgroup.controllers: $(cat "$BASE/cgroup.controllers")"
  fi
done
[ "$FAILS" -eq 0 ] || { echo "== governor-cgroup-check: layout not usable, not running tests"; exit 1; }

# ---- 2. governor tests, then the ignored sweep test ---------------------------
# --success-output immediate: the mode lines come from PASSING tests.

cd "$REPO_ROOT" || envfail "cannot cd to $REPO_ROOT"
cargo nextest run -p terminal-commander-probes -p terminal-commanderd \
  -E 'binary(governor_limits) | binary(governor_policy)' \
  --no-fail-fast --success-output immediate --color never 2>&1 | tee "$LOG"
rc=${PIPESTATUS[0]}
[ "$rc" -eq 0 ] && pass "governor tests exited 0" || fail "governor tests exited $rc"

# Run alone, by design: it rmdirs dead daemons' dirs in the shared parent.
cargo nextest run -p terminal-commander-probes -E 'test(sweep_removes_stale_dirs)' \
  --run-ignored ignored-only --no-fail-fast --success-output immediate --color never 2>&1 | tee -a "$LOG"
rc=${PIPESTATUS[0]}
[ "$rc" -eq 0 ] && pass "ignored sweep test exited 0" || fail "ignored sweep test exited $rc"

# ---- 3. cgroup mode must have really run -----------------------------------------

# Lines the tests print only when they ran (or fell back to) a given mode.
for want in \
  'linux governor mode: cgroup' \
  'available_mode: Cgroup' \
  'install_host_ceiling Ok(Cgroup)' \
  'orphan job: ok=true' \
  'unix governor mode: cgroup (ceiling asserted)' \
  'mode=Some(Cgroup)' \
  'mode_available=Cgroup' \
  'swept '; do
  if grep -qF -- "$want" "$LOG"; then pass "saw '$want'"; else fail "never saw '$want'"; fi
done
# Lines that mean a test took the rlimit lane or skipped its cgroup work.
rlimit_lane='linux governor mode: rlimit|unix governor mode: [^c]|unix pty governor mode:|host_ceiling_mode unavailable|rlimit host:|available_mode: Rlimit|mode_available=(Rlimit|Unavailable)'
if hits="$(grep -nE -- "$rlimit_lane" "$LOG")"; then
  fail "a governor test left cgroup mode:"
  printf '%s\n' "$hits" | sed 's/^/      /'
else
  pass "no governor test reported rlimit, unavailable or a skip"
fi

# ---- 4. nothing may be left behind --------------------------------------------------

left="$(find "$BASE" -mindepth 1 -maxdepth 1 -name 'tc-job*' 2>/dev/null)"
if [ -z "$left" ]; then
  pass "no tc-job* cgroup dirs remain under $BASE"
else
  fail "tc-job* cgroup dirs remain under $BASE:"
  printf '%s\n' "$left" | sed 's/^/      /'
fi

echo "== governor-cgroup-check: $FAILS failure(s)"
[ "$FAILS" -eq 0 ]
