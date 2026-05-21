# Goal Chain Index - terminal-commander-runtime

Target branch: `main`

| Goal | Title | Status | Depends on | Branch | Intended outcome |
|---|---|---|---|---|---|
| TC33 | Code Reality Audit And Runtime Pivot | Pending | [] | main | Produce an evidence-backed audit of the current `main` branch and pivot the next runtime goals toward the actual realtime signal-channel product state. |
| TC34 | Realtime Signal Channel Contract | Pending | TC33 | main | Codify the realtime signal-channel contract and MCP tool-control surface so every later implementation goal optimizes capability without noise. |
| TC35 | Persistent Audit Log V0003 | Pending | TC34 | main | Replace the in-memory audit placeholder seam with durable SQLite-backed audit records for policy-relevant runtime actions. |
| TC36 | Daemon Runtime Bootstrap And Config | Pending | TC35 | main | Make `terminal-commanderd` initialize a real daemon runtime state from explicit config instead of exiting as scaffold-only. |
| TC37 | Daemon UDS IPC And Peer Identity | Pending | TC36 | main | Implement the local Unix-domain-socket IPC boundary between the MCP adapter/CLI and daemon runtime, with peer-identity checks and bounded messages. |
| TC38 | Command Start Combed Process Wiring | Pending | TC37 | main | Wire `command_start_combed` into the daemon runtime so a policy-approved argv command starts a process probe and emits sifter-generated signal into a bucket. |
| TC39 | Bucket Wait And Event Context Daemon API | Pending | TC38 | main | Make the daemon API expose realtime bucket waits and event-context lookup by event ID so the LLM receives live signal and bounded context without knowing raw probe internals. |
| TC40 | Rmcp Stdio Adapter And Tool Discovery | Pending | TC39 | main | Implement the real rmcp stdio adapter so MCP clients can attach to Terminal Commander instead of only using in-process ToolSurface tests. |
| TC41 | Mcp Command And Bucket Tools | Pending | TC40 | main | Expose the command and bucket realtime control surface through MCP so an LLM can start work and wait for signal without terminal toil. |
| TC42 | Registry Hot Activation And Rule Binding | Pending | TC41 | main | Make registry rule selection/creation/testing/activation affect live probe runtimes by unique rule IDs, not only the persistent registry database. |
| TC43 | File Probe Search Watch And Bounded Read | Pending | TC42 | main | Expose bounded file intelligence through probes/tools so the LLM can ask for file lists, targeted search, line windows, and watched file changes without reading whole files. |
| TC44 | Posix Pty Spawn And Stdin Control | Pending | TC43 | main | Add POSIX/WSL PTY spawn and controlled stdin so interactive commands can be observed and steered while still emitting only filtered signal to the LLM. |
| TC45 | Parallel Probe Router And Multi Bucket Bindings | Pending | TC44 | main | Turn the runtime into a real filter/proxy/router by supporting multiple concurrent probes, multiple buckets, and dynamic routing/binding of rules to sources. |
| TC46 | Provider Harness Integration Smoke | Pending | TC45 | main | Prove a provider-neutral MCP harness can use Terminal Commander as the abstraction layer for command execution, file probing, bucket_wait, and bounded context. |
| TC47 | Load Noise And Backpressure Gate | Pending | TC46 | main | Validate that the runtime can comb megabyte-scale noisy terminal/file streams while preserving realtime signal, bounded memory, and explicit backpressure behavior. |
| TC48 | Beta Gate Evidence Review And Backlog Rerank | Pending | TC47 | main | Review the runtime chain evidence, correct source-status drift, rerank the backlog, and decide whether Terminal Commander is beta-ready as a realtime MCP signal abstraction layer. |
