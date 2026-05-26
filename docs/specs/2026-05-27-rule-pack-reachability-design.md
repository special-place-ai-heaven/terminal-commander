# Spec: Rule-Pack Reachability (TCE-ERG-PACK)

Status: Design (brainstormed 2026-05-27). Language: ASCII only.

## Objective

Make TC's already-authored rule packs usable by an LLM agent in-loop,
in one call, so the agent gets expert signal extraction (cargo / pytest
/ npm / etc.) without authoring any rule JSON. Then dogfood the result
to find out whether TC is actually preferred, honestly.

This is the next step on the agent-ergonomics chain
(`docs/specs/2026-05-26-agent-ergonomics-chain.md`, Phase 1 shipped).
It attacks the rule-writing cold-start tax: TC's value is gated behind
~6 calls + schema learning the agent will not pay casually. A named
pack collapses that to one call.

## Current state (verified 2026-05-27)

- Seven packs exist on disk: `rules/{generic.terminal,apt,cargo,npm,
  pytest,gcc,make}.json` (TC14, shipped).
- A store-layer importer exists: `EventStore::import_rule_pack(path)`
  / `import_rule_pack_str(json)` in `crates/store/src/import.rs`.
  Validates each rule, bounded-compiles regexes, inserts via
  `create_rule_version`. Tested (`import_all_seed_packs_from_repo`).
- GAP 1: import is store-layer only. No daemon IPC, no MCP tool. An
  agent CANNOT name a pack. (import.rs:12 "Activation on probe attach
  is reserved for TC21" -- never built.)
- GAP 2: every pack rule ships `status: "draft"`. The draft-poison
  gate (shipped Phase 0) rejects activating a draft. So even if
  imported, pack rules cannot be activated without per-rule status
  flips. Packs are currently unreachable AND unactivatable.
- GAP 3: `rules/cargo.json` has only 2 rules (compile-error,
  could-not-compile). Thin for "expert extraction": no warnings, no
  test failures, no panics, no "aborting due to N errors", no linker
  errors.
- `registry_upsert` stores whatever `status` a definition carries
  (server.rs:998-1017): it validates then `create_rule_version`. A
  rule with `status:"active"` lands active.
- `registry_activate` reuses `ActivationScope` (global/bucket/job) and
  rejects missing scope (registry_scope_required tests). The
  draft-poison gate lives in `handle_registry_activate`.
- Every IPC handler audits via `AuditEntry::new`.

## Non-goals

- Not building an LLM-driven eval harness. The agent (Claude, in
  normal use) IS the instrument; dogfooding is the eval. An automated
  regression gate is a possible FUTURE project, only after dogfooding
  shows TC is superior.
- Not changing the on-disk pack default (`draft` stays the safe seed
  default; promotion happens only on explicit import-with-activate).
- Not the inline-`ruleset`-on-start path (rejected in brainstorm in
  favor of the persistent registry model; could be added later).
- Not authoring new packs beyond thickening cargo (other packs already
  exist; thicken them later if dogfooding shows demand).
- Not `run_and_watch` / teaching-errors at large (TCE-ERG-3/4 remain
  future), though the unknown-pack error follows the teaching-error
  spirit.

## Design

### Decision: promote-and-activate on explicit import (option A)

The import tool resolves a pack by NAME (not path -- agents must not
know repo layout), imports its rules, and when the caller passes
`activate: true` + a scope, promotes each rule to `active` and
activates it in that scope, in one call. `activate: false` imports the
rules as their on-disk `draft` status (the vetting path) and activates
nothing.

Rationale: one call to usable rules (kills the cold-start tax); the
draft-poison gate is satisfied because promoted rules are `active`;
promotion is explicit and audited (the agent asked for it); the
on-disk `draft` default is preserved so a vetting import stays inert.
Rejected alternatives: import-then-separate-activate (reintroduces the
tax), pack-file-carries-active-status (any import goes live; worse
safety).

### TCE-ERG-PACK-1: pack name resolution (store)

A pack NAME resolves to a bundled pack definition. Add
`EventStore::import_rule_pack_by_name(name)` (or a resolver function)
that maps a validated pack name to its JSON. The pack source must be
locatable at runtime by the daemon, not relative to a test's
`CARGO_MANIFEST_DIR`. Options to settle in the plan: (a) embed the
seven pack JSONs into the binary via `include_str!` keyed by name
(robust, no filesystem dependency, recommended); (b) resolve a
configured rules directory. Decision in spec: **embed via
`include_str!`** -- the packs are small, versioned with the binary,
and an agent-facing tool must not depend on a repo checkout being
present at runtime.

- Acceptance: `resolve_pack("cargo")` returns the cargo pack JSON;
  `resolve_pack("nope")` returns a typed not-found carrying the list
  of known pack names.
- Crates: store (resolver + embed), and a single source of truth for
  the pack-name list.

### TCE-ERG-PACK-2: `registry_import_pack` IPC

New IPC request `RegistryImportPack { pack: String, activate: bool,
scope: Option<ActivationScope> }` and response
`RegistryImportPackResponse { pack, imported: Vec<String>, skipped:
Vec<String>, activated: Vec<String> }`.

Handler (`handle_registry_import_pack`):
1. Resolve pack by name; unknown -> `IpcErrorCode::InvalidParams` (or
   the existing nearest variant) with message listing known packs.
2. Import via the existing validate + bounded-regex-compile path,
   reusing `import_rule_pack_str` logic. Skipped rules reported, not
   fatal.
3. If `activate`: require `scope` (reject missing scope exactly like
   `registry_activate`). The promotion seam (plan must resolve): the
   existing activate path looks up the STORED def, which is `draft`,
   and the eligibility gate would then reject it. So promotion must
   land in the store BEFORE activate. Cleanest: import each pack rule
   with `status` already set to `Active` (a promoted clone written via
   `create_rule_version`) when `activate:true`, so the subsequent
   lookup-based activate finds an eligible def and the gate passes
   honestly. Then call the existing scoped-activate path per rule.
   Reuse, do not duplicate, the activation logic (no fourth copy --
   the merge-fn lesson from the draft-poison fix). When
   `activate:false`, import rules at their on-disk `draft` status and
   activate nothing.
4. Audit the import and each activation.

- Acceptance: an `import_pack {pack:"cargo", activate:true,
  scope:Global}` IPC call imports the cargo rules, activates them, and
  a subsequent command emitting a `error[E0425]: ...` stderr line
  produces a `compile_error` signal event. `activate:false` imports
  draft rules and activates nothing.
- Crates: daemon (ipc protocol + server handler), store (reused).

### TCE-ERG-PACK-3: `registry_import_pack` MCP tool

New `#[tool]` `registry_import_pack` forwarding 1:1 to the IPC, with an
agent-selfish description: name a pack, get expert signal extraction in
one call, no rule authoring; lists the available packs; states that
`activate:true` + scope makes them live immediately.

- Acceptance: the MCP tool round-trips; unknown pack yields one
  teaching error naming the available packs; tool appears in
  `system_discover` catalogue + a contract fixture.
- Crates: mcp.

### TCE-ERG-PACK-4: thicken the cargo pack

Expand `rules/cargo.json` from 2 to ~6 rules with real coverage:
existing `compile-error` + `could-not-compile`, plus warnings
(`^warning: ...` with a rate limit), test failures (`test result:
FAILED. N passed; M failed`), panics (`thread '...' panicked at ...`),
and the aggregate `error: aborting due to N previous error(s)`. Every
new regex stays within the bounded-compile limits and ships with a
fixture sample line (matching TC14's fixture discipline). Keep
`status: "draft"` on disk (promotion happens at import).

- Acceptance: each new rule has a fixture proving it matches a
  representative real cargo line and does not match obvious noise;
  `import_all_seed_packs_from_repo` still passes with the higher count.
- Crates: rules + store tests + fixtures.

### Dogfood activity (performed, not built)

After PACK-1..4 land and a daemon is built from main: run a battery of
real terminal tasks through TC-with-cargo-pack vs raw Bash and write an
honest report to `docs/audits/`:
- a cargo build with a real compile error (does TC surface it cleanly
  in fewer tokens; did import_pack feel like one easy call);
- a long-running / noisy command (does TC's start-and-watch beat Bash
  blocking/flooding);
- a passing build (does the no-silence receipt read right);
- a test run with failures (does the thickened pack catch them).
Report: where TC won, where Bash was better, where cold-start still
bit, and the single next fix the data points to. This report IS the
superiority evidence; honesty (reporting losses, not just wins) is the
discipline.

## Invariants preserved

- Bounded-output security: pack rules are normal sifter rules; no raw
  stream exposure. The no-silence receipt carve-out (Phase 1) is
  unaffected.
- Draft-poison gate intact: promotion to `active` happens before the
  gate, so only genuinely-active rules activate.
- Argv-only / shell-deny / policy gating unchanged.
- No fourth copy of activation logic: PACK-2 reuses the existing
  `handle_registry_activate` path.

## Verification discipline

Per repo norm: every product-code change runs cargo fmt + clippy
(`--all-targets -D warnings`) + nextest on touched crates; daemon IPC
tests are `#![cfg(unix)]` and run under WSL2
(`wsl.exe bash -lc "cd /mnt/c/... && cargo nextest run -p <crate>"`).
Each task verified before the next; <= 5 files per step where practical.
`cargo clean` at task boundary.

## Provenance

Brainstormed 2026-05-27 this session, after Phase 1 shipped
(commits 1ffed95, 176b1d9, 867638f, 836d6d5, e2fd03a). Predecessor:
TC14 (rule packs + store importer, done), TC13 (registry CRUD). The
eval-harness idea was raised and DROPPED: the agent in normal use is
the instrument, so dogfooding replaces a built harness.
