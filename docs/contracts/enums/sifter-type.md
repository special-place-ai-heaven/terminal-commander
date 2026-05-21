# Sifter-type enum (closed set)

The 11 canonical sifter-type discriminators are the enum domain for
the `rule.kind` field in the rule contract. The set is CLOSED:
adding a new discriminator requires amending this document AND
`docs/contracts/README.md` section 6.2 AND bumping the
`rule-definition.vN.json` schema.

| Discriminator | README "sifter name" mapping | Implementing goal |
|---|---|---|
| `keyword` | keyword | TC10 |
| `regex` | regex | TC10 |
| `prompt` | prompt detector | TC19 |
| `exit_code` | (new; covers command_exited/failed) | TC16 |
| `stream_marker` | (new; covers stdout/stderr/meta boundary events) | TC15 |
| `progress_collapse` | progress detector | TC11 |
| `dedupe` | dedupe rule | TC11 |
| `threshold` | numeric condition + stall detector | TC11 |
| `sequence` | multiline block + correlation rule | TC11 / TC10 |
| `anchor` | (new; pins a context window to a specific frame) | TC08 |
| `custom` | artifact parser + escape hatch | TC20 (artifact) / future |

Reserved-not-implemented at MVP draft: `prompt`, `exit_code`,
`stream_marker`, `progress_collapse`, `threshold`, `sequence`,
`anchor`, `custom`. `keyword` and `regex` are the MVP minimum.

Rule fixtures that exercise a reserved discriminator carry
`_meta.status = "reserved-not-implemented"` so they cannot mistakenly
look like live behavior.
