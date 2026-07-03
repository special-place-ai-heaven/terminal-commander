# Wave 2 (P2) Integration Evidence

**Date**: 2026-07-03
**Integrated tree**: `002-dogfood-remediation` HEAD after merging
US4 `2163fe8`, US5 `012d6d0`, US8 `ccc8ee0`, US6 `a9aef11`
(merge commits on top of Wave 1 `c4c6f82`)
**Compared against**: T001 baseline (`baseline.md`) and Wave 1
(`evidence-wave1.md`, Win 860 / WSL 1106)

## Both-platform gate on the merged tree

| Platform | Command | Result | Delta vs Wave 1 |
|---|---|---|---|
| Windows | cargo fmt --all --check | PASS | - |
| Windows | cargo clippy --workspace --all-targets -- -D warnings | PASS | - |
| Windows | cargo nextest run --workspace | PASS — 865 run: 865 passed (1 leaky), 1 skipped | +5 tests, 0 new failures |
| WSL | cargo fmt --all --check | PASS | - |
| WSL | cargo clippy --workspace --all-targets -- -D warnings | PASS | - |
| WSL | cargo nextest run --workspace | PASS — 1136 run: 1136 passed, 1 skipped | +30 tests, 0 new failures |

The Windows/WSL test-count gap widened because US5 (event_context,
pty_stdin) and US8 (WSL classifier lanes) land most of their behavioral
coverage in `#[cfg(unix)]` suites.

## Stories integrated

| Story | FRs | Note |
|---|---|---|
| US4 token-lean streaming | FR-030, FR-031 | compact on wait/events (adapter-only), liveness delta on sub_pull (wire opt-in, adapter always-on). SC-004 measured 84.7% byte reduction (target >=60%), asserted in `sc004_token_lean_bytes.rs`; see `evidence-sc004.md`. |
| US5 fewer round-trips | FR-040, FR-041 | event_context by event_id alone (bucket scan when omitted; supplied path byte-identical); pty_stdin bounded wait_ms returns combed signals in one call; no-wait response byte-identical. |
| US8 WSL nested-shell gate | FR-060, FR-061 | fail-closed classifier on both argv lanes (bare/`--` payloads shell-interpreted -> gated; `~` selector; unknown -> denied under allow_shell=false). Security profile 11/11. Stance documented in repo-root POLICY.md. |
| US6 file append | FR-022 | policy-gated append after the unchanged security-critical guard order; honest integrity contract (no interleave, partial-on-I/O-failure is an error, not all-or-nothing). |

## Notes

- Merge order US4 -> US5 -> US8 -> US6, all four via `--no-ff`. The ort
  strategy auto-resolved every shared-file overlap (protocol.rs, tools.rs,
  server.rs, pty*.rs) with ZERO manual conflicts — the additive wire
  posture and disjoint edit regions paid off vs Wave 1's export-list churn.
- POLICY.md path: the US8 contract named `docs/security/POLICY.md`, which
  does not exist; the canonical policy doc is repo-root `POLICY.md` (the
  file the constitution's derivation list actually references), so the
  FR-061 stance was correctly added there.
- The "1 leaky" Windows test is the same pre-existing one from baseline.
- The known load-sensitive flakes (4 CLI live-daemon tests; the
  compact_surface legacy timeout) did NOT recur on this quieter run —
  every test passed. Still recorded as US9 decision input.
- Story-level red->green evidence (per FR) is in the Wave 2 agent reports;
  each worktree passed its own Windows gate before merge, and US8
  additionally ran the security profile.

**Verdict**: US4, US5, US6, US8 integrated and verified. SC-008 holds:
zero new failures attributable to the wave on either platform. P2 complete.
Remaining: Wave 3 (US7 heuristics + US9 decision) and polish (T049-T052).
