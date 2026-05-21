# Probe-kind enum

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
