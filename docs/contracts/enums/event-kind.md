# Event-kind enum (open string)

Producers MAY emit any string. Consumers MUST treat unknown kinds as
data (display + log), not as errors. The canonical set below is the
MVP target; future probes/sifters may extend it without breaking
consumers.

| Kind | Severity default | Meaning |
|---|---|---|
| `command_started` | low | Process probe started a command. |
| `command_exited` | low | Command exited (any code; details in captures). |
| `command_failed` | critical | Command exited non-zero. |
| `missing_package` | high | Package manager could not locate a package. |
| `compile_error` | high | Compiler reported an error. |
| `test_failed` | high | Test runner reported a failure. |
| `permission_denied` | high | OS or tool refused due to permission. |
| `password_prompt` | critical | Terminal asked for a password (TC19). |
| `stalled` | medium | Probe detected no output for the stall window. |
| `file_changed` | low | File probe detected a change. |
| `artifact_generated` | low | Directory probe detected a new artifact (TC20). |
| `repeated_collapsed` | low | Dedupe/suppression collapsed N occurrences. |
| `threshold_crossed` | medium | Numeric/threshold sifter fired. |
| `prompt_detected` | medium | Prompt detector matched (login, shell, REPL). |
| `policy_denied` | high | Policy engine refused an action. |

Concrete usage: TC10 (sifter runtime), TC11 (dedupe/progress),
TC15 (process probe), TC18 (file probe), TC19 (PTY), TC20
(directory), TC22 (policy_denied).
