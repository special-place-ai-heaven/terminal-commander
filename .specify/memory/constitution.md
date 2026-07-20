<!--
SYNC IMPACT REPORT
Version change: 1.0.0 -> 2.1.0
Bump rationale: The committed 1.0.0 two-process boundary is redefined as one
engine boundary with two approved delivery modes, which is a major governance
change. Environment sensing then expands policy, audit, authenticated-channel,
diagnostic, and output rules and adds one narrow private-decoder exception,
which is the 2.1 minor amendment. Both changes are ratified together here; no
intermediate 2.0.0 constitution was committed.
Modified principles:
  - II. Policy-Before-Spawn, Default-Deny, Opt-In Capabilities
  - III. Combed, Bounded Output
  - IV. Local-Only Privilege Boundary
  - V. Audit Every Gated Action
  - VII. Honest Degradation and Suggest-Never-Auto-Activate
Added sections:
  - Core Principles I-VII
  - Additional Constraints (Technology & Doctrine)
  - Development Workflow & Quality Gates
  - Governance
Removed sections: none
Templates reviewed:
  - .specify/templates/plan-template.md       OK  (Constitution Check gate is
    filled per-feature by /speckit-plan from this file; no token drift)
  - .specify/templates/spec-template.md       OK  (no constitution-mandated
    section added/removed; scope/requirements model compatible)
  - .specify/templates/tasks-template.md      OK  (principle-driven task types
    -- policy, audit, no-mock, verification-gate -- expressible as-is)
Follow-up TODOs: none
-->

# Terminal Commander Constitution

Terminal Commander is a local, policy-gated terminal and file signal engine
that runs commands and returns STRUCTURED SIGNALS, never raw streams. It is
delivered either through a thin stdio MCP adapter plus daemon IPC or through the
same public daemon-library engine embedded in an approved host. These principles
are binding on every feature, goal, and pull request. They are derived from
`docs/security/PRIVILEGE_MODEL.md`, `POLICY.md`, `SECURITY.md`, `TESTING.md`,
`CONTRIBUTING.md`, and the omni-program security invariants in
`docs/plans/LLM-HANDOFF-tc-omni-program.md`.

## Core Principles

### I. One Engine Boundary, Two Delivery Modes (NON-NEGOTIABLE)

The privileged engine implemented by `terminal-commanderd` is the sole owner of
probes, sifters, buckets, policy, audit, persistence, and operating-system side
effects. MCP delivery MUST remain two-process: `terminal-commander-mcp` MUST NOT
spawn commands, open PTYs, read files for the caller, touch the OS process table,
or bypass local IPC. The grep-test that proves the adapter contains no
`Command::spawn` (or equivalent) MUST stay green.

An approved in-process host MAY embed the public `terminal-commanderd` library
and call the same engine state and typed operations without manufacturing an MCP
adapter or IPC hop. Embedding MUST NOT introduce a second runner, policy engine,
audit path, probe implementation, or storage authority; it converges on the same
engine boundary and preserves every gate that MCP delivery exercises.

Rationale: one execution chokepoint is what makes policy, audit, and bounded
output enforceable. Delivery topology may differ, but duplicating or bypassing
the engine would make every other guarantee untrustworthy.

### II. Policy-Before-Spawn, Default-Deny, Opt-In Capabilities (NON-NEGOTIABLE)

No command, shell line, PTY, privileged op, connector action, environment
sensor, or remote target runs until the policy engine governing that action has
evaluated it. Default profiles MUST deny shell passthrough, privileged
execution, and remote targets. New capability surfaces MUST be gated behind
explicit `[policy.caps]` flags (`allow_shell`, `allow_session`,
`allow_privileged`, `allow_remote`, ...) that default to `false` and are enabled
only by the operator. A new cap absent from an existing configuration resolves
`false` under every profile, including `full_access`; selecting an old broad
profile MUST NOT silently authorize a future cap after upgrade. A trusted fixed
helper does not bypass policy; both its sensor class and its underlying command,
file, probe, or connector action MUST be authorized before use. There is NO
generic `sudo`/`doas`/`su` path and NO argv smuggling: a shell line travels in a
dedicated request field, never as `argv[0]=bash` on the argv command path.
`SHELL_INTERPRETERS_DENY` on the argv path MUST remain intact.

Rationale: capability is granted, never assumed. The operator -- not the LLM --
decides what the daemon may do on the host.

### III. Combed, Bounded Output (NON-NEGOTIABLE)

The LLM MUST NOT receive unbounded raw stdout/stderr on a normal tool response.
User-invoked command, shell, PTY, and privileged output flows through the sifter
runtime and returns as bounded, structured signal events with summaries,
severity, and source pointers. Exploratory tails (`command_output_tail`) MUST be
explicitly capped. Even shell and privileged output goes through the sifter. A
quiet command MUST still return a bounded receipt (exit state + suppressed
counts + short tail), never silence.

A product-defined fixed sensor helper MAY use a private typed decoder instead of
the normal sifter path only when input is bounded while it is read, the helper
cannot contain caller-defined executable text, raw frames never enter buckets,
rings, tails, context, logs, audit, snapshots, or persistent output, raw buffers
are discarded after parsing, and only typed evidence plus non-content diagnostic
codes and counts leave the decoder. This exception is not a general command
output path.

Rationale: token-bounded structured signal is the product. Raw scrollback in the
context window is the failure mode TC exists to prevent.

### IV. Local-Only Privilege Boundary

The daemon binds a local endpoint only -- UDS on Unix, named pipe on Windows --
and records peer identity (uid/gid/pid or SID) on connect. There MUST be no
public TCP listener. Remote reach is achieved by tunnelling to an existing local
socket (e.g. SSH `-L` to a remote daemon's socket), never by opening the daemon
to the network. Bounded outputs, path-denial, and prompt-secrecy rules apply
identically on every transport.

Every cross-boundary action, policy verdict, and audit receipt MUST be bound to
an authenticated integrity-protected channel or equivalent per-message
authentication rooted in the attested peer identity. Any connector carrying a
caller-supplied secret or environment overlay value additionally requires
authenticated end-to-end confidentiality; reachability or a challenge response
alone is insufficient.

Rationale: the trust model is per-user, per-host. The network is never inside
the boundary.

### V. Audit Every Gated Action

Every accepted gated action -- command start, shell start, PTY start, privileged
op, connector use, sensor helper, registry activation -- MUST write an
append-only audit record with a peer subject at the engine that authorizes and
executes it. A multi-node campaign additionally records a bounded owning-engine
summary without pretending to be the authority for another node. Audit subjects
and metadata MUST be redacted for secret-shaped values before they are written;
argv and shell previews are truncated and masked, never logged verbatim. Audit
is a production sink, not a debug aid.

Rationale: an action the host cannot account for after the fact is an action the
host did not really control.

### VI. No-Mock Production Paths and the Verification Gate (NON-NEGOTIABLE)

Test-only helpers, mocks, and stubs MUST stay isolated to `tests/`, `fixtures/`,
or `#[cfg(test)]`. Production code paths MUST NOT reach into test-only logic; a
test that "passes" by exercising a test double in a production configuration is a
verification failure. Every behavior-bearing change MUST carry a source-status
label (`live`/`partial`/`degraded`/`disabled`/`test-only`/`mock`/`blocked`);
`unknown` is a hard fail at commit. "It compiled" is NEVER proof of behavior.

The verification gate for any Rust-code change is, at minimum:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

Policy/security work additionally runs the `security` profile; MCP-tool work
additionally runs at least one through-the-daemon integration test per new tool.

Rationale: green CI on a mocked path is a lie. Evidence -- live behavior, real
data -- is the only acceptance currency.

### VII. Honest Degradation and Suggest-Never-Auto-Activate

When IPC blips mid-operation, tools MUST return a DEGRADED result that is a
strict superset of the normal payload (`degraded: true`, `recover_hint`, the
known `job_id`/`bucket_id`/`cursor`), never a bare error that discards a live
job, and never a silently invented success state. Advertised limits (timeout
caps, byte caps) MUST be honored to the wire, not exceeded. Rule suggestion
(`registry_suggest_*`) MUST NEVER auto-activate a rule; the loop is always
suggest -> `registry_test` -> explicit `registry_upsert`/`registry_activate`.

Public, recovery, log, and audit diagnostics for gated operations MUST use closed
typed codes plus bounded numeric or safe-enum fields. Raw subprocess, OS,
library, connector, remote-policy, and parser strings remain private transient
input. Contextual paths, endpoints, and request fields require one shared
structural sanitizer and are omitted whenever safe representation is unprovable.

Rationale: an agent can only trust a channel that tells the truth when things go
wrong. A dishonest "success" or "done" is worse than an honest failure.

## Additional Constraints (Technology & Doctrine)

- Implementation is a Rust workspace of policy-scoped crates plus a Node/npm
  distribution wrapper. The MCP layer is rmcp (stdio); persistence is rusqlite
  in WAL mode with the repository-owned manual SQL migration runner (refinery is
  intentionally not linked because its rusqlite line is incompatible). MSRV is
  the declared `rust-version` (1.92.0 at ratification);
  lowering it requires an explicit goal.
- Documentation and normal agent output are ASCII-only. Non-ASCII is preserved
  only for exact user text, filenames, source code, or required technical data.
- The MCP tool catalogue has a single source-of-truth count. Adding or removing a
  tool MUST update every count anchor and the `system_discover` fixture in the
  same change; the CI count assertions MUST pass.
- Architecture has two supported delivery shapes: `LLM -> stdio MCP -> adapter
  -> local IPC -> engine` and `trusted host -> public daemon-library engine`.
  Both terminate at the same policy/probe/sifter/bucket/audit authority.
  Capabilities are EXPANDED through new policy-gated seams, never by removing an
  existing guard.

## Development Workflow & Quality Gates

- The canonical CI pipeline is the seven steps in `TESTING.md` section 2 (fmt,
  clippy, cargo-deny, feature-matrix, MSRV, nextest+doctests, cargo-machete).
  Steps 1, 2, and 6 (default profile) are the pre-commit subset.
- Work is phased. A single change set touches a bounded, declared file set;
  multi-file refactors are split into phases that each verify before the next.
  Cleanup, refactor, and feature work are not mixed in one commit unless a spec
  requires it.
- Commits follow Conventional Commits and explain the "why". Branch policy and
  release automation (release-please, OIDC trusted publishing) are operator-gated:
  no push, force-push, remote-branch deletion, PR merge, or publish without
  explicit human approval.
- Every goal/feature reports against `TESTING.md` section 10 evidence rules:
  branch, files changed, per-command PASS/FAIL, and source-status notes.

## Governance

This constitution supersedes ad-hoc practice. When a rule here conflicts with a
lower document, this document wins until amended; surface the conflict rather
than silently following the weaker rule.

Amendments MUST be made by editing this file with a Sync Impact Report, a
semantic-version bump, and propagation to the dependent templates
(`.specify/templates/*.md`) and any affected runtime guidance. Versioning policy:
MAJOR for backward-incompatible governance/principle removal or redefinition;
MINOR for a new principle/section or materially expanded guidance; PATCH for
clarifications and non-semantic refinements.

Compliance is verified at review time: every PR and every speckit plan MUST pass
the Constitution Check against these principles, and any justified violation MUST
be recorded in the plan's Complexity Tracking table with the simpler alternative
that was rejected and why. The NON-NEGOTIABLE principles (I, II, III, VI) are not
subject to per-feature waiver.

**Version**: 2.1.0 | **Ratified**: 2026-06-16 | **Last Amended**: 2026-07-17
