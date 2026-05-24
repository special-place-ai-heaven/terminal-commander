// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Windows named-pipe IPC client.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::windows::named_pipe::ClientOptions;

use crate::ipc::framing::read_frame;
use crate::ipc::protocol::{
    IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, RequestEnvelope, ResponseEnvelope,
    decode_payload,
};

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
            .map_err(|_| IpcError::new(IpcErrorCode::Internal, "request timed out"))??;
        if resp_env.correlation_id != correlation_id {
            return Err(IpcError::new(
                IpcErrorCode::Internal,
                format!(
                    "correlation mismatch: expected {correlation_id} got {}",
                    resp_env.correlation_id
                ),
            ));
        }
        match resp_env.result {
            IpcResult::Ok { response } => Ok(response),
            IpcResult::Err { error } => Err(error),
        }
    }

    async fn round_trip(&self, env: &RequestEnvelope) -> Result<ResponseEnvelope, IpcError> {
        let pipe_name = self.pipe_path.to_string_lossy().into_owned();
        let mut client = ClientOptions::new()
            .open(&pipe_name)
            .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("pipe connect: {e}")))?;
        let frame = super::protocol::encode_frame(env)?;
        client
            .write_all(&frame)
            .await
            .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("write: {e}")))?;
        let payload = read_frame(&mut client).await?;
        decode_payload::<ResponseEnvelope>(&payload)
    }
}
