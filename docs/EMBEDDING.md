# In-process embedding

Embed `terminal_commanderd::embedded::EmbeddedEngine` when a host needs the full
Terminal Commander engine in its own process. Its typed methods call the same
dispatcher, policy, command runtime, probes, sifters, buckets, store, registry,
recipes, subscriptions and audit paths used by the daemon. No socket or network
listener is required. Bootstrap never discovers or attaches another daemon.

The [parity matrix](embedding/PARITY.md) lists every operation family, including
MCP conveniences. The [migration guide](embedding/MIGRATION-0.1.86.md) covers
older probe-based integrations. This is a supported, revision-pinned `0.x` API;
the source changes here are not evidence of a published package or release.

## Dependency

Terminal Commander is still a `0.x` project with a fast-moving public Rust API.
Pin every TC crate to the same tested commit:

```toml
[dependencies]
terminal-commanderd = { git = "https://github.com/special-place-ai-heaven/terminal-commander", rev = "<tested-commit>" }
```

`embedded::{protocol, core, probes, sifters}` re-exports the aligned public types.
Use those exports or pin every directly imported TC crate to the same revision.
The host supplies a Tokio runtime with time and process/I/O support. Do not mix
independently selected TC crate versions.

## Construct the engine without IPC

`EmbeddedEngine::bootstrap(config)` uses the supplied configuration and local
SQLite store. It retains the real host OS identity for policy and audit. Handles
can be cloned; all clones share one store, boot identity and admission gate.
Bootstrap claims the same data-directory lifetime lock as the daemon and rejects
a directory already owned by another engine. Shutdown releases the claim only
after closing the store. Use distinct directories for distinct room engines.

```rust
use terminal_commanderd::{DaemonConfig, embedded::EmbeddedEngine};

let config = DaemonConfig::defaults_in("/var/lib/my-app/tc");
let engine = EmbeddedEngine::bootstrap(config)?;
let discovery = engine.system_discover().await?;
let health = engine.health().await?;
assert_eq!(discovery.identity.as_ref(), Some(&health.identity));
let shutdown = engine.shutdown().await?;
assert!(shutdown.store_closed && shutdown.lifecycle_drained);
```

Start argv commands with `command_start_combed(CommandStartParams::new(argv))`;
read status, tails, buckets and event context through typed methods. For a bounded
start-and-observe operation use `run_and_watch(start, RunAndWatchOptions::default())`.
It starts once, retains job/bucket IDs and cursor when observation degrades, and
does not replay the command. The complete example at
[`crates/daemon/examples/embed_in_process.rs`](../crates/daemon/examples/embed_in_process.rs)
shows the complete minimal path:

```bash
cargo run --offline -p terminal-commanderd --example embed_in_process
cargo run --offline -p terminal-commanderd --example embed_isolated
```

## Policy, room authority and transport

The host selects `DaemonConfig`, including the policy profile, repository root,
execution capabilities and resource limits. Full engine access remains available
to trusted hosts; per-room policy controls what each room is authorized to do.
`RepoOnly` path policy and an explicit cwd do not sandbox arbitrary child code.
Filesystem mounts, namespaces/VMs, network rights, transport authentication,
room/session/generation fencing and credentials belong to the host.

`EmbeddedAuthority::PolicyOnly` is the default. A trusted host can explicitly call
`bootstrap_with_authority(config, EmbeddedAuthority::HostAdministrator)` to grant
owner/admin facilities to its actual OS identity, without impersonating the CLI.
This local Rust grant is deliberately not serializable. Never derive it from a
guest request. Recipe, credential and other method-specific validation still runs;
the grant does not grant a child process the host's authority.

Embedded credential requests default to host-managed owner interaction. When a PTY job
needs owner input, `credential_request` returns `OwnerActionRequired` with a typed
`owner_action` challenge identifying the engine, job and prompt generation. It
does not open a native prompt or advertise a CLI command to an unbound endpoint.
The trusted host obtains the owner's answer through its own private UI and calls
`credential_provide_challenge`; the shared owner gate, size limits, echo-off check
and generation check still apply. A challenge is correlation data, not authority.
Keep owner answers outside model context, logs and general room event channels.
Existing `credential_provide` remains available for current-prompt delivery;
challenge-bound completion is preferred when an owner interaction takes time.

For the existing native OS/helper prompt, call `bootstrap_with_options` with
`EmbeddedOptions { authority, owner_interaction: EmbeddedOwnerInteraction::NativePrompt }`.
`HostManaged` is the default. These local Rust options are not serializable.
When a native prompt is unavailable, the embedded response still carries the
typed host challenge and never a CLI command to an unbound endpoint.

`credential_url` also remains available with explicit host authority. That operation
intentionally opens its own temporary loopback HTTP listener for the owner page;
bootstrap itself creates no IPC listener. Abandoning that page returns to host
interaction (or the explicitly selected native prompt). Standalone daemon and
CLI behavior is unchanged.

For a service bridge use `execute(IpcRequest)` or
`execute_envelope(RequestEnvelope) -> ResponseEnvelope`. Both use the shared
dispatch and frame-size limits. Request/response types are Serde serializable;
the host must bound input bytes before deserializing, authenticate the transport,
and enforce its deadline and room authority envelope. The embedded engine accepts
no caller-selected peer identity. Correlation IDs identify responses, not durable
idempotency or replay rights. Never retry a non-idempotent operation merely because
the transport lost its reply.

An engine can run inside each container, process sandbox or VM and expose this
boundary over a host-owned channel, including a Unix socket or VM/vsock bridge.
TC does not select a virtualization technology, provision rooms, or silently route
commands to a remote daemon. Remote target management remains an adapter/host
transport feature. AAP's themed tools, skills, services and rights remain AAP
orchestration. SymForge supplies complementary source intelligence and edits;
TC supplies execution, observation and durable receipts. Neither library requires
the other.

The [AAP/SymForge handoff](embedding/AAP-SISTER-PROJECT-HANDOFF.md) records the
downstream integration boundary and the work intentionally left to those hosts.

## Explicit environment and working directory

Existing starts retain inherited child environments for compatibility. Use
`command_start_isolated(IsolatedCommand::new(command, cwd))` for an explicit
cleared environment. It uses the common command runtime, requires an absolute
executable and an existing absolute cwd, and bounds argv, env entries, rules and
total request bytes. The child receives the supplied env entries plus the reserved
`TC_DAEMON_CHILD=1` marker, which prevents login-shell TC autostart recursion.
Explicit values other than `1` for that marker are rejected. There is no ambient
PATH/PATHEXT executable resolution or WSLENV forwarding in this path. Request
Debug output redacts argv/env values.

The serialized operation is `IpcRequest::CommandStartIsolated`; the corresponding
low-level probe option is `EnvironmentMode::Clear`, which itself adds no marker.
Shell, PTY and persistent
session starts retain their established environment behavior. A host requiring
room-wide environmental isolation must establish it when launching the engine,
or use the cleared command lane for each execution. TC path policy does not
prevent child code from reading accessible host files or environment sources.

## Identity, observations and recovery

Health and discovery carry the same API/schema versions, engine version, immutable
build identity and boot `instance_id`. Build identity includes a source-labelled
FNV-1a-128 fingerprint of TC Rust sources, manifests and lockfile plus compiler,
target, profile and enabled features. It is a build discriminator, not a release
checksum or cryptographic attestation. Optional `TC_BUILD_ID` is explicit build
provenance. The package version alone is not a build identity.

Each bootstrap generates a new instance ID. Restoring a VM memory snapshot can
clone an existing ID and in-memory authority: bootstrap a new engine after restore
and let the host renew its room/session generation. An ID does not establish
authorization. Capability statuses distinguish available, denied by policy,
unsupported platform, required host permission and required host transport.
Individual calls can still fail their path, command or resource-specific checks.

Command status reports raw bytes as soon as pipes are read, including partial
lines, separators and bytes discarded by framing caps. `last_output_age_ms`
therefore reflects pipe activity rather than only emitted frames. Optional
`process_observation` distinguishes each stream's reading, complete, failed and
incomplete states; read errors are content-free metadata. `process_cleanup`
distinguishes running, complete, reaping and uncertain ownership cleanup. Exit
state alone does not establish complete stream observation. PTY/watch lanes and
historical receipts lacking these observations leave them absent.

Optional `cpu` observations carry stable probe/process identity, accounting scope,
the actual retained interval and Known/Unknown/Unavailable state. One logical CPU
is 100%; parallel work can exceed 100%. Windows owned Job Objects and Linux owned
cgroups can account for exited descendants. Linux process-group snapshots provide
positive lower bounds; zero snapshots remain unknown, because short-lived work
between samples can be missed. Missing or unknown observations never establish
idleness or authorize an inactivity kill.

Durable receipts distinguish `observed`, `reconstructed` and `abandoned` outcomes.
A recorded start without a trustworthy completion is `JobLost`, never a fabricated
success or failed exit. Old receipts can lack counters or observations. In-memory
raw tails are not durable. Bootstrap and status lookup never replay recorded work.

Subscription registrations and consumer offsets are boot-local, not durable.
New subscriptions begin from the current tails of in-scope buckets; after a
restart, reopen them using the new boot identity. Explicit seeks clamp to events
still retained by the bucket. These consumer cursors are separate from durable
job receipts and persisted workspace snapshots.

New command-start audit metadata includes the originating engine instance, API
version and build fingerprint. `JobLost` error details identify the job and the
current responding instance/build. These content-free correlation fields help
join a start to a later recovery lookup; they do not claim the current instance
started the job, establish why an older process died, or provide exactly-once
execution. Older audit rows may lack this metadata. Environment values are not
added to these diagnostics.

## Lifetime and compatibility

Call `shutdown().await` before dropping the host runtime. It closes admission,
waits for admitted calls, stops command/PTY/session/watch lanes, drains bounded
lifecycle tasks, records abandonment where needed and closes the store. Inspect
`ShutdownReport::lifecycle_drained`; a timeout is not a confirmed clean drain.
Repeated shutdown returns the same report. Dropping the last handle performs
best-effort cancellation and store closure but cannot await asynchronous drain.
OS process ownership cleanup is independent of the spawning Tokio runtime.

The older public `DaemonState`, runtime and probe interfaces remain available for
advanced hosts, but direct component calls are lower-level and may omit dispatcher
policy, audit or lifecycle behavior. The facade is the normal host entry point.
Unix-only shell sessions remain Unix-only; Windows PTY uses ConPTY. Pin a tested
revision and run both examples plus the host integration suite before upgrades.
