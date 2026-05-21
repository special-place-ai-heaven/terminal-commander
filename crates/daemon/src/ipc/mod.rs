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
    DiscoverResponse, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, MAX_FRAME_BYTES,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PolicyStatusResponse, RequestEnvelope, ResponseEnvelope,
    SelfCheckResponse,
};

#[cfg(unix)]
pub use peer::PeerCred;

#[cfg(unix)]
pub use server::{IpcServer, ServerHandle};

#[cfg(unix)]
pub use client::DaemonClient;
