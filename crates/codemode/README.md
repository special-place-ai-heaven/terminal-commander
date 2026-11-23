# terminal-commander-codemode

Part of the [Terminal Commander](https://github.com/special-place-ai-heaven/terminal-commander) project.

Pure-Rust Python 3.14-subset Code Mode sandbox (`monty` + `monty-types`) exposing all 60 granular Terminal Commander operations as typed, keyword-only functions over `terminal_commander_ipc` or in-process `EmbeddedEngine`.

See the [main repository README](https://github.com/special-place-ai-heaven/terminal-commander#readme) for project overview, install instructions, and design docs.

## In-Process `EmbeddedEngine` Adapter

Host applications embedding `terminal_commanderd::embedded::EmbeddedEngine` can expose the 60-operation Code Mode sandbox with zero IPC sockets by implementing `AsyncIpcCaller`:

```rust
use std::sync::Arc;
use std::time::Duration;
use terminal_commander_codemode::backend::BoxFuture;
use terminal_commander_codemode::{
    AsyncBridgeBackend, AsyncIpcCaller, CodeBackendError, ExecuteRequest, execute_script_async,
};
use terminal_commanderd::embedded::EmbeddedEngine;
use terminal_commanderd::{IpcRequest, IpcResponse};

#[derive(Clone)]
pub struct EmbeddedCodeCaller(pub Arc<EmbeddedEngine>);

impl AsyncIpcCaller for EmbeddedCodeCaller {
    fn call_ipc<'a>(
        &'a self,
        request: IpcRequest,
        target_id: Option<&'a str>,
        _timeout: Duration,
    ) -> BoxFuture<'a, Result<IpcResponse, CodeBackendError>> {
        Box::pin(async move {
            if let Some(id) = target_id {
                return Err(CodeBackendError::refusal(
                    "unknown_target",
                    format!("unknown target_id '{id}' (embedded engine is local-only)"),
                ));
            }
            self.0
                .execute(request)
                .await
                .map_err(|err| CodeBackendError::from_ipc_error(&err))
        })
    }
}
```

## License

PolyForm-Noncommercial-1.0.0
