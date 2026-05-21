# R2-gamma Research Summary

Branch: feature/terminal-commander-mvp
Researcher: R2-gamma
Date: 2026-05-21
Scope: TC01 baseline group G (license, workspace layout, tooling).

## One-line recommendations

| Topic | Recommendation | Confidence |
|---|---|---|
| License (G1) | Apache-2.0 with SPDX header per file; commit LICENSE + minimal NOTICE; cargo-deny allows Category-A + CC0 (for `notify`). | High |
| Workspace layout (G2) | Flat virtual manifest, `members = ["crates/*"]`, `resolver = "3"`, all seven locked crate names under `crates/<name>/`, shared deps + metadata + lints centralized in `[workspace.*]`. | High |
| Tooling baseline (G3) | rustfmt + clippy (both via rustup), cargo-deny 0.19, cargo-machete 0.9, cargo-hack 0.6, cargo-nextest 0.9. Seven-step CI sequence: fmt -> clippy -> deny -> hack each-feature -> hack rust-version -> nextest+doctest -> machete. | High |

## Files written

- `docs/research/license-decision.md` (G1)
- `docs/research/workspace-layout.md` (G2)
- `docs/research/tooling-baseline.md` (G3)
- `docs/research/_R2-gamma-summary.md` (this file)

No files outside `docs/research/` touched. No git operations performed.

## HALT-worthy findings

None. All three areas resolved cleanly with consistent, externally cited
recommendations. No legal blockers, no abandoned tools, no version pin
conflicts surfaced during research.

## Key facts discovered (with citations)

| Claim | Source |
|---|---|
| Apache-2.0 SPDX id is `Apache-2.0`; OSI-approved permissive license. | https://spdx.org/licenses/Apache-2.0.html |
| Apache-2.0 NOTICE-propagation obligation lives in Section 4 of the license. | https://www.apache.org/licenses/LICENSE-2.0 |
| MIT, BSD 2/3-clause, and CC0 are ASF Category A (Apache-2.0 compatible inbound). | https://www.apache.org/legal/resolved.html |
| Apache-2.0 is incompatible with GPLv2 but compatible with GPLv3. | https://www.apache.org/foundation/license-faq.html |
| `notify` core crate is CC0-1.0; debouncer-full + file-id are MIT OR Apache-2.0. | https://github.com/notify-rs/notify (README) |
| `pty-process` is MIT (X11). | https://raw.githubusercontent.com/doy/pty-process/main/LICENSE |
| `portable-pty` (wezterm) is MIT. | https://github.com/wez/wezterm/blob/main/pty/Cargo.toml |
| `rmcp` SDK is in an Apache-2.0 relicensing transition (Apache-2.0 going forward, legacy MIT, docs CC-BY-4.0). | https://github.com/modelcontextprotocol/rust-sdk/blob/main/LICENSE |
| `rusqlite` is MIT. | https://github.com/rusqlite/rusqlite/blob/master/Cargo.toml |
| `workspace.package` and `workspace.dependencies` inheritance stable since Rust 1.64. | https://doc.rust-lang.org/cargo/reference/workspaces.html |
| `[workspace.lints]` table stabilized in Rust 1.74 (2023-11-16). | https://blog.rust-lang.org/2023/11/16/Rust-1.74.0/ |
| ruff uses `members = ["crates/*"]`, edition 2024, rust-version 1.93. | https://github.com/astral-sh/ruff/blob/main/Cargo.toml |
| bevy uses `resolver = "3"`, dual MIT OR Apache-2.0. | https://github.com/bevyengine/bevy/blob/main/Cargo.toml |
| tokio uses flat named member list, MIT. | https://github.com/tokio-rs/tokio/blob/master/Cargo.toml |
| cargo-deny 0.19.6 released 2026-05-11; covers licenses, bans, advisories, sources. | https://github.com/EmbarkStudios/cargo-deny |
| cargo-machete 0.9.2 released 2026-04-15; detects unused deps via static inspection. | https://github.com/bnjbvr/cargo-machete |
| cargo-hack 0.6.44 released 2026-03-20; flags include `--each-feature`, `--feature-powerset`, `--rust-version`. | https://github.com/taiki-e/cargo-hack |
| cargo-nextest 0.9.136 released 2026-05-17; dual MIT/Apache-2.0; does not run doctests. | https://github.com/nextest-rs/nextest |
| rustfmt v1.6.0 (2023-07-01); shipped via `rustup component add rustfmt`. | https://github.com/rust-lang/rustfmt |
| clippy bundled with stable toolchain; `clippy.toml` + `[workspace.lints.clippy]` are the two config surfaces. | https://github.com/rust-lang/rust-clippy |

## SOURCE_MAP reclassifications

None proposed. All three G-group questions resolved against primary upstream
sources (Apache Foundation, doc.rust-lang.org, official tool repos). No
prior R1 findings need to be re-tiered.

## Unverified / deferred items

These items came up during research and are explicitly **deferred** to
other research notes; they did not require closure to land G1/G2/G3:

1. Exact `rmcp` pin -> defer to `msrv.md` / `mcp-rust-sdk.md` (already
   owned by R1-alpha).
2. Final MSRV value (1.90 vs 1.92) -> defer to `msrv.md`. The workspace
   layout document carries 1.90 as a placeholder consistent with the
   planner's stated range.
3. The `imports_granularity` and `group_imports` rustfmt options are
   nightly-only; the final decision on whether to use a nightly rustfmt in
   CI is left to the implementer. Stable rustfmt is fine with those lines
   removed.
4. `refinery` 0.9 dual-license string was not re-fetched in this pass;
   the planner's lock-in says it is MIT OR Apache-2.0. cargo-deny will
   confirm or reject during the bootstrap PR.
5. Coverage tooling (`cargo-llvm-cov`, etc.) intentionally **not** in
   baseline; flagged in `tooling-baseline.md` as optional/deferred.

## Top three findings worth the planner's attention

1. **`notify` is CC0-1.0**, not MIT or Apache-2.0. ASF Category A blesses
   this for inbound use, but the `cargo-deny` `licenses` allowlist must
   include `CC0-1.0` explicitly or the build will fail at the supply-chain
   gate. (Captured in `license-decision.md` and `tooling-baseline.md`.)
2. **`[workspace.lints]` is the canonical 2026 lint-management surface**,
   stabilized in Rust 1.74 and inherited by member crates with the
   `[lints] workspace = true` opt-in. This is what `clippy.toml` does *not*
   solve - the two work together (clippy.toml for per-lint knobs, workspace
   lints for level configuration).
3. **rmcp itself is undergoing an Apache-2.0 relicensing transition**.
   The repository LICENSE file explicitly describes the migration: new code
   is Apache-2.0, legacy MIT contributions still apply, documentation is
   CC-BY-4.0. Practically, Terminal Commander's Apache-2.0 stance consumes
   rmcp cleanly today and will only become *more* aligned over time. No
   action required, but worth noting because `cargo-deny licenses` may see
   either `MIT OR Apache-2.0` or `Apache-2.0` depending on the rmcp release
   pinned - the allowlist already covers both.

## Branch-guard report

- Branch: `feature/terminal-commander-mvp` (unchanged - no git operations
  performed).
- Files touched (all created, none modified):
  - `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\license-decision.md`
  - `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\workspace-layout.md`
  - `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\tooling-baseline.md`
  - `C:\AI_STUFF\PROGRAMMING\terminal-commander\docs\research\_R2-gamma-summary.md`
- No source code touched. No `Cargo.toml`, no `LICENSE`, no CI workflow
  files created or modified. The bootstrap PR will write those.
