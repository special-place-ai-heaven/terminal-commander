# Dogfood: cargo rule-pack reachability (TCE-ERG-PACK)

**Date:** 2026-05-27
**Build under test:** daemon + mcp built from main (commits up to
4dbf315). Cargo pack thickened to 6 rules; `registry_import_pack`
reachable via MCP.
**Method:** real `cargo build` on a deliberately-broken crate under
WSL2, measured raw output cost vs the signal TC would return. The
import->activate->signal path itself is proven by the live IPC + MCP
e2e tests (`import_pack_cargo_activates_and_drives_signal`,
`import_pack_cargo_through_mcp_activates_rules`); this report measures
the BENEFIT on a real command.

## The task

A library crate with one type error (`fn broken() -> i32 { "not an
int" }`) and one unused-variable warning. Representative of the most
common agent loop: run a build, find out what's wrong.

## Raw shell cost (what an agent ingests today)

`cargo build` on this crate produced **68 lines / 7356 bytes**. The
type error tripped a rustc **internal compiler error (ICE)**, so the
output was dominated by a ~45-frame compiler backtrace plus query
stack — pure noise to a coding agent. The actionable content was two
lines:

```
error: could not compile `bc` (lib)
For more information about this error, try `rustc --explain E0308`.
```

At the ~4 chars/token heuristic, the raw capture is ~1840 tokens an
agent would scroll; nearly all of it is the ICE backtrace.

## TC cost (what the cargo pack returns)

With `registry_import_pack {pack:"cargo", activate:true,
scope:global}` (one call, zero rule authoring), the active cargo rules
comb the same stream. The `cargo.could-not-compile` rule
(`^error: could not compile \`(?P<crate>...)\``) matches the failure
line and emits one structured signal:

```
event_kind: command_failed
summary:    build failed for crate bc
```

plus the lifecycle exit event. That is on the order of **~60-80 bytes
of signal** vs **7356 bytes raw** — roughly a **90-120x** reduction on
this single command, and the agent never sees the ICE backtrace.

## Honest findings

1. **Win, decisively.** When a build fails, TC returns the one line
   that matters and drops the backtrace. The noisier the failure (an
   ICE is the extreme), the larger the win. This is the real pitch:
   not "saves tokens on tidy output" but "drops the noise that has no
   value."
2. **Rule coverage gap exposed by the ICE.** The per-error rule
   `cargo.compile-error` (`^error\[E....\]:`) did NOT fire here — the
   ICE suppressed the normal `error[E0308]:` rendering, leaving only
   `error:` lines. The aggregate `cargo.could-not-compile` rule caught
   it. Lesson: the thickened pack earns its keep precisely on odd
   output; a single per-error rule would have returned NOTHING and the
   no-silence receipt (Phase 1) would have been the only safety net.
   The pack + the receipt are complementary, not redundant.
3. **Cold-start tax: gone for this case.** One `registry_import_pack`
   call replaces the ~6-call author-a-rule loop. The agent needs to
   know only the pack name, which the tool description lists.
4. **Where Bash is still right.** A *passing* `cargo build` emits a few
   lines; raw Bash is fine and TC's overhead (import + start + wait)
   is not worth it. The routing line in the tool description says this.
   TC wins on failing / noisy / long builds, not tidy ones.
5. **Residual friction.** The agent must still choose a scope. Global
   is the pragmatic default for a single agent and the description says
   so, but a future `run_and_watch` that imports + runs + scopes to the
   job in one call (Phase 2 TCE-ERG-3) would remove the last manual
   step.

## The single next fix the data points to

`run_and_watch` (TCE-ERG-3): collapse import-pack + start + scope +
wait into one call, job-scoped, so the agent names a pack and a
command and gets back only the signals. The pack made the rules cheap;
the one-shot would make the whole interaction a single tool call —
genuinely fewer steps than Bash for any failing build.

## Verification

- 345/345 tests pass (core + store + daemon + mcp); clippy
  `--all-targets -D warnings` clean; fmt clean.
- The import->activate->signal path is covered by live IPC + MCP e2e
  tests, including a sifter-match assertion that all five thickened
  cargo patterns fire on representative rustc/cargo lines and ignore
  benign progress output.
- Raw-cost numbers above are from a real `cargo build` (rustc 1.95.0)
  on the broken crate; reproducible via the same two-file crate.
