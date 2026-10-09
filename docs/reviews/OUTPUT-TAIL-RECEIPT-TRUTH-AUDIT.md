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
