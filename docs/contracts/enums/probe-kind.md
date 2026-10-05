# Probe-kind enum

Status (2026-10-05): this table is the `source_type` domain (`SourceType`,
`crates/core/src/source.rs`). The runtime `ProbeKind` used by `probe_list`
and `[policy.probes]` is `command`, `file_watch`, `pty`
(`crates/ipc/src/protocol.rs`). Only process, terminal (PTY) and file
sources are produced; no directory, journal or artifact probe ships.

| Kind | Implementing goal | Notes |
|---|---|---|
| `process` | TC15 | Spawns a non-interactive command, captures stdout/stderr. |
| `terminal` (alias `pty`) | TC19 | PTY-attached interactive probe. ANSI normalization + prompt detection. |
| `file` | TC18 | Tail-follow + create-after-watch + rotation. |
| `directory` | TC20 | Watch a directory for new files / changed files. |
| `journal` | post-MVP | systemd journal probe. Out of MVP scope. |
| `artifact` | TC20 | Summary parser for generated reports (JUnit XML, coverage JSON). |

Closed set for MVP; new kinds require a goal that amends this
document.
