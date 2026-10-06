#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Real Linux install path of the npm wrapper, end to end (CI job
# `install-path-linux` in .github/workflows/npm-binary-build.yml).
#
# Unit tests, both gates and the runtime smoke all run the binaries from the
# build tree. This script installs them the way a user does: packed tarballs,
# `npm install -g` into ~/.npm-global, the postinstall bootstrap, the profile
# hook autostart, real login / non-interactive / interactive shells and the
# installed MCP adapter, all in a throwaway HOME.
#
# Usage: scripts/ci/install-path-check.sh <dir holding terminal-commanderd,
#        terminal-commander-mcp and terminal-commander (linux x64 builds)>
#
# Isolation: every product command runs under `env -i` with only HOME (a new
# temp dir), USER, LOGNAME, LANG, SHELL, TERM and PATH. No TC_*, no XDG_*, no
# CI variables (so the postinstall runs as on a user machine), no secrets.
# PATH puts $HOME/.npm-global/bin first and a fake systemctl that always fails
# ahead of the system one, so autostart takes the profile-hook mode and never
# touches a real systemd user manager. The wrapper's optionalDependencies are
# rewritten to the local linux-x64 tarball only (`file:`), so the install
# needs no registry.
#
# Every assertion prints PASS or FAIL with the decisive value. Exit 0 only
# when nothing failed; 2 for an environment problem. Values of environment
# variables are never printed, only names.
set -uo pipefail

BIN_SRC="${1:-}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARIES=(terminal-commanderd terminal-commander-mcp terminal-commander)
STD_PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

envfail() { echo "install-path-check: $*" >&2; exit 2; }

[ -n "$BIN_SRC" ] || envfail "usage: install-path-check.sh <binary dir>"
BIN_SRC="$(cd "$BIN_SRC" 2>/dev/null && pwd)" || envfail "binary dir not found: $1"
for b in "${BINARIES[@]}"; do
  [ -x "$BIN_SRC/$b" ] || envfail "missing executable $BIN_SRC/$b"
done
for t in node npm python3 script setsid; do
  command -v "$t" >/dev/null 2>&1 || envfail "missing tool: $t"
done
# autostart.sh runs the npm shims (#!/usr/bin/env node) with its own fixed
# PATH; a node outside it would make the autostart a silent no-op here.
env -i PATH="$STD_PATH" sh -c 'command -v node' >/dev/null 2>&1 \
  || envfail "node is not on the system PATH ($STD_PATH) that autostart.sh uses"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tc-install-path.XXXXXX")"
H="$WORK/home"
mkdir -p "$H" "$WORK/fakebin" "$WORK/stage" "$WORK/tarballs" "$WORK/final"
DATA_DIR="$H/.local/share/terminal-commanderd"
SOCK="$DATA_DIR/terminal-commanderd.sock"
PIDFILE="$DATA_DIR/terminal-commanderd.pid"
TEST_PATH="$H/.npm-global/bin:$WORK/fakebin:$STD_PATH"
FAILS=0

pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*"; FAILS=$((FAILS + 1)); }

# Run a command in the isolated user environment.
tenv() {
  env -i HOME="$H" USER="$(id -un)" LOGNAME="$(id -un)" LANG=C.UTF-8 \
    SHELL=/bin/bash TERM=dumb PATH="$TEST_PATH" "$@"
}

# Native daemon processes (argv0 basename terminal-commanderd, not the node
# shim) whose --data-dir lies under this HOME.
daemon_pids() {
  local p i
  local -a args
  for p in /proc/[0-9]*; do
    mapfile -d '' -t args 2>/dev/null < "$p/cmdline" || continue
    [ "${#args[@]}" -gt 1 ] || continue
    [ "${args[0]##*/}" = terminal-commanderd ] || continue
    for ((i = 1; i < ${#args[@]} - 1; i++)); do
      if [ "${args[i]}" = --data-dir ] && [[ "${args[i + 1]}" == "$H"/* ]]; then
        echo "${p#/proc/}"
        break
      fi
    done
  done
}

daemon_count() { daemon_pids | wc -l | tr -d ' '; }

# Poll until the daemon count equals $1, for at most $2 seconds.
wait_for_count() {
  local want="$1" deadline=$((SECONDS + $2))
  while [ "$(daemon_count)" != "$want" ]; do
    [ "$SECONDS" -lt "$deadline" ] || return 1
    sleep 0.5
  done
}

# Processes that reference the temp tree: argv mentions it, or HOME is it.
procs_referencing_home() {
  local p pid
  for p in /proc/[0-9]*; do
    pid="${p#/proc/}"
    [ "$pid" = "$$" ] && continue
    [ "$pid" = "$BASHPID" ] && continue
    if grep -qzF -- "$WORK" "$p/cmdline" 2>/dev/null \
      || grep -qzxF -- "HOME=$H" "$p/environ" 2>/dev/null; then
      echo "$pid"
    fi
  done
}

describe_pids() { # pid list -> "pid:argv0-basename ..."
  local pid a0 out=""
  for pid in "$@"; do
    a0="$(tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | head -1)"
    out+="${pid}:${a0##*/} "
  done
  echo "${out% }"
}

cleanup() {
  local pids
  pids="$(procs_referencing_home)"
  # shellcheck disable=SC2086
  [ -z "$pids" ] || kill -9 $pids 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "== install-path-check: HOME=$H"
echo "   node $(node --version), npm $(npm --version), $(script --version 2>&1 | head -1)"

# ---- a. pack and install ----------------------------------------------------

printf '#!/bin/sh\necho "install-path-check: fake systemctl (no systemd user manager)" >&2\nexit 1\n' \
  > "$WORK/fakebin/systemctl"
chmod 755 "$WORK/fakebin/systemctl"
printf 'prefix=%s\n' "$H/.npm-global" > "$H/.npmrc"

plat="$WORK/stage/linux-x64"
cp -R "$REPO_ROOT/packages/terminal-commander-linux-x64" "$plat"
rm -f "$plat/bin/"*.placeholder "$plat/bin/.gitkeep"
for b in "${BINARIES[@]}"; do
  cp "$BIN_SRC/$b" "$plat/bin/$b"
  chmod 755 "$plat/bin/$b"
done
(cd "$plat" && tenv npm pack --silent --pack-destination "$WORK/tarballs" >/dev/null) \
  || envfail "npm pack of the linux-x64 platform package failed"
(cd "$REPO_ROOT/packages/terminal-commander" \
  && tenv npm pack --silent --pack-destination "$WORK/tarballs" >/dev/null) \
  || envfail "npm pack of the wrapper failed"
plat_tgz="$(ls "$WORK"/tarballs/terminal-commander-linux-x64-*.tgz)"
wrap_tgz="$(ls "$WORK"/tarballs/terminal-commander-[0-9]*.tgz)"

mkdir -p "$WORK/stage/wrapper"
tar -xzf "$wrap_tgz" -C "$WORK/stage/wrapper"
node -e '
  const fs = require("fs");
  const [file, tgz] = process.argv.slice(1);
  const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
  pkg.optionalDependencies = { "@terminal-commander/linux-x64": "file:" + tgz };
  fs.writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
' "$WORK/stage/wrapper/package/package.json" "$plat_tgz"
(cd "$WORK/stage/wrapper/package" && tenv npm pack --silent --pack-destination "$WORK/final" >/dev/null) \
  || envfail "repack of the wrapper failed"
final_tgz="$(ls "$WORK"/final/terminal-commander-*.tgz)"
wrapper_version="$(node -p 'require(process.argv[1]).version' "$WORK/stage/wrapper/package/package.json")"
echo "   packed $(basename "$plat_tgz") + $(basename "$final_tgz") (wrapper $wrapper_version)"

(cd "$H" && tenv npm install -g --no-audit --no-fund --offline "$final_tgz") > "$WORK/npm-install.log" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then
  pass "npm install -g of the packed wrapper exited 0"
else
  fail "npm install -g exited $rc; log tail:"
  tail -20 "$WORK/npm-install.log" | sed 's/^/      /'
fi

resolved="$(tenv sh -c 'command -v terminal-commander' 2>/dev/null)"
if [ "$resolved" = "$H/.npm-global/bin/terminal-commander" ]; then
  pass "terminal-commander resolves to ~/.npm-global/bin"
else
  fail "terminal-commander resolves to '${resolved:-nothing}'"
fi

setup_json="$H/.local/state/terminal-commander/setup.json"
mode="$(node -p 'require(process.argv[1]).bootstrap_mode' "$setup_json" 2>/dev/null)"
if [ "$mode" = "install" ]; then
  pass "postinstall ran: setup.json bootstrap_mode=$mode"
else
  fail "postinstall evidence missing: setup.json bootstrap_mode='${mode:-absent}'"
fi

# ---- b. setup daemon-autostart -----------------------------------------------

out="$(tenv terminal-commander setup daemon-autostart </dev/null 2>&1)"
rc=$?
if [ "$rc" -eq 0 ]; then
  pass "setup daemon-autostart exited 0: $(printf '%s' "$out" | head -1)"
else
  fail "setup daemon-autostart exited $rc: $out"
fi

autostart="$H/.config/terminal-commander/autostart.sh"
snippet="$H/.config/terminal-commander/profile.d/terminal-commander.sh"
if [ -x "$autostart" ]; then pass "autostart.sh installed and executable"; else fail "autostart.sh missing or not executable"; fi
if [ -f "$snippet" ]; then pass "profile.d snippet installed"; else fail "profile.d snippet missing"; fi
if grep -qF "trap '' HUP" "$snippet" 2>/dev/null; then
  pass "snippet contains trap '' HUP"
else
  fail "snippet lacks trap '' HUP"
fi
for rcfile in .profile .bashrc .zshrc; do
  begins="$(grep -cxF '# terminal-commander autostart BEGIN' "$H/$rcfile" 2>/dev/null)"
  ends="$(grep -cxF '# terminal-commander autostart END' "$H/$rcfile" 2>/dev/null)"
  if [ "${begins:-0}" = 1 ] && [ "${ends:-0}" = 1 ]; then
    pass "$rcfile has exactly one managed autostart block"
  else
    fail "$rcfile managed block BEGIN=${begins:-0} END=${ends:-0}"
  fi
done

# postinstall and setup each ran autostart.sh once; the second must have
# found the first daemon alive.
if wait_for_count 1 10; then
  pass "exactly one daemon after install + setup (pid $(daemon_pids))"
else
  # shellcheck disable=SC2046
  fail "daemons after install + setup: $(daemon_count) ($(describe_pids $(daemon_pids)))"
fi

# Start the shell checks from zero daemons.
out="$(tenv terminal-commander session reap --all </dev/null 2>&1)"
rc=$?
if [ "$rc" -eq 0 ] && wait_for_count 0 10; then
  pass "session reap --all stopped the daemon: $(printf '%s' "$out" | head -1)"
else
  fail "session reap --all exited $rc, daemons left $(daemon_count): $out"
  # shellcheck disable=SC2046
  kill -9 $(daemon_pids) 2>/dev/null || true
fi

# ---- c. shell behaviour ------------------------------------------------------

out="$(tenv bash -l -c 'echo LOGIN_OK' </dev/null 2>&1)"
rc=$?
sleep 4
n="$(daemon_count)"
if [ "$rc" -eq 0 ] && [ "$out" = LOGIN_OK ] && [ "$n" = 0 ]; then
  pass "bash -l -c: printed LOGIN_OK, exit 0, daemons 0"
else
  fail "bash -l -c: exit $rc, output '$out', daemons $n"
fi

out="$(tenv bash -c 'echo NI_OK' </dev/null 2>&1)"
rc=$?
sleep 4
n="$(daemon_count)"
if [ "$rc" -eq 0 ] && [ "$out" = NI_OK ] && [ "$n" = 0 ]; then
  pass "bash -c: printed NI_OK, exit 0, daemons 0"
else
  fail "bash -c: exit $rc, output '$out', daemons $n"
fi

# util-linux script(1) with stdin at EOF: the pty closes as soon as the
# shell exits, so the terminal hangs up on whatever the profile launched.
interactive_shell() {
  tenv script -qec "bash -i -c 'echo I_OK'" /dev/null </dev/null 2>&1
}

out="$(interactive_shell)"
rc=$?
sleep 10
n="$(daemon_count)"
first_pid="$(daemon_pids | head -1)"
if [ "$rc" -eq 0 ] && grep -q 'I_OK' <<<"$out" && [ "$n" = 1 ]; then
  pass "fast-closing interactive shell: printed I_OK, exit 0, 10 s later daemons 1 (pid $first_pid)"
else
  fail "fast-closing interactive shell: exit $rc, I_OK $(grep -c I_OK <<<"$out"), daemons $n"
fi

out="$(interactive_shell)"
rc=$?
sleep 5
n="$(daemon_count)"
pid_now="$(daemon_pids | head -1)"
if [ "$rc" -eq 0 ] && [ "$n" = 1 ] && [ "$pid_now" = "$first_pid" ]; then
  pass "second interactive shell: daemons still 1, same pid $pid_now"
else
  fail "second interactive shell: exit $rc, daemons $n, pid '$pid_now' (was '$first_pid')"
fi

if [ -n "$first_pid" ]; then
  kill -9 "$first_pid" 2>/dev/null
  if wait_for_count 0 10; then
    pass "kill -9 of daemon $first_pid: daemons 0"
  else
    fail "daemons after kill -9: $(daemon_count)"
  fi
  if [ -S "$SOCK" ] && [ -f "$PIDFILE" ]; then
    pass "socket and pidfile left behind by the killed daemon"
  else
    fail "stale state not present: socket $([ -S "$SOCK" ] && echo yes || echo no), pidfile $([ -f "$PIDFILE" ] && echo yes || echo no)"
  fi
  out="$(tenv terminal-commander doctor daemon </dev/null 2>&1)"
  rc=$?
  if [ "$rc" -eq 0 ] && grep -q 'daemon_running: no' <<<"$out" \
    && grep -qF 'socket file present but no daemon answering' <<<"$out"; then
    pass "doctor daemon: 'daemon_running: no' with the stale-socket note"
  else
    fail "doctor daemon exit $rc: $(tr '\n' '|' <<<"$out")"
  fi

  out="$(interactive_shell)"
  rc=$?
  if wait_for_count 1 10; then
    sleep 2
    n="$(daemon_count)"
    pid_now="$(daemon_pids | head -1)"
    if [ "$rc" -eq 0 ] && [ "$n" = 1 ] && [ "$pid_now" != "$first_pid" ]; then
      pass "interactive shell over the stale socket: one new daemon (pid $pid_now) within 10 s"
    else
      fail "after stale-socket restart: exit $rc, daemons $n, pid '$pid_now'"
    fi
  else
    fail "no daemon within 10 s of an interactive shell over the stale socket (exit $rc, daemons $(daemon_count))"
  fi
else
  fail "no daemon pid to kill; stale-socket checks skipped"
fi

# ---- d. installed MCP adapter --------------------------------------------------

cat > "$WORK/mcp_check.py" <<'PYEOF'
import json, subprocess, sys, time

expected_version, stderr_path = sys.argv[1], sys.argv[2]
META = {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
    "io.modelcontextprotocol/clientCapabilities": {},
    "io.modelcontextprotocol/clientInfo": {"name": "install-path-check", "version": "0.0.0"},
}
proc = subprocess.Popen(["terminal-commander-mcp"], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, stderr=open(stderr_path, "w"),
                        text=True, bufsize=1)
next_id = 1

def call(method, params=None, deadline=30.0):
    global next_id
    rid = next_id
    next_id += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method,
                                 "params": {**(params or {}), "_meta": META}}) + "\n")
    proc.stdin.flush()
    end = time.time() + deadline
    while time.time() < end:
        line = proc.stdout.readline()
        if not line:
            if proc.poll() is not None:
                raise RuntimeError(f"adapter exited {proc.returncode}")
            time.sleep(0.05)
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("id") == rid:
            return msg
    raise RuntimeError(f"timed out waiting for {method}")

def tool(name, arguments, deadline=30.0):
    msg = call("tools/call", {"name": name, "arguments": arguments}, deadline)
    res = msg.get("result") or {}
    if "error" in msg or res.get("isError"):
        raise RuntimeError(f"{name} failed: {json.dumps(msg)[:300]}")
    return json.loads(res["content"][0]["text"])

def report(ok, text):
    print(("PASS  " if ok else "FAIL  ") + text)

try:
    sv = (call("server/discover").get("result") or {}).get("supportedVersions")
    report(isinstance(sv, list) and "2026-07-28" in sv,
           f"adapter server/discover supportedVersions={sv}")
    hl = tool("health", {})
    report(hl.get("ok") is True and hl.get("version") == expected_version,
           f"adapter health ok={hl.get('ok')} version={hl.get('version')} (wrapper {expected_version})")
    rw = tool("run_and_watch", {"argv": ["sh", "-c", "env"], "rules": [{"pattern": "^TC_"}],
                                "wait_until": "exit", "wait_ms": 20000, "max_signals": 100}, 40.0)
    lines = [s.get("summary", "") for s in rw.get("signals", [])]
    names = sorted({l.split("=", 1)[0] for l in lines})
    report(rw.get("exit_code") == 0, f"run_and_watch sh -c env exit_code={rw.get('exit_code')}")
    report("TC_DAEMON_CHILD=1" in lines, f"child env has TC_DAEMON_CHILD=1 (TC_ names seen: {names})")
    leaked = [n for n in ("TC_SOCKET", "TC_DATA") if n in names]
    report(not leaked, f"child env has no TC_SOCKET/TC_DATA (leaked: {leaked})")
except Exception as e:  # report, never a traceback with payloads
    report(False, f"adapter check aborted: {e}")
finally:
    try:
        proc.stdin.close()
    except Exception:
        pass
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
PYEOF

mcp_out="$(tenv python3 "$WORK/mcp_check.py" "$wrapper_version" "$WORK/mcp-stderr.log" 2>&1)"
printf '%s\n' "$mcp_out"
FAILS=$((FAILS + $(grep -c '^FAIL' <<<"$mcp_out")))
if ! grep -q '^PASS' <<<"$mcp_out"; then
  fail "adapter check produced no PASS lines"
fi

# ---- e. teardown -----------------------------------------------------------------

out="$(tenv terminal-commander session reap --all </dev/null 2>&1)"
rc=$?
deadline=$((SECONDS + 15))
left="$(procs_referencing_home)"
while [ -n "$left" ] && [ "$SECONDS" -lt "$deadline" ]; do
  sleep 0.5
  left="$(procs_referencing_home)"
done
if [ "$rc" -eq 0 ] && [ -z "$left" ]; then
  pass "session reap --all: zero processes reference the temp HOME"
else
  # shellcheck disable=SC2086
  fail "teardown: reap exit $rc, processes left: $(describe_pids $left)"
fi

echo "== install-path-check: $FAILS failure(s)"
[ "$FAILS" -eq 0 ]
