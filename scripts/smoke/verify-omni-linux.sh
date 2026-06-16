#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# US6 / T054: native Linux omni acceptance smoke.
#
# Builds a fresh daemon + MCP adapter and runs the RUNNABLE subset of the
# O-01..O-14 omni acceptance sequence against them, EXITING NON-ZERO on
# any RAN-gate failure. Gates that cannot run on a Linux host (privileged
# O-06 deferred; Windows O-07; macOS O-08; SSH/container O-09/O-10;
# IPC-fault O-13; provider-trust O-14) are SKIPPED WITH A LOUD NOTICE and
# are NEVER counted as a pass. The final summary prints exactly which
# gates ran vs were skipped.
#
# The O-sequence and per-gate checks live in scripts/smoke/omni-o-runner.py;
# the build/daemon/teardown harness lives in scripts/smoke/omni-common.sh.
#
# Usage:  bash scripts/smoke/verify-omni-linux.sh
# Exit:   0 = all RAN gates passed; 1 = a RAN gate failed; 2 = env problem.

set -euo pipefail

OMNI_PROFILE="linux"
export OMNI_PROFILE

# shellcheck source=scripts/smoke/omni-common.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/omni-common.sh"

# Run the sequence and propagate its exit code verbatim. `set +e` here so a
# non-zero return is captured rather than killing the script before `exit`.
set +e
omni_main
rc=$?
set -e
exit "$rc"
