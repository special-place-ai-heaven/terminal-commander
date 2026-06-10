# Field Review — Final Report (2026-06-10)

Companion to [`2026-06-10-field-review-findings.md`](./2026-06-10-field-review-findings.md)
(severity-ordered findings with evidence). This report records what was
verified, what was fixed, on which branches, and what was deliberately left
open.

All work was performed and verified ON the Windows host through Terminal
Commander's own MCP surface (dogfooding): every cargo/git/node command in this
campaign ran via `run_and_watch` / `command_start_combed`, which reproduced
four of the seed findings live as a side effect.

## Verification results

| Gate | Baseline (main @ 4f2d9e4) | Final (branch tips) |
|---|---|---|
| `cargo fmt --all --check` | **FAIL** (TC49 drift) | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | PASS |
| `cargo nextest run --workspace` (parallel) | **637/639, 2 FAIL** (Windows-only) | **650/650 PASS** (1 pre-existing leaky, 1 skipped = new ignored child-helper) |
| windows-gate binaries (`windows_no_console_spawn` incl. ignored, `windows_spawn_site_coverage`) | PASS | PASS (unchanged code paths) |
| npm wrapper `node --test` | **359/360, 1 FAIL** | **360/360 PASS** |
| TC47 load gate / doctests / cargo-deny / cargo-hack | NOT RUN locally (linux-gate / not-in-gate) | NOT RUN locally — CI covers |

## Findings disposition

| # | Finding | Verdict | Fix | Branch |
|---|---|---|---|---|
| S1 | `command_status` counters zero until exit (seed #4) | CONFIRMED (code + live) | `status()` snapshots `metrics_live` for non-terminal jobs + regression test | `fix/tc-field-review-daemon-runtime` |
| S2 | inline `rules_json` silently dropped without `"status":"active"` | CONFIRMED (live) | rules_json definitions normalize to Active in `parse_bucket_and_rules` + test | `fix/tc-field-review-mcp-surface` |
| S3 | Option-typed schemas stripped by real clients → stringified params rejected (seeds #1/#3/#8a) | CONFIRMED (live, repeatedly) | plain `"type"` via `#[schemars(with)]` on ~30 optional fields + lenient string→number/array/object/bool coercion + schema/serde tests | `fix/tc-field-review-mcp-surface` |
| S4 | idle reaper fires with live jobs → orphaned children, dropped waits (seed #5) | restarts = BY DESIGN (idle reap, log-verified); orphan/drop = DEFECT | `has_live_work()` vetoes the reap + regression test | `fix/tc-field-review-daemon-runtime` |
| S5 | two versions of one rule id active in one scope → duplicate events (seed #7) | CONFIRMED (live) | activate-supersedes same-scope versions; `superseded_versions` in response + restart-survival test | `fix/tc-field-review-daemon-runtime` |
| S6 | pack rule brands foreign output as `compile_warning` rust/cargo (seed #6) | CONFIRMED (live, twice) | honest relabel: `event_kind: warning`, language-claim tags dropped, scoped-activation guidance in description | `fix/tc-field-review-tests-and-packs` |
| S7 | Windows CLI live-daemon tests collide on constant `TC_SESSION` pipe + probe misreads pipe instance-gap as down | CONFIRMED (3 runs + manual repro) | unique per-process tokens (3 harnesses); probe retries `ERROR_FILE_NOT_FOUND` boundedly; vacuous-pass assertion tightened | `fix/tc-field-review-tests-and-packs` |
| S8 | main not fmt-clean | CONFIRMED | `cargo fmt --all` | `fix/tc-field-review-mcp-surface` |
| S9 | npm test branches on host WSL state (`TC_WSL_DISTRO` leak) | CONFIRMED | env selectors stripped in the test | `fix/tc-field-review-tests-and-packs` |
| S10 | shell-bridge guard launderable via `wsl.exe` | CONFIRMED (deny list, argv[0]-only) | NOT FIXED — operator decision; documented below | open |
| S11 | `severity: "error"` rejected (seed #2 residual; #2 itself fixed at HEAD) | CONFIRMED | alias map error/err→high, warn/warning→medium, fatal→critical + test | `fix/tc-field-review-mcp-surface` |
| S12 | `ShellInterpreterDenied` doesn't teach remedy (seed #8b) | CONFIRMED | message now names argv remedy + shell_exec lane | `fix/tc-field-review-mcp-surface` |
| S13 | stale global activations poison later sessions | by-design durability; sting reduced by S5+S6 | README guidance (scoped activation TIP) | docs |

## Branches (stacked, in merge order)

1. `fix/tc-field-review-mcp-surface` — findings report, fmt, S2+S3+S11 (mcp), S12 (daemon message). Gates: clippy PASS, mcp+daemon 289/289.
2. `fix/tc-field-review-daemon-runtime` — S1+S4+S5. Gates: workspace clippy PASS, targeted suites 14/14, full suite green at C.
3. `fix/tc-field-review-tests-and-packs` — S6+S7+S9. Gates: fmt PASS, clippy PASS, **workspace 650/650 parallel**, npm 360/360.
4. `docs/readme-overhaul` — README rewrite (docs-only; no code gates affected).

Branches are stacked (each builds on the previous) — merge in order, or merge
`docs/readme-overhaul` which contains all of them. Conventional commits
throughout; the daemon/mcp changes are `fix:` so release-please will cut a
patch release on merge.

## Live evidence collected during this session (dogfooding)

- `rules` array and `wait_ms`/`max_lines`/`start_line`/`limit` numerics
  rejected as strings against the running 0.1.47 daemon (S3) — multiple times,
  including `registry_deactivate.scope` as a stringified object.
- `rules_json` without `status` produced zero signals while the receipt
  claimed "zero rules matched"; identical call with `"status":"active"`
  produced both expected signals (S2).
- One `node -e` stderr line emitted TWO identical `compile_warning`
  rust/cargo events (rule v1+v2, globally active from a prior session) —
  S5 + S6 + S13 in a single response.
- `command_status` mid-`cargo build`: `bytes_total: 0, frames_total: 0` while
  `command_output_tail` returned 50+ captured lines (S1).
- Daemon log: 27 idle-reaps since 2026-05-29, two during the field session
  (09:01:26 idle=1845s, 09:48:06 idle=1819s) — seed #5's "restarts" (S4).
- One spontaneous `degraded: true` "IPC error interrupted the wait" during
  `cargo fmt --check`; the recover_hint flow worked exactly as documented.

## Deliberately left open

- **Graceful long-poll completion on daemon shutdown** — an in-flight
  `bucket_wait` at reap time still gets a dropped connection (degraded
  receipt) rather than a clean "shutting down" reply.
- **Adapter respawn-retry for idempotent RPCs** after idle eviction — the
  first post-reap call still fails `daemon_unavailable` before the next call
  respawns.
- **S10 (`wsl`/`wsl.exe` deny-list entry)** — one-line change, but whether the
  interpreter deny list should claim WSL is an operator/policy decision; the
  guard is interpreter hygiene, not the security boundary (the policy engine
  is).
- **Stale "7 crates" comment** in `release-please.yml` (line ~1005): the chain
  publishes 8 (ipc added by PR #85).
- **Compact response mode** for heavy event objects (repeated metadata per
  event) — ergonomics improvement, not a defect.
- **CLI lacks `rules activate/deactivate`** subcommands — registry hygiene
  currently requires MCP or raw IPC.
- The **1 leaky** test in the workspace suite (pre-existing on main) was not
  investigated.

## Proposed wiki structure (no wiki exists today; not created)

Mirroring the SymForge wiki: Architecture and How It Works · Tool Reference
(per-tool request/response with the contract fixtures as source) · Rules and
Packs Guide · Sessions and Lifecycle (idle reap, reap CLI, boot_id) · Policy
and Security Model · Benchmarks (the 2,802-line → 5-line receipt case study)
· Troubleshooting (degraded receipts, daemon_unavailable, exit 69).

## Recommended next steps

1. Merge the four branches in order; let release-please cut the patch release
   (the S3 interop fix alone is worth shipping fast — it unblocks every
   Claude-family MCP client currently unable to pass `rules` or numerics).
2. Decide S10 (wsl deny-list) and fix the stale workflow comment.
3. Schedule the two reliability follow-ups (graceful long-poll shutdown,
   adapter respawn-retry) as a small TC-5x campaign.
4. Consider porting the S7 unique-token pattern to `status_pid.rs` /
   `session_reap.rs` (single-test files; lower risk, same latent stale-orphan
   exposure).
