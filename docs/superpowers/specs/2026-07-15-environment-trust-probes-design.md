# Environment trust probes

Status: proposed; implementation requires review.

## Decision

Preserve the intent of CAP01, but do not revive its generalized capability
registry.

Terminal Commander should add one bounded, read-only environment inspection
operation to its existing discovery engine. That operation should inventory
environment-variable **names only** at each reachable hop and verify a small,
typed set of runtime and dependency prerequisites. A built-in probe catalogue
selects safe implementations. The existing rule registry remains exclusively a
sifter-rule registry.

Reusable prerequisite profiles may be added later as declarative data over the
typed operation, after repeated real use demonstrates that persistence saves
LLM work. Profiles must never contain commands, shell text, scripts,
credentials, or arbitrary argv.

This is a narrow hybrid:

- automatic discovery continues to find and rank viable access routes;
- an on-demand typed inspection verifies facts about the selected route;
- optional declarative profiles may later group repeated checks;
- no second policy engine, runner, scheduler, or generic capability executor is
  introduced.

## Product invariant

The product test is the banner promise: **Full signal. No noise.**

An environment result is useful only when it:

1. removes a concrete LLM uncertainty;
2. is observed through the actual route the LLM can use;
3. is bounded, structured, and explicit about incomplete evidence;
4. cannot disclose an environment-variable value or credential;
5. does not create a command-execution bypass; and
6. is available from both the daemon library embed and the MCP delivery
   surface without duplicating the engine.

## Code-backed baseline

The current implementation already supplies most of the required foundation:

- `environment::discover_host_environment` probes shells and a fixed common
  tool catalogue concurrently, confirms WSL, ranks access routes, and selects a
  beachhead.
- `DaemonState::discover_environment` applies the active shell capability to
  the same result used by daemon `system_discover`.
- `EnvironmentRouter` forwards environment-bearing requests to a WSL runner;
  the MCP target router forwards requests to an operator-configured remote
  daemon.
- `ProcessProbe` uses overlay environment semantics: children inherit the
  engine process environment and supplied names replace or add entries.
- `terminal-commanderd` is a public library. Its `DaemonState` owns policy,
  routing, commands, probes, sifters, subscriptions, audit, and persistence
  without requiring MCP or CLI.
- The persistent registry accepts validated `RuleDefinition` values and its
  SQLite schema, FTS index, activation records, and import packs are all
  rule-specific.

The real gaps are narrower than CAP01:

- discovery does not expose a names-only environment census for the adapter,
  engine, WSL execution environment, remote daemon, or effective child;
- `target_probe` discards the remote `SystemDiscover` environment and reports
  only reachability/version;
- the fixed tool catalogue cannot answer an explicit project prerequisite
  query such as Python runtime plus Python distribution versions;
- shell-session status currently serializes its environment overlay as
  `(name, value)` pairs through IPC and MCP. That violates the new outward
  names-only contract even though the values originated with the caller.

## Options considered

### 1. Keep only `system_discover`

This has the smallest implementation cost and remains the correct first call
for route selection. It does not answer which environment names exist at each
hop or whether a requested project dependency is installed. LLMs would still
fall back to ad-hoc shell probes, repeating the uncertainty this work is meant
to remove.

Decision: insufficient by itself.

### 2. Revive CAP01 as a generalized capability registry

This would expand the rule registry into information probes, action probes,
bootstrap probes, adapters, arbitrary schemas, risk declarations, and
credential requirements.

The present code has no neutral registry abstraction to extend. The registry is
intentionally coupled to `RuleDefinition`, sifter validation, FTS fields,
activation scopes, and hot rule binding. Generalizing it would either distort
that working model or create a parallel orchestration framework beside
`DaemonState`, `PolicyEngine`, `EnvironmentRouter`, and `Router`.

Decision: rejected. CAP01 remains useful product doctrine, not the current
architecture.

### 3. Add parameterized typed probes only

This adds the missing facts with no persistent schema and no arbitrary runner.
It is simple, auditable, and exercises the existing route and policy layers.
The tradeoff is that an LLM repeats a requirement list across sessions.

Decision: recommended for the first implementation.

### 4. Add typed probes plus declarative prerequisite profiles

Profiles can later reduce repeated calls by naming a bounded list of typed
requirements. They must compile to the same typed request as option 3; a
profile grants no authority and contains no executable material.

Decision: retain as an evidence-gated extension. Do not add storage or a public
profile API in the first implementation.

## Proposed architecture

### One engine operation

Add a public `DaemonState::inspect_environment` operation and one matching IPC
request/response. The MCP compact surface exposes it as one new `status` action,
not a new top-level MCP tool.

Conceptual input:

```text
environment_inspect
  environment?: local | wsl_distro
  target_id?: delivery-layer remote target
  include_env_names?: false
  requirements?: [bounded typed requirement]
  format?: concise | detailed
```

`target_id` remains an MCP delivery concern and selects the daemon client.
`environment` remains an engine concern and uses the existing
`EnvironmentSpec`/`EnvironmentRouter`. An in-process embed calls the same engine
operation directly and has no synthetic adapter hop.

The default concise response returns hop identity, reachability, observation
time, environment-name count/completeness, and prerequisite summary. Full names
appear only when `include_env_names=true`. Detailed prerequisite evidence is
bounded independently from the names list.

### Hop model

Each observation identifies where it was measured:

- `adapter`: the MCP adapter process, when an adapter exists;
- `engine`: the daemon or embedding host process;
- `execution`: the selected local or WSL runner environment;
- `remote_engine`: the daemon reached through a configured remote target; and
- `child_effective`: engine/execution names plus explicit command overlay names
  when TC can prove overlay semantics.

Every hop reports `observed_at`, `name_count`, `complete`, and optional sorted
names. Unknown or unreachable hops remain explicit results; TC never fills them
from platform assumptions.

Local names come from the process API without reading values. A WSL execution
probe must run through the real selected route and emit or privately capture a
NUL-delimited names-only result. Raw `NAME=VALUE` output must never enter a
bucket, context ring, audit record, log, error, or response. Remote inspection
executes on the remote daemon rather than projecting the adapter's environment.

Names are sorted and deduplicated using target-platform case semantics. Each
list has independent item, per-name byte, and total-byte caps. A cap produces
`complete=false`; it never silently presents a partial list as complete.

### Outward no-values boundary

Before adding inspection, change the shell-session outward status contract so
MCP/IPC returns `env_names`, never `env_snapshot` values. The session runtime
may retain overlay values internally because workspace snapshot restoration
uses them, but no LLM-facing response may serialize them.

Canary tests must place recognizable sensitive-looking values in the engine,
session overlay, WSL environment, and remote environment, then assert those
values are absent from:

- IPC and MCP JSON;
- errors and retry guidance;
- audit rows and logs;
- buckets, context rings, and output tails; and
- snapshots returned to callers.

Variable names are permitted. Derived terminal/CI facts may remain separate
normalized fields; they must not be confused with the raw environment census.

### Typed prerequisite catalogue

The catalogue maps stable IDs to code-owned probe implementations. It is not a
database and does not accept executable definitions.

The first useful set should be deliberately small:

- `executable`: resolve an executable and obtain its version using a built-in
  adapter selected by executable ID;
- `python_runtime`: resolve the selected Python interpreter and report its
  implementation/version;
- `python_distribution`: query installed distribution metadata by validated
  distribution name without importing or executing package code.

Callers supply names and version requirements as data. They never supply
version argv, shell text, scripts, module imports, or executable paths for a
read-only probe. Unknown catalogue IDs fail with a bounded list of supported
IDs and a corrective example.

Each item returns one of:

- `satisfied`;
- `missing`;
- `incompatible`;
- `unknown` (including unparseable or unsupported version semantics); or
- `probe_failed` with retryability and bounded recovery guidance.

Version evaluation must use ecosystem-correct parsers. Python distributions
require PEP 440 semantics; semver must not be substituted. When TC cannot prove
a comparison, it returns `unknown` and the observed version rather than
guessing.

Python distribution inspection must use metadata only. Importing a package to
test it is forbidden because import hooks and package top-level code can have
side effects. The fixed helper runs with bounded time/output and treats the
distribution name only as validated data.

### Policy and audit

Environment inspection is read-only but may spawn fixed probe helpers. Give it
its own `PolicyAction::EnvironmentInspect` rather than borrowing
`CommandStart`. This keeps it available to a read-only observer while making
the authorization and audit trail explicit.

The action permits only compiled catalogue implementations. It cannot widen
command, shell, session, PTY, file, or remote capabilities. Remote selection
continues to require `allow_remote`; WSL routing continues through the current
environment router. Audit records operation, hop, catalogue IDs, counts,
duration, and verdicts—never environment values or raw helper output.

### Bounds and lifecycle

The operation has independent caps for:

- hops;
- environment names and bytes per hop;
- requirement items;
- per-probe output bytes;
- per-probe timeout;
- total timeout; and
- probe concurrency.

Inspection is a short one-shot operation. It creates no long-lived job, PTY,
watch, subscription, bucket, or historical runtime entry. Results may be cached
in memory for the lifetime of a connection with `observed_at` and an explicit
refresh path; they are not persisted in the first implementation.

## Compatibility and migrations

- Keep `system_discover` and its beachhead semantics unchanged. It remains the
  default route-discovery call.
- Add one IPC request/response and one `status` action. Older daemons are
  detected through the existing advertised-method/version handshake; the
  adapter must not send an unsupported request.
- Do not change the rule registry schema or public rule operations.
- Do not add a store migration for prerequisite profiles in the first phase.
- Treat the shell-session response hardening as an intentional 0.x security
  correction. Internal restoration data remains available inside the engine;
  the public LLM surface changes from values to names and must be called out in
  the changelog.
- Export the engine request/response types and `DaemonState` method so AAP's
  `terminal-commanderd` embed receives the capability without MCP or CLI.
- Keep all OS restrictions truthful. A missing WSL runner or unsupported
  platform returns an explicit unavailable hop, not a synthetic local result.

## Verification gates

Implementation is not complete until all of these pass:

1. RED/GREEN protocol tests for omitted defaults, caps, stable status values,
   and unsupported catalogue IDs.
2. Linux and Windows unit tests proving sorted names and platform-correct name
   equality.
3. MCP live tests for local, WSL, and forwarded remote hops, including proof
   that the observation came from the selected environment.
4. Canary non-disclosure tests across IPC, MCP, errors, audit, logs, buckets,
   rings, tails, and session status.
5. Policy tests proving read-only inspection is allowed, remote inspection
   still requires `allow_remote`, and arbitrary commands/argv are
   unrepresentable.
6. Python tests for present, missing, incompatible, malformed-version, timeout,
   and no-import behavior using isolated fixtures.
7. The in-process embed example and an AAP-side integration check against the
   pinned `terminal-commanderd` revision.
8. Agent-call contract tests: representative prompts must select the status
   action and produce a valid first call without memorizing action-specific
   field subtraction.
9. Full formatting, clippy, workspace nextest, doctest, Linux gate, and Windows
   gate required by repository policy.

## Deferred profile layer

Reconsider profiles only after real usage shows repeated requirement lists. A
profile would contain an ID, description, and bounded declarative requirements
that compile to `environment_inspect`. It would not contain permissions, risk
levels, credentials, commands, adapters, or runners; those already belong to
the existing policy and engine.

If persistence is eventually justified, use a separate typed store contract.
Do not overload the sifter rule tables or call the result a generalized
capability registry.

## Non-goals

- installing, upgrading, or repairing dependencies;
- executing caller-provided diagnostic commands;
- importing user Python packages;
- scanning or returning environment values;
- discovering credentials;
- replacing command/process probes;
- replacing `system_discover` or its beachhead;
- adding a second scheduler, policy engine, audit system, or route selector;
- storing arbitrary capability definitions; or
- promising exact child environment state where TC has not measured it.
