#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# Sync Cargo workspace + crate-local internal-dep versions to a target version.
# Used by .github/workflows/release-pr-sync.yml to keep Cargo.toml lockstep
# with the npm package version that release-please bumped.
#
# release-please's own `extra-files` mechanism doesn't allow paths outside
# the package directory (no `..`), so we bump Cargo.toml here in the release
# PR sync step instead.
#
# Targets every version field marked with `# x-release-please-version` in:
#   - Cargo.toml (workspace.package.version + workspace.dependencies internal pins)
#   - crates/daemon/Cargo.toml (supervisor literal dep)
#   - crates/mcp/Cargo.toml (sifters, store, terminal-commanderd, supervisor literal deps)
#   - crates/probes/Cargo.toml (sifters literal dep)
#
# Also runs `cargo check --workspace` afterwards so Cargo.lock updates land
# in the same commit.

import re
import subprocess
import sys
from pathlib import Path

TARGET_FILES = [
    "Cargo.toml",
    "crates/daemon/Cargo.toml",
    "crates/mcp/Cargo.toml",
    "crates/probes/Cargo.toml",
]

# Matches any of these forms (semver pattern) ending with the marker comment:
#   version = "X.Y.Z" # x-release-please-version
#   version = "X.Y.Z" } # x-release-please-version          (path-first form)
#   version = "X.Y.Z", path = "..." } # x-release-please-version  (version-first form)
# Strategy: process line-by-line. Match any line ending in `# x-release-please-version`
# and substitute version = "X.Y.Z" → version = "${target}" within that line only.
LINE_VERSION_RE = re.compile(r'version\s*=\s*"\d+\.\d+\.\d+"')


def sync_file(path: Path, target: str) -> int:
    """Returns number of substitutions made."""
    src = path.read_text(encoding="utf-8")
    lines = src.split("\n")
    bumped = 0
    for i, line in enumerate(lines):
        if "# x-release-please-version" not in line:
            continue
        new_line = LINE_VERSION_RE.sub(f'version = "{target}"', line, count=1)
        if new_line != line:
            lines[i] = new_line
            bumped += 1
    if bumped > 0:
        path.write_text("\n".join(lines), encoding="utf-8")
    return bumped


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: sync-cargo-versions.py <target-version>", file=sys.stderr)
        return 2
    target = sys.argv[1]
    if not re.fullmatch(r"\d+\.\d+\.\d+", target):
        print(f"target version '{target}' is not semver MAJOR.MINOR.PATCH", file=sys.stderr)
        return 2

    repo_root = Path(__file__).resolve().parent.parent.parent
    total = 0
    for rel in TARGET_FILES:
        p = repo_root / rel
        if not p.is_file():
            print(f"SKIP {rel} (file missing)")
            continue
        n = sync_file(p, target)
        print(f"  {rel}: {n} version site(s) bumped")
        total += n
    print(f"total: {total} version sites bumped to {target}")

    if total > 0:
        print("running cargo check --workspace to update Cargo.lock")
        rc = subprocess.call(
            ["cargo", "check", "--workspace", "--quiet"],
            cwd=str(repo_root),
        )
        if rc != 0:
            print(f"cargo check failed (rc={rc})", file=sys.stderr)
            return rc
    return 0


if __name__ == "__main__":
    sys.exit(main())
