# In-process embedding

Embed the `terminal-commanderd` library when a host application needs Terminal
Commander's engine in its own process. This includes policy, commands, sifters,
probes, buckets, context rings, subscriptions, file watches, PTY support,
registry/store, audit, liveness state, and environment discovery. The MCP and
CLI crates are delivery surfaces and are not required.

## Dependency

Terminal Commander is still a `0.x` project with a fast-moving public Rust API.
Pin every TC crate to the same tested commit:

```toml
[dependencies]
terminal-commanderd = { git = "https://github.com/special-place-ai-heaven/terminal-commander", rev = "<tested-commit>" }
```

`terminal-commanderd` pulls its engine crates transitively, but Rust does not
make transitive dependencies directly importable. Applications using
`DaemonState` need only the dependency above. Applications importing
`ProcessProbe`, `SifterRuntime`, or core types directly must also declare the
corresponding `terminal-commander-probes`, `terminal-commander-sifters`, or
`terminal-commander-core` git dependency at the same revision.

## Construct the engine without IPC

`DaemonState::bootstrap(config)` constructs the complete engine and its local
SQLite store actor. It does not bind a socket, start the daemon accept loop, or
open a network listener. IPC begins only if the host separately calls an IPC
runtime such as `run_ipc_server`/`IpcServer`/`PipeServer`.

```rust
use terminal_commanderd::{DaemonConfig, DaemonState};

let config = DaemonConfig::defaults_in("/var/lib/my-app/tc");
let state = DaemonState::bootstrap(config)?;

// Shared with daemon system_discover: raw probes plus active-cap filtering.
let environment = state.discover_environment();

// Public engine components are available directly.
let router = &state.router;
let commands = &state.command;
let watches = &state.watch;
let subscriptions = &state.subscriptions;
```

Start argv commands with `state.command.start_combed(...)`; read bounded status
with `state.command.status(job_id)` and signals through the router/bucket APIs.
The compiling example at
[`crates/daemon/examples/embed_in_process.rs`](../crates/daemon/examples/embed_in_process.rs)
shows the complete minimal path:

```bash
cargo check -p terminal-commanderd --example embed_in_process
```

## Contract boundaries

- Keep the normal `DaemonConfig` policy and capability gates. Embedding does not
  bypass shell, path, command, PTY, or OS restrictions.
- Use `DaemonState::discover_environment`, not raw
  `discover_host_environment`, when selecting an execution route. The state
  method applies the same shell-capability filtering as daemon IPC.
- Unix-only facilities remain Unix-only. Windows uses ConPTY for PTY commands;
  persistent shell sessions remain Unix-only.
- Do not call the IPC runtime unless the host intentionally wants a local
  cross-process control surface.
- Until a semver-stable embed facade is declared, treat public Rust types as a
  revision-pinned contract and run the example plus the host application's
  integration suite before updating the pin.
