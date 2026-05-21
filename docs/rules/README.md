# Rule Packs - Terminal Commander

Status: TC14 baseline.

This directory documents the seed rule packs imported into the
registry at first daemon start (or on demand via the admin CLI in
TC25). Rule packs live in `/rules/*.json`.

Language: ASCII only.

## 1. Pack files

| File | Pack id | Purpose |
|---|---|---|
| `rules/generic.terminal.json` | `generic.terminal` | Cross-tool terminal errors (permission, command not found). |
| `rules/apt.json` | `apt` | APT package manager errors. |
| `rules/cargo.json` | `cargo` | rustc / cargo build failures. |
| `rules/npm.json` | `npm` | npm install/build errors. |
| `rules/pytest.json` | `pytest` | pytest collection and failure summaries. |
| `rules/gcc.json` | `gcc` | gcc/g++ compile + linker errors. |
| `rules/make.json` | `make` | GNU make recipe failures (architect-added seventh pack). |

## 2. Format

Each pack is a JSON object with `_meta` and `rules`, where each
rule is a `RuleDefinition` v1 (see TC09). The fields are documented
in `crates/core/src/rule.rs` and validated by
`RuleDefinition::validate()`.

Status of every seed rule is `draft`: the import installs the rules
into the registry but does NOT activate them. Activation lives in
TC21+ probe wiring and TC24 MCP tools.

## 3. Safety rules at import (TC14)

1. Every rule must pass `RuleDefinition::validate()`. This already
   enforces pattern length cap (4096 bytes), no lookaround, no
   backreferences, balanced summary template placeholders, and tag
   length/count caps.
2. Each regex pattern is compiled with `regex::RegexBuilder` using
   `size_limit(65_536)` and `dfa_size_limit(65_536)`. Oversize
   compilations fail the import.
3. Per-rule rate limits SHOULD be set when the rule could fire
   frequently on noisy output (apt percentage updates, npm progress).
4. `redact` SHOULD list any capture whose value could contain a
   secret. The sifter replaces redacted values with `<redacted>`
   before emission.
5. Tags MUST be lowercase and <= 64 bytes; at most 16 per rule.

## 4. Contributing a new rule

1. Add a `_meta`'d entry to the right pack file (or create a new
   pack in `rules/`).
2. Include at least one `examples` entry per rule.
3. Run `cargo nextest run -p terminal-commander-store
   --filter-expr 'test(import)'` (or the workspace-level test)
   to verify the rule imports cleanly.
4. The author of a new rule pack documents pack-level
   false-positive risks in this README under section 5.

## 5. Known false-positive risks (initial packs)

- `npm.err-generic` matches the literal string `npm error` /
  `npm ERR!`. Some non-error npm output mentions these strings
  in advisory contexts. Mitigate by raising severity from `high`
  to `medium` in operator profile if the noise is excessive.
- `cargo.compile-error` matches any rustc `error[Ennnn]` line.
  Doctest blocks intentionally emit these as part of test output;
  TC11 dedupe collapses repeats within the 5-second window.
- `make.recipe-error` matches the canonical "Error N" line; some
  upstream Makefiles set $(error ...) and emit the same shape.
  These are real failures and the rule still applies.

## 6. Source-status

| Component | Status |
|---|---|
| `import_rule_pack` API | live (TC14) |
| Seven seed packs | live (TC14) |
| Rule activation on probe attach | reserved for TC21 |
| MCP `registry_search` / `registry_get` | reserved for TC24 |
