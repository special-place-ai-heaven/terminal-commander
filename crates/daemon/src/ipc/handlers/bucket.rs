// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use super::common::map_bucket_error;
use crate::ipc::protocol::{
    BucketEventsSinceParams, BucketEventsSinceResponse, BucketSummaryParams, BucketSummaryResponse,
    BucketWaitParams, BucketWaitResponse, ContextUnavailableReason, DEFAULT_BUCKET_READ_LIMIT,
    DEFAULT_CONTEXT_AFTER, DEFAULT_CONTEXT_BEFORE, EventContextParams, EventContextResponse,
    IpcContextFrame, IpcError, IpcErrorCode, IpcResponse, MAX_BUCKET_READ_LIMIT, MAX_CONTEXT_BYTES,
    MAX_CONTEXT_FRAMES, SeverityHistogram,
};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn handle_bucket_events_since(
    state: &Arc<DaemonState>,
    params: &BucketEventsSinceParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::BucketReadRequest;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_BUCKET_READ_LIMIT)
        .min(MAX_BUCKET_READ_LIMIT);
    let req = BucketReadRequest {
        cursor: params.cursor,
        severity_min: params.severity_min,
        kind_filter: params.kind_filter.clone(),
        limit: Some(limit),
    };
    let resp = state
        .router
        .bucket_events_since(params.bucket_id, &req)
        .map_err(map_bucket_error)?;
    Ok(IpcResponse::BucketEventsSince(BucketEventsSinceResponse {
        bucket_id: params.bucket_id,
        cursor_in: resp.cursor_in,
        next_cursor: resp.next_cursor,
        has_more: resp.has_more,
        dropped_count: resp.dropped_count,
        events: resp.events,
    }))
}

pub(in crate::ipc::server) async fn handle_bucket_wait(
    state: &Arc<DaemonState>,
    params: &BucketWaitParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::BucketWaitRequest;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_BUCKET_READ_LIMIT)
        .min(MAX_BUCKET_READ_LIMIT);
    let req = BucketWaitRequest {
        cursor: params.cursor,
        severity_min: params.severity_min,
        kind_filter: params.kind_filter.clone(),
        limit: Some(limit),
        timeout: params.timeout(),
    };
    let resp = state
        .router
        .bucket_wait(params.bucket_id, req)
        .await
        .map_err(map_bucket_error)?;
    Ok(IpcResponse::BucketWait(BucketWaitResponse {
        bucket_id: params.bucket_id,
        cursor_in: resp.cursor_in,
        next_cursor: resp.next_cursor,
        heartbeat: resp.heartbeat,
        dropped_count: resp.dropped_count,
        events: resp.events,
    }))
}

pub(in crate::ipc::server) fn handle_bucket_summary(
    state: &Arc<DaemonState>,
    params: &BucketSummaryParams,
) -> Result<IpcResponse, IpcError> {
    let s = state
        .router
        .bucket_summary(params.bucket_id)
        .map_err(map_bucket_error)?;
    Ok(IpcResponse::BucketSummary(BucketSummaryResponse {
        bucket_id: params.bucket_id,
        head_seq: s.head_seq,
        tail_seq: s.tail_seq,
        event_count: s.event_count,
        dropped_count: s.dropped_count,
        by_severity: SeverityHistogram {
            trace: s.by_severity.trace,
            debug: s.by_severity.debug,
            info: s.by_severity.info,
            low: s.by_severity.low,
            medium: s.by_severity.medium,
            high: s.by_severity.high,
            critical: s.by_severity.critical,
        },
    }))
}

#[allow(clippy::too_many_lines)] // straight-line pipeline; splitting hurts clarity
pub(in crate::ipc::server) fn handle_event_context(
    state: &Arc<DaemonState>,
    params: &EventContextParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::{BucketId, BucketReadRequest, Severity, SignalEvent};

    // 1. Resolve the owning bucket + the anchor event by event_id. We
    //    scan a bucket from cursor 0 in MAX_BUCKET_READ_LIMIT pages;
    //    buckets are bounded rings (TC07) so every scan terminates.
    //
    //    - `bucket_id` supplied (FR-040): exactly today's single-bucket
    //      path. The event being absent from THAT bucket is
    //      `EventNotFound` (a contradicting bucket_id is an error, never
    //      silently corrected), byte-identical to today.
    //    - `bucket_id` absent: scan in-scope buckets for the
    //      globally-unique event id (UUIDv7 -> at most one owner). Not
    //      found in ANY live bucket (including an evicted owner) is the
    //      same honest `EventNotFound`, phrased WITHOUT a bucket name.
    let scan_bucket = |bucket_id: BucketId| -> Result<Option<SignalEvent>, IpcError> {
        let mut cursor: u64 = 0;
        loop {
            let page = state
                .router
                .bucket_events_since(
                    bucket_id,
                    &BucketReadRequest {
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: Some(MAX_BUCKET_READ_LIMIT),
                    },
                )
                .map_err(map_bucket_error)?;
            if let Some(ev) = page.events.iter().find(|e| e.event_id == params.event_id) {
                return Ok(Some(ev.clone()));
            }
            if !page.has_more {
                return Ok(None);
            }
            cursor = page.next_cursor;
        }
    };

    let (bucket_id, event) = if let Some(requested) = params.bucket_id {
        match scan_bucket(requested)? {
            Some(ev) => (requested, ev),
            None => {
                return Err(IpcError::new(
                    IpcErrorCode::EventNotFound,
                    format!(
                        "event {} not found in bucket {}",
                        params.event_id.to_wire_string(),
                        requested.to_wire_string()
                    ),
                ));
            }
        }
    } else {
        let mut resolved = None;
        for bid in state.buckets.list_bucket_ids() {
            if let Some(ev) = scan_bucket(bid)? {
                resolved = Some((bid, ev));
                break;
            }
        }
        match resolved {
            Some(pair) => pair,
            None => {
                return Err(IpcError::new(
                    IpcErrorCode::EventNotFound,
                    format!(
                        "event {} not found in any live bucket",
                        params.event_id.to_wire_string()
                    ),
                ));
            }
        }
    };

    // 2. Pointer / unavailable-reason path. Below-Medium events
    //    carry no pointer by design; surface that explicitly.
    let Some(pointer) = event.pointer.as_ref() else {
        let reason = if event.pointer_unavailable_reason.is_some() {
            ContextUnavailableReason::SyntheticEvent
        } else if event.severity < Severity::Medium {
            ContextUnavailableReason::NoPointer
        } else {
            // TC02 invariant: severity>=Medium without pointer MUST
            // carry pointer_unavailable_reason. We surface what the
            // event itself recorded.
            ContextUnavailableReason::SyntheticEvent
        };
        return Ok(IpcResponse::EventContext(EventContextResponse {
            bucket_id,
            event_id: params.event_id,
            anchor_missing: false,
            unavailable_reason: Some(reason),
            pointer_unavailable_reason: event.pointer_unavailable_reason,
            frames: Vec::new(),
            total_bytes: 0,
            truncated: false,
        }));
    };

    // 3. Clamp request limits.
    let before = params
        .before
        .unwrap_or(DEFAULT_CONTEXT_BEFORE)
        .min(MAX_CONTEXT_FRAMES);
    let after = params
        .after
        .unwrap_or(DEFAULT_CONTEXT_AFTER)
        .min(MAX_CONTEXT_FRAMES);
    let max_bytes = params
        .max_bytes
        .unwrap_or(MAX_CONTEXT_BYTES)
        .min(MAX_CONTEXT_BYTES);

    // 4. Window resolution.
    let window = state
        .router
        .event_context(
            event.source.probe_id,
            pointer.frame_id,
            before,
            after,
            Some(max_bytes),
        )
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, e.to_string()))?;

    // 5. anchor_missing path (ring eviction).
    if window.anchor_missing {
        return Ok(IpcResponse::EventContext(EventContextResponse {
            bucket_id,
            event_id: params.event_id,
            anchor_missing: true,
            unavailable_reason: Some(ContextUnavailableReason::AnchorEvicted),
            pointer_unavailable_reason: event.pointer_unavailable_reason.clone(),
            frames: Vec::new(),
            total_bytes: 0,
            truncated: false,
        }));
    }

    // 6. Project ContextLine -> IpcContextFrame. The wire form
    //    carries no extra fields beyond what the ring frame already
    //    holds. No raw stream beyond the bounded text already
    //    capped by the ring.
    let mut frames: Vec<IpcContextFrame> = Vec::with_capacity(window.frames.len());
    let mut total_bytes: usize = 0;
    for line in &window.frames {
        total_bytes = total_bytes.saturating_add(line.text.len());
        frames.push(IpcContextFrame {
            probe_id: event.source.probe_id,
            frame_id: line.frame_id,
            stream: line.stream.clone(),
            line: line.line,
            text: line.text.clone(),
        });
    }

    let truncated = window.truncated_before
        || window.truncated_after
        || window.truncated_bytes
        || window.truncated_frames;
    Ok(IpcResponse::EventContext(EventContextResponse {
        bucket_id,
        event_id: params.event_id,
        anchor_missing: false,
        unavailable_reason: None,
        pointer_unavailable_reason: event.pointer_unavailable_reason.clone(),
        frames,
        total_bytes,
        truncated,
    }))
}
