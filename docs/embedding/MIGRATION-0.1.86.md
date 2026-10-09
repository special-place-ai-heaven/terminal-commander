# Migrating an integration pinned to 0.1.86

Use the supported `terminal_commanderd::embedded::EmbeddedEngine` facade for new
host integrations. Older direct `ProcessProbe` orchestration does not automatically
include current daemon policy, registry/recipes, store receipts, audit, file lanes
or subscriptions. The facade includes those existing engine paths in process.

These changes are source in the current development workspace. The workspace
package version is not proof that a registry artifact containing them exists.
No publication, release checksum or release attestation is claimed. Pin the exact
reviewed commit when integrating, then update all TC dependencies together.

```toml
[dependencies]
terminal-commanderd = { git = "https://github.com/special-place-ai-heaven/terminal-commander", rev = "<reviewed-commit-containing-embedding>" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "process", "io-util"] }
```

Import the aligned `embedded::{protocol, core, probes, sifters}` re-exports. If
existing code imports TC crates directly, pin each to that same revision. Follow
the repository's `rust-toolchain.toml` and workspace Rust version requirement;
do not assume the toolchain used by a 0.1.86 integration still suffices. The
PolyForm Noncommercial license remains unchanged.

1. Give each independent engine an explicit data directory. Select the policy,
   repository root and capabilities through `DaemonConfig`; bootstrap does not
   load ambient daemon configuration or attach a running MCP daemon.
2. Replace ad hoc probe startup with typed `command_start_combed` or the shared
   `run_and_watch`. Keep returned job/bucket/probe IDs and cursors. Use status,
   tail, context and subscription methods from the same engine instance.
3. Use `command_start_isolated` when a cleared env is required. Supply an absolute
   executable, existing absolute cwd and explicit env entries. The daemon adds
   reserved `TC_DAEMON_CHILD=1`; conflicting caller values are rejected. Existing command,
   shell, PTY and session defaults still inherit their environment. Host-level
   isolation belongs to the room launcher.
4. Treat `process_observation`, `process_cleanup` and `cpu` as optional structured
   evidence. Raw bytes and activity timestamps count pipe reads rather than only
   complete decoded frames. `Unknown` CPU is not zero. Old receipts can lack the
   new fields, and in-memory tails do not survive a restart.
5. Preserve `observed`/`reconstructed`/`abandoned` and `JobLost` semantics.
   Missing completion is not success. A lost reply or restored room snapshot is
   not permission to re-execute a command automatically.
6. Call `shutdown().await` before stopping the Tokio runtime and inspect the
   report. Last-handle drop is best effort and cannot replace an awaited drain.

Embedded owner prompts now return a typed `owner_action` challenge, with no
daemon CLI fallback. A host with explicit owner authority obtains the answer in
its private owner UI and completes `credential_provide_challenge`; engine and
prompt generation are checked before delivery. Existing `credential_provide`
and temporary loopback `credential_url` remain available. Do not expose owner
answers to model context or ordinary event forwarding.

Existing public request structs retain their previous fields; new isolated
execution is a separate request type and low-level environment selection is an
additive spawn method. Responses have additive optional observation/identity
fields with Serde defaults for older messages. Rust consumers constructing
response literals may need to initialize those new fields; this is still a
revision-pinned `0.x` Rust API. The explicit API/schema versions identify the
current serialized engine contract, while build and instance IDs distinguish
the actual executable source and runtime lifetime.

For VM/container rooms, keep this typed payload inside the host's authenticated
room/session/generation envelope. The host owns transport credentials, rights,
filesystem/network isolation, snapshots and event forwarding. Select
`EmbeddedAuthority::HostAdministrator` only in trusted local bootstrap code when
the host intentionally grants owner methods; never deserialize that choice.
Bootstrap after restoring a memory snapshot to avoid cloning boot identity and
in-memory capabilities. This migration changes TC only; downstream AAP/SymForge
adapters should be updated after accepting this contract.

Run the concrete examples and then the downstream integration suite:

```text
cargo run --offline -p terminal-commanderd --example embed_in_process
cargo run --offline -p terminal-commanderd --example embed_isolated
cargo nextest run --offline -p terminal-commanderd --test embedded_engine
cargo nextest run --offline -p terminal-commander-ipc --test compound
```

See [the embedding guide](../EMBEDDING.md) and [full parity matrix](PARITY.md) for
the authority, recovery, platform and lifetime contracts.
