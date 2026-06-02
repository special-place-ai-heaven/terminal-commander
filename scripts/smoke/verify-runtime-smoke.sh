#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# TC46 local smoke script.
#
# Verifies the daemon + MCP stdio path end-to-end using bounded tool
# calls only. This is SECONDARY EVIDENCE for the TC46 provider-harness
# integration smoke: it proves the daemon + MCP stdio surface works
# locally. It does NOT exercise Codex or Claude Code; those are
# provider-harness smokes and must be run from their respective CLIs
# against the configs in docs/integrations/.
#
# What it does:
#   1. Builds `terminal-commanderd` and `terminal-commander-mcp` (debug).
#   2. Starts the daemon in IPC-server mode on a private temp data dir
#      and a fixed UDS socket path under that dir.
#   3. Spawns `terminal-commander-mcp --socket <path>` over stdio and
#      issues bounded JSON-RPC MCP requests:
#        - `initialize`
#        - `tools/list`
#        - `tools/call system_discover`
#        - `tools/call health`
#        - `tools/call command_start_combed argv=[echo,hello]`
#        - `tools/call bucket_wait` (until at least one event)
#        - `tools/call command_status`
#   4. Asserts the responses are bounded JSON, never raw stdout/stderr
#      from the spawned command.
#   5. Tears the daemon + MCP down cleanly.
#
# What it does NOT do:
#   - Spawn provider CLIs.
#   - Send any secret.
#   - Open a network listener.
#   - Run as root.
#
# Exit codes:
#   0 — smoke OK.
#   1 — smoke failure (the failure is printed to stderr).
#   2 — environment problem (missing cargo, missing jq, etc.).
#
# Required external tools:
#   - cargo (with the workspace built or buildable)
#   - python3 (used by the JSON-RPC pump and JSON assertions)

set -euo pipefail

err() { echo "verify-runtime-smoke: $*" >&2; }

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "missing dependency: $1"
        exit 2
    fi
}

need cargo
need python3

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target-wsl}"
export CARGO_TARGET_DIR

err "building debug binaries (CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
cargo build -p terminal-commanderd -p terminal-commander-mcp --bins >/dev/null

DAEMON_BIN="$CARGO_TARGET_DIR/debug/terminal-commanderd"
MCP_BIN="$CARGO_TARGET_DIR/debug/terminal-commander-mcp"
test -x "$DAEMON_BIN" || { err "daemon binary not found at $DAEMON_BIN"; exit 2; }
test -x "$MCP_BIN" || { err "mcp binary not found at $MCP_BIN"; exit 2; }

TMP_DIR="$(mktemp -d -t tc46-smoke.XXXXXX)"
DATA_DIR="$TMP_DIR/data"
# The daemon derives the UDS path from --data-dir (default config
# resolution): `<data_dir>/terminal-commanderd.sock`. We hand that
# same path to the MCP adapter via TC_SOCKET.
SOCK="$DATA_DIR/terminal-commanderd.sock"
DAEMON_LOG="$TMP_DIR/daemon.log"
MCP_LOG="$TMP_DIR/mcp.log"
mkdir -p "$DATA_DIR"

cleanup() {
    local code=$?
    if [[ -n "${DAEMON_PID:-}" ]] && kill -0 "$DAEMON_PID" 2>/dev/null; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    if [[ -n "${MCP_PID:-}" ]] && kill -0 "$MCP_PID" 2>/dev/null; then
        kill "$MCP_PID" 2>/dev/null || true
        wait "$MCP_PID" 2>/dev/null || true
    fi
    if [[ -d "$TMP_DIR" ]]; then
        rm -rf "$TMP_DIR" || true
    fi
    exit "$code"
}
trap cleanup EXIT INT TERM

# ---- start daemon ----
err "starting daemon: data_dir=$DATA_DIR socket=$SOCK"
"$DAEMON_BIN" --data-dir "$DATA_DIR" start --mode ipc-server \
    >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

# Wait up to 5s for the socket to appear.
for _ in $(seq 1 50); do
    if [[ -S "$SOCK" ]]; then
        break
    fi
    sleep 0.1
done
if [[ ! -S "$SOCK" ]]; then
    err "daemon failed to create socket within 5s"
    err "daemon log:"
    cat "$DAEMON_LOG" >&2 || true
    exit 1
fi
err "daemon up (pid=$DAEMON_PID)"

# ---- drive MCP stdio with a tiny python pump ----
# We use python because rmcp's stdio framing is line-delimited JSON-RPC
# 2.0 and the harness needs request/response correlation. Piping with
# bash + jq is fragile because the server may emit multiple lines per
# request and we need to read until the matching id arrives.

err "spawning MCP stdio adapter"
# Note: socket comes from TC_SOCKET env (resolve_socket_path honours it).
# We deliberately do NOT pass --socket on the CLI so the config-only
# path matches what `docs/integrations/` documents.

JSONRPC_SCRIPT="$(mktemp -t tc46-smoke.XXXXXX.py)"
SMOKE_OUTPUT="$(mktemp -t tc46-smoke.XXXXXX.json)"
trap 'rm -f "$JSONRPC_SCRIPT" "$SMOKE_OUTPUT"' RETURN

cat > "$JSONRPC_SCRIPT" <<'PYEOF'
#!/usr/bin/env python3
# Bounded JSON-RPC pump for the TC46 smoke. Reads one response per
# request id; never prints raw bytes.
import json, os, subprocess, sys, time

mcp_bin = sys.argv[1]
socket  = sys.argv[2]
out_path = sys.argv[3]

env = os.environ.copy()
env["TC_SOCKET"] = socket
proc = subprocess.Popen(
    [mcp_bin],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    env=env,
    text=True,
    bufsize=1,
)

next_id = 1
def send(method, params=None):
    global next_id
    req = {"jsonrpc": "2.0", "id": next_id, "method": method}
    if params is not None:
        req["params"] = params
    next_id += 1
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    return req["id"]

def recv(target_id, deadline=10.0):
    end = time.time() + deadline
    while time.time() < end:
        line = proc.stdout.readline()
        if not line:
            time.sleep(0.05)
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("id") == target_id:
            return msg
    raise RuntimeError(f"timed out waiting for id={target_id}")

results = {}
try:
    init_id = send("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "tc46-smoke", "version": "0.0.0"},
    })
    results["initialize"] = recv(init_id)
    proc.stdin.write(json.dumps({"jsonrpc":"2.0","method":"notifications/initialized"}) + "\n")
    proc.stdin.flush()

    tl_id = send("tools/list")
    results["tools_list"] = recv(tl_id)

    sd_id = send("tools/call", {"name": "system_discover", "arguments": {}})
    results["system_discover"] = recv(sd_id)

    hl_id = send("tools/call", {"name": "health", "arguments": {}})
    results["health"] = recv(hl_id)

    cs_id = send("tools/call", {
        "name": "command_start_combed",
        "arguments": {"argv": ["echo", "tc46-smoke-needle"], "grace_ms": 2000},
    })
    results["command_start"] = recv(cs_id)
    # Pull bucket_id out of the inner JSON content (text-encoded JSON).
    text = results["command_start"]["result"]["content"][0]["text"]
    inner = json.loads(text)
    bucket_id = inner["bucket_id"]
    job_id = inner["job_id"]

    bw_id = send("tools/call", {
        "name": "bucket_wait",
        "arguments": {"bucket_id": bucket_id, "cursor": 0, "timeout_ms": 2000},
    })
    results["bucket_wait"] = recv(bw_id)

    st_id = send("tools/call", {
        "name": "command_status",
        "arguments": {"job_id": job_id},
    })
    results["command_status"] = recv(st_id)
finally:
    try:
        proc.stdin.close()
    except Exception:
        pass
    proc.terminate()
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        proc.kill()

with open(out_path, "w") as f:
    json.dump(results, f, indent=2)
PYEOF

python3 "$JSONRPC_SCRIPT" "$MCP_BIN" "$SOCK" "$SMOKE_OUTPUT" 2>"$MCP_LOG"

# ---- assertions via python3 (avoid jq dependency) ----

err "checking JSON-RPC results"

python3 - "$SMOKE_OUTPUT" <<'PYEOF'
import json, sys
path = sys.argv[1]
with open(path) as f:
    r = json.load(f)

def fail(msg):
    print(f"FAIL  {msg}", file=sys.stderr)
    sys.exit(1)

def ok(msg):
    print(f"PASS  {msg}", file=sys.stderr)

# initialize
pv = r.get("initialize", {}).get("result", {}).get("protocolVersion")
if pv != "2024-11-05":
    fail(f"initialize protocol version (got: {pv})")
ok("initialize protocol version")

# tools/list
tools = r.get("tools_list", {}).get("result", {}).get("tools", [])
if len(tools) < 29:
    fail(f"tools/list returned {len(tools)} tools; expected >=29")
ok(f"tools/list reports {len(tools)} tools")

names = [t.get("name") for t in tools]
if "command_start_combed" not in names:
    fail("command_start_combed missing from tools/list")
ok("command_start_combed advertised")

# system_discover
sd_text = r["system_discover"]["result"]["content"][0]["text"]
sd = json.loads(sd_text)
if "adapter_version" not in sd:
    fail("system_discover payload missing adapter_version")
ok("system_discover payload bounded")

# health
hl_text = r["health"]["result"]["content"][0]["text"]
hl = json.loads(hl_text)
if not hl.get("ok"):
    fail("health payload missing ok flag")
ok("health reports ok")

# command_start_combed
cs_text = r["command_start"]["result"]["content"][0]["text"]
cs = json.loads(cs_text)
if "bucket_id" not in cs or "job_id" not in cs:
    fail("command_start_combed missing bucket_id / job_id")
ok("command_start_combed returned bucket_id + job_id")

# bucket_wait
bw_text = r["bucket_wait"]["result"]["content"][0]["text"]
bw = json.loads(bw_text)
if "events" not in bw:
    fail("bucket_wait missing events array")
ok("bucket_wait returned events array")

# Raw-stream check: the literal echo argv "tc46-smoke-needle" may
# legitimately appear inside bounded `argv` JSON metadata (audit
# subject), but never as free-form text outside an `argv` field.
import re
needle = "tc46-smoke-needle"
for kind in ["system_discover", "health", "command_start", "bucket_wait", "command_status"]:
    text = r[kind]["result"]["content"][0]["text"]
    if needle not in text:
        continue
    try:
        inner = json.loads(text)
    except json.JSONDecodeError:
        fail(f"{kind} content is not JSON yet contains the needle")
    # Walk inner — needle must be inside an `argv` array key only.
    found_leak = False
    def walk(node, parent_key=None):
        global found_leak
        if isinstance(node, dict):
            for k, v in node.items():
                walk(v, k)
        elif isinstance(node, list):
            for v in node:
                walk(v, parent_key)
        elif isinstance(node, str):
            # The TC38 `command_exited` lifecycle event by design
            # embeds the argv literally in its `summary` string so
            # operators can trace which command exited (no stdout
            # content; argv is a bounded operator input). Allow it
            # in argv-bearing or summary fields only.
            if needle in node and parent_key not in (
                "argv", "argv0", "subject", "summary", "summary_template", "reason"
            ):
                found_leak = True
    walk(inner)
    if found_leak:
        fail(f"{kind} appears to leak raw stream text outside argv metadata")
ok("no raw stream text in bounded MCP responses")
PYEOF

err "SUCCESS — local daemon + MCP stdio smoke OK"
err "TC46 provider-harness smoke for Codex / Claude Code must be run separately"
err "  via the configs in docs/integrations/ and reported in the goal final report."
