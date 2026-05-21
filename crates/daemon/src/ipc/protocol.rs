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

use std::time::Duration;

use serde::{Deserialize, Serialize};
use terminal_commander_core::{BucketId, EventId, SignalEvent};

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

/// Maximum wait timeout the daemon will accept for `bucket_wait`.
/// Requests above this are clamped at the dispatcher.
pub const MAX_BUCKET_WAIT_MS: u64 = 30_000;
/// Default wait timeout when the client omits one.
pub const DEFAULT_BUCKET_WAIT_MS: u64 = 5_000;
/// Hard cap on events returned by `bucket_events_since` /
/// `bucket_wait`. Mirrors the codebase `MAX_READ_LIMIT`.
pub const MAX_BUCKET_READ_LIMIT: usize = 10_000;
/// Default events-per-call when the client omits a limit.
pub const DEFAULT_BUCKET_READ_LIMIT: usize = 200;
/// Hard cap on context-window frames returned by `event_context`.
pub const MAX_CONTEXT_FRAMES: u32 = 1024;
/// Default `before` count when the client omits one.
pub const DEFAULT_CONTEXT_BEFORE: u32 = 5;
/// Default `after` count when the client omits one.
pub const DEFAULT_CONTEXT_AFTER: u32 = 5;
/// Hard cap on event_context payload bytes. Mirrors the per-ring
/// `max_bytes` cap; the dispatcher clamps oversize values.
pub const MAX_CONTEXT_BYTES: usize = 64 * 1024;

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
    /// Cursor-based bucket read. Bounded by `MAX_BUCKET_READ_LIMIT`.
    BucketEventsSince(BucketEventsSinceParams),
    /// Realtime wait. Returns heartbeat on timeout, never raw text.
    BucketWait(BucketWaitParams),
    /// Bounded summary (counters + severity histogram).
    BucketSummary(BucketSummaryParams),
    /// Bounded context window around the event's source pointer.
    /// Resolved by `(bucket_id, event_id)`; the daemon walks the
    /// bucket to find the matching event, then resolves
    /// `(probe_id, pointer.frame_id)` against the context ring.
    EventContext(EventContextParams),
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
    BucketEventsSince(BucketEventsSinceResponse),
    BucketWait(BucketWaitResponse),
    BucketSummary(BucketSummaryResponse),
    EventContext(EventContextResponse),
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
    /// The requested bucket does not exist.
    BucketNotFound,
    /// The requested event id was not found in the bucket.
    EventNotFound,
    /// The cursor is invalid (e.g. far above the current tail).
    InvalidCursor,
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

/// Parameters for `bucket_events_since`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEventsSinceParams {
    pub bucket_id: BucketId,
    pub cursor: u64,
    /// Optional minimum severity. Omitted = `trace` (no filter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_min: Option<terminal_commander_core::Severity>,
    /// Optional exact-match kind filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_filter: Option<String>,
    /// Result count cap. Clamped to `MAX_BUCKET_READ_LIMIT` at the
    /// dispatcher. Omitted = `DEFAULT_BUCKET_READ_LIMIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Response shape for `bucket_events_since` / `bucket_wait`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEventsSinceResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    pub has_more: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// Parameters for `bucket_wait`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketWaitParams {
    pub bucket_id: BucketId,
    pub cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_min: Option<terminal_commander_core::Severity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Maximum wait in milliseconds. Clamped to `MAX_BUCKET_WAIT_MS`.
    /// Omitted = `DEFAULT_BUCKET_WAIT_MS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl BucketWaitParams {
    /// Resolve the effective `Duration`, clamping to the hard cap.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        let raw = self.timeout_ms.unwrap_or(DEFAULT_BUCKET_WAIT_MS);
        Duration::from_millis(raw.min(MAX_BUCKET_WAIT_MS))
    }
}

/// Response shape for `bucket_wait`. Identical to
/// `BucketEventsSinceResponse` plus a `heartbeat` flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketWaitResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    /// `true` when the wait timed out and no matching events
    /// arrived. The `events` array MUST be empty in that case.
    pub heartbeat: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// Parameters for `bucket_summary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSummaryParams {
    pub bucket_id: BucketId,
}

/// Response shape for `bucket_summary`. Counters only; never raw
/// stream content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSummaryResponse {
    pub bucket_id: BucketId,
    pub head_seq: u64,
    pub tail_seq: u64,
    pub event_count: u64,
    pub dropped_count: u64,
    /// Per-severity histogram (trace / debug / info / low / medium /
    /// high / critical), in wire-stable order.
    pub by_severity: SeverityHistogram,
}

/// Wire-stable severity histogram. Independent of the in-memory
/// `BucketSummary` so the protocol locks the field order.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SeverityHistogram {
    pub trace: u64,
    pub debug: u64,
    pub info: u64,
    pub low: u64,
    pub medium: u64,
    pub high: u64,
    pub critical: u64,
}

/// Parameters for `event_context`. Resolves the event's source
/// pointer and returns bounded context around that frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContextParams {
    pub bucket_id: BucketId,
    pub event_id: EventId,
    /// Frames to include BEFORE the anchor. Clamped to
    /// `MAX_CONTEXT_FRAMES`. Omitted = `DEFAULT_CONTEXT_BEFORE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<u32>,
    /// Frames to include AFTER the anchor. Clamped to
    /// `MAX_CONTEXT_FRAMES`. Omitted = `DEFAULT_CONTEXT_AFTER`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<u32>,
    /// Hard byte cap on the response. Clamped to
    /// `MAX_CONTEXT_BYTES`. Omitted = `MAX_CONTEXT_BYTES`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

/// One context frame on the wire. Same shape as `core::ContextLine`
/// but kept inside the IPC module so the protocol owns its serde
/// surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcContextFrame {
    pub probe_id: terminal_commander_core::ProbeId,
    pub frame_id: terminal_commander_core::FrameId,
    pub stream: terminal_commander_core::SourceStream,
    pub line: Option<u64>,
    pub text: String,
}

/// Reasons a context window may be empty or partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextUnavailableReason {
    /// Severity below Medium: event carries no pointer by design.
    NoPointer,
    /// Event carried a `pointer_unavailable_reason` instead of a
    /// pointer (synthetic lifecycle events).
    SyntheticEvent,
    /// Anchor frame already evicted from the ring.
    AnchorEvicted,
    /// Probe id not found in the context-ring manager.
    UnknownProbe,
}

/// Response shape for `event_context`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContextResponse {
    pub bucket_id: BucketId,
    pub event_id: EventId,
    /// `true` when the anchor frame was no longer in the ring at
    /// resolution time.
    pub anchor_missing: bool,
    /// Set when the daemon could not produce a window for a
    /// non-error reason (severity below threshold, synthetic event,
    /// etc.). When set, `frames` is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<ContextUnavailableReason>,
    /// Echo of the event's `pointer_unavailable_reason` if the event
    /// carried one. Never raw stream content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_unavailable_reason: Option<String>,
    /// Bounded window. Empty when `anchor_missing` or
    /// `unavailable_reason` is set.
    pub frames: Vec<IpcContextFrame>,
    /// Reported by the underlying ring; helps clients reason about
    /// truncation.
    pub total_bytes: usize,
    pub truncated: bool,
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
