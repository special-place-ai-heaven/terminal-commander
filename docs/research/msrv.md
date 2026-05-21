# MSRV (Minimum Supported Rust Version) for Terminal Commander

Topic: A3
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: medium-high

## Recommendation

Target Rust 1.90 stable (matching rmcp 0.16.0 rust-toolchain.toml).
If the project later upgrades rmcp to 1.x, raise MSRV to 1.92.

Edition: 2024 (matches rmcp workspace edition).

## MSRV floor computation

The project MSRV must be `max(MSRV of each direct dependency)`.

| Dependency | Declared MSRV / pinned channel | Source |
|---|---|---|
| rmcp 0.16.0 | rust-toolchain.toml `channel = "1.90"` | GH `rmcp-v0.16.0/rust-toolchain.toml` |
| rmcp 1.7.0 (main) | rust-toolchain.toml `channel = "1.92"` | GH main `rust-toolchain.toml` |
| tokio 1.52.x | 1.71 | https://lib.rs/crates/tokio |
| notify 9.0.0-rc.4 | 1.88 | https://lib.rs/crates/notify |
| pty-process 0.5.3 | not specified; edition 2024 | https://lib.rs/crates/pty-process |
| portable-pty 0.9.0 | not specified; edition 2018 | https://raw.githubusercontent.com/wez/wezterm/main/pty/Cargo.toml |
| nix 0.31.3 | 1.69 | https://lib.rs/crates/nix |
| rusqlite 0.39.0 | latest stable at release time | https://lib.rs/crates/rusqlite |
| sqlx 0.8.6 stable | not specified | https://lib.rs/crates/sqlx |
| process-wrap 9.1.0 | 1.87.0 | https://github.com/watchexec/process-wrap |
| sd-notify 0.5.0 | not specified | https://lib.rs/crates/sd-notify |
| daemonize 0.5.0 | not specified; edition 2015 | https://lib.rs/crates/daemonize |
| serde 1.x | well below 1.71 | (not the binding constraint) |
| serde_json 1.x | well below 1.71 | (not the binding constraint) |
| tokio-rusqlite 0.7.0 | not specified | https://lib.rs/crates/tokio-rusqlite |

### Binding constraints

For rmcp 0.16.0 path:

- rmcp 0.16.0 toolchain pin: 1.90.
- Edition 2024 requires Rust 1.85 or newer.
- Floor: 1.90 (from rmcp 0.16.0).

For rmcp 1.7.0 path (latest):

- rmcp 1.7.0 toolchain pin: 1.92.
- Floor: 1.92.

`pty-process` 0.5.3 declares `edition = "2024"`; edition 2024 itself
requires Rust 1.85 stable or newer. That is implicitly a 1.85 floor.

`notify` 9.0.0-rc.4 MSRV is 1.88. The notify 9.x release line is still
release-candidate as of 2026-05; if the project prefers a stable
notify line, use notify 8.x (verify its MSRV before locking).

## Recommendation in detail

### Pick one of two profiles

Profile A: pin rmcp 0.16.0 (user pre-confirmed):

```text
rust-version = "1.90"
edition = "2024"
```

Profile B: upgrade to rmcp 1.7.0 (current latest):

```text
rust-version = "1.92"
edition = "2024"
```

This is a user decision. The user-pinned evidence is rmcp 0.16.0 with
edition 2024 + streamable HTTP server session module. The latest crates.io
version is 1.7.0 published 2026-05-13. The README at HEAD still
references 0.16.0 in its install snippet, so 0.16.0 is plausibly
intended; however, downstream architecture should not assume 0.16.0 is
the final pin without explicit user confirmation.

REQUIRES USER DECISION: which rmcp line to pin.

### Rust toolchain declaration

In the workspace `Cargo.toml`:

```toml
[workspace.package]
edition = "2024"
rust-version = "1.90"   # Profile A; switch to 1.92 for Profile B
```

In a project `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.90"   # Profile A; switch to 1.92 for Profile B
components = ["rustc", "rust-std", "cargo", "clippy", "rustfmt", "rust-docs"]
```

## Confidence

Medium-high. The MSRV-floor logic is mechanical. The remaining
uncertainty is which rmcp line (0.16.0 vs 1.7.0) the user intends to
pin, which moves the floor by 2 minor versions.

## SOURCE_MAP reclassification

MSRV >= 1.90: was inferred, now evidence-backed via
rmcp-v0.16.0/rust-toolchain.toml `channel = "1.90"`.

MSRV >= 1.85 from edition 2024: evidence-backed via rmcp workspace
package and pty-process Cargo.toml both declaring `edition = "2024"`.
