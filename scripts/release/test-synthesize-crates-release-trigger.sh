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

seed_repo "$tmp/incremental"
printf 'first\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'fix(mcp): first attributed crate change'
first_sha="$(git rev-parse HEAD)"
{
  echo '# release-please crates trigger'
  echo '# Auto-generated test fixture.'
  echo 'fingerprint: prior'
  echo 'base_tag: v0.1.81'
  echo 'crate_commits:'
  # Match a Windows checkout: exact SHA membership must tolerate CRLF.
  printf '  - %s\r\n' "$first_sha"
} > packages/terminal-commander/.release-please-crates-trigger
git add packages/terminal-commander/.release-please-crates-trigger
git commit -qm 'fix(mcp): first attributed crate change' \
  -m 'Synthetic attribution already emitted for the first crate commit.'

printf 'second\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'fix(mcp): second incremental crate change'
second_sha="$(git rev-parse HEAD)"
output="$(DRY_RUN=1 bash "$repo_root/scripts/release/synthesize-crates-release-trigger.sh")"
grep -Fq 'fix(mcp): second incremental crate change' <<<"$output" || {
  echo 'new incremental crate message was not synthesized' >&2
  exit 1
}
if grep -Fq 'fix(mcp): first attributed crate change' <<<"$output"; then
  echo 'previously attributed crate message was synthesized again' >&2
  exit 1
fi
for sha in "$first_sha" "$second_sha"; do
  grep -Fq "  - $sha" <<<"$output" || {
    echo "cumulative sentinel omitted crate commit: $sha" >&2
    exit 1
  }
done

seed_repo "$tmp/fingerprint-is-not-state"
printf 'changed\n' >> crates/demo/lib.rs
git add crates/demo/lib.rs
git commit -qm 'fix(mcp): exact SHA membership controls attribution'
only_sha="$(git rev-parse HEAD)"
matching_fingerprint="$(printf '%s\n' "$only_sha" | sha256sum | cut -c1-16)"
{
  echo '# release-please crates trigger'
  echo "fingerprint: $matching_fingerprint"
  echo 'base_tag: v0.1.81'
  echo 'crate_commits:'
} > packages/terminal-commander/.release-please-crates-trigger
git add packages/terminal-commander/.release-please-crates-trigger
git commit -qm 'chore: incomplete attribution checkpoint fixture'
output="$(DRY_RUN=1 bash "$repo_root/scripts/release/synthesize-crates-release-trigger.sh")"
grep -Fq 'fix(mcp): exact SHA membership controls attribution' <<<"$output" || {
  echo 'matching fingerprint incorrectly suppressed an unrecorded crate SHA' >&2
  exit 1
}

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
