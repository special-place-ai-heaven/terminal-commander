#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# US6 / T055: macOS omni acceptance smoke.
#
# Same O-01..O-14 sequence as verify-omni-linux.sh. macOS reuses the full
# POSIX runtime (cfg(unix)): shell pipelines, persistent sessions, and the
# posix PTY backend all run, so O-08 (the macOS full agent smoke) IS this
# run. Gates that cannot run here (privileged O-06; Windows O-07;
# SSH/container O-09/O-10; IPC-fault O-13; provider-trust O-14) are
# SKIPPED WITH A LOUD NOTICE and never counted as a pass.
#
# The common harness adds Homebrew bin dirs (/opt/homebrew/bin,
# /usr/local/bin) to PATH so a non-login invocation finds cargo/python3.
#
# Usage:  bash scripts/smoke/verify-omni-macos.sh
# Exit:   0 = all RAN gates passed; 1 = a RAN gate failed; 2 = env problem.

set -euo pipefail

OMNI_PROFILE="macos"
export OMNI_PROFILE

# shellcheck source=scripts/smoke/omni-common.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/omni-common.sh"

# Run the sequence and propagate its exit code verbatim.
set +e
omni_main
rc=$?
set -e
exit "$rc"
