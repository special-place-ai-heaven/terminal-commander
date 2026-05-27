# C — Deferred work (non-goals only)

**Do not implement from this table.** Rationale pointers only.

| Topic | Why deferred | Pointer |
|-------|--------------|---------|
| Session lifecycle supervisor (mint registry, idle reap) | F1 spec non-goal; launcher sets token only | `docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md:179-184` |
| Auto-derive `TC_SESSION` from tty/console | Fragile for backgrounded daemons | Same spec `:191-192` |
| Central router / aggregator / proxy | Explicitly rejected | Same spec `:185-186` |
| Full PTY spawn in probes (POSIX harness) | Deferred at probe layer | `crates/probes/src/pty.rs:17-22` |
| Command runtime in `command.rs` vs TC44 | PTY/command surface split | `crates/daemon/src/command.rs:28` |
| IPC transport evolution (TC37) | Router defers transport | `crates/daemon/src/router.rs:13` |
| Noise dedupe persistence across restarts (TC12) | In-memory only | `crates/sifters/src/noise.rs:20` |
| Context eviction policies (TC18/TC22) | Partial implementation | `crates/core/src/context.rs:21` |
| CLI UDS adapter (TC21) | MVP in-process path | `crates/cli/src/main.rs:8` |
| macOS / Windows-native without WSL | Product decision | `ARCHITECTURE.md:333`, `docs/research/_USER_DECISIONS.md` (cited in ARCHITECTURE) |
| TC21 “IPC deferred” in ARCHITECTURE | Doc predates shipped daemon IPC | `ARCHITECTURE.md:28,100-108` — **doc drift**; update under D hygiene, not new feature |
| Endpoint coverage / legacy 5-tool `ToolSurface` | Parallel stale surface | `.planning/endpoint-coverage-hardening.md`, `crates/mcp/src/lib.rs:77-87` |
| PTY command surface (TC44 naming in code) | Ongoing domain; not open-work bucket | `crates/daemon/src/pty_command.rs:4`, `goals/TC19-terminal-pty-probe-and-prompt-detection.md` |

**Related shipped work (do not re-plan):** F1 session endpoint Rust (`docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md:51`), pidfile/replace (`crates/supervisor/src/pidfile.rs`, `replace.rs`).
