#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

seed_repo() {
  local dir="$1"
  mkdir -p "$dir"
  cd "$dir"
  git init -q
  git config user.name test
  git config user.email test@example.invalid
  mkdir -p packages/terminal-commander crates/demo
  printf '{"version":"0.1.81"}\n' > packages/terminal-commander/package.json
  printf 'seed\n' > crates/demo/lib.rs
  git add .
  git commit -qm 'chore: seed release fixture'
  git tag v0.1.81
}

seed_repo "$tmp/multiple"

printf 'changed\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'feat(mcp): accept natural compact calls' \
  -m $'fix(supervisor): honor caller deadlines\n\nfeat(files): search bounded directory trees'

output="$(DRY_RUN=1 bash "$repo_root/scripts/release/synthesize-crates-release-trigger.sh")"
for entry in \
  'feat(mcp): accept natural compact calls' \
  'fix(supervisor): honor caller deadlines' \
  'feat(files): search bounded directory trees'
do
  grep -Fq "$entry" <<<"$output" || {
    echo "missing synthesized release entry: $entry" >&2
    exit 1
  }
done

seed_repo "$tmp/breaking"
message_file="$tmp/large-commit-message"
{
  printf 'chore: retain historical context\n\n'
  for ((i = 0; i < 5000; i++)); do
    printf 'historical release context line %05d remains intentionally verbose\n' "$i"
  done
} > "$message_file"
printf 'historical\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qF "$message_file"
printf 'changed\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'chore: change public contract' -m 'BREAKING-CHANGE: callers must migrate'
output="$(DRY_RUN=1 bash "$repo_root/scripts/release/synthesize-crates-release-trigger.sh")"
grep -Fq 'feat!: change public contract' <<<"$output" || {
  echo 'hyphenated BREAKING-CHANGE footer did not synthesize a breaking entry' >&2
  exit 1
}

seed_repo "$tmp/malformed"
printf 'changed\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'feat:missing required whitespace'
output="$(DRY_RUN=1 bash "$repo_root/scripts/release/synthesize-crates-release-trigger.sh")"
grep -Fq 'none are release-please-compatible' <<<"$output" || {
  echo 'malformed conventional subject did not skip cleanly' >&2
  exit 1
}

seed_repo "$tmp/breaking-subject"
printf 'changed\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'refactor!: remove old API'
output="$(DRY_RUN=1 bash "$repo_root/scripts/release/synthesize-crates-release-trigger.sh")"
grep -Fq 'feat!: remove old API' <<<"$output" || {
  echo 'non-feature breaking subject did not synthesize a breaking entry' >&2
  exit 1
}
