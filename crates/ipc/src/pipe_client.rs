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

/// Win32 ERROR_FILE_NOT_FOUND (2): at this instant NO instance of the pipe
/// name exists. With a single-pending-instance accept loop this is the same
/// transient accept/recreate gap as ERROR_PIPE_BUSY, just observed when the
/// consumed instance has fully closed before the replacement is listening.
/// Under CPU starvation (heavy builds) the gap widens from microseconds to
/// whole scheduler quanta, so connects land in it routinely (dogfood
/// 2026-07-02, BACKLOG P1.0f: healthy daemon reported unavailable under
/// load). Retried until the enclosing call deadline; a genuinely absent
/// daemon still fails at that caller-selected bound. The supervisor/self-heal
/// path handles truly dead daemons, not this loop.
const ERROR_FILE_NOT_FOUND_OS: i32 = windows::Win32::Foundation::ERROR_FILE_NOT_FOUND
    .0
    .cast_signed();

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
        self.call_with_timeout(correlation_id, request, self.request_timeout)
            .await
    }

    /// Run one round trip with an explicit per-CALL transport deadline,
    /// overriding the client-level `request_timeout`. Required for requests
    /// that legitimately BLOCK daemon-side (e.g. `bucket_wait` holds the
    /// response up to `MAX_BUCKET_WAIT_MS`): their transport deadline must
    /// COVER the daemon's promised blocking budget or a healthy long wait is
    /// cancelled client-side and misread as a dead daemon.
    pub async fn call_with_timeout(
        &self,
        correlation_id: u64,
        request: IpcRequest,
        timeout: Duration,
    ) -> Result<IpcResponse, IpcError> {
        let env = RequestEnvelope {
            correlation_id,
            request,
        };
        let resp_env = tokio::time::timeout(timeout, self.round_trip(&env))
            .await
            .map_err(|_| {
                IpcError::transport(format!("request timed out after {}ms", timeout.as_millis()))
            })??;
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
        // `call_with_timeout` owns the one authoritative deadline. A second,
        // shorter retry budget here made healthy but CPU-starved daemons look
        // absent before the caller's promised timeout elapsed.
        let mut client = {
            loop {
                match ClientOptions::new().open(&pipe_name) {
                    Ok(p) => break p,
                    // Both errors are the transient accept/recreate gap of a
                    // single-pending-instance server: BUSY = instances exist
                    // but none listening; FILE_NOT_FOUND = the consumed
                    // instance closed before the replacement was listening
                    // (routine under CPU starvation -- see the constant docs).
                    Err(e)
                        if matches!(
                            e.raw_os_error(),
                            Some(ERROR_PIPE_BUSY_OS | ERROR_FILE_NOT_FOUND_OS)
                        ) =>
                    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::windows::named_pipe::ServerOptions;

    #[tokio::test]
    async fn client_retries_transient_absence_until_the_call_deadline() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let pipe_name = format!(
            r"\\.\pipe\terminal-commander-delayed-{}-{nonce}",
            std::process::id()
        );
        let server_name = pipe_name.clone();
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let mut pipe = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&server_name)
                .expect("create delayed pipe");
            pipe.connect().await.expect("accept delayed client");

            let payload = read_frame(&mut pipe).await.expect("read health request");
            let request = decode_payload::<RequestEnvelope>(&payload).expect("decode request");
            let response = ResponseEnvelope {
                correlation_id: request.correlation_id,
                result: IpcResult::Ok {
                    response: IpcResponse::Health {
                        uptime_secs: 1,
                        idle_secs: Some(0),
                        version: "test".to_owned(),
                    },
                },
            };
            let frame = crate::protocol::encode_frame(&response).expect("encode response");
            pipe.write_all(&frame).await.expect("write health response");
        });

        let response = DaemonClient::new(&pipe_name)
            .with_timeout(Duration::from_secs(3))
            .call(1, IpcRequest::Health)
            .await
            .expect("client must keep retrying until the call deadline");
        assert!(matches!(response, IpcResponse::Health { .. }));
        server.await.expect("delayed server task");
    }
}
