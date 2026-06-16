#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# US6 / T054 + T055: shared bash O-runner harness for the POSIX omni
# smokes (verify-omni-linux.sh, verify-omni-wsl.sh, verify-omni-macos.sh).
#
# This file is SOURCED, not executed. The caller sets OMNI_PROFILE (one
# of: linux, wsl, macos) and then calls `omni_main`. The harness:
#   1. Builds `terminal-commanderd` + `terminal-commander-mcp` (debug).
#   2. Writes a SMOKE-ONLY full-capability config (allow_shell +
#      allow_session) to a private temp data dir so the gated lanes (O-01
#      shell pipeline, O-02 session) can actually run. This config is
#      smoke-only and never leaves the temp dir.
#   3. Starts the daemon in ipc-server mode on a private UDS.
#   4. Invokes scripts/smoke/omni-o-runner.py with the platform profile,
#      which runs the RUNNABLE subset of O-01..O-14 and EXITS NON-ZERO on
#      any RAN gate failure. Blocked gates are SKIPPED-WITH-LOUD-NOTICE.
#   5. Tears the daemon down cleanly and propagates the runner exit code.
#
# Exit codes (propagated to the caller):
#   0 -- every RAN gate passed; skipped gates reported loudly.
#   1 -- a RAN gate FAILED (real omni regression).
#   2 -- environment/harness problem (missing cargo/python3, build fail).

set -euo pipefail

omni_err() { echo "verify-omni: $*" >&2; }

omni_need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        omni_err "missing dependency: $1"
        exit 2
    fi
}

# Resolve repo root from this file's location (scripts/smoke/).
OMNI_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OMNI_REPO_ROOT="$(cd "$OMNI_SCRIPT_DIR/../.." && pwd)"

# Cleanup state. Script-global (NOT function-local) so the EXIT trap can read
# them after omni_main returns, under `set -u`. The trap NEVER calls `exit` so
# it cannot clobber the real (runner) exit code -- the entry script propagates
# omni_main's return value itself.
OMNI_DAEMON_PID=""
OMNI_TMP_DIR=""

# shellcheck disable=SC2317  # invoked indirectly via trap; call site is hidden
omni_cleanup() {
    if [[ -n "$OMNI_DAEMON_PID" ]] && kill -0 "$OMNI_DAEMON_PID" 2>/dev/null; then
        kill "$OMNI_DAEMON_PID" 2>/dev/null || true
        wait "$OMNI_DAEMON_PID" 2>/dev/null || true
    fi
    if [[ -n "$OMNI_TMP_DIR" ]] && [[ -d "$OMNI_TMP_DIR" ]]; then
        rm -rf "$OMNI_TMP_DIR" || true
    fi
}
trap omni_cleanup EXIT INT TERM

omni_platform_preamble() {
    # On macOS, ensure Homebrew bin dirs are on PATH so a non-login
    # invocation still finds cargo / python3. No-op elsewhere.
    case "$(uname -s 2>/dev/null || echo unknown)" in
        Darwin)
            for brew_bin in /opt/homebrew/bin /usr/local/bin; do
                if [[ -d "$brew_bin" ]] && [[ ":$PATH:" != *":$brew_bin:"* ]]; then
                    PATH="$brew_bin:$PATH"
                fi
            done
            export PATH
            ;;
        *) : ;;
    esac
}

omni_main() {
    local profile="${OMNI_PROFILE:?OMNI_PROFILE must be set by the caller}"
    omni_platform_preamble
    omni_err "profile=$profile platform=$(uname -s 2>/dev/null || echo unknown) repo_root=$OMNI_REPO_ROOT"

    omni_need cargo
    omni_need python3

    cd "$OMNI_REPO_ROOT"

    local target_dir="${CARGO_TARGET_DIR:-target-wsl}"
    export CARGO_TARGET_DIR="$target_dir"

    omni_err "building debug binaries (CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
    cargo build -p terminal-commanderd -p terminal-commander-mcp --bins >/dev/null

    local daemon_bin="$CARGO_TARGET_DIR/debug/terminal-commanderd"
    local mcp_bin="$CARGO_TARGET_DIR/debug/terminal-commander-mcp"
    test -x "$daemon_bin" || { omni_err "daemon binary not found at $daemon_bin"; exit 2; }
    test -x "$mcp_bin" || { omni_err "mcp binary not found at $mcp_bin"; exit 2; }

    local tmp_dir data_dir sock daemon_log config
    tmp_dir="$(mktemp -d -t omni-smoke.XXXXXX)"
    OMNI_TMP_DIR="$tmp_dir"
    data_dir="$tmp_dir/data"
    sock="$data_dir/terminal-commanderd.sock"
    daemon_log="$tmp_dir/daemon.log"
    config="$tmp_dir/terminal-commanderd.toml"
    mkdir -p "$data_dir"

    # SMOKE-ONLY config: enable the gated capability lanes so O-01 (shell
    # pipeline) and O-02 (session) can run. allow_privileged stays FALSE --
    # the privileged helper is plan-only and the matrix must reflect that.
    cat > "$config" <<TOMLEOF
[daemon]
data_dir = "$data_dir"

[policy]
profile = "developer_local"
profile_version = "1"

[policy.caps]
allow_shell = true
allow_session = true
allow_privileged = false
allow_remote = false

[sifters]
universal_extractors = true
TOMLEOF

    local runner_rc=0

    omni_err "starting daemon: data_dir=$data_dir socket=$sock"
    "$daemon_bin" --config "$config" --data-dir "$data_dir" start --mode ipc-server \
        >"$daemon_log" 2>&1 &
    OMNI_DAEMON_PID=$!

    local waited=0
    while (( waited < 50 )); do
        [[ -S "$sock" ]] && break
        sleep 0.1
        waited=$(( waited + 1 ))
    done
    if [[ ! -S "$sock" ]]; then
        omni_err "daemon failed to create socket within 5s"
        omni_err "daemon log:"
        cat "$daemon_log" >&2 || true
        exit 2
    fi
    omni_err "daemon up (pid=$OMNI_DAEMON_PID)"

    omni_err "running O-01..O-14 sequence (profile=$profile)"
    set +e
    OMNI_REPO_ROOT="$OMNI_REPO_ROOT" \
        python3 "$OMNI_SCRIPT_DIR/omni-o-runner.py" "$mcp_bin" "$sock" "$profile"
    runner_rc=$?
    set -e

    omni_err "O-runner exit code: $runner_rc"
    return "$runner_rc"
}
