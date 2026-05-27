# Spec: crates/ Release Trigger (synthesized attribution)

Status: Design (Codex-reviewed + assumption-verified 2026-05-27, no open
questions). Language: ASCII only.

## Objective

Make a conventional-commit `feat:`/`fix:` touching `crates/` (the Rust
binary source) automatically cut a versioned release + npm publish with
no human step, by bumping the EXISTING version source
(`packages/terminal-commander/package.json`) -- not a competing one.

## Why this is needed (verified)

release-please attributes commits to a package BY PATH PREFIX. All six
components live under `packages/`. A commit touching only `crates/`
yields "No commits for path: packages/..., skipping" -> no release PR
-> the existing auto-merge (release-pr-sync.yml) + publish never fire.
Every Rust feature this session (receipt, registry_import_pack, cargo
pack) hit this and never released.

A prior attempt (a `.` `simple` root component + `version.txt`, reverted
in commit 30929d9) FAILED: it created a SECOND version source;
linked-versions did not reconcile it; no release was cut. Do not repeat.

## Decision (Codex consult + independent verification, 2026-05-27)

**Mechanism: synthesized release-attribution pre-step (Codex option b).**
A step in `release-please.yml`, BEFORE the release-please action,
detects releasable `crates/**` commits since the last release tag and,
if found, pushes ONE deterministic sentinel commit under
`packages/terminal-commander/`. That commit is path-attributed to the
canonical component, so the next release-please run bumps the existing
version source and the whole existing chain (linked-versions ->
release-pr-sync auto-merge -> build -> presmoke -> publish) runs
unchanged.

Why this and not the alternatives (all confirmed against the
release-please schema + docs):
- **No config-only fix exists.** release-please's schema has
  `exclude-paths` but NO `include-paths`. A package cannot be told to
  watch a sibling dir. (Verified against
  schemas/config.json.)
- **`.` root component:** `.` = "any change" (too broad);
  `release-type: node` needs a root `package.json` (absent);
  `release-type: simple` reintroduces the `version.txt` competing
  source that already failed.
- **`extra-files` / `changelog-sections`:** run only AFTER a candidate
  release exists; they don't make release-please CONSIDER `crates/**`
  commits.
- **Custom plugin/fork:** inherits release-please internals for one
  path-alias feature; worse operationally.

**Verified assumption (the one Codex did not check):** `main` is NOT
branch-protected (GitHub API: "Branch not protected", 404). So a direct
CI push to `main` with `RELEASE_PLEASE_TOKEN_TC` succeeds. The
mechanism is viable with no protection workaround.

## Design

### New: scripts/release/synthesize-crates-release-trigger.sh

A bash script run by `release-please.yml`. Behavior:

1. Resolve the canonical version + base tag:
   `ver=$(node -p "require('./packages/terminal-commander/package.json').version")`,
   `base_tag="v${ver}"`.
2. Collect releasable crate commits since the tag:
   `git log --format=%H%x09%s "${base_tag}..HEAD" -- crates`, keep
   subjects whose conventional type is `feat`, `fix`, or breaking
   (`feat!`/`fix!`/`BREAKING CHANGE`). Determine the STRONGEST type
   present (breaking > feat > fix) -> the sentinel commit's conventional
   type, so release-please computes the right bump (major-ish/minor/
   patch under the repo's `bump-*-pre-major` rules).
3. Compute a loop-guard fingerprint: a short hash of the sorted crate
   commit SHAs in range (e.g. `git log ... -- crates | sha256 | head`).
4. Sentinel file: `packages/terminal-commander/.release-please-crates-trigger`.
   If it already exists AND its stored fingerprint == the computed one,
   do nothing (`trigger_pushed=false`) -- prevents infinite re-trigger
   loops (the sentinel commit itself is under packages/, would otherwise
   re-trigger forever).
5. Otherwise write the sentinel (fingerprint + the crate SHAs as a
   human-readable manifest), commit it as
   `<type>: release Rust crate changes` (type from step 2) with the
   github-actions bot identity + `RELEASE_PLEASE_TOKEN_TC`, push to
   `main`, set output `trigger_pushed=true`.
6. No releasable crate commits -> no-op (`trigger_pushed=false`).

The script is idempotent: re-running on the same HEAD with the sentinel
already present + matching fingerprint is a no-op. The fingerprint
covers exactly the crate commits in `(base_tag, HEAD]`, so once a
release tags a new version, base_tag advances, the range empties (or
holds only new commits), and the fingerprint changes only when new
crate work lands.

### Modified: .github/workflows/release-please.yml

Before the `release-please (manifest mode)` step (~line 117), add:
- Checkout `main` with `fetch-depth: 0` + `token:
  RELEASE_PLEASE_TOKEN_TC` (full history needed for the tag..HEAD log;
  token needed to push).
- A step `id: crates_trigger` running the script, gated
  `if: github.event_name == 'push' && inputs.recovery_release_as == ''`
  (don't run on workflow_dispatch recovery).
- Gate the existing `release-please (manifest mode)` step with
  `if: steps.crates_trigger.outputs.trigger_pushed != 'true'` -- when a
  trigger was just pushed, SKIP release-please in THIS run; the pushed
  commit starts a fresh `push: main` run that release-please handles
  normally (clean handoff, no double-processing).
- Gate the `ensure-release` job similarly:
  `&& needs.release-please.outputs.trigger_pushed != 'true'` (expose
  `trigger_pushed` as a release-please job output) so the recovery job
  doesn't fire on the trigger-push run.

### Net flow

```
push feat: under crates/
  -> synthesize-crates-release-trigger.sh: finds the feat, writes +
     pushes packages/terminal-commander/.release-please-crates-trigger
     as "feat: release Rust crate changes"   [trigger_pushed=true]
  -> release-please step SKIPPED this run
  -> the trigger push fires a NEW push:main run
     -> release-please attributes the sentinel commit to
        packages/terminal-commander -> bumps version -> opens release PR
     -> release-pr-sync auto-merges (no human)
     -> merge -> build -> presmoke (bookworm load) -> publish npm + cargo
  => npm install -g terminal-commander gets the new binary
```

A `docs:`/`chore:`/`test:` crates commit -> no releasable type -> no
sentinel -> no release (correct).

## Verification

release-please + Actions can't run locally. Proof is the next push:
- A `feat:` touching `crates/` triggers the synth step -> sentinel
  pushed -> a follow-up run cuts e.g. 0.1.14 -> auto-merge -> presmoke
  -> publish -> `npm install -g terminal-commander@0.1.14` works on a
  clean bookworm box.
- A `docs:`-only push -> no sentinel, no release.
Local static checks before push:
- `bash -n scripts/release/synthesize-crates-release-trigger.sh` (syntax).
- A dry-run harness: run the script's detection logic against the real
  repo history with push DISABLED (a `--dry-run`/env guard) to confirm
  it correctly identifies the current crate commits since v0.1.13 and
  computes a stable fingerprint, WITHOUT pushing. Runs in WSL.
- `python3 -c yaml.safe_load` on release-please.yml.

## Risks + mitigations

- **Trigger loop:** the sentinel commit is under packages/ and would
  re-trigger release-please forever. Mitigated by the fingerprint guard
  (same crate-SHA set -> no-op) AND the action-skip on the trigger-push
  run.
- **Extra commit on main:** each crates release adds one bot commit.
  Acceptable; it carries the crate SHAs as an audit trail.
- **Token push to main:** verified main is unprotected, so the push
  succeeds. If protection is added later, the token needs bypass or the
  step needs a PR path.
- **Double release:** the action-skip ensures only the follow-up run
  processes the release; the trigger run does nothing release-wise.

## Out of scope

- Re-cutting v0.1.13 (already shipped) or the missed crates releases
  from this session -- the first crates feat: after this lands will
  sweep them all into one bump (the tag..HEAD range includes them).
- The pre-publish presmoke gate (already shipped, commit d0fd8b4).
- macOS/windows release mechanics (unchanged).

## Provenance

Codex CLI 0.133.0 consult (option-b recommendation, fingerprint guard,
action-skip handoff), 2026-05-27, this session. Independent verification:
release-please schema has no include-paths; main is not branch-protected
(GitHub API 404). Reverted prior attempt: commit 30929d9. Canonical
version source: scripts/release/sync-optional-dependencies.js reads
packages/terminal-commander/package.json.
