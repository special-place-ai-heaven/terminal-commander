# Contributing to Terminal Commander

Status: Baseline (TC01 wave 0 deliverable).
Language: ASCII only. No smart quotes, no em-dashes.

This document defines how changes land in Terminal Commander. It is
mandatory reading for any contributor (human or AI). Every rule here
is enforced by branch policy, CI, or the goal-driven workflow.

## 1. Branch policy

The default working branch for the MVP chain is:

```text
feature/terminal-commander-mvp
```

- `main` and `master` are prohibited working branches for any goal
  in the `terminal-commander-mvp` chain. Direct edits to `main` or
  `master` are not allowed.
- Every goal file declares `target_branch` and `prohibited_branches`
  in its frontmatter. The branch guard at the top of each goal file
  must be run before any edit.
- A new branch may be created only when an explicit goal scopes it.
- Push/force-push/PR creation against `main` requires explicit user
  approval per the gstack CLAUDE.md rules.

Branch-guard command (run before any edit):

```bash
git branch --show-current
git status --short
```

The output of `git branch --show-current` MUST match the goal file's
`target_branch` exactly. If it does not, stop and switch.

## 2. License and per-file header

Terminal Commander is licensed under the PolyForm Noncommercial
License 1.0.0. You may inspect, study, and use the source for
noncommercial purposes; commercial use requires a separate license.

- SPDX identifier: `PolyForm-Noncommercial-1.0.0`
- Repo root: `LICENSE` (full PolyForm Noncommercial 1.0.0 text) and
  `NOTICE` file (project notice + third-party dependency notices).
- Cargo manifest field on every member crate:
  `license.workspace = true` with the workspace setting being
  `license = "PolyForm-Noncommercial-1.0.0"`.
- By contributing, you agree your contributions are licensed under the
  same PolyForm Noncommercial License 1.0.0.

Every source file MUST carry the short SPDX header:

```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
```

Long-form headers are also acceptable but not required; pick one
form and stay consistent within a file. Generated files
(e.g. `target/`, `Cargo.lock`) are exempt.

## 3. Toolchain

- MSRV floor: Rust 1.92.0 (set by rmcp 1.7.0). Edition 2024. This is
  the *documented* minimum-supported version, NOT a CI-enforced gate:
  no workflow runs `cargo hack --rust-version`, so the 1.92.0 claim is
  a promise, not an automated check (see section 6).
- Active dev/CI pin: Rust 1.95.0, pinned in `rust-toolchain.toml`
  (`channel = "1.95.0"`). This is the toolchain the gate scripts and CI
  actually run. Install it via rustup; the repo's `rust-toolchain.toml`
  selects it automatically.
- Required components: `rustfmt`, `clippy`.
- All package names in this workspace use hyphens; in-code module
  names use underscores (e.g. crate `terminal-commander-core` is
  imported as `terminal_commander_core`).

## 4. Tooling baseline

Source: `docs/research/tooling-baseline.md`. The full baseline lands
in TC04. Summary:

| Tool | Purpose | Status |
|---|---|---|
| `rustfmt` | format Rust source | required (gate) |
| `clippy` | lint Rust source (warnings = errors) | required (gate) |
| `cargo-nextest` 0.9+ | faster test runner | required (gate) |
| `cargo-deny` 0.19+ | license / advisories / bans / sources policy | recommended (not yet in CI) |
| `cargo-machete` 0.9+ | detect unused dependencies | recommended (not yet in CI) |
| `cargo-hack` 0.6+ | feature matrix + MSRV gate | recommended (not yet in CI) |

`required (gate)` = enforced by the PR gate scripts that CI runs
(`scripts/linux-gate.sh` / `scripts/windows-gate.ps1`; see section 6).
`recommended (not yet in CI)` = part of the aspirational baseline in
`docs/research/tooling-baseline.md` but NOT wired into any workflow
today; run them locally if you wish, but they do not gate merges.

Coverage tooling and `cargo-vet` are not part of the MVP baseline.

### 4.1 cargo-deny license allowlist

`notify` core ships as CC0-1.0; `notify-debouncer-full` and
`file-id` are MIT OR Apache-2.0. The `cargo-deny` license allowlist
MUST include `CC0-1.0` explicitly or the supply-chain gate breaks at
build time. See `docs/research/tooling-baseline.md` section 3.

## 5. Pre-commit subset

The authoritative pre-push check is the PR gate itself. CI invokes
`scripts/linux-gate.sh` (and `scripts/windows-gate.ps1`) directly, so
running them locally is the same check that gates your PR — there is no
drift to second-guess:

```bash
bash scripts/linux-gate.sh        # linux/mac
pwsh scripts/linux-gate.ps1       # Windows (runs the linux gate via WSL)
pwsh scripts/windows-gate.ps1     # Windows-only regression gate
```

For a fast inner loop you can run the gate's hot subset by hand:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --profile default --no-fail-fast
```

`cargo deny`, `cargo hack`, and `cargo machete` are NOT part of the PR
gate (they are not wired into any workflow; see section 4). Run them
locally only if you want the extra coverage.

## 6. CI sequence

The PR gate CI actually runs is `scripts/linux-gate.sh` (linux) plus
`scripts/windows-gate.ps1` (windows). Those scripts are the single
source of truth: `npm-binary-build.yml`'s `pre-build-gates*` jobs invoke
them, so the commands in the scripts ARE the gate. Read the scripts for
the exact, current command list.

The seven-step pipeline below is the ASPIRATIONAL baseline recorded in
`docs/research/tooling-baseline.md`. It is NOT the PR gate today: steps
3, 4, 5, and 7 (`cargo deny` / `cargo hack` / `cargo machete`) are not
wired into any workflow, and step 2's clippy here uses `--all-features`
whereas the real gate (`scripts/linux-gate.sh`) runs clippy WITHOUT
`--all-features`. Treat this block as the target state, not the
authoritative gate:

```bash
# 1. Format check.
cargo fmt --all -- --check

# 2. Clippy on the full workspace.
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 3. License / advisory / dup / source policy.
cargo deny --all-features check

# 4. Compile matrix (each individual feature on/off).
cargo hack check --workspace --each-feature --no-dev-deps

# 5. MSRV gate: compile with the declared rust-version.
cargo hack check --workspace --rust-version

# 6. Tests (nextest for unit + integration, cargo test for doctests).
cargo nextest run --workspace
cargo test --workspace --doc

# 7. Unused-dep audit.
cargo machete --with-metadata
```

Rationale: format and lint fail in seconds and catch most style
issues; cargo-deny gates the supply chain before the long compile;
cargo-hack gates feature combinations before the slow test phase;
machete runs last because it needs a green compile.

### 6.1 OS-specific code

When a change touches `cfg(unix)` / `cfg(windows)` / `target_os` code
or ANY test, run both gates before pushing:

```bash
# Windows
pwsh scripts/linux-gate.ps1     # runs the linux gate inside WSL
pwsh scripts/windows-gate.ps1   # runs the windows-only regression gate

# linux / mac
bash scripts/linux-gate.sh
```

A single-OS `cargo test` / `cargo clippy` does NOT compile the other
OS's `cfg` paths, so an OS-gated change can pass locally and still break
the other platform's gate in CI. Running both gates is the only way to
exercise both `cfg` worlds before you push.

When fixing a platform-asymmetric daemon surface (a `cfg` gate that hides
behavior on one OS), ship three pieces together: the production fix, a
Windows or Unix regression test, and — for Windows cfg sentinels that
headless CI cannot exercise live — registration in `scripts/windows-gate.ps1`
so the guard runs before merge. Live dogfood on the affected OS remains a
required author step with evidence; mark live acceptance `UNVERIFIED` only
with an explicit stated reason in the PR.

`scripts/linux-gate.sh` IS the linux PR gate that CI runs (the single
source of truth — `npm-binary-build.yml` invokes it directly).
`scripts/dev/verify-baseline.sh` remains a separate fixture / doctrine
check and is NOT the PR gate; do not conflate the two.

One-time WSL provisioning (Windows devs, so `scripts/linux-gate.ps1`
can run the linux gate):

- rustup, plus the pinned `1.95.0` toolchain (matches
  `rust-toolchain.toml`)
- `cargo-nextest`
- `node`
- `python3` (the TC47 load gate self-skips to a false pass without it)

## 7. Goal-driven workflow

Every behavior-changing edit in this repository must trace to a
goal file under `.agent/goals/terminal-commander-mvp/`. The goal
file's mini-spec is authoritative for that change.

Workflow per edit:

1. Identify the goal. If no goal scopes the change, stop and create
   one (or escalate).
2. Read the goal file end to end. Run the branch guard.
3. Set `status` to `In progress` and set `started_at` in the
   frontmatter.
4. Execute strictly inside `allowed_files_or_area`. Stop on any
   `stop_conditions` hit.
5. Run the goal's `verification_command`. The verification must
   prove the acceptance criteria, not just compile.
6. Commit verified work. Update the frontmatter: set `status` to
   `Completed`, set `completed_at`, set `completion_commit` to the
   verified work commit hash. Commit the frontmatter update as a
   separate commit.
7. If blocked, set `status` to `Blocked`, set `blocked_reason`,
   leave `completion_commit` empty unless a verified partial commit
   exists. Report the blocker rather than guess.

### 7.1 Allowed-file discipline

Each goal lists `allowed_files_or_area` and `forbidden_files`. The
allowed set is the entire scope of edits for that goal. A change
that requires files outside the allowed set is out of scope; create
a follow-on goal rather than expanding silently.

### 7.2 Evidence

Each goal requires `evidence_required` items. The final report MUST
contain that evidence. Examples: command output summaries, file
paths changed, verification command results, route or status
evidence, and a source-status note for any partial/disabled/test-
only behavior touched.

## 8. Commit conventions

- One concise subject line (under 70 chars where practical).
  Imperative mood. No emojis.
- Body explains the why, not the what. Reference the goal id in the
  body where applicable, e.g. `TC15: ...`.
- A commit must touch only files within the active goal's allowed
  set unless it is a goal-status-update commit (a separate commit
  that touches only the goal frontmatter).
- Co-author footer for AI contributions:

```text
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

## 9. No-mock invariant

Per the project's prime directive:

- No stubs, mocks, TODOs, placeholders, dormant actors, unwired
  ports, synthetic completion paths, or fake-success paths may be
  shipped as "completed" functionality.
- Test-only helpers stay isolated to `tests/` or `fixtures/`. They
  must not be reachable from a production build path.
- A component that is intentionally not implemented in this goal
  must surface that explicitly (e.g. via `system_discover` reporting
  `deferred`, or via an explicit MCP error, or via a clearly named
  feature flag that is off by default). It must never present as
  `live`.
- The "did it compile" check is not a verification of behavior. Any
  goal whose acceptance criteria mention behavior MUST run the
  relevant test, demo, or evidence-producing command, not just
  `cargo check`.

## 10. Documentation expectations

- Every public type, MCP tool, and CLI subcommand must be documented
  before it is treated as live.
- ARCHITECTURE.md, SPEC.md, ROADMAP.md, and this file are the
  cross-goal contracts. Changes to those land through a goal that
  lists them in `allowed_files_or_area`. Do not update them as a
  side effect of an implementation goal.
- Research documents under `docs/research/` are immutable historical
  evidence for TC01. New research lands in new files, not in edits
  to existing TC01 research files.
