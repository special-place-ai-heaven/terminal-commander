// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Local IPC for terminal-commanderd (TC37).
//!
//! Unix domain socket transport. NO network listener. NO command
//! execution. NO raw stream lane. Every request is bounded by frame
//! size, every response is bounded, every accepted request is
//! audited through the TC35 PersistentAudit sink.
//!
//! Platform support:
//! - Linux / WSL2 / macOS / BSD: live via tokio `UnixListener` +
//!   SO_PEERCRED (Linux) for peer uid/gid/pid.
//! - Windows native: NOT SUPPORTED. The IPC modules compile (so the
//!   workspace builds), but `Server::bind` returns
//!   `IpcError::UnsupportedPlatform`. WSL2 is the Windows story.

// The wire protocol, framing, and the cross-platform `DaemonClient`
// transport moved to the `terminal-commander-ipc` crate (Phase P1).
// They are re-exported here as modules so every existing call site
// (`crate::ipc::protocol::*`, `crate::ipc::framing::*`,
// `crate::ipc::DaemonClient`) keeps resolving unchanged. The daemon
// keeps the SERVER side (dispatch, peer identity, listeners) local.
pub use terminal_commander_ipc::framing;
pub use terminal_commander_ipc::protocol;

pub mod peer;

pub mod server;

#[cfg(windows)]
pub mod pipe_server;

#[cfg(windows)]
pub mod peer_windows;

#[cfg(windows)]
pub mod pipe_acl;

pub use protocol::{
    AuditRowWire, AuditSinceParams, AuditSinceResponse, BucketEventsSinceParams,
    BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse, BucketWaitParams,
    BucketWaitResponse, BulkDeactivateOutcome, BulkOutcomeKind, CommandOutputTailParams,
    CommandOutputTailResponse, CommandStartParams, CommandStartResponse, CommandStatusParams,
    CommandStatusResponse, CommandStopParams, CommandStopResponse, ContextUnavailableReason,
    DEFAULT_BUCKET_READ_LIMIT, DEFAULT_BUCKET_WAIT_MS, DEFAULT_CONTEXT_AFTER,
    DEFAULT_CONTEXT_BEFORE, DEFAULT_FILE_READ_BYTES, DEFAULT_FILE_READ_LINES,
    DEFAULT_FILE_SEARCH_MATCHES, DEFAULT_FILE_SEARCH_SNIPPET_BYTES, DEFAULT_PULL_TIMEOUT_MS,
    DEFAULT_REGISTRY_SEARCH_LIMIT, DiscoverResponse, EventContextParams, EventContextResponse,
    FileLine, FileReadWindowParams, FileReadWindowResponse, FileSearchMatch, FileSearchParams,
    FileSearchResponse, FileWatchListEntry, FileWatchListResponse, FileWatchStartParams,
    FileWatchStartResponse, FileWatchStopParams, FileWatchStopResponse, FileWriteParams,
    FileWriteResponse, IpcContextFrame, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult,
    ListLimitParams, Liveness, MAX_BUCKET_READ_LIMIT, MAX_BUCKET_WAIT_MS,
    MAX_BUCKETS_PER_SUBSCRIPTION, MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_GRACE_MS,
    MAX_COMMAND_INLINE_RULES, MAX_CONTEXT_BYTES, MAX_CONTEXT_FRAMES, MAX_FILE_READ_BYTES,
    MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES, MAX_FILE_SEARCH_SCAN_BYTES,
    MAX_FILE_SEARCH_SNIPPET_BYTES, MAX_FILE_WRITE_BYTES, MAX_FRAME_BYTES, MAX_LIST_LIMIT,
    MAX_PTY_ARGV_ITEMS, MAX_PTY_STDIN_BYTES, MAX_PULL_EVENTS, MAX_PULL_TIMEOUT_MS,
    MAX_REGISTRY_SEARCH_LIMIT, MAX_REGISTRY_TEST_SAMPLE_BYTES, MAX_REGISTRY_TEST_SAMPLES,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, MAX_SESSION_ENV_ITEMS, MAX_SESSION_LINE_BYTES,
    MAX_SUBSCRIPTIONS, MAX_TAIL_BYTES, MAX_TAIL_LINES, PolicyStatusResponse, ProbeKind,
    ProbeListEntry, ProbeListResponse, ProbeStatusParams, ProbeStatusResponse, PtyCommandListEntry,
    PtyCommandListResponse, PtyCommandStartParams, PtyCommandStartResponse, PtyCommandStopParams,
    PtyCommandStopResponse, PtyCommandWriteStdinParams, PtyCommandWriteStdinResponse,
    RegistryActivateParams, RegistryActivateResponse, RegistryActiveEntry,
    RegistryDeactivateBulkParams, RegistryDeactivateBulkResponse, RegistryDeactivateParams,
    RegistryDeactivateResponse, RegistryGetParams, RegistryGetResponse, RegistryImportFailure,
    RegistryImportPackParams, RegistryImportPackResponse, RegistryListActiveResponse,
    RegistrySearchHit, RegistrySearchParams, RegistrySearchResponse, RegistryTestMatch,
    RegistryTestParams, RegistryTestResponse, RegistryTestSample, RegistryUpsertParams,
    RegistryUpsertResponse, RequestEnvelope, ResponseEnvelope, RuntimeActiveRule,
    RuntimeBucketSummary, RuntimeStateResponse, SelfCheckResponse, SessionState, SeverityHistogram,
    ShellExecParams, ShellSessionExecParams, ShellSessionExecResponse, ShellSessionListEntry,
    ShellSessionListResponse, ShellSessionStartParams, ShellSessionStartResponse,
    ShellSessionStatusParams, ShellSessionStatusResponse, ShellSessionStopParams,
    ShellSessionStopResponse, SourceLiveness, SubscriptionCloseParams, SubscriptionCloseResponse,
    SubscriptionEvent, SubscriptionListParams, SubscriptionListResponse, SubscriptionOpenParams,
    SubscriptionOpenResponse, SubscriptionPredicate, SubscriptionPullParams,
    SubscriptionPullResponse, SubscriptionSeekParams, SubscriptionSeekResponse,
    SubscriptionSourceSel, SubscriptionSummary, WorkspaceSnapshotApplyParams,
    WorkspaceSnapshotApplyResponse, WorkspaceSnapshotCreateParams, WorkspaceSnapshotCreateResponse,
};

// `CommandStartResponse` / `CommandStatusResponse` now live in
// `terminal_commander_ipc::protocol` and are re-exported at the crate
// root via `pub use command::{...}` (command.rs re-imports them from
// the ipc crate). We deliberately leave them out of the flat list
// above to avoid an E0252 clash with that crate-root re-export.

pub use peer::PeerCred;

pub use server::dispatch_envelope;

#[cfg(unix)]
pub use server::{IpcServer, ServerHandle};

#[cfg(windows)]
pub use pipe_server::{PipeServer, PipeServerHandle};

// Cross-platform client transport: UDS on Unix, named pipe on Windows.
// `DaemonClient` is the platform-dispatched alias resolved inside the
// `terminal-commander-ipc` crate.
pub use terminal_commander_ipc::DaemonClient;
