# terminal-commander-ipc

Part of the [Terminal Commander](https://github.com/special-place-ai-heaven/terminal-commander) project.

This crate is published to crates.io to support the public-API distribution path.

It carries the shared IPC wire protocol (request/response envelopes and the
closed `IpcRequest` / `IpcResponse` / `IpcError` set), the length-prefixed JSON
framing helpers, and the cross-platform `DaemonClient` transport: a Unix domain
socket client on Unix and a Windows named-pipe client on Windows.
