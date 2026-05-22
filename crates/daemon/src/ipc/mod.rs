// SPDX-License-Identifier: Apache-2.0
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

pub mod protocol;

#[cfg(unix)]
pub mod peer;

#[cfg(unix)]
pub mod server;

#[cfg(unix)]
pub mod client;

pub use protocol::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, CommandStartParams, CommandStartResponse,
    CommandStatusParams, CommandStatusResponse, ContextUnavailableReason,
    DEFAULT_BUCKET_READ_LIMIT, DEFAULT_BUCKET_WAIT_MS, DEFAULT_CONTEXT_AFTER,
    DEFAULT_CONTEXT_BEFORE, DEFAULT_FILE_READ_BYTES, DEFAULT_FILE_READ_LINES,
    DEFAULT_FILE_SEARCH_MATCHES, DEFAULT_FILE_SEARCH_SNIPPET_BYTES, DEFAULT_REGISTRY_SEARCH_LIMIT,
    DiscoverResponse, EventContextParams, EventContextResponse, FileLine, FileReadWindowParams,
    FileReadWindowResponse, FileSearchMatch, FileSearchParams, FileSearchResponse,
    FileWatchListEntry, FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse,
    FileWatchStopParams, FileWatchStopResponse, IpcContextFrame, IpcError, IpcErrorCode,
    IpcRequest, IpcResponse, IpcResult, MAX_BUCKET_READ_LIMIT, MAX_BUCKET_WAIT_MS,
    MAX_COMMAND_ENV_ITEMS, MAX_COMMAND_GRACE_MS, MAX_COMMAND_INLINE_RULES, MAX_CONTEXT_BYTES,
    MAX_CONTEXT_FRAMES, MAX_FILE_READ_BYTES, MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES,
    MAX_FILE_SEARCH_SCAN_BYTES, MAX_FILE_SEARCH_SNIPPET_BYTES, MAX_FRAME_BYTES, MAX_PTY_ARGV_ITEMS,
    MAX_PTY_STDIN_BYTES, MAX_REGISTRY_SEARCH_LIMIT, MAX_REGISTRY_TEST_SAMPLE_BYTES,
    MAX_REGISTRY_TEST_SAMPLES, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PolicyStatusResponse,
    PtyCommandListEntry, PtyCommandListResponse, PtyCommandStartParams, PtyCommandStartResponse,
    PtyCommandStopParams, PtyCommandStopResponse, PtyCommandWriteStdinParams,
    PtyCommandWriteStdinResponse, RegistryActivateParams, RegistryActivateResponse,
    RegistryActiveEntry, RegistryDeactivateParams, RegistryDeactivateResponse, RegistryGetParams,
    RegistryGetResponse, RegistryListActiveResponse, RegistrySearchHit, RegistrySearchParams,
    RegistrySearchResponse, RegistryTestMatch, RegistryTestParams, RegistryTestResponse,
    RegistryTestSample, RegistryUpsertParams, RegistryUpsertResponse, RequestEnvelope,
    ResponseEnvelope, SelfCheckResponse, SeverityHistogram,
};

// `CommandStartResponse` / `CommandStatusResponse` are re-exported by
// `protocol` via `pub use crate::command::...` and they are already
// re-exported at the crate root via `pub use command::{...}`, so we
// deliberately leave them out of this list to avoid an E0252 clash.

#[cfg(unix)]
pub use peer::PeerCred;

#[cfg(unix)]
pub use server::{IpcServer, ServerHandle};

#[cfg(unix)]
pub use client::DaemonClient;
