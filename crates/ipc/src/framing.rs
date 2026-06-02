// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Length-prefixed JSON framing shared by UDS and named-pipe transports.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::protocol::{
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

/// Outcome of [`read_request_classified`].
///
/// Distinguishes a clean peer disconnect (`Eof`) from a malformed
/// frame / protocol error (`Err`) so a server can mirror the UDS
/// behaviour: break silently on EOF, but write a typed error envelope
/// to the client on a protocol error before closing. [`read_request`]
/// alone cannot make this distinction because it collapses an
/// `UnexpectedEof` on the length prefix into an [`IpcErrorCode::Internal`]
/// error like any other read failure.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum ReadOutcome {
    /// A complete, decoded request envelope.
    Ok(RequestEnvelope),
    /// The peer closed the connection cleanly before sending the next
    /// length prefix. Servers break the read loop silently.
    Eof,
    /// A malformed frame or protocol error. Servers write a typed
    /// error envelope to the client, then close.
    Err(IpcError),
}

/// Read and decode one request envelope, classifying a clean EOF on the
/// length prefix separately from a malformed-frame / protocol error.
///
/// This is the transport-generic equivalent of the UDS server's
/// `read_envelope`: a half-read length prefix at the start of a frame
/// (`ErrorKind::UnexpectedEof`) is a clean disconnect; every other
/// failure carries a typed [`IpcError`] with the same code the
/// non-classified [`read_request`] path would surface
/// ([`IpcErrorCode::FrameTooLarge`], [`IpcErrorCode::MalformedJson`],
/// or [`IpcErrorCode::Internal`]).
pub async fn read_request_classified<R: AsyncRead + Unpin>(reader: &mut R) -> ReadOutcome {
    // 4-byte length prefix. A clean EOF here (nothing buffered) means
    // the peer disconnected between frames; any other error is a real
    // read/protocol failure.
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return ReadOutcome::Eof,
        Err(e) => {
            return ReadOutcome::Err(IpcError::new(
                IpcErrorCode::Internal,
                format!("read length: {e}"),
            ));
        }
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return ReadOutcome::Err(IpcError::new(
            IpcErrorCode::FrameTooLarge,
            format!("frame {len} bytes > MAX_FRAME_BYTES {MAX_FRAME_BYTES}"),
        ));
    }
    let mut payload = vec![0_u8; len];
    if let Err(e) = reader.read_exact(&mut payload).await {
        return ReadOutcome::Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("read payload: {e}"),
        ));
    }
    match decode_payload::<RequestEnvelope>(&payload) {
        Ok(env) => ReadOutcome::Ok(env),
        Err(err) => ReadOutcome::Err(err),
    }
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
                result: crate::protocol::IpcResult::Err {
                    error: IpcError::new(err.code, err.message),
                },
            };
            encode_frame(&small)
                .map_err(|e| std::io::Error::other(format!("encode small err: {}", e.message)))?
        }
    };
    writer.write_all(&frame).await
}
