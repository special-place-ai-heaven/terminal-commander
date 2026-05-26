# Spec: Agent-Ergonomics Chain (TCE-ERG)

Status: Design (council-reviewed 2026-05-26). Language: ASCII only.

## Objective

Make an LLM coding agent TRUST and PREFER Terminal Commander (TC) over
raw shell. The 48-goal MVP + runtime chains proved TC correct, bounded,
and secure. None made an agent *prefer* it. An LLM selects a tool in one
decision on expected (cost vs success); TC must win that bet on the
first call.

## Non-goals

- Not changing the bounded-output security invariant (TC29/TC47 gates hold).
- Not a macOS/Windows-native port (out-of-MVP stays out).
- Not the rule-pack library / multi-agent fleet work (deferred; see Phase 4).

## Evidence this chain exists to fix (measured this session)

- First-time use by a capable LLM: ~10 tool calls to first signal,
  ~6 schema-rejection errors learning the rule JSON by trial and error.
- The draft-poison footgun (now fixed: commit 8b2eb21 + fc8468d).
- `wsl uname -a` through TC returned NOTHING (no rule matched -> no
  output). An LLM trained on shell reads silence as "broken" and falls
  back to Bash permanently.
- Proven benefit once working: `agent_superiority_bench` shows ~423x
  token reduction (142,509 -> 337 tokens) on a 5,000-line noisy task,
  exact error surfaced.

## Council verdict (5 advisors + 5 peer reviews, 2026-05-26)

Reordered the naive design. Key findings:

1. **Silence is the root trust-killer.** A zero-rule command returning
   nothing reads as breakage. This caused the 10-call thrash. The fix
   (a receipt, not silence) is the single highest-leverage change and
   also solves cold-start and signal provenance.
2. **The incentive gap (strongest insight, 5/5 reviewers).** 423x saves
   the USER's tokens; the AGENT has no in-loop incentive to handicap
   itself. Reframe the pitch as agent-selfish: "returns the matching
   signal instead of 4,800 lines you would scroll; runs commands too
   big to fit in context." Put it where the agent reads it.
3. **The tool DESCRIPTION string IS the selection mechanism.** Agents
   pattern-match descriptions to choose tools. Invest there first.
4. **"Refuse small commands" is rejected** (council's worst idea):
   refusal manufactures the distrust we are killing. TC must never
   bounce the agent to Bash by erroring.
5. **Trust is behavioral.** Definition of done = measured TC-vs-Bash
   selection rate + zero-fallback on a fixed suite, not "it compiles."
6. **Deferred moat:** curated rule-packs, rule-suggestion-from-bucket,
   shared-bucket multi-agent observability are the strongest long-term
   value but presuppose an already-trusted tool. After Phase 3.

## Goals + phases

Each phase ends at the behavioral eval, not at "it compiles."

### Phase 1 - The trust floor: kill silence, show the pitch

- **TCE-ERG-1 No-silence default.** When a command finishes and ZERO
  rules matched, the response MUST include exit code, suppressed-line
  count, and a bounded tail (last N lines, byte-capped). `uname -a`
  must return `exited 0; 0 lines suppressed; output: Linux ...`. A
  noisy no-rule run returns `exited 0; 4,812 lines suppressed; tail:
  <last 5 lines>`. Never empty.
  - Acceptance: a no-rule `command_start` surfaces non-empty, bounded,
    truthful output through the MCP surface; payload <= MAX_RESPONSE_BYTES.
  - Crates: daemon (bucket/lifecycle event carries tail + suppressed
    count; naive last-N from the existing bounded context ring is
    acceptable for Phase 1, full ring buffer is Phase 2 polish).
- **TCE-ERG-2 Agent-selfish description + routing line.** Rewrite the
  MCP server `instructions` and the command-tool descriptions to lead
  with: (a) the no-output-by-default signal model; (b) the
  agent-selfish pitch; (c) a routing rule stating when Bash is
  correctly the right call (tiny / interactive / one-off) so the agent
  is not miscalibrated into using TC for everything.
  - Acceptance: instructions string names the no-output model in its
    first sentence and contains the routing rule; tool descriptions
    carry the pitch. Pure text + rmcp `#[tool]` descriptions.
  - Crates: mcp.

### Phase 2 - Collapse the call count, teach in-band

- **TCE-ERG-3 `run_and_watch` one-shot tool.** One MCP call: start +
  inline watch keywords/regex + bounded wait + return matched signals +
  exit code + (on zero matches) the Phase-1 receipt. Bash-equivalent
  ergonomics; composes existing `command_start_combed` + inline
  `rules_json` + `bucket_wait` + `command_status`.
  - Acceptance: a single `run_and_watch` call on the canonical noisy
    task returns the error signal + exit code with no other calls.
  - Crates: mcp (+ daemon if a new IPC convenience is warranted; prefer
    composing existing IPC).
- **TCE-ERG-4 Teaching errors.** Every rule/command rejection returns
  the full expected shape + a copy-pasteable correct example + the
  remedy, in one error (not one-missing-field-at-a-time). Subsumes the
  separate schema tool: `input_examples` on `run_and_watch` + inline
  examples in errors replace a standalone `rule_schema` call.
  - Acceptance: feeding a malformed rule yields one error containing a
    valid example; no multi-round field-by-field rejection.
  - Crates: daemon (ipc error construction), mcp (input_examples).

### Phase 3 - Prove trust moved, then gate it

- **TCE-ERG-5 Trust-regression gate.** Two parts: (a) the existing
  `agent_superiority_bench` token-reduction floor (already committed);
  (b) a fresh-LLM behavioral eval measuring TC-vs-Bash selection rate
  and zero-fallback on a fixed task suite, plus the decision-boundary
  check (correctly picks Bash for tiny/interactive). Non-determinism
  handled by a pass threshold over N runs, not a single assertion.
  - Acceptance: the behavioral eval runs and reports a selection-rate
    number; CI gates on the deterministic token-floor now and on the
    selection-rate threshold once stable.
  - Crates: daemon/tests + ci. The eval harness design is itself a
    sub-task (LLM-in-the-loop; keep it cheap + thresholded).

### Hygiene (not a phase)

- **TCE-ERG-6 Consolidate `merge_active_and_inline`.** Three identical
  copies (command.rs, file_watch.rs, pty_command.rs) -> one guarded
  helper in the sifters crate. Done while touching sifters; eliminates
  the fix-one-miss-two duplication class that hid the draft-poison bug
  in two of three paths. Already partially mitigated (all three now
  carry the eligibility filter); this collapses them to one.

### Deferred moat (after Phase 3 proves trust)

- Curated rule-packs (cargo / pytest / npm / k8s) for zero-config
  expert signal extraction.
- Rule-suggestion-from-bucket ("4,000 lines, 3 patterns dominate, want
  rules?") = triage co-pilot.
- Shared-bucket multi-agent observability.

## The one thing to do first

Make a zero-rule command return a receipt instead of silence
(`exited 0; N lines suppressed; tail: <last 5>`) AND put the
agent-selfish pitch in the tool description. One coupled change: the
description is what makes the agent CHOOSE TC; the receipt is what makes
it KEEP trusting TC after it does. Kills the documented `uname -a`
failure with no ring buffer required.

## Verification discipline

Per repo norm: every product-code change runs cargo fmt + clippy +
nextest on touched crates; daemon IPC tests are `#![cfg(unix)]` and run
under WSL2 on this host. Each phase verified before the next; <= 5 files
touched per step where practical.

## Provenance

Council transcript + per-advisor reasoning: this commit's session.
Bug-fix predecessor commits: 8b2eb21, fc8468d. Adversarial review of
the bug fix: docs/audits/2026-05-26-draft-poison-fix-review.md.
