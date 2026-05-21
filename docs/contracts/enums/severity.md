# Severity enum

Amended 2026-05-21 at TC06 start (7-value union, locked by operator).

| Value | Order | Use |
|---|---:|---|
| `trace` | 0 | Diagnostic frames; never a default for an event. |
| `debug` | 1 | Verbose detail for development; off by default in non-debug profiles. |
| `info` | 2 | Routine informational events. |
| `low` | 3 | Informational events the operator may want to see. |
| `medium` | 4 | Notable events; default for unrecognized errors. |
| `high` | 5 | Things that usually need human follow-up (missing package, compile error). |
| `critical` | 6 | Things that have stopped progress (command failed with non-zero exit, password prompt, permission denied). |

Ordering: `trace < debug < info < low < medium < high < critical`.
Producers MUST order-compare by these integer ranks. Consumers MUST
tolerate unknown severity strings by treating them as `medium`.

Wire form: lowercase snake_case strings exactly matching the table
column above (one word, no underscores in any value).

Concrete Rust `Severity` enum: TC06 (this goal). Storage backing
in TC12 stores the rank as a SQLite INTEGER for ORDER BY.
