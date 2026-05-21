# Workspace Layout

Status: RECOMMENDED
Last verified: 2026-05-21
Researcher: R2-gamma

## Decision summary

Terminal Commander adopts a **flat Cargo workspace** at the repo root using a
**virtual manifest** (no top-level package), with all seven crates living
under `crates/<name>/`. Shared metadata and shared dependency versions are
centralized in `[workspace.package]` and `[workspace.dependencies]`, and
inherited by each member crate with `*.workspace = true`. Lints are managed
via `[workspace.lints]`.

This pattern is the prevailing 2026-era convention (ruff, bevy, tokio, and
~every multi-crate Rust project use a variant of it).

Sources:
- Cargo workspaces reference:
  https://doc.rust-lang.org/cargo/reference/workspaces.html
- Specifying dependencies (workspace inheritance):
  https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html
- Workspace lints (stabilized Rust 1.74):
  https://blog.rust-lang.org/2023/11/16/Rust-1.74.0/

## Locked crate names

Per the planner's lock-in (G2 ledger entry), the seven crates are:

| Path | Crate name | Role |
|---|---|---|
| `crates/core/` | `terminal-commander-core` | Domain types, errors, traits shared by all components. No I/O. |
| `crates/sifters/` | `terminal-commander-sifters` | Output sifters (PTY stream filters, redaction, line classification). |
| `crates/probes/` | `terminal-commander-probes` | Probe runners (per-shell/per-command health checks). |
| `crates/store/` | `terminal-commander-store` | SQLite + refinery migrations; persistence boundary. |
| `crates/daemon/` | `terminal-commanderd` | Long-running daemon binary; ties sifters + probes + store + transports. |
| `crates/mcp/` | `terminal-commander-mcp` | MCP server adapter (rmcp). Talks to daemon via internal IPC. |
| `crates/cli/` | `terminal-commander-cli` | CLI binary; talks to daemon. |

Notes on naming:

- Daemon binary name `terminal-commanderd` follows the BSD/Linux `<name>d`
  convention. The Cargo *package* name is `terminal-commanderd`; the produced
  binary is also `terminal-commanderd` by default.
- All other crates use the `terminal-commander-*` hyphenated prefix so they
  cluster on crates.io if/when published. Underscores would be the in-code
  module form (`terminal_commander_core`); Cargo names use hyphens.

## Flat vs nested layout

Recommended: **flat** under `crates/`.

```
terminal-commander/
  Cargo.toml             # virtual manifest (workspace root)
  Cargo.lock
  rust-toolchain.toml
  rustfmt.toml
  deny.toml
  LICENSE
  NOTICE
  README.md
  crates/
    core/
      Cargo.toml
      src/lib.rs
    sifters/
      Cargo.toml
      src/lib.rs
    probes/
      Cargo.toml
      src/lib.rs
    store/
      Cargo.toml
      src/lib.rs
      migrations/      # refinery .sql
    daemon/
      Cargo.toml
      src/main.rs
    mcp/
      Cargo.toml
      src/lib.rs       # or src/main.rs if it becomes its own binary
    cli/
      Cargo.toml
      src/main.rs
  docs/
  tests/               # workspace-level integration tests, if needed
```

Rationale for flat layout:

1. Easiest globbing in `members = ["crates/*"]`.
2. Matches the ruff workspace pattern (`members = ["crates/*"]`,
   resolver = "2"), which is widely cited as best-of-class organization for
   large Rust workspaces. Source:
   https://github.com/astral-sh/ruff/blob/main/Cargo.toml
3. Nested layouts (e.g. `crates/server/api/`,
   `crates/server/transport/`) are useful when you have functional clusters
   of 10+ crates each. Terminal Commander has 7 - that does not yet warrant
   a second hierarchy level.
4. Future growth (e.g. `crates/sifters-tools/`, `crates/store-bench/`) still
   fits under the same glob.

If a future crate is **internal only** (not meant to be reachable through the
public daemon/cli APIs - say a fuzzing harness), use the `publish = false`
field rather than nesting.

## Resolver version

Use `resolver = "3"` in `[workspace]`. Edition 2024 implies resolver = 3 by
default, but specifying it explicitly avoids surprises and matches what
bevy main is doing (https://github.com/bevyengine/bevy/blob/main/Cargo.toml).

Cargo workspaces reference:
https://doc.rust-lang.org/cargo/reference/workspaces.html

## workspace.package inheritance (stable since Rust 1.64)

The fields that all member crates should inherit:

```toml
[workspace.package]
version       = "0.1.0"
edition       = "2024"
rust-version  = "1.90"           # or 1.92, pending rmcp pin verification
license       = "Apache-2.0"
repository    = "https://github.com/<owner>/terminal-commander"
homepage      = "https://github.com/<owner>/terminal-commander"
authors       = ["The Terminal Commander Authors"]
description   = "OVERRIDE PER CRATE"   # placeholder; each crate sets its own
readme        = "README.md"
keywords      = ["mcp", "terminal", "daemon", "automation"]
categories    = ["command-line-utilities"]
```

Source for the supported keys list:
https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table

Each member references them with `.workspace = true`:

```toml
[package]
name          = "terminal-commander-core"
description   = "Domain types and shared traits for Terminal Commander."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
homepage.workspace     = true
authors.workspace      = true
readme.workspace       = true
keywords.workspace     = true
categories.workspace   = true
```

`description` must be set per-crate (workspace value is a placeholder). Cargo
does not error if a crate accidentally inherits the placeholder, but
`cargo publish` for that crate will fail with a meaningful description error.

## workspace.dependencies (stable since Rust 1.64)

Centralize **every shared dep version** in the workspace root. Member crates
then say `dep.workspace = true` plus, optionally, additional features.

```toml
[workspace.dependencies]
# Async runtime
tokio = { version = "1.40", default-features = false, features = ["macros", "rt-multi-thread", "io-util", "fs", "process", "signal", "sync", "time"] }
tokio-util = { version = "0.7", default-features = false }

# MCP SDK (pin verified separately - see msrv.md)
rmcp = { version = "0.x", default-features = false }

# Persistence
rusqlite = { version = "0.39", default-features = false, features = ["bundled"] }
refinery = { version = "0.9", default-features = false, features = ["rusqlite"] }

# Filesystem watching
notify = { version = "8.2", default-features = false }
notify-debouncer-full = { version = "0.7", default-features = false }

# PTY (Linux/WSL primary; Windows path deferred)
pty-process = { version = "0.5.3", default-features = false }

# Cross-cutting utility
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

# Internal crate path deps (the seven crates above)
terminal-commander-core    = { path = "crates/core",    version = "0.1.0" }
terminal-commander-sifters = { path = "crates/sifters", version = "0.1.0" }
terminal-commander-probes  = { path = "crates/probes",  version = "0.1.0" }
terminal-commander-store   = { path = "crates/store",   version = "0.1.0" }
terminal-commander-mcp     = { path = "crates/mcp",     version = "0.1.0" }
```

Version numbers above are placeholders pulled from the other R1 research
notes. The pin for `rmcp` must be reconfirmed against `msrv.md` once the SDK
version is locked.

Member crates then inherit cleanly:

```toml
[dependencies]
terminal-commander-core.workspace = true
tokio.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile = "3"
```

For the rare case where a single member needs an extra feature:

```toml
[dependencies]
tokio = { workspace = true, features = ["test-util"] }
```

Cargo merges this with the workspace's feature list; the requested feature is
**unioned in**, not overridden. Source:
https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html

## Top-level Cargo.toml sketch

Drop-in starting point for the workspace root:

```toml
# Cargo.toml (workspace root - virtual manifest, no [package])

[workspace]
resolver = "3"
members  = ["crates/*"]
# default-members can be set later if cargo run/test at root should target
# only a subset (e.g. just the daemon and cli).

[workspace.package]
version       = "0.1.0"
edition       = "2024"
rust-version  = "1.90"
license       = "Apache-2.0"
repository    = "https://github.com/<owner>/terminal-commander"
homepage      = "https://github.com/<owner>/terminal-commander"
authors       = ["The Terminal Commander Authors"]
readme        = "README.md"

[workspace.dependencies]
# (see preceding section)

[workspace.lints.rust]
unsafe_code        = "forbid"
unused_must_use    = "deny"
rust_2018_idioms   = { level = "warn", priority = -1 }

[workspace.lints.clippy]
pedantic    = { level = "warn", priority = -1 }
nursery     = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
todo        = "warn"
dbg_macro   = "deny"
print_stdout = "warn"     # daemon should log, not println
print_stderr = "warn"

# Release profile tuning is optional; keep defaults at MVP.
[profile.release]
lto           = "thin"
codegen-units = 1
strip         = "symbols"
```

The `priority = -1` trick lets us downgrade individual lints from the broad
`pedantic` / `nursery` group (e.g. allow `clippy::missing_errors_doc`) without
the global level overriding the per-lint level. This is the convention used
in ruff and several other large workspaces.

Workspace lints feature reference (stabilized 1.74):
https://blog.rust-lang.org/2023/11/16/Rust-1.74.0/

## Prior art (real-world references)

| Project | Layout | Resolver | License | Source |
|---|---|---|---|---|
| ruff | `members = ["crates/*"]`, ~60 crates flat under `crates/` | 2 | MIT | https://github.com/astral-sh/ruff/blob/main/Cargo.toml |
| bevy | `members = ["crates/*", "tools/*", ...]` plus example workspaces | 3 | MIT OR Apache-2.0 | https://github.com/bevyengine/bevy/blob/main/Cargo.toml |
| tokio | flat list of named members under root | 2 | MIT | https://github.com/tokio-rs/tokio/blob/master/Cargo.toml |

Takeaways for Terminal Commander:

- ruff's flat `crates/*` glob with 60 crates proves the pattern scales far
  beyond 7. No reason to over-engineer with sub-groupings now.
- bevy's `resolver = "3"` with Edition 2024 confirms that the resolver-3 +
  edition-2024 combo is production-ready as of May 2026.
- tokio's per-crate `Cargo.toml` keeps `description` and `readme` per-crate
  even when the rest of the metadata is inherited - this is the right model
  for us.

## Lock-in checklist (for the bootstrap PR)

1. Create the workspace root `Cargo.toml` per the sketch above.
2. Create the seven `crates/<name>/Cargo.toml` files with
   `name`, `description`, the inheritance lines, and the dependency lines.
3. Each crate gets a minimal `src/lib.rs` (or `src/main.rs` for the two
   binaries: daemon and cli).
4. Confirm `cargo metadata --format-version 1 | jq '.workspace_members'`
   lists exactly the seven expected crates.
5. Confirm `cargo check --workspace` builds with empty crates - this proves
   the inheritance graph is wired correctly before any feature code lands.
6. Add `[workspace.lints]` block and the `lints.workspace = true` opt-in in
   every crate's `[lints]` table.

## Non-goals (explicitly out of scope here)

- Cross-compilation matrix and per-target dep selection
  (`[target.'cfg(...)'.dependencies]`). That belongs in the
  `wsl-boundary.md` and portable-pty deferral notes, not here.
- Feature-flag design for the daemon (`features = ["systemd", "launchd"]`,
  etc.). That belongs in the daemon-lifecycle research.
- Publishing strategy (which crates go to crates.io, version-bump policy).
  That is a release-process decision separate from layout.
