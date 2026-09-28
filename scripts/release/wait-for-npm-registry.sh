#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Poll the npm registry until every "<name>@<version>" argument is visible,
# or fail loudly after ~10 minutes.
#
# The post-publish verify jobs used to `sleep 60` and then `npm install`.
# That fixed guess was not long enough on runs 36490194477 (0.2.1) and
# 36494348084 (0.3.0): npm reported `npm publish` success but the registry's
# read replicas had not caught up yet, so the verify job's install failed
# with ETARGET seconds later even though the version existed and a manual
# re-run a few minutes after passed. Poll instead of guessing a fixed delay.
#
# Runs on every verify-* job (Linux, macOS, and Windows via `shell: bash` --
# GitHub-hosted Windows runners ship Git Bash) so there is one wait
# implementation, not five copies.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <name@version> [name@version ...]" >&2
  exit 2
fi

interval_secs=20
max_attempts=30 # 30 * 20s = 10 minutes

for spec in "$@"; do
  attempt=0
  until npm view "$spec" version >/dev/null 2>&1; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge "$max_attempts" ]; then
      echo "::error::${spec} did not appear on the npm registry after $((max_attempts * interval_secs / 60)) minutes"
      exit 1
    fi
    echo "waiting for ${spec} on npm registry (attempt ${attempt}/${max_attempts})..."
    sleep "$interval_secs"
  done
  echo "registry-ready: ${spec}"
done
