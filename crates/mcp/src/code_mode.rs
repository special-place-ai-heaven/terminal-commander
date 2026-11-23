// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! `TC_SURFACE=code` MCP Code Mode adapter types, `AsyncIpcCaller` bridge,
//! server instructions, and 0-arg `status` helper.

use std::borrow::Cow;
use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use terminal_commander_codemode::backend::BoxFuture;
use terminal_commander_codemode::{
    AsyncBridgeBackend, AsyncIpcCaller, CodeBackendError, ExecuteRequest, TcCodeLimits,
    execute_script_async, render_stubs,
};
use terminal_commanderd::ipc::protocol::{
    IpcRequest, IpcResponse, ListLimitParams, PolicyStatusResponse, RuntimeStateResponse,
};

use crate::target_router::TargetRouteError;
use crate::tools::TerminalCommanderMcpServer;

/// Description for `code_search` on `TC_SURFACE=code` and the router.
pub const CODE_SEARCH_DESCRIPTION: &str = "Search the 60 Terminal Commander Code Mode Python functions and return exact keyword-only TypedDict signatures (no daemon needed). Omit query to list all 60 one-line signatures; pass a domain or keyword (e.g. 'run_and_watch', 'bucket', 'registry', 'file', 'pty') for full TypedDict stubs.";

/// Description for `code_execute` on `TC_SURFACE=code` and the router.
pub const CODE_EXECUTE_DESCRIPTION: &str = "Execute a Python 3.14-subset script in a pure-Rust zero-egress sandbox with all 60 Terminal Commander operations pre-bound as keyword-only functions (e.g. run_and_watch(argv=[...]), bucket_events_since(...), event_context(...), command_output_tail(...)). Intermediate outputs stay in Rust RAM; the final expression is returned as JSON.";

/// Description for the 0-arg `status` tool on `TC_SURFACE=code`.
pub const CODE_STATUS_DESCRIPTION: &str = "Daemon liveness, version, policy caps, and bounded runtime counts (jobs, PTYs, file watches, buckets, active rules). Takes no arguments.";

/// Server-level `initialize` instructions returned when `TC_SURFACE=code` is active.
/// Kept strictly `<= 850` chars so MCP clients that prepend instructions stay compact.
pub const CODE_SURFACE_INSTRUCTIONS: &str = "Terminal Commander Code Mode (`TC_SURFACE=code`) exposes all 60 STRUCTURED-SIGNAL operations through 3 tools to save context tokens. \
1) `code_search(query=...)` inspects exact Python `TypedDict` stubs (omit `query` to list all 60 signatures; no daemon required). \
2) `code_execute(code=...)` runs a stateless Python 3.14-subset script where all 60 TC tools are pre-bound globals taking KEYWORD arguments ONLY (`fn(k=v)`). End your script with the dict/list you want returned as JSON (`print` goes to `stdout`). Compose multi-step flows in one turn: `run_and_watch(argv=[...])` -> `bucket_events_since` / `bucket_wait` -> `event_context` / `command_output_tail` (quiet commands return a bounded receipt). Use `shell_exec` only for pipelines/redirects. \
3) `status()` checks daemon health, policy caps, and active runtime counts.";

/// Input schema for `code_search`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CodeSearchInput {
    /// Optional keyword or function-name query; omit or pass empty string to list all 60 one-line signatures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Maximum full `TypedDict` stubs to render when `query` is non-empty (default 5, max 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Input schema for `code_execute`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CodeExecuteInput {
    /// Python 3.14-subset script calling keyword-only TC functions; final expression is returned as JSON.
    pub code: String,
    /// Optional wall-clock execution budget in ms (default 120000, max 300000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Optional mutating-call cap override (default 8, max 32).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_mutations: Option<usize>,
    /// Optional default remote target_id from targets.toml (omit for local daemon).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// [`AsyncIpcCaller`] adapter routing Code Mode host calls through [`TerminalCommanderMcpServer`].
///
/// Preserves daemon reachability, self-heal, version-skew checks, `allow_remote`
/// policy gating, and `TargetRouter` local-socket forwarding.
#[derive(Debug, Clone)]
pub struct McpCodeCaller {
    server: TerminalCommanderMcpServer,
    default_target: Option<String>,
}

impl McpCodeCaller {
    /// Wrap a [`TerminalCommanderMcpServer`] with an optional default remote `target_id`.
    #[must_use]
    pub const fn new(server: TerminalCommanderMcpServer, default_target: Option<String>) -> Self {
        Self {
            server,
            default_target,
        }
    }
}

fn map_mcp_guard_error(err: McpError) -> CodeBackendError {
    let kind = err
        .data
        .as_ref()
        .and_then(|d| d.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("daemon_unavailable")
        .to_owned();
    CodeBackendError {
        kind,
        message: err.message.into_owned(),
        data: err.data,
    }
}

impl AsyncIpcCaller for McpCodeCaller {
    fn call_ipc<'a>(
        &'a self,
        request: IpcRequest,
        target_id: Option<&'a str>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<IpcResponse, CodeBackendError>> {
        Box::pin(async move {
            let effective_target = target_id.or(self.default_target.as_deref());
            if effective_target.is_some() {
                let daemon = self
                    .server
                    .daemon_for_target(effective_target)
                    .await
                    .map_err(map_mcp_guard_error)?;
                return daemon
                    .call_with_timeout(request, timeout)
                    .await
                    .map_err(|e| CodeBackendError::from_ipc_error(&e));
            }

            match &request {
                IpcRequest::Health | IpcRequest::SystemDiscover => {
                    self.server
                        .ensure_daemon_reachable()
                        .await
                        .map_err(map_mcp_guard_error)?;
                }
                _ => {
                    self.server
                        .ensure_daemon_available()
                        .await
                        .map_err(map_mcp_guard_error)?;
                }
            }

            self.server
                .daemon_client()
                .call_with_timeout(request, timeout)
                .await
                .map_err(|e| CodeBackendError::from_ipc_error(&e))
        })
    }

    fn list_targets(&self) -> BoxFuture<'_, Result<Value, CodeBackendError>> {
        Box::pin(async move {
            let mut targets = Vec::new();
            for t in self.server.target_router().targets() {
                let reachable = self.server.probe_target(t).await.is_some();
                targets.push(json!({
                    "target_id": t.target_id,
                    "host": t.host,
                    "transport": t.transport,
                    "local_forward_socket": t.local_forward_socket.to_string_lossy(),
                    "reachable": reachable,
                }));
            }
            Ok(json!({ "targets": targets }))
        })
    }

    fn probe_target<'a>(
        &'a self,
        target_id: &'a str,
    ) -> BoxFuture<'a, Result<Value, CodeBackendError>> {
        Box::pin(async move {
            self.server
                .ensure_remote_allowed(target_id)
                .await
                .map_err(map_mcp_guard_error)?;
            let target = self
                .server
                .target_router()
                .target(target_id)
                .ok_or_else(|| {
                    map_mcp_guard_error(crate::tools::route_error_to_mcp(
                        &TargetRouteError::UnknownTarget(target_id.to_owned()),
                    ))
                })?;
            let daemon_version = self.server.probe_target(target).await;
            Ok(json!({
                "target_id": target.target_id,
                "reachable": daemon_version.is_some(),
                "daemon_version": daemon_version,
            }))
        })
    }
}

/// Offline `code_search` execution. Does not require a running daemon.
#[must_use]
pub fn run_code_search(input: &CodeSearchInput) -> CallToolResult {
    let limit = input.limit.map(|n| n as usize);
    let rendered = render_stubs(input.query.as_deref(), limit);
    CallToolResult::success(vec![ContentBlock::text(rendered)])
}

/// Execute a stateless Python 3.14-subset script against `server`.
pub async fn run_code_execute(
    server: &TerminalCommanderMcpServer,
    input: CodeExecuteInput,
) -> Result<CallToolResult, McpError> {
    let mut limits = TcCodeLimits::default();
    if let Some(timeout_ms) = input.timeout_ms {
        limits.max_wall_duration = Duration::from_millis(timeout_ms);
    }
    if let Some(max_mutations) = input.max_mutations {
        limits.max_mutation_calls = max_mutations;
    }
    let request = ExecuteRequest {
        code: input.code,
        limits: limits.clamped(),
        spill_dir: None,
    };
    let caller = McpCodeCaller::new(server.clone(), input.target);
    let backend = AsyncBridgeBackend::new(tokio::runtime::Handle::current(), caller);
    let outcome = execute_script_async(request, backend)
        .await
        .map_err(|err| McpError::internal_error(Cow::Owned(err), None))?;
    crate::tools::json_tool_result(&outcome)
}

/// Compact 0-arg `status` payload served when `TC_SURFACE=code` calls `"status"`.
pub async fn code_surface_status(
    server: &TerminalCommanderMcpServer,
) -> Result<CallToolResult, McpError> {
    server.ensure_daemon_reachable().await?;
    let daemon = server.daemon_client();

    let (daemon_version, uptime_secs) = match daemon.call(IpcRequest::Health).await {
        Ok(IpcResponse::Health {
            uptime_secs,
            version,
            ..
        }) => (version, uptime_secs),
        Ok(other) => return Err(crate::tools::unexpected_variant(&other)),
        Err(e) => return Err(crate::tools::into_mcp_error(&e)),
    };

    let (policy_profile, caps) = match daemon.call(IpcRequest::PolicyStatus).await {
        Ok(IpcResponse::PolicyStatus(PolicyStatusResponse { profile, caps, .. })) => (
            profile,
            json!({
                "allow_shell": caps.allow_shell,
                "allow_session": caps.allow_session,
                "allow_remote": caps.allow_remote,
            }),
        ),
        Ok(other) => return Err(crate::tools::unexpected_variant(&other)),
        Err(e) => return Err(crate::tools::into_mcp_error(&e)),
    };

    let counts = match daemon
        .call(IpcRequest::RuntimeState(ListLimitParams { limit: Some(0) }))
        .await
    {
        Ok(IpcResponse::RuntimeState(RuntimeStateResponse {
            command_jobs,
            pty_jobs,
            file_watches,
            bucket_count,
            active_rules_count,
            ..
        })) => json!({
            "command_jobs": command_jobs,
            "pty_jobs": pty_jobs,
            "file_watches": file_watches,
            "buckets": bucket_count,
            "active_rules": active_rules_count,
        }),
        Ok(other) => return Err(crate::tools::unexpected_variant(&other)),
        Err(e) => return Err(crate::tools::into_mcp_error(&e)),
    };

    crate::tools::json_tool_result(&json!({
        "ok": true,
        "adapter_version": crate::tools::ADAPTER_VERSION,
        "surface": "code",
        "daemon_version": daemon_version,
        "uptime_secs": uptime_secs,
        "policy_profile": policy_profile,
        "caps": caps,
        "counts": counts,
    }))
}
