// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Length-prefixed JSON framing shared by UDS and named-pipe transports.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::protocol::{
    IpcError, IpcErrorCode, MAX_FRAME_BYTES, RequestEnvelope, ResponseEnvelope, decode_payload,
    encode_frame,
};

/// Read one request/response frame from any async byte stream.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, IpcError> {
    let mut len_buf = [0_u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("read length: {e}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::FrameTooLarge,
            format!("frame {len} bytes > MAX_FRAME_BYTES {MAX_FRAME_BYTES}"),
        ));
    }
    let mut payload = vec![0_u8; len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("read payload: {e}")))?;
    Ok(payload)
}

/// Decode a request envelope from a stream.
pub async fn read_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<RequestEnvelope, IpcError> {
    let payload = read_frame(reader).await?;
    decode_payload::<RequestEnvelope>(&payload)
}

/// Write a response envelope to a stream.
pub async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    env: &ResponseEnvelope,
) -> Result<(), std::io::Error> {
    let frame = match encode_frame(env) {
        Ok(bytes) => bytes,
        Err(err) => {
            let small = ResponseEnvelope {
                correlation_id: env.correlation_id,
                result: super::protocol::IpcResult::Err {
                    error: IpcError::new(err.code, err.message),
                },
            };
            encode_frame(&small)
                .map_err(|e| std::io::Error::other(format!("encode small err: {}", e.message)))?
        }
    };
    writer.write_all(&frame).await
}
