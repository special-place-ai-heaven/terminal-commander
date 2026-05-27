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

# Releasable crate commits in (base_tag, HEAD]: conventional type feat/fix
# or breaking (! or BREAKING CHANGE). Subject form: "type(scope)!: ...".
mapfile -t crate_shas < <(git log --format='%H' "${base_tag}..HEAD" -- crates)
if [ "${#crate_shas[@]}" -eq 0 ]; then
  echo "[synth] no crate commits since ${base_tag}. Skipping."
  emit trigger_pushed false
  exit 0
fi

strongest=""   # "" < fix < feat < breaking
# Parse SUBJECTS only (one per line; bodies are not newline-safe in a
# read loop). Detect breaking separately via a body-aware grep below.
while IFS= read -r subject; do
  [ -n "$subject" ] || continue
  type_tok="${subject%%:*}"            # e.g. "feat(daemon)!" or "fix"
  case "$type_tok" in
    *"!"*) strongest="breaking";;
  esac
  base_type="${type_tok%%(*}"; base_type="${base_type%%!*}"
  if [ "$base_type" = "feat" ] && [ "$strongest" != "breaking" ]; then strongest="feat";
  elif [ "$base_type" = "fix" ] && [ -z "$strongest" ]; then strongest="fix";
  fi
done < <(git log --format='%s' "${base_tag}..HEAD" -- crates)
# Body-level BREAKING CHANGE detection (newline-safe: grep over full log).
if git log --format='%b' "${base_tag}..HEAD" -- crates | grep -q "BREAKING CHANGE"; then
  strongest="breaking"
fi

if [ -z "$strongest" ]; then
  echo "[synth] crate commits exist but none are feat/fix/breaking. Skipping."
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

case "$strongest" in
  breaking) commit_type="feat!";;
  feat)     commit_type="feat";;
  fix)      commit_type="fix";;
esac
echo "[synth] strongest type: ${strongest} -> commit '${commit_type}: release Rust crate changes'"

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
  echo "[synth] DRY_RUN=1: would commit '${commit_type}: release Rust crate changes' and push to main."
  echo "[synth] sentinel content:"; sed 's/^/    /' "$SENTINEL"
  git checkout -- "$SENTINEL" 2>/dev/null || rm -f "$SENTINEL"
  emit trigger_pushed false
  exit 0
fi

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add "$SENTINEL"
git commit -m "${commit_type}: release Rust crate changes

Synthesized by synthesize-crates-release-trigger.sh so release-please
attributes crates/** changes to the canonical version source. Crate
commits since ${base_tag}: ${#crate_shas[@]} (fingerprint ${fingerprint})."
git push origin HEAD:main
emit trigger_pushed true
