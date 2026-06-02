#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# NPM04 local-install smoke.
#
# What it does:
#
#   1. Builds the three Rust binaries for the current host
#      (linux-x64 OR linux-arm64) via `cargo build --release`.
#   2. Stages the binaries into a TEMP staging tree that mirrors the
#      committed `packages/terminal-commander-linux-<arch>/` layout —
#      WITHOUT touching the committed repo placeholders.
#   3. Runs `npm pack` against the host-matching staged platform
#      package + the committed root wrapper.
#   4. Installs both tarballs into a temporary `--prefix`.
#   5. Verifies the three commands resolve via the npm shims and
#      respond to bounded health/self-check calls — no provider
#      harness, no network listener.
#   6. Re-runs a bounded MCP stdio JSON-RPC pump against the
#      npm-installed `terminal-commander-mcp` to prove the shim
#      forwards through the daemon UDS.
#   7. Tears down the temp prefix + staging tree unconditionally.
#
# What it does NOT do:
#
#   - Publish to the npm registry.
#   - Spawn provider CLIs (Codex / Claude Code / Cursor).
#   - Commit built Rust binaries into the repo.
#   - Touch committed `packages/.../bin/*.placeholder` files.
#   - Run as root or modify the user's global npm prefix.
#   - Build linux-arm64 binaries on a linux-x64 host (host-arch only).
#
# Exit codes:
#   0 — smoke OK.
#   1 — smoke failure (printed to stderr).
#   2 — environment problem (missing cargo, missing npm, missing
#       python3, missing node, unsupported host arch).

set -euo pipefail

err() { echo "verify-npm-local-install: $*" >&2; }

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "missing dependency: $1"
        exit 2
    fi
}

need cargo
need npm
need node
need python3

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target-wsl}"
export CARGO_TARGET_DIR

host_kernel="$(uname -s)"
host_machine="$(uname -m)"
case "$host_kernel/$host_machine" in
    Linux/x86_64)  HOST_PLAT=linux-x64;   HOST_TARGET=x86_64-unknown-linux-gnu ;;
    Linux/aarch64) HOST_PLAT=linux-arm64; HOST_TARGET=aarch64-unknown-linux-gnu ;;
    *)
        err "unsupported host $host_kernel/$host_machine — local smoke only supports linux x86_64 and linux aarch64"
        exit 2
        ;;
esac
err "host platform = $HOST_PLAT (target $HOST_TARGET)"

TMP_ROOT="$(mktemp -d -t tc-npm04-smoke.XXXXXX)"
STAGE_DIR="$TMP_ROOT/stage"
TARBALL_DIR="$TMP_ROOT/tarballs"
PREFIX="$TMP_ROOT/prefix"
LOG_DIR="$TMP_ROOT/log"
mkdir -p "$STAGE_DIR" "$TARBALL_DIR" "$PREFIX" "$LOG_DIR"

DAEMON_PID=""
cleanup() {
    local code=$?
    if [[ -n "${DAEMON_PID:-}" ]] && kill -0 "$DAEMON_PID" 2>/dev/null; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    if [[ -d "$TMP_ROOT" ]]; then
        rm -rf "$TMP_ROOT" || true
    fi
    exit "$code"
}
trap cleanup EXIT INT TERM

err "temp root = $TMP_ROOT"

# ---- 1. Build release binaries ----
err "building release binaries via cargo (CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
cargo build --release \
    -p terminal-commanderd \
    -p terminal-commander-mcp \
    -p terminal-commander-cli \
    >"$LOG_DIR/cargo-build.log" 2>&1

for bin in terminal-commanderd terminal-commander-mcp terminal-commander; do
    src="$CARGO_TARGET_DIR/release/$bin"
    if [[ ! -x "$src" ]]; then
        err "expected binary not found: $src"
        cat "$LOG_DIR/cargo-build.log" >&2 || true
        exit 1
    fi
done
err "PASS  cargo build produced three binaries"

# ---- 2. Stage the host-matching platform package ----
STAGED_PLATFORM_PKG="$STAGE_DIR/terminal-commander-$HOST_PLAT"
mkdir -p "$STAGED_PLATFORM_PKG/bin"
cp "packages/terminal-commander-$HOST_PLAT/package.json" "$STAGED_PLATFORM_PKG/package.json"
cp "packages/terminal-commander-$HOST_PLAT/LICENSE"      "$STAGED_PLATFORM_PKG/LICENSE"
for bin in terminal-commanderd terminal-commander-mcp terminal-commander; do
    cp "$CARGO_TARGET_DIR/release/$bin" "$STAGED_PLATFORM_PKG/bin/$bin"
    chmod +x "$STAGED_PLATFORM_PKG/bin/$bin"
done
err "PASS  staged $STAGED_PLATFORM_PKG with real binaries"

STAGED_ROOT_PKG="$STAGE_DIR/terminal-commander"
mkdir -p "$STAGED_ROOT_PKG"
cp -R packages/terminal-commander/. "$STAGED_ROOT_PKG/"

# ---- 3. npm pack the staged packages ----
err "npm pack: staged platform package"
(
    cd "$STAGED_PLATFORM_PKG"
    npm pack --pack-destination "$TARBALL_DIR" >"$LOG_DIR/pack-platform.log" 2>&1
)
PLATFORM_TARBALL="$(ls "$TARBALL_DIR"/terminal-commander-"$HOST_PLAT"-*.tgz 2>/dev/null | head -n1)"
if [[ -z "$PLATFORM_TARBALL" || ! -f "$PLATFORM_TARBALL" ]]; then
    err "expected platform tarball not produced under $TARBALL_DIR"
    cat "$LOG_DIR/pack-platform.log" >&2 || true
    exit 1
fi
err "PASS  platform tarball = $PLATFORM_TARBALL"

err "npm pack: staged root wrapper"
(
    cd "$STAGED_ROOT_PKG"
    npm pack --pack-destination "$TARBALL_DIR" >"$LOG_DIR/pack-root.log" 2>&1
)
ROOT_TARBALL="$(ls "$TARBALL_DIR"/terminal-commander-*.tgz 2>/dev/null | grep -v "terminal-commander-$HOST_PLAT-" | head -n1)"
if [[ -z "$ROOT_TARBALL" || ! -f "$ROOT_TARBALL" ]]; then
    err "expected root tarball not produced under $TARBALL_DIR"
    cat "$LOG_DIR/pack-root.log" >&2 || true
    exit 1
fi
err "PASS  root tarball     = $ROOT_TARBALL"

# ---- 4. Install into a sandboxed --prefix ----
err "installing into sandbox prefix $PREFIX"
(
    cd "$PREFIX"
    npm install -g \
        --prefix "$PREFIX" \
        --no-audit --no-fund --no-save \
        "$PLATFORM_TARBALL" \
        "$ROOT_TARBALL" \
        >"$LOG_DIR/npm-install.log" 2>&1
)
err "PASS  npm install -g into $PREFIX"

# ---- 5. Verify installed command shims ----
PREFIX_BIN="$PREFIX/bin"
for cmd in terminal-commanderd terminal-commander-mcp terminal-commander; do
    if [[ ! -x "$PREFIX_BIN/$cmd" ]]; then
        err "expected installed command missing or not executable: $PREFIX_BIN/$cmd"
        ls -la "$PREFIX_BIN" >&2 || true
        cat "$LOG_DIR/npm-install.log" >&2 || true
        exit 1
    fi
done
err "PASS  three commands present under $PREFIX_BIN"

export PATH="$PREFIX_BIN:$PATH"

for cmd in terminal-commanderd terminal-commander-mcp terminal-commander; do
    if ! "$cmd" --help >"$LOG_DIR/help-$cmd.log" 2>&1; then
        err "$cmd --help exited non-zero"
        head -20 "$LOG_DIR/help-$cmd.log" >&2 || true
        exit 1
    fi
done
err "PASS  --help responses bounded for all three commands"

DAEMON_DATA="$TMP_ROOT/daemon-data"
mkdir -p "$DAEMON_DATA"
if ! terminal-commanderd --data-dir "$DAEMON_DATA" check >"$LOG_DIR/daemon-check.log" 2>&1; then
    err "terminal-commanderd check failed"
    cat "$LOG_DIR/daemon-check.log" >&2 || true
    exit 1
fi
if ! grep -q "summary: .* 0 failures" "$LOG_DIR/daemon-check.log"; then
    err "daemon self-check did not report 0 failures"
    cat "$LOG_DIR/daemon-check.log" >&2 || true
    exit 1
fi
err "PASS  terminal-commanderd self-check (0 failures)"

# ---- 6. Bounded MCP stdio smoke through npm-installed binaries ----
DAEMON_DATA2="$TMP_ROOT/daemon-data2"
SOCK="$DAEMON_DATA2/terminal-commanderd.sock"
mkdir -p "$DAEMON_DATA2"

err "starting npm-installed daemon: data_dir=$DAEMON_DATA2 socket=$SOCK"
terminal-commanderd --data-dir "$DAEMON_DATA2" start --mode ipc-server \
    >"$LOG_DIR/daemon.log" 2>&1 &
DAEMON_PID=$!

for _ in $(seq 1 50); do
    if [[ -S "$SOCK" ]]; then
        break
    fi
    sleep 0.1
done
if [[ ! -S "$SOCK" ]]; then
    err "daemon did not create socket within 5s"
    cat "$LOG_DIR/daemon.log" >&2 || true
    exit 1
fi

JSONRPC_SCRIPT="$TMP_ROOT/jsonrpc.py"
SMOKE_OUT="$TMP_ROOT/npm-smoke-output.json"
cat > "$JSONRPC_SCRIPT" <<'PYEOF'
import json, os, subprocess, sys, time

mcp_bin  = sys.argv[1]
socket   = sys.argv[2]
out_path = sys.argv[3]

env = os.environ.copy()
env["TC_SOCKET"] = socket
proc = subprocess.Popen(
    [mcp_bin],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    env=env, text=True, bufsize=1,
)

nid = 1
def send(method, params=None):
    global nid
    req = {"jsonrpc":"2.0", "id":nid, "method":method}
    if params is not None:
        req["params"] = params
    nid += 1
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    return req["id"]

def recv(target_id, deadline=8.0):
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
        "clientInfo": {"name":"npm04-smoke", "version":"0.0.0"},
    })
    results["initialize"] = recv(init_id)
    proc.stdin.write(json.dumps({"jsonrpc":"2.0","method":"notifications/initialized"}) + "\n")
    proc.stdin.flush()

    tl_id = send("tools/list")
    results["tools_list"] = recv(tl_id)

    sd_id = send("tools/call", {"name":"system_discover", "arguments":{}})
    results["system_discover"] = recv(sd_id)

    hl_id = send("tools/call", {"name":"health", "arguments":{}})
    results["health"] = recv(hl_id)
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

python3 "$JSONRPC_SCRIPT" "$PREFIX_BIN/terminal-commander-mcp" "$SOCK" "$SMOKE_OUT" 2>"$LOG_DIR/mcp.log"

kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""

python3 - "$SMOKE_OUT" <<'PYEOF'
import json, sys
r = json.load(open(sys.argv[1]))

def fail(m):
    print(f"FAIL  {m}", file=sys.stderr)
    sys.exit(1)
def ok(m):
    print(f"PASS  {m}", file=sys.stderr)

pv = r.get("initialize", {}).get("result", {}).get("protocolVersion")
if pv != "2024-11-05":
    fail(f"initialize protocol version (got: {pv})")
ok("initialize protocol version")

tools = r.get("tools_list", {}).get("result", {}).get("tools", [])
if len(tools) < 29:
    fail(f"tools/list returned {len(tools)} tools; expected >=29")
ok(f"tools/list reports {len(tools)} tools")

sd_text = r["system_discover"]["result"]["content"][0]["text"]
sd = json.loads(sd_text)
if "adapter_version" not in sd:
    fail("system_discover payload missing adapter_version")
ok("system_discover payload bounded")

hl_text = r["health"]["result"]["content"][0]["text"]
hl = json.loads(hl_text)
if not hl.get("ok"):
    fail("health payload missing ok flag")
ok("health reports ok")
PYEOF

err "SUCCESS — local npm install + MCP stdio smoke OK (host $HOST_PLAT)"
err "arm64 cross-arch execution NOT covered; only $HOST_PLAT was actually run."
err "NPM05 GitHub Actions covers the cross-arch matrix."
