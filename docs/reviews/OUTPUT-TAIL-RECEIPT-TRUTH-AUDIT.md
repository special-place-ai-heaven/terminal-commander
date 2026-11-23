# Output-tail receipt truth: SymForge dogfood incident

As of 2026-10-09. TC owns this defect. SymForge did not apply the incomplete
transfer: its builder split the transfer into 1,000-character lines and
verified the complete byte count before parsing.

## Independently observed incident

The original job is `job_01a12242e13773b6b7b5826c1072d8e1`. Reading it again
through the unchanged installed TC MCP/daemon produced:

| Evidence | Observed value |
|---|---|
| Job outcome | `exited`, exit 0, `outcome_trust=observed` |
| Raw output byte counter | 50,001 |
| Captured frames | 1 |
| Tail request | 200 lines, 65,536 bytes, ANSI stripping enabled |
| Returned lines | 1 |
| Returned text length | 8,192 characters / 8,192 UTF-8 bytes |
| `truncated_bytes` | `false` - incorrect |
| `truncated_lines` | `false` |

The requested byte budget did not cause this loss. A single retained line can
correctly have `truncated_lines=false`; its capture loss must still set
`truncated_bytes=true`. Exit 0 proves the child's exit outcome, not complete
payload capture or a safe-to-parse transfer.

## Root cause and committed repair

`SourceFrame::new` caps frame text to `MAX_FRAME_BYTES=8192` and records omitted
bytes. The old ring-tail projection discarded that existing loss evidence:
it checked the returned byte budget without consulting each selected frame's
`truncated_bytes`. The daemon and MCP then faithfully exposed the incorrect
ring result.

Commit `705dd871745d81d574db859c4bf41587cfcc799c`, published on
`origin/feat/embed-parity`, fixes the shared ring used by daemon/MCP and embed.
It preserves upstream capture loss, enforces strict UTF-8 tail byte budgets
including zero, and retains line-eviction evidence. The corresponding head
projection also preserves upstream loss after carriage-return normalization.
The capture cap remains bounded; the repair does not recover discarded bytes.

Source anchors: `crates/core/src/context.rs` (`SourceFrame::new`, `tail`,
`head`), `crates/daemon/src/ipc/handlers/command.rs`
(`handle_command_output_tail`), and `crates/mcp/src/tools.rs`
(`command_output_tail_payload`).

## Verification and deployment distinction

The earlier RED/GREEN regressions and complete Windows/Linux gates are recorded
in [the implementation report](EMBED-PARITY-IMPLEMENTATION.md). Both original
full-suite receipts still report observed exit 0.

For this audit, only the three existing live output regressions were rerun:

```text
cargo nextest run -p terminal-commanderd --test output_tail_bounds -E 'test(live_)'
```

Observed exit 0: three passed, one fixture test filtered out. The ASCII case
emits a 50,000-character line; Unicode and head-only CR-padding cases cover
UTF-8 bounds and loss provenance. Receipt:
`job_01a12286334a71e1b685c99974328407`.

No competing full suite, SF implementation change, or replay of the reported
job was performed. The installed daemon was not upgraded or restarted and
still exhibits the incident. Published source verification does not certify
the installed binary; a verified upgrade is required before its receipts can
be trusted for this case. An old discarded payload cannot be recovered by
reading its tail again.

## Owner-forwarded SymForge recurrence — 2026-10-10

The owner forwarded another SF builder report from the unchanged installed TC
daemon. The transfer observations below are SF-reported; TC independently
rechecked job status metadata, without reading or reproducing the patch payload.

Original transfer: `job_01a1236b1e5c7162a14cf587b416031f`.

- Tail request: `max_bytes=64000`, `max_lines=2`.
- Returned 8,192 base64 characters, decoding to 6,144 bytes.
- Both `truncated_bytes` and `truncated_lines` were false; the `End Patch`
  sentinel was missing. No patch was applied from this incomplete transfer.
- The original exact patch byte count was not emitted. Its size was reported
  only as approximately 16.6 KiB; do not assign it the retry's exact byte count.

Wrapped retry: `job_01a1236bd3b9764c9c517f3c77041e20`.

The retry declared `PATCH_BYTES=16565`. SF reports verifying all 16,565 decoded
bytes and the final sentinel. This establishes completeness for that retry;
it does not make the original truncation receipt truthful.

TC's status-only recheck returned:

| Job | State / exit | Outcome trust | `bytes_total` | stdout / stderr frames |
|---|---|---|---|---|
| Original transfer | `exited` / 0 | `observed` | 22,089 | 1 / 0 |
| Wrapped retry | `exited` / 0 | `observed` | 22,112 | 7 / 0 |

These counters describe command output, including encoding and any headers or
line delimiters; they are not decoded patch sizes or proof of complete capture.
The reported decoded counts, tail flags and sentinel checks were not independently
rerun by TC. Exit 0 does not establish transfer completeness.

This is a recurrence on the unchanged installed daemon, not evidence that the
published source repair failed. The v0.3.15 release and registry verification are
recorded in [the implementation report](EMBED-PARITY-IMPLEMENTATION.md).
The bounded frame cap still discards oversized-line suffixes; the repair makes
that loss observable rather than recovering discarded bytes. No reported job was
replayed, no patch was applied by TC, and no daemon restart or competing test
suite was performed for this report.
