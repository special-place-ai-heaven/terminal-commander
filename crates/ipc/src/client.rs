// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon UDS client (TC37).
//!
//! `DaemonClient` opens a UnixStream to the daemon socket and runs
//! one bounded request/response round trip per call. Each call is
//! self-contained: connect, write request, read response, drop the
//! connection.
//!
//! The client enforces the same frame-size cap as the server and
//! refuses to even attempt to read more than [`MAX_FRAME_BYTES`]
//! bytes of payload.
//!
//! This client is daemon-local. The MCP adapter (TC40) will wrap
//! this client behind the rmcp tool dispatcher; it is NOT exposed
//! directly to MCP clients.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::protocol::{
    IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, MAX_FRAME_BYTES, RequestEnvelope,
    ResponseEnvelope, decode_payload, encode_frame,
};

/// Client to a local daemon UDS.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
    request_timeout: Duration,
}

impl DaemonClient {
    /// Construct a client pointed at the daemon socket. Does not
    /// open a connection until [`DaemonClient::call`] is invoked.
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            request_timeout: Duration::from_secs(5),
        }
    }

    /// Override the per-call timeout. Default is 5 seconds.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Connection target.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Run one request/response round trip.
    pub async fn call(
        &self,
        correlation_id: u64,
        request: IpcRequest,
    ) -> Result<IpcResponse, IpcError> {
        let env = RequestEnvelope {
            correlation_id,
            request,
        };
        let resp_env = tokio::time::timeout(self.request_timeout, self.round_trip(&env))
            .await
            .map_err(|_| IpcError::transport("request timed out"))??;
        if resp_env.correlation_id != correlation_id {
            return Err(IpcError::transport(format!(
                "correlation mismatch: expected {correlation_id} got {}",
                resp_env.correlation_id
            )));
        }
        match resp_env.result {
            IpcResult::Ok { response } => Ok(response),
            IpcResult::Err { error } => Err(error),
        }
    }

    async fn round_trip(&self, env: &RequestEnvelope) -> Result<ResponseEnvelope, IpcError> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| IpcError::transport(format!("connect: {e}")))?;
        let frame = encode_frame(env)?;
        stream
            .write_all(&frame)
            .await
            .map_err(|e| IpcError::transport(format!("write: {e}")))?;
        let mut len_buf = [0_u8; 4];
        stream
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| IpcError::transport(format!("read length: {e}")))?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(IpcError::new(
                IpcErrorCode::FrameTooLarge,
                format!("response {len} bytes > MAX_FRAME_BYTES {MAX_FRAME_BYTES}"),
            ));
        }
        let mut payload = vec![0_u8; len];
        stream
            .read_exact(&mut payload)
            .await
            .map_err(|e| IpcError::transport(format!("read payload: {e}")))?;
        decode_payload::<ResponseEnvelope>(&payload)
    }
}
