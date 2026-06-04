// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Windows named-pipe IPC client.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::windows::named_pipe::ClientOptions;

use crate::framing::read_frame;
use crate::protocol::{
    IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, RequestEnvelope, ResponseEnvelope,
    decode_payload,
};

/// Win32 ERROR_PIPE_BUSY (231 / 0xE7): server exists but no instance is
/// currently waiting for a connection (between accept and recreate).
const ERROR_PIPE_BUSY_OS: i32 = windows::Win32::Foundation::ERROR_PIPE_BUSY.0.cast_signed();

/// Maximum number of retries when the named pipe is busy.
const PIPE_BUSY_RETRIES: u32 = 50; // 50 × 20 ms = 1 000 ms

/// Delay between retries when the named pipe is busy.
const PIPE_BUSY_DELAY_MS: u64 = 20;

/// Client to the parent daemon named pipe.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    pipe_path: PathBuf,
    request_timeout: Duration,
}

impl DaemonClient {
    #[must_use]
    pub fn new(pipe_path: impl Into<PathBuf>) -> Self {
        Self {
            pipe_path: pipe_path.into(),
            request_timeout: Duration::from_secs(5),
        }
    }

    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.pipe_path
    }

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
        let pipe_name = self.pipe_path.to_string_lossy().into_owned();

        // The accept loop keeps one pending pipe instance and recreates it after
        // each connect (see pipe_server.rs accept loop).  Between the accept of
        // client A and the recreate for client B, any concurrent open returns
        // ERROR_PIPE_BUSY (Win32 231) even though the daemon is healthy.
        //
        // Tokio docs recommend retrying on this error:
        // https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ClientOptions.html
        //
        // Budget: PIPE_BUSY_RETRIES × PIPE_BUSY_DELAY_MS = 1 000 ms, well within
        // the outer 5-second request timeout set in `call`.
        let mut client = {
            let mut attempt = 0u32;
            loop {
                match ClientOptions::new().open(&pipe_name) {
                    Ok(p) => break p,
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY_OS) => {
                        attempt += 1;
                        if attempt >= PIPE_BUSY_RETRIES {
                            return Err(IpcError::transport(format!(
                                "pipe connect: ERROR_PIPE_BUSY after {attempt} retries: {e}"
                            )));
                        }
                        tokio::time::sleep(Duration::from_millis(PIPE_BUSY_DELAY_MS)).await;
                    }
                    Err(e) => {
                        return Err(IpcError::transport(format!("pipe connect: {e}")));
                    }
                }
            }
        };
        let frame = crate::protocol::encode_frame(env)?;
        client
            .write_all(&frame)
            .await
            .map_err(|e| IpcError::transport(format!("write: {e}")))?;
        // `read_frame` is shared with the server's request-read path, so its
        // read errors arrive as plain `Internal`. On the CLIENT side a failed
        // response read IS a transport failure (the daemon dropped the pipe
        // mid-call), so re-tag it; a `FrameTooLarge` is a real protocol fault
        // and is left untouched.
        let payload = read_frame(&mut client).await.map_err(|e| {
            if e.code == IpcErrorCode::Internal {
                IpcError::transport(e.message)
            } else {
                e
            }
        })?;
        decode_payload::<ResponseEnvelope>(&payload)
    }
}
