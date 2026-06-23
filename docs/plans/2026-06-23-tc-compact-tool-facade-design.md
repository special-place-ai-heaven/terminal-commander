# Terminal Commander — Compact Tool Facade (LLM-Effectiveness) Design

**Date:** 2026-06-23
**Status:** Design (approved in brainstorming; pending written implementation plan)
**Driver:** Routing quality / LLM-effectiveness — *not* token economics, *not* symforge parity.

---

## 1. North star

**An LLM with zero prior Terminal Commander knowledge can run a command and get
its result in one obvious call, and only reaches deeper when it genuinely needs
to — never improvising its own orchestration.**

This is an *optimization so LLMs don't lose their heads using TC*. The facade is
the instrument; LLM-effectiveness is the goal. We infer from symforge's success
(it collapsed ~35 code-intel tools to 3) — we do **not** replicate its number or
its machinery. TC's domain is different (run / observe / interactive / files /
rules / system), so the cut matches *our* domain.

### Firsthand friction (the author drove TC live this session)

Real failure modes observed while using TC to run a multi-stage cargo gate:

1. **No obvious default for "run a command."** `command_start_combed` vs
   `run_and_watch` vs `shell_exec` vs pty vs session — deliberation is pure
   overhead. This is the routing-quality problem in the flesh.
2. **Improvised orchestration.** Wanted one job doing `clippy && test && embed…`;
   argv-only / shell-denied blocked it (learned by hitting it), so ran 5
   sequential jobs **and hand-rolled a `tasklist | grep cargo.exe` waiter**
   between each — because `bucket_wait` wasn't trusted/found to fire on a clean
   exit. The tool *had* the primitive; the consumer reinvented it.
3. **Three IDs from one call** (`job_id`, `bucket_id`, `probe_id`) — used one,
   ignored two, never sure that was right.
4. **`status` vs `output_tail` vs `events`** — which yields content vs counters
   took trial reasoning.

The design must kill #1–#4.

---

## 2. Acceptance criteria (how we measure success)

The success test is **not** "did we hit 5 facades." It is:

- **AC1 — one-call happy path:** a TC-naive LLM completes a *run-and-observe*
  task (run a command, learn pass/fail + see output) in **one** obvious call
  (the promoted composite), without polling glue.
- **AC2 — self-documenting verbs:** the available operations are discoverable
  from the tool schema alone (the `action` enum), no external catalog needed.
- **AC3 — no improvised waiters:** the workflow ("start → wait → collect") is
  taught by the schema/descriptions; the LLM never hand-rolls process-polling.
- **AC4 — nothing breaks:** the full 50-tool surface and every existing parity /
  count test still pass; existing clients that hard-code tool names keep working
  (backward-compat aliases).
- **AC5 — honest + lossless:** no input param is ever silently dropped (conformance
  test); no number reaches the wire unless measured or `est.`-labelled.
- **AC6 (bonus, not the goal):** `tools/list` payload shrinks materially under
  `TC_SURFACE=compact`.

**Live-dogfood gate:** after a version ships to npm, reconnect the MCP and have
the author (an LLM) drive the compact surface end-to-end. AC1–AC3 are judged from
that lived session, then adjusted. This loop is the real acceptance test.

---

## 3. Non-goals

- **Not** replicating symforge's L2 economics / L3 bypass / L4 ledger layers.
  Those earned their keep for token-budget routing, not routing quality. (They
  are also actively being de-fabricated in symforge — we do not want that debt.)
- **Not** an NL-`query` intent planner. TC ops are precise; the agent knows the
  verb. A closed `action` enum routes more precisely than NL classification.
- **No daemon / IPC changes.** The MCP surface is reshaped; `IpcRequest` /
  `IpcResponse` and all daemon handlers are untouched.
- **Not** removing the 50 tools. They remain as `TC_SURFACE=full` + aliases.
- **No new mutation/preview engine** (see §8).

---

## 4. Architecture (L0 + L1 + guard, over the unchanged IPC union)

Three layers only:

```
 MCP client
    │  tools/call  { name: "command", arguments: { action: "run_and_watch", … } }
    ▼
 L0  Action vocabulary  — a few intent-scoped facade tools, each exposing a
     closed `action` enum + per-action typed params (a discriminated union
     mirroring the IpcRequest variants).
    ▼
 L1  Deterministic dispatch — (facade, action) → the exact existing handler.
     A pure match; no NL, no planner, no scoring.
    ▼
 [no-silent-drop guard]  every input field resolves to
     Routed | Forwarded | Refused | NotApplicable (conformance test).
    ▼
 UNCHANGED: IpcRequest (tagged union, ~49 variants) → daemon handlers.
```

**Why this is mechanical for TC:** today every one of the 50 MCP tools is a
near-identical 4-step forward (`ensure_daemon → marshal IpcRequest::X →
daemon.call → match IpcResponse::X → json`). The daemon protocol is *already* a
`#[serde(tag="method", content="params")]` tagged union; the current MCP layer
merely re-expands that union into 50 named tools. The compact surface re-collapses
it into a few verb-dispatched tools. The hard decoupling (surface ⟂ daemon) is
already done and is proven by the fact that some IPC methods already have no MCP
tool and some MCP tools do no IPC round-trip.

### Surface model

- **`TC_SURFACE=compact|full`**, env-gated, enforced at **both** `tools/list`
  (advertise) **and** `tools/call` (reject a hidden name — hiding alone is not
  enough).
- **`full` stays** (all 50 tools) as the escape hatch; **backward-compat aliases**
  keep old callers and every parity test green.
- **Ship `full` as the default first** (de-risks against the existing "omni"
  expansion roadmap), then **flip the default to `compact`** once the live
  dogfood loop proves routing quality. Never the "replace" option.

---

## 5. The five facades

Cut by **agent intent** (validated by the mechanical test in §6). Each facade
carries an `action` enum; composites preserved as their own actions. This is a
**starting point — we add facades/actions if dogfooding proves it lacking.**

| Facade | Intent | Actions (~) |
|---|---|---|
| **`command`** | run + observe + stream a one-shot command | `run` (start_combed), **`run_and_watch`** ⭐, `exec` (shell_exec), `status`, `output_tail`, `stop`, `events` (bucket_events_since), `wait` (bucket_wait), `summary` (bucket_summary), `event_context`, `sub_open`, `sub_pull`*, `sub_seek`, `sub_close`, `sub_list` (15) |
| **`session`** | hold a live interactive/persistent shell | `pty_start`, `pty_stdin`, `pty_stop`, `pty_list`, `sh_start`, `sh_exec`, `sh_status`, `sh_stop`, `sh_list` (9) |
| **`files`** | local workspace I/O | `read`, `write`, `search`, `watch_start`, `watch_stop`, `watch_list`, `snapshot_create`, `snapshot_apply` (8) |
| **`registry`** | manage the recognition-rule library | `search`, `get`, `upsert`, `test`, `activate`, `deactivate`, `list_active`, `import_pack`, `suggest_from_samples` (9) |
| **`status`** | inspect system / daemon / target state | `health`, `self_check`, `policy_status`, `runtime_state`, `probe_list`, `probe_status`, `system_discover`, `target_list`, `target_probe` (9) |

(15 + 9 + 8 + 9 + 9 = 50 ✓)

⭐ **`run_and_watch` is the loud, promoted happy path** — the `command` facade
description leads with it: *"To run a command and get its result in one call, use
`action:"run_and_watch"`. It does start + wait + collect for you."* This single
move targets AC1/AC3 directly (kills the hand-rolled-waiter failure).

\* `sub_pull` keeps its dedicated long-poll client + 12s timeout; `run_and_watch`
keeps its multi-IPC composite body. Composites are distinct actions, never forced
through the generic forward.

---

## 6. Validation test (the routing-quality guard)

For each of the 50 actions: *"if an agent wants this, what facade does it look in
first?"* 46/50 are unambiguous. The 4 two-home seams and their resolutions:

| Seam | Action(s) | Resolution |
|---|---|---|
| S1 | `target_list`, `target_probe` | **Moved** files → `status` (remote-target discovery/health; siblings of `system_discover`; read-only). Makes `files` cleanly local-I/O. |
| S2 | `shell_exec` | Keep in `command` as `command.exec`; cross-ref in `session` description ("one-shot commands → `command`"). |
| S3 | `probe_list`, `probe_status` | Keep in `status` (introspection over all probes); cross-ref from `command`. |
| S4 | `file_watch_*` | Keep in `files` (file-scoped); cross-ref from `command` (event streams). |

Only S1 is a move; S2–S4 are description cross-refs ("patch ambiguity with prose,
not more facades"). **This test becomes a maintained invariant:** every action
has exactly one primary facade; adding an action requires assigning its single
home (or an explicit cross-ref).

---

## 7. Dispatch design

- Each facade is one rmcp `#[tool]` whose input is a struct with a **closed
  `action` enum** + the per-action params, modeled as a **discriminated union**
  that mirrors the relevant `IpcRequest` variants. This preserves **typed,
  per-action schema validation** — strictly better than a loose `{action,
  params}` blob — and dissolves the "schema collapse weakens validation" risk.
- `(facade, action)` → a pure `match` to the existing handler (the same handler
  the named tool calls today). Handlers are **encapsulated, not rewritten**.
- Discriminated-union generation should derive from / stay in lockstep with the
  `IpcRequest` tagged union so the action surface cannot silently drift from the
  protocol. (Mechanism — derive vs. generated mirror vs. hand-written-with-parity-
  test — is an implementation-plan decision; the invariant is "no drift.")

---

## 8. Mutation lifecycle (the writes)

Writes (`files.write`, `registry.upsert`, `files.snapshot_apply`) are scattered
across read-heavy facades. Decision: **no new preview/apply engine.**

- **`registry.upsert`** already has its preview: `registry.test` (test a rule
  before committing). The `test → upsert → activate` flow *is* the lifecycle.
- **`files.snapshot_apply`** — the one genuinely destructive op (can clobber a
  whole workspace). No preview *layer*; instead a **loudly destructive
  description** and, if the daemon already supports it, a `dry_run` flag. Honest-
  envelope, not new machinery.
- **`files.write`** — stays simple; rely on existing daemon semantics, plus an
  `overwrite`/`if_match` guard **only if** the IPC method already exposes one.

The no-silent-drop guard + honest descriptions carry write-safety. Build stays at
L0 + L1 + guard.

---

## 9. Discoverability (how the schema teaches the workflow)

Routing quality is carried by the **action name**, not the grouping — so the
schema must be a self-teaching menu:

- **The `action` enum is the verb list** (AC2): one look at the schema shows
  every operation a facade offers.
- **Descriptions encode the happy path + cross-refs** (AC1/AC3): each facade
  description leads with the recommended action and the start→wait→collect idiom;
  S2–S4 cross-refs live here.
- **A `help` / `list` action** per facade (or a single glossary MCP *resource*)
  returns the verb catalog + one-line intents + the canonical workflow. (`help`-
  action vs glossary-resource is an open item — §11.)
- **Honest response envelope** (AC5): outputs carry only measured or `est.`-
  labelled values; never decorate with un-measured numbers.

---

## 10. Honesty contracts (baked in from day 1)

1. **Lossless-or-loud:** every input field resolves to `Routed | Forwarded |
   Refused | NotApplicable` — never silently dropped. Enforced by a
   `ParamDisposition`-style conformance test (mirrors symforge
   `tests/stel_param_disposition.rs`). Retrofitting this later is expensive;
   it goes in at the start.
2. **Honest-envelope:** only measured or explicitly `est.`-labelled values reach
   the wire.

---

## 11. Testing & rollout

**Tests**
- Existing 50-tool **parity/count tests stay green** under `TC_SURFACE=full`
  (AC4).
- **Validation-test invariant** (§6): every action has exactly one primary facade.
- **ParamDisposition conformance** (§10.1).
- **`tools/call` gate test**: a hidden legacy name is rejected under `compact`.
- **Schema-byte budget** as a unit test (AC6, bonus).
- **Live-dogfood acceptance** (AC1–AC3): the author drives the compact surface
  after npm install and records residual friction.

**Rollout / dogfood loop**
```
implement → cargo gate green → release-please → npm
   → reconnect MCP with TC_SURFACE=compact
   → author (LLM) drives compact surface live
   → measure vs AC1–AC3, record friction
   → adjust (add actions/facades "if it proves lacking") → flip default to compact
```

---

## 12. Open questions / risks

- **Facade naming:** `command` / `session` / `files` / `registry` / `status` —
  final names TBD (could mirror a `<noun>` / `<noun>_edit` convention if preferred).
- **Discoverability surface:** per-facade `help` action vs a single glossary MCP
  resource (or both).
- **Discriminated-union source of truth:** derive from `IpcRequest`, generated
  mirror, or hand-written + drift test (decide in the plan; invariant = no drift).
- **Process tension:** the existing "omni" roadmap expands the named surface;
  full-default-first defuses it, but the relationship between the two programs
  should be stated explicitly so they don't fight.
- **`snapshot_apply` dry-run:** depends on whether the daemon already supports it.

---

## 13. Summary

Collapse TC's 50 advertised MCP tools into **5 intent-scoped facades** with typed
`action`-enum dispatch over the **unchanged** daemon IPC union — built as **L0
vocabulary + L1 deterministic dispatch + a no-silent-drop guard only**, env-gated
and reversible (`TC_SURFACE`, full + aliases retained, full-default → flip to
compact). Success is measured by **a fresh LLM running-and-observing in one
obvious call without improvising a waiter**, validated by a **live dogfood loop**,
not by the facade count. Start at 5; **add more if it proves lacking.**
