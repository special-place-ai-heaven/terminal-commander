#!/usr/bin/env bash
# Synthesize a release-please attribution commit when releasable crates/**
# commits exist since the last release tag.
#
# release-please attributes commits BY PATH PREFIX and has no include-paths.
# crates/** changes therefore never trigger a release on their own. This
# script detects releasable (feat/fix/breaking) crate commits since the
# canonical version tag and pushes ONE sentinel commit under
# packages/terminal-commander/ so release-please bumps the canonical version
# source on the next push:main run.
#
# Idempotent: a SHA-fingerprint of the in-range crate commits is stored in
# the sentinel; a matching fingerprint is a no-op (loop guard). The caller
# SKIPS the release-please action when this script reports trigger_pushed=true,
# so the pushed commit (not this run) drives the release.
#
# DRY_RUN=1 detects + prints but does not commit/push (local verification).
set -euo pipefail

SENTINEL="packages/terminal-commander/.release-please-crates-trigger"
PKG_JSON="packages/terminal-commander/package.json"

emit() { # emit <key> <value> -> GITHUB_OUTPUT when set, else stdout
  if [ -n "${GITHUB_OUTPUT:-}" ]; then echo "$1=$2" >> "$GITHUB_OUTPUT"; fi
  echo "[synth] $1=$2"
}

ver=$(node -p "require('./${PKG_JSON}').version")
base_tag="v${ver}"
echo "[synth] canonical version ${ver}, base tag ${base_tag}"

if ! git rev-parse -q --verify "refs/tags/${base_tag}" >/dev/null 2>&1; then
  echo "[synth] base tag ${base_tag} not found; nothing to compare. Skipping."
  emit trigger_pushed false
  exit 0
fi

# Releasable crate commits in (base_tag, HEAD]: conventional type feat/fix/perf
# or breaking (!, BREAKING CHANGE, or BREAKING-CHANGE). Subject form:
# "type(scope)!: ...".
# `perf` is treated as a fix-level bump because user-observable perf changes ship
# in Rust crates and must produce a release (e.g. PR #47 /proc liveness).
mapfile -t crate_shas < <(git log --reverse --format='%H' "${base_tag}..HEAD" -- crates)
if [ "${#crate_shas[@]}" -eq 0 ]; then
  echo "[synth] no crate commits since ${base_tag}. Skipping."
  emit trigger_pushed false
  exit 0
fi

strongest=""   # "" < fix < feat < breaking
breaking_sha=""
for sha in "${crate_shas[@]}"; do
  subject=$(git show -s --format='%s' "$sha")
  type_tok="${subject%%:*}"            # e.g. "feat(daemon)!" or "fix"
  case "$type_tok" in
    *"!"*)
      if printf '%s\n' "$subject" | grep -E '^[a-z]+(\([^)]*\))?!:[[:space:]]' >/dev/null; then
        strongest="breaking"
        breaking_sha="$sha"
      fi
      ;;
  esac
  base_type="${type_tok%%(*}"; base_type="${base_type%%!*}"
  if [ "$base_type" = "feat" ] && [ "$strongest" != "breaking" ]; then strongest="feat";
  elif [ "$base_type" = "fix" ] && [ -z "$strongest" ]; then strongest="fix";
  elif [ "$base_type" = "perf" ] && [ -z "$strongest" ]; then strongest="fix";
  fi
done
# Body-level breaking-footer detection. Consume each body completely so
# pipefail cannot turn an early grep match into a false negative via SIGPIPE.
for sha in "${crate_shas[@]}"; do
  if git show -s --format='%b' "$sha" | grep -E '^BREAKING[ -]CHANGE:' >/dev/null; then
    strongest="breaking"
    breaking_sha="$sha"
  fi
done

if [ -z "$strongest" ]; then
  echo "[synth] crate commits exist but none are feat/fix/perf/breaking. Skipping."
  emit trigger_pushed false
  exit 0
fi

# Loop-guard fingerprint: hash of the sorted in-range crate SHAs.
fingerprint=$(printf '%s\n' "${crate_shas[@]}" | sort | sha256sum | cut -c1-16)

if [ -f "$SENTINEL" ] && grep -q "fingerprint: ${fingerprint}" "$SENTINEL" 2>/dev/null; then
  echo "[synth] sentinel already carries fingerprint ${fingerprint}; no-op."
  emit trigger_pushed false
  exit 0
fi

# release-please supports multiple conventional messages in one commit when the
# additional messages are placed at the bottom of the body. Preserve every
# releasable message from crate commit subjects and bodies instead of collapsing
# the range into one synthetic "N crate fixes" bullet.
is_release_message() {
  printf '%s' "$1" | grep -Eq '^(feat|fix|perf)(\([^)]*\))?!?:[[:space:]]'
}
strip_cc_type() { printf '%s' "$1" | sed -E 's/^[a-z]+(\([^)]*\))?!?:[[:space:]]*//'; }

release_messages=()
for sha in "${crate_shas[@]}"; do
  while IFS= read -r line; do
    if is_release_message "$line"; then release_messages+=("$line"); fi
  done < <(git show -s --format='%B' "$sha")
done

# A body-only BREAKING CHANGE still needs a breaking conventional header.
if [ "$strongest" = "breaking" ] && ! printf '%s\n' "${release_messages[@]}" | grep -E '^[^:]+!:' >/dev/null; then
  if [ "${#release_messages[@]}" -eq 0 ]; then
    if [ -n "$breaking_sha" ]; then
      fallback_subject=$(git show -s --format='%s' "$breaking_sha")
      release_messages+=("feat!: $(strip_cc_type "$fallback_subject")")
    fi
  else
    release_messages[0]="feat!: $(strip_cc_type "${release_messages[0]}")"
  fi
fi

if [ "${#release_messages[@]}" -eq 0 ]; then
  echo "[synth] crate commits exist but none are release-please-compatible feat/fix/perf/breaking messages. Skipping."
  emit trigger_pushed false
  exit 0
fi

commit_subject="${release_messages[0]}"
# Body: provenance first, then every additional release-please message at the
# bottom where its parser treats each as a distinct changelog entry.
commit_body="Synthesized by synthesize-crates-release-trigger.sh so release-please
attributes crates/** changes to the canonical version source. Crate commits
since ${base_tag}: ${#crate_shas[@]} (fingerprint ${fingerprint})."
for message in "${release_messages[@]:1}"; do
  commit_body+=$'\n\n'"$message"
done

echo "[synth] strongest type: ${strongest} -> commit subject: '${commit_subject}'"

# Write the sentinel (fingerprint + audit trail of the crate SHAs).
{
  echo "# release-please crates trigger"
  echo "# Auto-generated. Forces release-please to attribute crates/** changes"
  echo "# to packages/terminal-commander. Do not edit by hand."
  echo "fingerprint: ${fingerprint}"
  echo "base_tag: ${base_tag}"
  echo "crate_commits:"
  printf '  - %s\n' "${crate_shas[@]}"
} > "$SENTINEL"

if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "[synth] DRY_RUN=1: would commit '${commit_subject}' and push to main."
  echo "[synth] commit body:"; printf '%s\n' "$commit_body" | sed 's/^/    /'
  echo "[synth] sentinel content:"; sed 's/^/    /' "$SENTINEL"
  git checkout -- "$SENTINEL" 2>/dev/null || rm -f "$SENTINEL"
  emit trigger_pushed false
  exit 0
fi

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add "$SENTINEL"
git commit -m "${commit_subject}" -m "${commit_body}"
git push origin HEAD:main
emit trigger_pushed true
