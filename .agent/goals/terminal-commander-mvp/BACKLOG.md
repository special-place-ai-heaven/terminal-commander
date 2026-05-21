# Backlog (post-MVP)

Status: TC32 deliverable. Consolidated list of every deferral and
follow-up recorded during the TC02-TC31 chain.

Language: ASCII only.

## P0 (block-on-real-deployment)

| Item | Origin | Notes |
|---|---|---|
| rmcp 1.7.0 stdio adapter | TC23 | ToolSurface lands; transport glue is the next goal. Without this MCP clients cannot attach. |
| pty-process spawn path | TC15 / TC19 | ANSI normalization + prompt detection are live; the actual PTY spawn is deferred to POSIX harness. |
| daemon IPC transport (UDS) | TC21 | In-process Router runs now; UDS + peer-cred check is the next goal once MCP server crosses a process boundary. |
| Persistent audit log writes | TC22 | PolicyEngine returns verdicts; AuditPlaceholder records actions in memory. SQLite-backed audit log (V0003 migration) is the next goal. |

## P1 (correctness + ergonomics)

| Item | Origin | Notes |
|---|---|---|
| process-wrap process-group integration | TC15 | Replaces `tokio::process::Command` with `process-wrap 9.1`; enables SIGTERM-to-group + grace ladder per TC15 contract. |
| notify 8.2 + notify-debouncer-full 0.7 file probe | TC18 | Replaces 250ms polling with native inotify on Linux/WSL ext4, forces PollWatcher on 9P. |
| Native inotify directory probe | TC20 | Same as above for the directory probe. |
| Move-event detection | TC20 | 9P polling sees move as delete+create. notify path adds real move events. |
| Audit log hash chain | SECURITY.md §8 | Tamper-evidence for the audit log. Accepted post-MVP. |
| Coverage gating | TESTING.md §11 | cargo-llvm-cov in CI. Not a CI gate at MVP. |
| Mutation testing | TESTING.md §11 | cargo-mutants on the sifter runtime. |

## P2 (platform expansion)

| Item | Origin | Notes |
|---|---|---|
| macOS support | SPEC.md §6 | sandbox-d + Seatbelt + cap-std on Apple. |
| Windows-native | SPEC.md §6 | Job Objects + Console PTY + analogous policy primitives. |
| Landlock ruleset compiler | POLICY.md §6.2 | Kernel-enforced policy on Linux 5.13+. ABI v4+ also gives network controls. |
| seccomp-bpf allow-list | POLICY.md §6.2 | Syscall-level enforcement. |
| Distribution packages | docs/install/README.md | deb / rpm / aur. Manual cargo install in MVP. |

## P3 (developer ergonomics)

| Item | Origin | Notes |
|---|---|---|
| Doc-rigor pass | Cargo.toml workspace.lints | Re-enable missing_docs + doc_markdown + doc_lazy_continuation per crate as public surfaces stabilize. |
| Property-based tests | TESTING.md §11 | proptest on sifter runtime + JSON contract round-trips. |
| Fuzz harness | TESTING.md §11 | cargo-fuzz on rule definition JSON ingest + the regex compile path. |

## Reconsidered at TC32

Every item with `Reconsider at TC32` in a code comment lands here.
This list is the consolidated view.

## TC02 invariants still upheld

The pre-MVP invariants from SECURITY.md / PRIVILEGE_MODEL.md /
POLICY.md remain LOCKED across the chain:

- LLM-facing MCP server is NOT an unrestricted root shell.
- No setuid, no polkit rule, no installed system service.
- No outbound network egress (verified: TC29 grep tests).
- Default-deny sensitive paths (verified: TC29 covers all 14 suffixes).
- Bounded outputs everywhere (verified: bucket reads bounded by
  MAX_READ_LIMIT, file_read_window bounded by MAX_FILE_WINDOW_BYTES,
  event_context bounded by MAX_WINDOW_BYTES).
- Every signal event has a pointer OR a pointer_unavailable_reason
  for severity>=Medium (verified: TC06 SignalEvent::validate +
  TC02 negative-contract fixture forbidden/missing-pointer.v1.json).
