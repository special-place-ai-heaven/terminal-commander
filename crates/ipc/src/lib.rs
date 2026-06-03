// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Terminal Commander shared IPC crate (TC37).
//!
//! Houses the wire protocol, the length-prefixed JSON framing, and the
//! cross-platform `DaemonClient` transport that every IPC client (MCP
//! adapter, admin CLI) and the daemon server share. Splitting these out
//! of the daemon keeps the dependency arrow pointing at a small,
//! protocol-only crate: `core <- ipc <- {daemon, mcp, cli}`.
//!
//! Platform support for the client transport:
//! - Linux / WSL2 / macOS / BSD: live via tokio `UnixStream` (see
//!   [`client`]).
//! - Windows native: live via tokio named pipe `ClientOptions` (see
//!   [`pipe_client`]).
//!
//! The server side (dispatch, peer identity, `IpcServer` / `PipeServer`)
//! stays in the daemon crate; this crate is wire types + framing +
//! client transport only.
//!
//! Source-status: live (TC37).

pub mod protocol;

pub mod framing;

#[cfg(unix)]
pub mod client;

#[cfg(windows)]
pub mod pipe_client;

pub use protocol::{
    AuditRowWire, AuditSinceParams, AuditSinceResponse, BucketEventsSinceParams,
    BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse, BucketWaitParams,
    BucketWaitResponse, CommandOutputTailParams, CommandOutputTailResponse, CommandReceipt,
    CommandStartParams, CommandStartResponse, CommandStatusParams, CommandStatusResponse,
    ContextUnavailableReason, DEFAULT_AUDIT_READ_LIMIT, DEFAULT_BUCKET_READ_LIMIT,
    DEFAULT_BUCKET_WAIT_MS, DEFAULT_CONTEXT_AFTER, DEFAULT_CONTEXT_BEFORE, DEFAULT_FILE_READ_BYTES,
    DEFAULT_FILE_READ_LINES, DEFAULT_FILE_SEARCH_MATCHES, DEFAULT_FILE_SEARCH_SNIPPET_BYTES,
    DEFAULT_PULL_TIMEOUT_MS, DEFAULT_REGISTRY_SEARCH_LIMIT, DiscoverResponse, EventContextParams,
    EventContextResponse, FileLine, FileReadWindowParams, FileReadWindowResponse, FileSearchMatch,
    FileSearchParams, FileSearchResponse, FileWatchListEntry, FileWatchListResponse,
    FileWatchStartParams, FileWatchStartResponse, FileWatchStopParams, FileWatchStopResponse,
    IpcContextFrame, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, ListLimitParams,
    Liveness, MAX_AUDIT_READ_LIMIT, MAX_BUCKET_READ_LIMIT, MAX_BUCKET_WAIT_MS,
    MAX_BUCKETS_PER_SUBSCRIPTION, MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_GRACE_MS,
    MAX_COMMAND_INLINE_RULES, MAX_CONTEXT_BYTES, MAX_CONTEXT_FRAMES, MAX_FILE_READ_BYTES,
    MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES, MAX_FILE_SEARCH_SCAN_BYTES,
    MAX_FILE_SEARCH_SNIPPET_BYTES, MAX_FRAME_BYTES, MAX_LIST_LIMIT, MAX_PTY_ARGV_ITEMS,
    MAX_PTY_STDIN_BYTES, MAX_PULL_EVENTS, MAX_PULL_TIMEOUT_MS, MAX_REGISTRY_SEARCH_LIMIT,
    MAX_REGISTRY_TEST_SAMPLE_BYTES, MAX_REGISTRY_TEST_SAMPLES, MAX_REQUEST_BYTES,
    MAX_RESPONSE_BYTES, MAX_SUBSCRIPTIONS, MAX_TAIL_BYTES, MAX_TAIL_LINES, PolicyStatusResponse,
    ProbeKind, ProbeListEntry, ProbeListResponse, ProbeStatusParams, ProbeStatusResponse,
    PtyCommandListEntry, PtyCommandListResponse, PtyCommandStartParams, PtyCommandStartResponse,
    PtyCommandStopParams, PtyCommandStopResponse, PtyCommandWriteStdinParams,
    PtyCommandWriteStdinResponse, RegistryActivateParams, RegistryActivateResponse,
    RegistryActiveEntry, RegistryDeactivateParams, RegistryDeactivateResponse, RegistryGetParams,
    RegistryGetResponse, RegistryImportPackParams, RegistryImportPackResponse,
    RegistryListActiveResponse, RegistrySearchHit, RegistrySearchParams, RegistrySearchResponse,
    RegistryTestMatch, RegistryTestParams, RegistryTestResponse, RegistryTestSample,
    RegistryUpsertParams, RegistryUpsertResponse, RequestEnvelope, ResponseEnvelope,
    RuntimeActiveRule, RuntimeBucketSummary, RuntimeStateResponse, SelfCheckResponse,
    SeverityHistogram, SourceLiveness, SubscriptionCloseParams, SubscriptionCloseResponse,
    SubscriptionEvent, SubscriptionListParams, SubscriptionListResponse, SubscriptionOpenParams,
    SubscriptionOpenResponse, SubscriptionPredicate, SubscriptionPullParams,
    SubscriptionPullResponse, SubscriptionSourceSel, SubscriptionSummary, decode_payload,
    encode_frame,
};

pub use framing::{ReadOutcome, read_frame, read_request, read_request_classified, write_response};

#[cfg(unix)]
pub use client::DaemonClient;

#[cfg(windows)]
pub use pipe_client::DaemonClient;
