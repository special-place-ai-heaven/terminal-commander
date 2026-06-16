#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# US6 / T055: WSL omni acceptance smoke.
#
# Same O-01..O-14 sequence as verify-omni-linux.sh. WSL is a POSIX host
# (the daemon runs the shared cfg(unix) runtime: sessions + posix PTY are
# available), so the runnable set matches Linux. The only WSL nuance is
# the 9P `/mnt/c` file-watch fallback, which is exercised by the runtime
# smoke, not the omni acceptance gates. Gates that cannot run here
# (privileged O-06; Windows O-07; macOS O-08; SSH/container O-09/O-10;
# IPC-fault O-13; provider-trust O-14) are SKIPPED WITH A LOUD NOTICE and
# never counted as a pass.
#
# Usage:  bash scripts/smoke/verify-omni-wsl.sh
# Exit:   0 = all RAN gates passed; 1 = a RAN gate failed; 2 = env problem.

set -euo pipefail

OMNI_PROFILE="wsl"
export OMNI_PROFILE

# shellcheck source=scripts/smoke/omni-common.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/omni-common.sh"

# Run the sequence and propagate its exit code verbatim.
set +e
omni_main
rc=$?
set -e
exit "$rc"
