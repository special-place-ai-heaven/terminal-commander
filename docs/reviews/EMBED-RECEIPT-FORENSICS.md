# Missing terminal receipt: correlation findings

As of 2026-10-09. Read-only investigation of the exact job in
[CODEX-REPORT-24-TERMINAL-COMMANDER.md](CODEX-REPORT-24-TERMINAL-COMMANDER.md).
No original command was rerun and no daemon was stopped for this investigation.

Job: `job_01a121f4f7a977eb9ca129d1d014a877`.

| Evidence | Finding |
|---|---|
| Live installed TC `command_status` | Typed `JobLost`; no trustworthy terminal outcome |
| `audit_records`, audit ID `180108` | Allowed `command_start` at `2026-10-09T18:37:44.7504665Z` |
| `job_receipts` in the matching database | No row for the job |
| Start audit metadata | Contains `argv` and `nested_shell` keys; no recorded boot/instance correlation ID. Values were not reproduced. |
| Matching daemon log, lines `6942`, `6944` | New named-pipe binding at `18:39:00.129829Z` and IPC server bound at `18:39:00.130795Z` |
| Relevant log interval | No matched panic, receipt persistence failure or actual shutdown notification for the old owner. The new startup's “Send Ctrl-C to shut down” text is not a shutdown event. |

Database:
`C:/Users/poslj/AppData/Local/terminal-commanderd/state/tc-0ee23a9fc331/terminal-commander.db`.

Log:
`C:/Users/poslj/AppData/Local/terminal-commanderd/state/tc-0ee23a9fc331/logs/terminal-commanderd.log`.

This establishes a durable start, missing terminal receipt, and a subsequent
daemon startup in the same session store. It does not establish why the prior
owner disappeared, whether the command completed its effects, or whether a
terminal write was attempted. The old audit lacks boot identity, and the observed
log does not supply a causal termination record. Preserve uncertainty; never
turn this start into a successful receipt or automatic command retry.

The embed implementation adds consistent engine/build/boot identity to health
and discovery. Correlation IDs in a request envelope identify a request/reply;
they are not durable exactly-once execution or replay authority. Deterministic
receipt-write, cancellation/abandonment and abrupt-loss regressions are part of
the accompanying implementation verification rather than an explanation of this
historical loss.
