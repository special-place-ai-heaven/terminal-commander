#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Usage: ./scripts/release/recover-partial-publish.sh <version>
#
# Queries npm + crates.io for each artifact at <version>. Reports
# which shipped vs which are missing. Prints republish guidance for
# missing artifacts. Idempotent — safe to re-run.

set -euo pipefail

VER="${1:?usage: recover-partial-publish.sh <version> (e.g., 0.2.0)}"

NPM_PKGS=(
  "terminal-commander"
  "@terminal-commander/linux-x64"
  "@terminal-commander/linux-arm64"
  "@terminal-commander/windows-x64"
  "@terminal-commander/mac-x64"
  "@terminal-commander/mac-arm64"
)

CARGO_CRATES=(
  "terminal-commander-core"
  "terminal-commander-sifters"
  "terminal-commander-probes"
  "terminal-commander-store"
  "terminal-commander-supervisor"
  "terminal-commanderd"
  "terminal-commander-mcp"
)

missing_npm=()
missing_cargo=()

echo "═══ npm registry — checking @ ${VER} ═══"
for name in "${NPM_PKGS[@]}"; do
  if npm view "${name}@${VER}" version >/dev/null 2>&1; then
    printf "  OK    %s@%s\n" "$name" "$VER"
  else
    printf "  MISS  %s@%s\n" "$name" "$VER"
    missing_npm+=("$name")
  fi
done

echo
echo "═══ crates.io — checking @ ${VER} (HTTP API, not cargo search) ═══"
# cargo search output is human-formatted, ANSI-prone, header-mixed.
# Use the crates.io HTTP API: 200 = exists, 404 = missing. Deterministic.
for crate in "${CARGO_CRATES[@]}"; do
  http_code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "User-Agent: terminal-commander-recovery (https://github.com/special-place-administrator/terminal-commander)" \
    "https://crates.io/api/v1/crates/${crate}/${VER}")
  case "$http_code" in
    200)
      printf "  OK    %s@%s\n" "$crate" "$VER" ;;
    404)
      printf "  MISS  %s@%s\n" "$crate" "$VER"
      missing_cargo+=("$crate") ;;
    *)
      printf "  ??    %s@%s (HTTP %s — treating as missing)\n" "$crate" "$VER" "$http_code"
      missing_cargo+=("$crate") ;;
  esac
done

echo
if [ ${#missing_npm[@]} -eq 0 ] && [ ${#missing_cargo[@]} -eq 0 ]; then
  echo "✔ All artifacts present at version ${VER}. Nothing to recover."
  exit 0
fi

echo "═══ Recovery actions ═══"
if [ ${#missing_npm[@]} -gt 0 ]; then
  echo
  echo "Missing npm packages:"
  for n in "${missing_npm[@]}"; do echo "  - $n"; done
  echo
  echo "Re-run release-please.yml workflow with:"
  echo "  gh workflow run release-please.yml -f force_publish=true"
  echo "(E409-tolerant; already-shipped packages exit success, only missing ones republish.)"
fi
if [ ${#missing_cargo[@]} -gt 0 ]; then
  echo
  echo "Missing crates.io crates:"
  for c in "${missing_cargo[@]}"; do echo "  - $c"; done
  echo
  echo "Republish manually (must be on a checkout at tag v${VER}):"
  for c in "${missing_cargo[@]}"; do
    echo "  cargo publish -p $c"
  done
fi
exit 1
