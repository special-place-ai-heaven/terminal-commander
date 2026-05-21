# Severity enum

| Value | Order | Use |
|---|---:|---|
| `trace` | 0 | Diagnostic frames; never a default for an event. |
| `low` | 1 | Informational events the operator may want to see. |
| `medium` | 2 | Notable events; default for unrecognized errors. |
| `high` | 3 | Things that usually need human follow-up (missing package, compile error). |
| `critical` | 4 | Things that have stopped progress (command failed with non-zero exit, password prompt, permission denied). |

Ordering: `trace < low < medium < high < critical`. Producers MUST
order-compare by these integer ranks. Consumers MUST tolerate
unknown severity strings by treating them as `medium`.

Concrete Rust `Severity` enum: TC06.
