# User Decisions (TC01)

Captured during TC01 research wave. Architect MUST treat these as locked.

Date: 2026-05-21
Source: direct user input in goal session.

## Decisions

| Topic | Decision | Notes |
|---|---|---|
| Language | Rust, edition 2024 | Confirmed. |
| License | Apache-2.0 | SPDX `Apache-2.0`. Single license, not dual MIT/Apache. |
| rmcp version | `=1.7.0` exact pin | crates.io. MSRV floor = Rust 1.92. Edition 2024. |
| Rust toolchain pin (amended 2026-05-21) | `1.95.0` | Operator decision at TC04 start: use locally-available 1.95.0 instead of the 1.92.0 MSRV floor to avoid a `rustup` download. rmcp 1.7.0 still works (it floors AT 1.92, does not require =1.92). CI `cargo-hack --rust-version` continues to gate the MSRV claim. |
| Async runtime | tokio | Forced by rmcp 1.7.0 dependency. |
| Daemon process model | Two-process | thin `terminal-commander-mcp` (rmcp stdio) + `terminal-commanderd` daemon. IPC details deferred to TC21 (daemon local API and router). ARCH locks the split conceptually only. |
| Workspace | Rust 2024 multi-crate workspace | Flat `crates/<name>/` layout. 7 crates per TC04. |
| Storage | rusqlite 0.39 bundled + refinery 0.9 + FTS5 | Per R1-beta `sqlite-fts5.md`. |
| File watcher | notify 8.2 + notify-debouncer-full 0.7 | Per R1-beta `file-watcher.md`. |
| WSL `/mnt/c` handling | Force `PollWatcher`; detect 9P at probe construction via `/proc/self/mountinfo` | Per R1-beta `wsl-boundary.md`. inotify silently broken on 9P (microsoft/WSL#4739). |
| PTY (MVP) | pty-process 0.5.3 with `async` feature | Linux + WSL. portable-pty deferred for Windows native. |
| Policy enforcement (MVP) | Advisory in-process + audit log | Kernel-level Landlock/seccomp-bpf documented as post-MVP hardening roadmap. |
| Prior-art research | Behavior survey only | No copied source. Cite for inspiration / differentiation only. |
| Platforms | Linux native + WSL2 primary | macOS / Windows-native deferred. |
| 2026-05-24 | Tier-1 native runtime decision | Windows-x64 + Linux-x64 are tier-1 native targets; macOS is build-only tier-3; WSL = Linux artifact (no bridge). See `docs/adr/ADR-native-tier1-runtime.md`. | accepted |

## Implications for architect

- SPEC.md crate list = 7 crates (must reconcile README's 6-crate gap with explicit note).
- Cargo.toml SPDX header per crate: `license = "Apache-2.0"`.
- LICENSE file at repo root: Apache-2.0 full text. NOTICE file optional but recommended.
- Per-file header convention per Apache-2.0 appendix: include in CONTRIBUTING.md.
- ARCHITECTURE.md must document two-process model + name the IPC decision as deferred to TC21.
- ROADMAP.md Wave 0 covers TC01-TC03; Wave 1 = TC04 (workspace + toolchain).
- ASSUMPTIONS.md must list IPC-transport as the one remaining architect-level decision (TC21).

## Risks acknowledged

- WSL 9P inotify silent-fail: treat as TC18 acceptance criterion, not optimization.
- "Policy enforcement" wording in spec: must qualify as advisory in MVP.
