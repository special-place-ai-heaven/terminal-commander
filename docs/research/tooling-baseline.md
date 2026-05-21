# Tooling Baseline

Status: RECOMMENDED
Last verified: 2026-05-21
Researcher: R2-gamma

## Scope

Required CI / pre-commit tooling for the Terminal Commander 2026-era Rust
workspace. Every tool below was verified as actively maintained (latest
release date and commit cadence checked) and confirmed installable from a
canonical source.

| Tool | Purpose | Latest version | License | Source |
|---|---|---|---|---|
| `rustfmt` | Format Rust source | v1.6.0 (2023-07-01) plus rustup nightly track | MIT OR Apache-2.0 | https://github.com/rust-lang/rustfmt |
| `clippy` | Lint Rust source | bundled with each stable toolchain (25k+ commits, active) | MIT OR Apache-2.0 | https://github.com/rust-lang/rust-clippy |
| `cargo-deny` | License / advisories / dup / bans / source policy | 0.19.6 (2026-05-11) | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/cargo-deny |
| `cargo-machete` | Detect unused dependencies | 0.9.2 (2026-04-15) | MIT | https://github.com/bnjbvr/cargo-machete |
| `cargo-hack` | Feature matrix testing | 0.6.44 (2026-03-20) | MIT OR Apache-2.0 | https://github.com/taiki-e/cargo-hack |
| `cargo-nextest` | Faster test runner | 0.9.136 (2026-05-17) | MIT OR Apache-2.0 | https://github.com/nextest-rs/nextest |

All tools confirmed active within ~60 days of the verification date. No
abandoned-project risk for any baseline tool.

## 1. rustfmt

Source: https://github.com/rust-lang/rustfmt

Install (via rustup, the supported path):

```bash
rustup component add rustfmt
```

It ships in the official Rust toolchain. Pin the toolchain in
`rust-toolchain.toml` so CI and every developer machine match:

```toml
# rust-toolchain.toml
[toolchain]
channel    = "1.90.0"           # or 1.92.0, resolved by msrv.md
components = ["rustfmt", "clippy"]
profile    = "minimal"
```

Configuration goes in `rustfmt.toml` at the workspace root:

```toml
# rustfmt.toml
edition       = "2024"
style_edition = "2024"
max_width     = 100
use_field_init_shorthand = true
use_try_shorthand        = true
imports_granularity      = "Module"   # nightly-only knob; remove if running stable rustfmt
group_imports            = "StdExternalCrate"  # nightly-only
newline_style            = "Unix"
```

The rustfmt README explicitly recommends "configuring both the edition and
style_edition in your rustfmt.toml" to ensure `cargo fmt` and direct
`rustfmt` invocations agree. The two nightly-only options are nice-to-haves;
either commit to running formatting on a nightly toolchain in CI (common
pattern), or drop them on the stable channel.

Source: https://github.com/rust-lang/rustfmt (README, configuration section)

CI invocation:

```bash
cargo fmt --all -- --check
```

Pre-commit invocation (faster, no `--check` so it auto-formats):

```bash
cargo fmt --all
```

## 2. clippy

Source: https://github.com/rust-lang/rust-clippy

Install (bundled with rustup; declared in `rust-toolchain.toml` above).

Configure in **two complementary places**:

### `clippy.toml` (per-lint knobs)

```toml
# clippy.toml
avoid-breaking-exported-api = false
msrv = "1.90"            # matches rust-version in workspace.package
disallowed-macros = ["std::dbg"]
```

Source: https://github.com/rust-lang/rust-clippy (configuration section)

### `[workspace.lints]` in `Cargo.toml`

This is the canonical 2026 path - declare lint *levels* in Cargo.toml so they
flow through `cargo check`/`cargo clippy` and are inherited by every member
crate. See workspace-layout.md for the full block. Minimum:

```toml
[workspace.lints.rust]
unsafe_code     = "forbid"
unused_must_use = "deny"

[workspace.lints.clippy]
pedantic    = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
dbg_macro   = "deny"
todo        = "warn"
```

And in every member crate `Cargo.toml`:

```toml
[lints]
workspace = true
```

The `[lints]` table was stabilized in **Rust 1.74** (2023-11-16). MSRV 1.90
clears that. Source:
https://blog.rust-lang.org/2023/11/16/Rust-1.74.0/

CI invocation:

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`-D warnings` upgrades every warning to an error so CI fails on regressions.

## 3. cargo-deny

Source: https://github.com/EmbarkStudios/cargo-deny

Install:

```bash
cargo install --locked cargo-deny
# or pinned: cargo install --locked --version 0.19.6 cargo-deny
```

Init template:

```bash
cargo deny init
```

This writes `deny.toml` at the workspace root. Customize it for Terminal
Commander's Apache-2.0 + Category-A policy:

```toml
# deny.toml (excerpt - the four sections cargo-deny checks)

[graph]
targets = [
  { triple = "x86_64-unknown-linux-gnu" },
  # Add aarch64-unknown-linux-gnu later if ARM CI is added.
  # Windows triples can be added once portable-pty path lands.
]
all-features = false
no-default-features = false

[licenses]
version = 2
# We are Apache-2.0; we accept Category-A inbound deps per ASF policy.
# https://www.apache.org/legal/resolved.html
allow = [
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "MIT",
  "MIT-0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "CC0-1.0",        # notify core crate
  "Unicode-3.0",    # commonly seen in unicode-ident, icu, etc.
  "Unicode-DFS-2016",
]
# unlicense flagged separately - decide case-by-case before allowing.
confidence-threshold = 0.93

[bans]
multiple-versions = "warn"
wildcards = "deny"
deny = []
skip = []
skip-tree = []

[advisories]
version = 2
db-path = "~/.cargo/advisory-db"
db-urls = ["https://github.com/rustsec/advisory-db"]
yanked  = "deny"
ignore  = []   # Document any explicit ignores here with rationale.

[sources]
unknown-registry = "deny"
unknown-git      = "deny"
allow-registry   = ["https://github.com/rust-lang/crates.io-index"]
allow-git        = []   # Add explicit git mirrors here only.
```

CI invocation:

```bash
cargo deny --all-features check
```

This runs all four subchecks (licenses, bans, advisories, sources). For
faster pre-commit runs, you can scope it:

```bash
cargo deny check licenses
cargo deny check bans
cargo deny check advisories
cargo deny check sources
```

A GitHub Action wrapper exists:
https://github.com/EmbarkStudios/cargo-deny-action

## 4. cargo-machete

Source: https://github.com/bnjbvr/cargo-machete

Install:

```bash
cargo install --locked cargo-machete
```

Purpose: detects dependencies declared in `Cargo.toml` but never used in
source. Reduces compile time and shrinks the attack surface for supply-chain
issues.

Configuration: per-crate metadata in each `Cargo.toml` for false positives:

```toml
[package.metadata.cargo-machete]
ignored = ["uuid"]    # only if cargo-machete misidentifies; document why
```

CI invocation:

```bash
cargo machete --with-metadata
```

Exit codes: 0 means clean; 1 means unused deps found (CI should fail);
2 means tool error. The `--with-metadata` flag enables better detection of
re-exports and macro-driven usage at the cost of a slower run.

## 5. cargo-hack

Source: https://github.com/taiki-e/cargo-hack

Install:

```bash
cargo install --locked cargo-hack
```

Purpose: drive feature-combination matrices over `cargo check`/`cargo test`.
Critical for a multi-crate workspace once feature flags appear (e.g. the
daemon's optional `systemd`/`launchd` integrations).

CI invocations for the MVP:

```bash
# Verify no feature combination breaks compilation (cheap):
cargo hack check --workspace --feature-powerset --no-dev-deps

# Verify the package compiles at the declared MSRV:
cargo hack check --workspace --rust-version
```

`--feature-powerset` enumerates every subset of features. `--each-feature`
is a cheaper subset that only tests each feature individually plus default +
no-default. Start with `--each-feature` on PRs and run `--feature-powerset`
only on main / nightly cron jobs as feature surface grows.

Source for the flag semantics:
https://github.com/taiki-e/cargo-hack (README, feature options section)

## 6. cargo-nextest

Source: https://github.com/nextest-rs/nextest

Install:

```bash
cargo install --locked cargo-nextest
```

Purpose: faster Rust test runner with better isolation and richer output
than `cargo test`. Per the nextest README, it is "a next-generation test
runner for Rust" and is dual-licensed Apache-2.0/MIT. Active maintenance:
0.9.136 released 2026-05-17, 445 releases total.

Configuration in `.config/nextest.toml`:

```toml
# .config/nextest.toml
[profile.default]
fail-fast    = false
retries      = 0
slow-timeout = { period = "30s", terminate-after = 2 }

[profile.ci]
fail-fast    = false
retries      = 2          # tolerate the rare flaky integration test
slow-timeout = { period = "60s", terminate-after = 3 }
final-status-level = "all"
```

CI invocation:

```bash
cargo nextest run --workspace --profile ci
```

Keep `cargo test` working as well (it remains the canonical doctest runner;
nextest does not run doc-tests). Recommended pattern:

```bash
cargo nextest run --workspace --profile ci
cargo test --workspace --doc
```

Why faster: nextest spawns each test as a separate process (vs. cargo test's
single binary per test crate), enabling true parallel scheduling and proper
isolation of static/global state. It also surfaces failures earlier and
deduplicates output.

Optional (not baseline, future): nextest has a `partition` flag for
sharding tests across CI runners. Useful only once the test suite reaches
~5 minutes wall-clock.

## Recommended CI command sequence

For a GitHub Actions workflow (or any CI), the canonical fail-fast sequence
is **format -> lint -> deny -> compile -> test -> machete**:

```bash
# 1. Format check (fast, fails immediately on diff).
cargo fmt --all -- --check

# 2. Clippy on the full workspace with all targets and features.
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 3. License/advisory/duplicate/source policy.
cargo deny --all-features check

# 4. Compile matrix (each individual feature on/off).
cargo hack check --workspace --each-feature --no-dev-deps

# 5. MSRV gate: compile with the declared rust-version.
cargo hack check --workspace --rust-version

# 6. Tests (nextest for unit + integration, cargo test for doctests).
cargo nextest run --workspace --profile ci
cargo test --workspace --doc

# 7. Unused-dep audit.
cargo machete --with-metadata
```

Ordering rationale:

- Format/lint first because they fail in seconds and surface 90% of style
  issues without touching the compile graph deeply.
- `cargo deny` before the full compile so a forbidden license fails the run
  before we burn the long tests.
- `cargo hack` before `nextest` so a broken feature is caught before the
  slow test phase.
- `machete` last because it requires a green compile to be informative.

## Recommended pre-commit sequence

Pre-commit must be **fast** (seconds, not minutes). Suggested local hook:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --profile default --no-fail-fast
```

Defer `cargo deny`, `cargo hack`, and `cargo machete` to CI - they add tens
of seconds and are not essential for catching the kinds of mistakes that
escape during local iteration.

If using the `pre-commit` framework (https://pre-commit.com/), both
cargo-deny and cargo-machete publish official pre-commit hooks. cargo-deny
hook docs: https://github.com/EmbarkStudios/cargo-deny (README; see
"pre-commit hook" section). cargo-machete hook: shipped in-repo at
https://github.com/bnjbvr/cargo-machete (README mentions it as a
"pre-commit hook" install option).

## Optional / deferred tooling (not baseline)

- `cargo-audit` (rustsec/rustsec) - **subsumed by cargo-deny advisories**.
  Don't dual-run; pick one. cargo-deny wins for this project because we
  already need its license/bans/sources checks.
- `cargo-udeps` - similar goal to cargo-machete but requires nightly.
  cargo-machete is the stable-toolchain equivalent and the right pick.
- `cargo-llvm-cov` / `tarpaulin` - coverage reporting. Useful later; not
  required for MVP.
- `cargo-msrv` (https://github.com/foresterre/cargo-msrv) - automatically
  resolves the lowest passing MSRV. Useful when negotiating an MSRV bump;
  cargo-hack's `--rust-version` is sufficient for *checking* against a
  fixed MSRV, which is the MVP need.
- `cargo-vet` (Google/Mozilla) - third-party-crate review attestation. Adopt
  only if the project takes external contributions at scale.

## Action items for repo bootstrap

1. Commit `rust-toolchain.toml` pinning the chosen MSRV (1.90 or 1.92).
2. Commit `rustfmt.toml`, `clippy.toml`, `deny.toml`, `.config/nextest.toml`
   with the templates above.
3. Add `[workspace.lints]` block to root `Cargo.toml` and
   `[lints] workspace = true` to each crate `Cargo.toml`.
4. Wire the seven-step CI command sequence into the GitHub Actions workflow
   (or whatever CI is chosen).
5. Verify each tool installs cleanly in CI from `cargo install --locked`
   (use `taiki-e/install-action` on GitHub Actions for cached prebuilt
   binaries of cargo-hack, cargo-nextest, cargo-deny, cargo-machete -
   roughly 10x faster than `cargo install`).

## Source verification log

All tool versions/maintenance status fetched 2026-05-21:

- rustfmt: v1.6.0 released 2023-07-01; component shipped with every stable
  rustup channel. Repository shows 6,106 commits, ongoing activity.
- clippy: 25,324 commits, 2.5k open issues, 323 PRs - actively maintained.
- cargo-deny: 0.19.6 released 2026-05-11.
- cargo-machete: 0.9.2 released 2026-04-15.
- cargo-hack: 0.6.44 released 2026-03-20.
- cargo-nextest: 0.9.136 released 2026-05-17 (445 releases lifetime).
