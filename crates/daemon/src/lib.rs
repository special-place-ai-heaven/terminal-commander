// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commanderd` library entry point. Exposes the local API
//! router (TC21), the policy engine (TC22), the persistent audit
//! sink (TC35), the daemon runtime bootstrap (TC36), and the local
//! UDS IPC transport (TC37) so the daemon binary, future MCP
//! adapter, admin CLI, and tests can build against the same
//! in-process / cross-process API.
//!
//! Source-status:
//! - Router + Policy + Audit: live.
//! - Daemon runtime bootstrap: live (TC36).
//! - Local IPC transport: live (TC37) — UDS on Unix, named pipe on Windows.
//!   Optional environment runners (WSL, SSH) route probe ops from the parent.

pub mod activation;
pub mod audit;
pub mod command;
pub mod config;
pub mod environment;
pub mod file_watch;
pub mod ipc;
pub mod policy;
#[cfg(unix)]
pub mod pty_command;
pub mod router;
pub mod runtime;
pub mod state;

pub use activation::{ActivationRegistry, ActivationRegistryHandle};
pub use audit::{AuditSink, InMemoryAudit, PersistentAudit};
pub use command::{
    CommandError, CommandRuntime, CommandStartRequest, CommandStartResponse, CommandStatusResponse,
    DEFAULT_COMMAND_SEVERITY_MIN, MAX_ARGV_ITEM_BYTES, MAX_ARGV_ITEMS, SHELL_INTERPRETERS_DENY,
};
pub use config::{
    ConfigError, DaemonConfig, DaemonSection, LimitsSection, PolicySection, RetentionSection,
    RuntimeMode,
};
pub use file_watch::{LiveWatchIdentity, WatchError, WatchRebindReport, WatchRuntime};
pub use ipc::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CommandOutputTailParams, CommandOutputTailResponse,
    CommandStartParams, CommandStatusParams, ContextUnavailableReason, DEFAULT_BUCKET_READ_LIMIT,
    DEFAULT_BUCKET_WAIT_MS, DEFAULT_CONTEXT_AFTER, DEFAULT_CONTEXT_BEFORE, DEFAULT_FILE_READ_BYTES,
    DEFAULT_FILE_READ_LINES, DEFAULT_FILE_SEARCH_MATCHES, DEFAULT_FILE_SEARCH_SNIPPET_BYTES,
    DEFAULT_REGISTRY_SEARCH_LIMIT, DiscoverResponse, EventContextParams, EventContextResponse,
    FileLine, FileReadWindowParams, FileReadWindowResponse, FileSearchMatch, FileSearchParams,
    FileSearchResponse, FileWatchListEntry, FileWatchListResponse, FileWatchStartParams,
    FileWatchStartResponse, FileWatchStopParams, FileWatchStopResponse, IpcContextFrame, IpcError,
    IpcErrorCode, IpcRequest, IpcResponse, IpcResult, MAX_BUCKET_READ_LIMIT, MAX_BUCKET_WAIT_MS,
    MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_GRACE_MS, MAX_COMMAND_INLINE_RULES, MAX_CONTEXT_BYTES,
    MAX_CONTEXT_FRAMES, MAX_FILE_READ_BYTES, MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES,
    MAX_FILE_SEARCH_SCAN_BYTES, MAX_FILE_SEARCH_SNIPPET_BYTES, MAX_FRAME_BYTES, MAX_PTY_ARGV_ITEMS,
    MAX_PTY_STDIN_BYTES, MAX_REGISTRY_SEARCH_LIMIT, MAX_REGISTRY_TEST_SAMPLE_BYTES,
    MAX_REGISTRY_TEST_SAMPLES, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, MAX_TAIL_BYTES,
    MAX_TAIL_LINES, PolicyStatusResponse, ProbeKind, ProbeListEntry, ProbeListResponse,
    ProbeStatusParams, ProbeStatusResponse, PtyCommandListEntry, PtyCommandListResponse,
    PtyCommandStartParams, PtyCommandStartResponse, PtyCommandStopParams, PtyCommandStopResponse,
    PtyCommandWriteStdinParams, PtyCommandWriteStdinResponse, RegistryActivateParams,
    RegistryActivateResponse, RegistryActiveEntry, RegistryDeactivateParams,
    RegistryDeactivateResponse, RegistryGetParams, RegistryGetResponse, RegistryImportPackParams,
    RegistryImportPackResponse, RegistryListActiveResponse, RegistrySearchHit,
    RegistrySearchParams, RegistrySearchResponse, RegistryTestMatch, RegistryTestParams,
    RegistryTestResponse, RegistryTestSample, RegistryUpsertParams, RegistryUpsertResponse,
    RequestEnvelope, ResponseEnvelope, RuntimeActiveRule, RuntimeBucketSummary,
    RuntimeStateResponse, SelfCheckResponse, SeverityHistogram,
};
#[cfg(unix)]
pub use ipc::{DaemonClient, IpcServer, PeerCred, ServerHandle};
#[cfg(windows)]
pub use ipc::{DaemonClient, PeerCred, PipeServer, PipeServerHandle};
pub use policy::{
    COMMANDS_DENY, DEFAULT_DENY_PATH_SUFFIXES, PolicyAction, PolicyDecision, PolicyEngine,
    PolicyProfile, PolicyVerdict,
};
#[cfg(unix)]
pub use pty_command::{
    LivePtyIdentity, PtyRebindReport, PtyRuntime, PtyRuntimeError, PtyStartRequest,
    PtyStartResponse, PtyWriteResponse,
};
pub use router::Router;
pub use runtime::{
    RuntimeError, SelfCheckReport, run_foreground_idle, run_ipc_server, run_self_check,
};
pub use state::{BootstrapError, DaemonState};
