# TC Omni-Tool Program — Planning Index

Status: Active program (post v0.1.47).  
Authority: Closes the vision gap documented in [cursor_report_03.md](../cursor_report_03.md).

## Goal

Make Terminal Commander a **fully self-reliant omni tool** for any LLM: human terminal parity **plus** LLM-native advantages (structured signals, bounded context, parallel orchestration, auto-parsing, audit, policy).

## Documents

| Document | Purpose |
|---|---|
| [docs/plans/2026-06-09-tc-omni-tool-master-program.md](../../docs/plans/2026-06-09-tc-omni-tool-master-program.md) | Master program: waves, acceptance criteria, dependencies, effort |
| [docs/plans/2026-06-09-tc-omni-wave1-shell-session.md](../../docs/plans/2026-06-09-tc-omni-wave1-shell-session.md) | Wave 1 implementation plan: shell + persistent session |
| [docs/plans/2026-06-09-tc-omni-wave2-platform-parity.md](../../docs/plans/2026-06-09-tc-omni-wave2-platform-parity.md) | Wave 2: Windows ConPTY + macOS + notify backends |
| [docs/plans/2026-06-09-tc-omni-wave3-parse-and-packs.md](../../docs/plans/2026-06-09-tc-omni-wave3-parse-and-packs.md) | Wave 3: auto-parse bridge + rule pack expansion |
| [docs/plans/2026-06-09-tc-omni-wave4-privilege-helper.md](../../docs/plans/2026-06-09-tc-omni-wave4-privilege-helper.md) | Wave 4: gated privileged operations |
| [docs/plans/2026-06-09-tc-omni-wave5-remote-federation.md](../../docs/plans/2026-06-09-tc-omni-wave5-remote-federation.md) | Wave 5: remote hosts, containers, federation |
| [docs/plans/2026-06-09-tc-omni-wave6-omni-certification.md](../../docs/plans/2026-06-09-tc-omni-wave6-omni-certification.md) | Wave 6: certification, docs, provider parity |

## Execution order

```text
Wave 0 (trust hardening)  →  Wave 1 (shell+session)  →  Wave 2 (platform)
        ↓                           ↓
   verify BACKLOG P0/P1      Wave 3 (parse) can parallel Wave 2
        ↓
Wave 4 (privilege) after Wave 1 policy framework
Wave 5 (remote) after Wave 1+2 stable
Wave 6 (certification) continuous; gate on all waves
```

## Success definition

Program complete when [Omni Acceptance Matrix](../../docs/plans/2026-06-09-tc-omni-tool-master-program.md#omni-acceptance-matrix) is green on Linux, WSL, macOS, and native Windows.
