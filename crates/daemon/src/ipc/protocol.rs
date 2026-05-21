// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! IPC wire protocol (TC37).
//!
//! Length-prefixed JSON frames. Every frame begins with a 4-byte
//! big-endian `u32` payload length, followed by exactly that many
//! bytes of UTF-8 JSON. Frames larger than [`MAX_FRAME_BYTES`] are
//! rejected before the payload is decoded.
//!
//! Method set at TC37 is deliberately tiny:
//! - `system_discover` — version + capabilities + tool list.
//! - `health` — daemon liveness ping.
//! - `policy_status` — active profile + the daemon-side caps.
//! - `self_check` — re-run the bounded TC36 self-check report.
//!
//! No `command_*`, no `bucket_*`, no `event_context`, no
//! `file_read_*` — those land in TC38 (process wiring), TC39
//! (bucket/event daemon API), and TC41 (MCP tool surface). TC37
//! deliberately ships a minimal, safe method set so the transport
//! lock-in does not race ahead of policy / wiring goals.
//!
//! Source-status: live (TC37).

use serde::{Deserialize, Serialize};

/// Hard cap on a complete frame (length prefix + payload). Anything
/// above this is rejected without payload parse.
///
/// 256 KiB is large enough for the JSON shapes ahead (e.g. a full
/// `bucket_events_since` response capped at 10000 events ~ 25 bytes
/// per event minimum overhead) and small enough that one malicious
/// client cannot exhaust daemon memory by streaming a giant frame.
/// Bucket / event-context responses still carry their own per-call
/// caps; this is the transport-layer envelope cap.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// Soft cap on the request side.
///
/// Applied before the request is dispatched. Currently identical to
/// [`MAX_FRAME_BYTES`]; kept as a named constant so future tools can
/// raise / lower it independently from response sizing.
pub const MAX_REQUEST_BYTES: usize = MAX_FRAME_BYTES;

/// Soft cap on the response side. Matches the frame cap today.
pub const MAX_RESPONSE_BYTES: usize = MAX_FRAME_BYTES;

/// Wire correlation id.
///
/// The client picks; the server echoes. Used to distinguish responses
/// on a multiplexed connection. Today the connection is request /
/// response one-at-a-time; the field future-proofs the protocol.
pub type CorrelationId = u64;

/// Top-level request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub correlation_id: CorrelationId,
    pub request: IpcRequest,
}

/// Top-level response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub correlation_id: CorrelationId,
    pub result: IpcResult,
}

/// Method-typed request union. Method names are namespaced
/// `<domain>_<verb>` (matching the MCP tool naming so the eventual
/// rmcp adapter at TC40 maps 1:1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum IpcRequest {
    /// Get daemon version, MCP spec revision, callable method list.
    SystemDiscover,
    /// Liveness ping. Returns the daemon uptime in seconds.
    Health,
    /// Report active policy profile + the configured per-call caps.
    PolicyStatus,
    /// Re-run the TC36 self-check; returns the report as text.
    SelfCheck,
}

/// Success / error union. Success carries a typed payload per method;
/// error carries a structured code + message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum IpcResult {
    Ok { response: IpcResponse },
    Err { error: IpcError },
}

/// Method-typed success union. Each variant matches one
/// [`IpcRequest`] variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "method")]
pub enum IpcResponse {
    SystemDiscover(DiscoverResponse),
    Health { uptime_secs: u64 },
    PolicyStatus(PolicyStatusResponse),
    SelfCheck(SelfCheckResponse),
}

/// `system_discover` payload. Mirrors the contract laid out in
/// `docs/mcp/TOOL_CONTROL_SURFACE.md`. The advertised method list is
/// tied to the dispatcher's actual handler set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverResponse {
    pub version: String,
    pub mcp_spec: String,
    pub policy_profile: String,
    pub methods: Vec<String>,
}

/// `policy_status` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStatusResponse {
    pub profile: String,
    pub commands_deny_count: usize,
    pub default_deny_path_suffix_count: usize,
    /// Per-call file_read_window cap (from `LimitsSection`, clamped
    /// at config load to the codebase hard cap).
    pub file_window_bytes: usize,
    /// Per-call bucket-read cap.
    pub bucket_read_limit: usize,
}

/// `self_check` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfCheckResponse {
    pub report: String,
    pub failures: u32,
}

/// Structured error code. Closed set. Adding a variant requires a
/// goal-file amendment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    /// Frame exceeded [`MAX_FRAME_BYTES`].
    FrameTooLarge,
    /// Payload was not valid UTF-8 JSON.
    MalformedJson,
    /// Payload decoded but didn't match the wire schema.
    SchemaMismatch,
    /// Method not recognized.
    UnknownMethod,
    /// Policy engine denied the request.
    PolicyDenied,
    /// Daemon-internal error while handling the request.
    Internal,
    /// Peer credential check failed; connection refused.
    PeerCredentialFailure,
    /// Platform does not support UDS (Windows native).
    UnsupportedPlatform,
}

/// Structured error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
}

impl IpcError {
    /// Constructor.
    #[must_use]
    pub fn new(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Serialize an envelope to a length-prefixed wire frame. Returns
/// the bytes ready to write to the socket. Rejects payloads larger
/// than [`MAX_FRAME_BYTES`].
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, IpcError> {
    let json = serde_json::to_vec(value)
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("serialize: {e}")))?;
    if json.len() > MAX_FRAME_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::FrameTooLarge,
            format!(
                "payload {} bytes > MAX_FRAME_BYTES {MAX_FRAME_BYTES}",
                json.len()
            ),
        ));
    }
    let len_u32 = u32::try_from(json.len())
        .map_err(|_| IpcError::new(IpcErrorCode::Internal, "len overflow"))?;
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&len_u32.to_be_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

/// Deserialize the JSON portion of a frame (length-prefix stripped
/// by the caller). The caller has already validated that
/// `payload.len() <= MAX_FRAME_BYTES`.
pub fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, IpcError> {
    serde_json::from_slice::<T>(payload)
        .map_err(|e| IpcError::new(IpcErrorCode::MalformedJson, format!("decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_envelope_round_trip() {
        let req = RequestEnvelope {
            correlation_id: 42,
            request: IpcRequest::SystemDiscover,
        };
        let frame = encode_frame(&req).unwrap();
        // 4-byte length + JSON
        assert!(frame.len() > 4);
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(len, frame.len() - 4);
        let back: RequestEnvelope = decode_payload(&frame[4..]).unwrap();
        assert_eq!(back.correlation_id, 42);
        assert!(matches!(back.request, IpcRequest::SystemDiscover));
    }

    #[test]
    fn malformed_json_rejected_with_typed_code() {
        let bad = b"{not valid json";
        let err: IpcError = decode_payload::<RequestEnvelope>(bad).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::MalformedJson);
    }

    #[test]
    fn schema_mismatch_is_malformed_json_today() {
        // serde_json reports as a parse error; we surface that as
        // MalformedJson because the variant set is closed-set.
        let s = br#"{"correlation_id": 1, "request": {"method": "totally_bogus"}}"#;
        let err: IpcError = decode_payload::<RequestEnvelope>(s).unwrap_err();
        // Either MalformedJson or SchemaMismatch is acceptable; both
        // keep the bad payload out of the dispatcher.
        assert!(matches!(
            err.code,
            IpcErrorCode::MalformedJson | IpcErrorCode::SchemaMismatch
        ));
    }

    #[test]
    fn frame_too_large_rejected_before_serialize_attempt() {
        // Construct an envelope that, once serialized, would exceed
        // MAX_FRAME_BYTES. Easiest way: a SelfCheck response with a
        // huge report string.
        let huge = "x".repeat(MAX_FRAME_BYTES + 1024);
        let env = ResponseEnvelope {
            correlation_id: 1,
            result: IpcResult::Ok {
                response: IpcResponse::SelfCheck(SelfCheckResponse {
                    report: huge,
                    failures: 0,
                }),
            },
        };
        let err = encode_frame(&env).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::FrameTooLarge);
    }

    #[test]
    fn response_envelope_err_round_trips() {
        let env = ResponseEnvelope {
            correlation_id: 7,
            result: IpcResult::Err {
                error: IpcError::new(IpcErrorCode::PolicyDenied, "nope"),
            },
        };
        let frame = encode_frame(&env).unwrap();
        let back: ResponseEnvelope = decode_payload(&frame[4..]).unwrap();
        match back.result {
            IpcResult::Err { error } => {
                assert_eq!(error.code, IpcErrorCode::PolicyDenied);
                assert_eq!(error.message, "nope");
            }
            IpcResult::Ok { .. } => panic!("expected err variant"),
        }
    }
}
