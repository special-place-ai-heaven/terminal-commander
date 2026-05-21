#!/usr/bin/env bash
#
# verify-baseline.sh
# ------------------
# Non-destructive baseline verification for Terminal Commander.
# Runs on WSL2 / Linux. No systemd, no network egress, no Windows tooling.
#
# This script is intentionally lightweight: it MUST pass even before
# any Rust code is committed. It verifies the doctrine files exist,
# the fixture directory layout is intact, every JSON fixture parses,
# and no fixture contains common secret patterns.
#
# Dependencies:
#   - bash (the shebang above)
#   - python3 (for `python3 -m json.tool`)
#   - git (for branch check)
#   - find, grep, awk (POSIX)
#
# Exit codes:
#   0 = all checks PASS
#   1 = one or more checks FAIL (each printed individually)
#   2 = required tool missing
#
# Source-status: live (this script is real verification, not a stub).

set -u
# Note: we DO NOT use `set -e`. We want every failed check to be
# reported, not just the first one. The script tracks a fail counter.

FAIL_COUNT=0

pass() { printf "PASS  %s\n" "$1"; }
fail() { printf "FAIL  %s\n" "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
info() { printf "INFO  %s\n" "$1"; }

require_tool() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf "MISSING  required tool: %s\n" "$tool" >&2
    exit 2
  fi
}

# ---- Tool checks ----------------------------------------------------
require_tool bash
require_tool python3
require_tool git
require_tool find
require_tool grep
require_tool awk

# ---- Resolve repo root ----------------------------------------------
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -z "${REPO_ROOT}" ] || [ ! -d "${REPO_ROOT}" ]; then
  echo "FAIL  not inside a git repository" >&2
  exit 1
fi
cd "${REPO_ROOT}"

# ---- 1. Branch guard ------------------------------------------------
EXPECTED_BRANCH="feature/terminal-commander-mvp"
CURRENT_BRANCH="$(git branch --show-current 2>/dev/null || true)"
if [ "${CURRENT_BRANCH}" = "${EXPECTED_BRANCH}" ]; then
  pass "branch=${CURRENT_BRANCH}"
else
  fail "branch is '${CURRENT_BRANCH}', expected '${EXPECTED_BRANCH}'"
fi

# ---- 2. Doctrine files exist ----------------------------------------
DOCTRINE_FILES=(
  "README.md"
  "LICENSE"
  "NOTICE"
  "CONTRIBUTING.md"
  "SECURITY.md"
  "POLICY.md"
  "docs/security/PRIVILEGE_MODEL.md"
  "SPEC.md"
  "ARCHITECTURE.md"
  "ROADMAP.md"
  "TESTING.md"
)
for f in "${DOCTRINE_FILES[@]}"; do
  if [ -f "${f}" ]; then
    pass "exists: ${f}"
  else
    fail "missing: ${f}"
  fi
done

# ---- 3. Fixture directory layout ------------------------------------
FIXTURE_DIRS=(
  "tests/fixtures"
  "tests/fixtures/terminal"
  "tests/fixtures/command-output"
  "tests/fixtures/files"
  "tests/fixtures/buckets"
  "tests/fixtures/rules"
  "tests/fixtures/context"
  "tests/fixtures/policy"
  "tests/fixtures/probes/wsl-mountinfo"
)
for d in "${FIXTURE_DIRS[@]}"; do
  if [ -d "${d}" ]; then
    pass "exists: ${d}/"
  else
    fail "missing dir: ${d}/"
  fi
done

# ---- 4. JSON fixtures parse with python3 ----------------------------
JSON_COUNT=0
JSON_FAIL=0
while IFS= read -r jf; do
  JSON_COUNT=$((JSON_COUNT + 1))
  if python3 -m json.tool "${jf}" >/dev/null 2>&1; then
    :
  else
    fail "json parse: ${jf}"
    JSON_FAIL=$((JSON_FAIL + 1))
  fi
done < <(find tests/fixtures -type f -name '*.json' 2>/dev/null | sort)
if [ "${JSON_COUNT}" -gt 0 ] && [ "${JSON_FAIL}" -eq 0 ]; then
  pass "json fixtures parse (${JSON_COUNT} files)"
elif [ "${JSON_COUNT}" -eq 0 ]; then
  info "json fixtures: 0 files (acceptable pre-TC05)"
fi

# ---- 5. Secret-leak grep on fixtures --------------------------------
# Patterns are conservative (high signal, low false-positive).
LEAK_PATTERNS=(
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'aws_secret_access_key'
  'xoxb-[A-Za-z0-9-]{10,}'
  'ghp_[A-Za-z0-9]{20,}'
  'sk-[A-Za-z0-9]{20,}'
  'AIza[A-Za-z0-9_-]{20,}'
)
LEAK_HITS=0
for pat in "${LEAK_PATTERNS[@]}"; do
  if grep -rIEl "${pat}" tests/fixtures 2>/dev/null | grep -q .; then
    fail "fixture contains secret-like pattern: ${pat}"
    LEAK_HITS=$((LEAK_HITS + 1))
  fi
done
if [ "${LEAK_HITS}" -eq 0 ]; then
  pass "no secret-like patterns in fixtures"
fi

# ---- 6. Fixture size cap (256 lines or 16 KiB) ----------------------
SIZE_HITS=0
while IFS= read -r ff; do
  lines=$(wc -l < "${ff}" 2>/dev/null || echo 0)
  bytes=$(wc -c < "${ff}" 2>/dev/null || echo 0)
  if [ "${lines}" -gt 256 ] || [ "${bytes}" -gt 16384 ]; then
    fail "fixture over cap (256 lines OR 16 KiB): ${ff} (${lines}L ${bytes}B)"
    SIZE_HITS=$((SIZE_HITS + 1))
  fi
done < <(find tests/fixtures -type f 2>/dev/null | sort)
if [ "${SIZE_HITS}" -eq 0 ]; then
  pass "fixture size cap honored"
fi

# ---- 7. Source-status table -----------------------------------------
echo ""
echo "SOURCE-STATUS table (from tests/README.md section 9):"
printf "  %-32s %s\n" "fixtures/terminal/"             "test-only"
printf "  %-32s %s\n" "fixtures/command-output/"       "test-only"
printf "  %-32s %s\n" "fixtures/files/"                "test-only"
printf "  %-32s %s\n" "fixtures/buckets/"              "informative-until-TC05"
printf "  %-32s %s\n" "fixtures/rules/"                "informative-until-TC05"
printf "  %-32s %s\n" "fixtures/context/"              "informative-until-TC05"
printf "  %-32s %s\n" "fixtures/policy/"               "informative-until-TC22"
printf "  %-32s %s\n" "fixtures/probes/wsl-mountinfo/" "test-only"
echo ""

# ---- Exit -----------------------------------------------------------
if [ "${FAIL_COUNT}" -eq 0 ]; then
  echo "verify-baseline.sh: ALL CHECKS PASS"
  exit 0
else
  echo "verify-baseline.sh: ${FAIL_COUNT} CHECK(S) FAILED"
  exit 1
fi
