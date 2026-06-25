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
# or breaking (! or BREAKING CHANGE). Subject form: "type(scope)!: ...".
# `perf` is treated as a fix-level bump because user-observable perf changes ship
# in Rust crates and must produce a release (e.g. PR #47 /proc liveness).
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
  elif [ "$base_type" = "perf" ] && [ -z "$strongest" ]; then strongest="fix";
  fi
done < <(git log --format='%s' "${base_tag}..HEAD" -- crates)
# Body-level BREAKING CHANGE detection (newline-safe: grep over full log).
if git log --format='%b' "${base_tag}..HEAD" -- crates | grep -q "BREAKING CHANGE"; then
  strongest="breaking"
fi

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

case "$strongest" in
  breaking) commit_type="feat!";;
  feat)     commit_type="feat";;
  fix)      commit_type="fix";;
esac

# Build the synthetic commit message FROM THE REAL crate subjects in range so
# release-please's generated changelog describes the actual fixes instead of a
# static "release Rust crate changes" (which buried every real crates/** change
# -- the bug that produced misleading 0.1.64 / 0.1.65 changelogs).
#
# release-please turns ONE commit into ONE changelog bullet (it uses the SUBJECT;
# the body is parsed only for BREAKING CHANGE footers, not split into bullets).
# Since this script pushes exactly one synthetic commit, the changelog gets one
# bullet per release. So: single real fix -> reuse its subject verbatim (perfect
# bullet); multiple -> a faithful summary subject + every real subject in the
# body for the audit trail.
# UPGRADE PATH (only if a real multi-fix release proves it's needed): emit N
# synthetic commits, or post-process release-please's changelog. Out of scope here.
mapfile -t crate_subjects < <(git log --format='%s' "${base_tag}..HEAD" -- crates)
# True if a subject already starts with a conventional-commit type (so release-please
# will classify it correctly and we can use it verbatim, scope and all).
is_conventional() { printf '%s' "$1" | grep -Eq '^[a-z]+(\([^)]*\))?!?:[[:space:]]'; }
# Strip a leading conventional-commit type so we can re-prefix with the canonical
# ${commit_type} without doubling it (used only for non-conventional / multi cases).
strip_cc_type() { printf '%s' "$1" | sed -E 's/^[a-z]+(\([^)]*\))?!?:[[:space:]]*//'; }
if [ "${#crate_subjects[@]}" -eq 1 ]; then
  # Single fix: use the real subject VERBATIM when it's already conventional
  # (keeps the scope, e.g. "fix(daemon): ..."); otherwise prefix the canonical type.
  if is_conventional "${crate_subjects[0]}"; then
    commit_subject="${crate_subjects[0]}"
  else
    commit_subject="${commit_type}: ${crate_subjects[0]}"
  fi
else
  commit_subject="${commit_type}: ${#crate_subjects[@]} crate fixes -- $(strip_cc_type "${crate_subjects[0]}") (+$(( ${#crate_subjects[@]} - 1 )) more)"
fi
# Body: provenance + every real subject (audit trail / full attribution).
commit_body="Synthesized by synthesize-crates-release-trigger.sh so release-please
attributes crates/** changes to the canonical version source. Crate commits
since ${base_tag}: ${#crate_shas[@]} (fingerprint ${fingerprint}).

Crate changes in this release:
$(printf '* %s\n' "${crate_subjects[@]}")"

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
