# crates/ Release Trigger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `feat:`/`fix:` commit touching `crates/` auto-cuts a release by pushing one deterministic sentinel commit under `packages/terminal-commander/`, which release-please then attributes to the canonical version source and publishes through the existing auto-merge chain.

**Architecture:** A bash script (`synthesize-crates-release-trigger.sh`) runs as the first step of the `release-please` job. It scans `git log v<ver>..HEAD -- crates` for releasable commits, and if found (and not already triggered, per a SHA fingerprint) pushes a sentinel commit to `main` with the strongest conventional type. On that run the release-please action is SKIPPED; the sentinel push fires a fresh run where release-please bumps `packages/terminal-commander` normally. A fingerprint loop-guard + action-skip prevent infinite re-triggering.

**Tech Stack:** GitHub Actions, bash, git, node (for reading package.json version), `RELEASE_PLEASE_TOKEN_TC`. release-please-action (manifest mode). Verified: `main` is NOT branch-protected, so the CI push to main succeeds.

---

## Background: verified facts (do not re-derive)

- `release-please.yml` job `release-please` (line 105): `outputs:` block (111-115) exposes `releases_created`/`version`/`tag_name` from `steps.final`. NO checkout step precedes the action (the action checks out internally). First step is `release-please (manifest mode)` `id: release` at line 117.
- `steps.final` (163-184) aggregates `releases_created` from `steps.release` + `steps.release_retry`, and reads version via `steps.release.outputs['packages/terminal-commander--version']`. A sentinel commit under `packages/terminal-commander/` makes THAT component release, so this aggregation works unchanged.
- `ensure-release` job (211): `needs: release-please`, `if: github.event_name == 'push' && inputs.recovery_release_as == ''`.
- `release-pr-sync.yml` already pushes to a branch with `RELEASE_PLEASE_TOKEN_TC` (line 88 `git push origin HEAD:...`) using the github-actions bot identity (lines 84-85). Mirror that identity for the sentinel commit.
- Canonical version source: `packages/terminal-commander/package.json` `.version` (sync-optional-dependencies.js reads it). Current: 0.1.13. Release tags are `v<version>` (`include-v-in-tag: true`, `include-component-in-tag: false` → tag is `v0.1.13`, not component-prefixed).
- A skipped step's `outputs.*` reference returns empty string (not an error) in GitHub Actions, so gating `release` with `if:` and still running `final` is safe (`final` computes `releases_created=false`).

## File Structure

- **Create:** `scripts/release/synthesize-crates-release-trigger.sh` — detection + sentinel push, idempotent, dry-run capable.
- **Modify:** `.github/workflows/release-please.yml` — checkout + synth step before the action; gate the action + `ensure-release` on `trigger_pushed`.

---

## Task 1: The synthesis script (with a dry-run path testable locally)

**Files:**
- Create: `scripts/release/synthesize-crates-release-trigger.sh`

- [ ] **Step 1: Write the script**

Create `scripts/release/synthesize-crates-release-trigger.sh`:

```bash
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
```

- [ ] **Step 2: Syntax-check + dry-run locally (WSL)**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && bash -n scripts/release/synthesize-crates-release-trigger.sh && echo 'syntax ok'"
```
Expected: `syntax ok`.

Then a real dry-run against current history (push disabled). Strip CR first (Windows checkout):
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && sed 's/\r$//' scripts/release/synthesize-crates-release-trigger.sh > ~/synth.sh && DRY_RUN=1 bash ~/synth.sh; rm -f ~/synth.sh"
```
Expected: it resolves version 0.1.13, base tag v0.1.13, finds THIS session's crate commits (the receipt/import_pack/cargo work are feat: under crates/), reports `strongest -> feat`, prints the sentinel content, and `trigger_pushed=false` (dry run). If `v0.1.13` tag isn't fetched locally, the script prints "base tag not found; Skipping" -- acceptable for the dry run; the CI checkout has `fetch-depth: 0` so the tag is present there. (To exercise detection locally, `git fetch heaven --tags` first.)

- [ ] **Step 3: Make it executable + commit**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && chmod +x scripts/release/synthesize-crates-release-trigger.sh"
git add scripts/release/synthesize-crates-release-trigger.sh
git commit -F <msg-file>
```
Subject: `feat(ci): synthesize-crates-release-trigger script (crates/ -> release)`
(NOTE: this is `feat(ci):` touching `scripts/`, not `crates/`, so it does NOT itself trigger the crates path -- correct; it's infra.)

---

## Task 2: Wire the script into release-please.yml

**Files:**
- Modify: `.github/workflows/release-please.yml`

- [ ] **Step 1: Add the trigger_pushed job output**

In the `release-please` job `outputs:` block (after line 115 `version:`), add:

```yaml
      trigger_pushed: ${{ steps.crates_trigger.outputs.trigger_pushed }}
```

- [ ] **Step 2: Add checkout + synth steps before the release-please action**

Immediately after `steps:` (line 116), BEFORE the `release-please (manifest mode)` step, insert:

```yaml
      - name: Checkout main (full history for crates trigger)
        if: github.event_name == 'push' && inputs.recovery_release_as == ''
        uses: actions/checkout@v4
        with:
          ref: main
          fetch-depth: 0
          token: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }}

      - name: Synthesize crates release attribution
        id: crates_trigger
        if: github.event_name == 'push' && inputs.recovery_release_as == ''
        shell: bash
        run: bash scripts/release/synthesize-crates-release-trigger.sh
```

- [ ] **Step 3: Gate the release-please action on no trigger push**

Add an `if:` to the `release-please (manifest mode)` step (currently has only `id: release`):

```yaml
      - name: release-please (manifest mode)
        id: release
        if: steps.crates_trigger.outputs.trigger_pushed != 'true'
        uses: googleapis/release-please-action@5c625bfb5d1ff62eadeeb3772007f7f66fdcf071
```
(leave the existing `with:` block unchanged.)

- [ ] **Step 4: Gate ensure-release on no trigger push**

Change the `ensure-release` job `if:` (line 214) from:
```yaml
    if: github.event_name == 'push' && inputs.recovery_release_as == ''
```
to:
```yaml
    if: github.event_name == 'push' && inputs.recovery_release_as == '' && needs.release-please.outputs.trigger_pushed != 'true'
```

- [ ] **Step 5: Validate YAML**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import yaml; yaml.safe_load(open(\".github/workflows/release-please.yml\")); print(\"yaml ok\")'"
```
Expected: `yaml ok`.

- [ ] **Step 6: Commit**

Subject: `feat(ci): wire crates release trigger into release-please.yml`

---

## Task 3: Push + prove live

- [ ] **Step 1: Push the infra (does NOT itself release)**

Tasks 1+2 are `feat(ci):` under `scripts/` + `.github/` -- NOT `crates/` -- so they don't trigger the crates path. They just install the mechanism.
```
cd "C:/Users/poslj/terminal-commander"
git fetch heaven; git rev-list --left-right --count heaven/main...main
git push heaven main
```

- [ ] **Step 2: Watch the FIRST post-install push detect this session's crate work**

The synth step runs on this very push. It will find the session's crate commits (receipt, import_pack, cargo pack -- all feat: under crates/) since v0.1.13, push the sentinel, and skip release-please. Watch:
```
gh run list --repo special-place-ai-heaven/terminal-commander --limit 6
```
Expect: the push's release-please run shows the synth step pushing a sentinel; a SECOND run (from the sentinel push) opens a release PR for 0.1.14; release-pr-sync auto-merges; build -> presmoke -> publish.
```
gh run watch <sentinel-run-id> --exit-status
gh pr list --repo special-place-ai-heaven/terminal-commander --state all --limit 3
```

- [ ] **Step 3: Confirm 0.1.14 published + installable**

```
gh release list --repo special-place-ai-heaven/terminal-commander --limit 3
wsl.exe bash -lc 'docker run --rm node:22-bookworm-slim sh -c "npm install -g terminal-commander@0.1.14 && terminal-commander-mcp --version"'
```
Expected: v0.1.14 release exists; clean-box install + run works.

- [ ] **Step 4: Verify idempotency (no loop)**

After 0.1.14 releases, the sentinel commit is in history but the base tag is now v0.1.14; the next unrelated push runs synth -> finds no NEW crate commits since v0.1.14 -> `trigger_pushed=false` -> release-please runs normally. Confirm no runaway re-trigger:
```
gh run list --repo special-place-ai-heaven/terminal-commander --limit 5
```
Expect: no infinite chain of release-please runs; it settles.

- [ ] **Step 5: Report (DSPIVR)**

Objective, changes (1 script + workflow wiring), verification (dry-run local; live: sentinel pushed -> 0.1.14 cut -> auto-merge -> presmoke -> publish -> clean-box install; idempotency confirmed), evidence (sentinel commit SHA, release PR #, run ids, install output), known gaps (sentinel adds one bot commit per crates release; main must stay unprotected or the push needs bypass).

---

## Spec coverage check

- Synthesis script (detect, fingerprint guard, strongest-type, dry-run) -> Task 1.
- Workflow wiring (checkout, synth step, action-skip gate, ensure-release gate, trigger_pushed output) -> Task 2.
- CI-only verification + live 0.1.14 proof + idempotency -> Task 3.
- Canonical version source untouched (sentinel attributes to packages/terminal-commander) -> Task 1 script writes under that dir.
- No competing version source / no version.txt -> by construction (no new version file).

## Notes carried from Codex + verification

- No include-paths in release-please schema -> config-only fix impossible -> synthesized attribution is the mechanism.
- Fingerprint guard (sorted in-range crate SHAs) + action-skip on trigger run = no infinite loop.
- main is NOT branch-protected -> the CI push to main succeeds (verified via GitHub API 404).
- The sentinel commit's conventional type (feat!/feat/fix) drives the bump magnitude under the repo's bump-*-pre-major rules.
