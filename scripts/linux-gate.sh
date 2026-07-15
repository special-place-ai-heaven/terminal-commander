#!/usr/bin/env bash
# scripts/linux-gate.sh — the linux pre-build-gates gate, identical to CI.
# CI (npm-binary-build.yml pre-build-gates) INVOKES this script, so it IS the
# gate; running it locally (natively on linux/mac, or via WSL on Windows) is a
# faithful pre-push check. Fails loud on a missing tool — never skips to green.
set -euo pipefail
export CARGO_TERM_COLOR=always
export CARGO_TARGET_DIR="${TC_LINUX_TARGET:-$HOME/tc-linux-target}"

require() { command -v "$1" >/dev/null 2>&1 || { echo "tc-gate: missing '$1' — $2" >&2; exit 127; }; }
require cargo         "install rustup + the pinned toolchain"
require node          "verify-optional-dependencies.js needs node"
require cargo-nextest "cargo install cargo-nextest"
# python3: the TC47 load test SELF-SKIPS (and a skipped #[test] reports PASS)
# when python3 is absent. The load test probes /usr/bin|/usr/local/bin|/bin for
# python3, so a bare PATH 'python3' is the right precheck. This REDUCES the
# false-skip; Step-2 asserts the load tests actually executed (the real guard).
require python3       "the TC47 load gate self-skips to a false pass without python3"

# Toolchain fidelity: parse [toolchain].channel from rust-toolchain.toml and
# require the active rustc to match it (so a distro 'cargo' on PATH cannot run a
# different toolchain whose clippy diverges from CI).
# tr -d '\r': when run via WSL against a Windows working tree (/mnt/...), the
# checked-out rust-toolchain.toml may be CRLF, so the captured channel would
# carry a trailing \r and never match the (LF) `rustc --version` output. On
# CI/native-linux the file is LF and this is a no-op.
exp="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' rust-toolchain.toml | tr -d '\r')"
[ -n "$exp" ] || { echo "tc-gate: could not read channel from rust-toolchain.toml" >&2; exit 1; }
rustc --version | grep -q "$exp" || { echo "tc-gate: rustc != pinned $exp (is rustup the active cargo?)" >&2; exit 1; }

# Pin-consistency (AC8): CI pins the toolchain via the npm-binary-build.yml
# RUST_TOOLCHAIN env (NOT from this toml), so the two pins can silently drift.
# Assert they agree so a bump to one without the other fails fast here.
wf_pin="$(grep -E 'RUST_TOOLCHAIN:' .github/workflows/npm-binary-build.yml | head -1 | sed -E 's/.*"([0-9.]+)".*/\1/' | tr -d '\r')"
[ "$wf_pin" = "$exp" ] || { echo "tc-gate: workflow RUST_TOOLCHAIN ($wf_pin) != rust-toolchain.toml channel ($exp)" >&2; exit 1; }

echo "== verify-optional-dependencies =="; node scripts/release/verify-optional-dependencies.js
echo "== verify crate release-note synthesis =="; bash scripts/release/test-synthesize-crates-release-trigger.sh
echo "== fmt ==";    cargo fmt --all --check
echo "== clippy =="; cargo clippy --workspace --all-targets -- -D warnings
echo "== nextest =="; cargo nextest run --workspace
echo "== TC47 load gate =="
# Assert the load tests EXECUTED (no python3 self-skip slipping through). nextest
# prints a summary line "Summary [..] N tests run: N passed"; require N>0 and no skip line.
# Use `cargo test` (matches the CI load-gate step exactly); parse "running N tests".
# The self-skip is an early-return that STILL counts as a passed test, so the
# eprintln "skipping: python3" line on stderr is the authoritative skip detector.
load_out="$(cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture 2>&1)"; echo "$load_out"
echo "$load_out" | grep -qiE "skipping: python3" && { echo "tc-gate: load gate SELF-SKIPPED (python3) — false pass refused" >&2; exit 1; }
echo "$load_out" | grep -qE "running [1-9][0-9]* test" || { echo "tc-gate: load gate ran 0 tests — refusing false pass" >&2; exit 1; }

echo "== MCP guard 1 (no spawn/socket in crates/mcp/src) =="
out="$(grep -RE "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp/src || true)"
echo "$out"
if echo "$out" | grep -E "^crates/mcp/src/[^:]+:[[:space:]]*(let|use|fn|pub|impl|let mut)" >/dev/null; then
  echo "tc-gate: MCP guard 1 — non-doc match in production source" >&2; exit 1
fi
echo "== MCP guard 2 (no direct fs in crates/mcp/src) =="
if grep -RE "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src; then
  echo "tc-gate: MCP guard 2 — direct-fs path" >&2; exit 1
fi
echo "tc-gate: linux gate PASSED"
