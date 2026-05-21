// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Bounded per-probe context ring.
//!
//! A [`ContextRing`] stores normalized [`SourceFrame`]s for one probe.
//! Each frame is ANSI-stripped + CR-collapsed BEFORE it arrives here
//! (that work lives in TC15 / TC19); the ring is agnostic to the
//! stripper.
//!
//! Capacity is bounded by frame count AND total payload bytes; on
//! overflow the head frame is evicted and `evicted_frames` is bumped
//! so the caller can detect dropped history via a truncation marker
//! in [`ContextWindowResponse`].
//!
//! Frame text is capped at [`MAX_FRAME_BYTES`]; oversize frames are
//! truncated at append time and carry `truncated_bytes` so consumers
//! see explicit loss instead of silent corruption.
//!
//! Source-status: live (TC08). Persistence and per-probe spool
//! eviction policies are deferred to TC18 / TC22.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ids::{FrameId, ProbeId};
use crate::pointer::SourcePointer;
use crate::source::SourceStream;

/// Default frame-count cap per ring.
pub const DEFAULT_RING_FRAMES: usize = 4096;

/// Default total-payload cap per ring (bytes).
pub const DEFAULT_RING_BYTES: usize = 1 << 20; // 1 MiB

/// Hard cap on a single frame's text length (bytes). Oversize frames
/// are truncated at append time.
pub const MAX_FRAME_BYTES: usize = 8192;

/// Hard cap on a single context-window response, in bytes. Protects
/// against accidental over-sized responses.
pub const MAX_WINDOW_BYTES: usize = 64 * 1024;

/// Hard cap on the number of frames returned by a single window.
pub const MAX_WINDOW_FRAMES: usize = 256;

/// A single normalized frame held in the ring.
///
/// `text` is the post-ANSI-strip, post-CR-collapse content. The
/// ring never stores raw escape sequences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFrame {
    pub frame_id: FrameId,
    pub probe_id: ProbeId,
    pub stream: SourceStream,
    pub timestamp: OffsetDateTime,
    pub text: String,
    /// 1-based line number within the probe's stream, if the
    /// upstream stripper assigned one. Optional.
    pub line: Option<u64>,
    /// Byte offset into the probe's spool, if known.
    pub byte_offset: Option<u64>,
    /// Number of bytes dropped when the frame was capped at
    /// [`MAX_FRAME_BYTES`]. Zero when the frame fits.
    pub truncated_bytes: u32,
}

impl SourceFrame {
    /// Construct a frame for a probe.
    ///
    /// If `text` exceeds [`MAX_FRAME_BYTES`] it is truncated at the
    /// last UTF-8 character boundary that fits, and `truncated_bytes`
    /// reflects the difference.
    #[must_use]
    pub fn new(probe_id: ProbeId, stream: SourceStream, text: String) -> Self {
        let (text, truncated_bytes) = cap_text(text);
        Self {
            frame_id: FrameId::new(),
            probe_id,
            stream,
            timestamp: OffsetDateTime::now_utc(),
            text,
            line: None,
            byte_offset: None,
            truncated_bytes,
        }
    }

    /// Builder: attach a 1-based line number.
    #[must_use]
    pub const fn with_line(mut self, line: u64) -> Self {
        self.line = Some(line);
        self
    }

    /// Builder: attach a byte offset.
    #[must_use]
    pub const fn with_byte_offset(mut self, off: u64) -> Self {
        self.byte_offset = Some(off);
        self
    }
}

/// Truncate `text` at the last UTF-8 boundary that fits in
/// [`MAX_FRAME_BYTES`] bytes; return the kept text and the number of
/// bytes dropped.
fn cap_text(text: String) -> (String, u32) {
    if text.len() <= MAX_FRAME_BYTES {
        return (text, 0);
    }
    let original_len = text.len();
    // Find the last char boundary at or before MAX_FRAME_BYTES.
    let mut end = MAX_FRAME_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut kept = text;
    kept.truncate(end);
    let dropped = u32::try_from(original_len - end).unwrap_or(u32::MAX);
    (kept, dropped)
}

/// Configuration for a single context ring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextRingConfig {
    /// Maximum number of frames retained. FIFO eviction on overflow.
    pub max_frames: usize,
    /// Maximum total payload bytes (sum of `text.len()`).
    pub max_bytes: usize,
}

impl Default for ContextRingConfig {
    fn default() -> Self {
        Self {
            max_frames: DEFAULT_RING_FRAMES,
            max_bytes: DEFAULT_RING_BYTES,
        }
    }
}

/// Request shape for [`ContextRingManager::window`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextWindowRequest {
    pub probe_id: ProbeId,
    /// Anchor frame (the source pointer that an event referenced).
    pub anchor: FrameId,
    /// Number of frames to include BEFORE the anchor.
    pub before: u32,
    /// Number of frames to include AFTER the anchor (the anchor
    /// itself is always included when present).
    pub after: u32,
    /// Optional caller-side byte cap for the response.
    pub max_bytes: Option<usize>,
}

/// Response shape for a context window.
///
/// Note: four boolean flags (`anchor_missing`, `truncated_before`,
/// `truncated_after`, `truncated_bytes`, `truncated_frames`) trip
/// `clippy::struct_excessive_bools`. They are distinct, independent
/// truncation signals required by the contract; collapsing them into
/// a bitflag enum would obscure the wire shape that
/// `tests/fixtures/contracts/event-context-response.v1.json`
/// documents. Allowed locally.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextWindowResponse {
    pub probe_id: ProbeId,
    pub anchor: FrameId,
    /// Frames in chronological order. Includes the anchor when
    /// present.
    pub frames: Vec<ContextLine>,
    /// True when the anchor frame was not found (anchor either
    /// evicted or never appended).
    pub anchor_missing: bool,
    /// True when the requested `before` window could not be filled
    /// (head of the ring reached or anchor was near the head).
    pub truncated_before: bool,
    /// True when the requested `after` window could not be filled
    /// (tail of the ring reached).
    pub truncated_after: bool,
    /// True when the response was capped by `max_bytes` or by the
    /// hard [`MAX_WINDOW_BYTES`] cap.
    pub truncated_bytes: bool,
    /// True when the response was capped by [`MAX_WINDOW_FRAMES`].
    pub truncated_frames: bool,
    /// Number of frames evicted from this ring since creation. Lets
    /// callers detect that history before the ring's current head is
    /// not retrievable.
    pub evicted_frames: u64,
}

/// A frame line, shaped for client consumption.
///
/// Mirrors the contract fixture `event-context-response.v1.json`
/// frame shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextLine {
    pub frame_id: FrameId,
    pub stream: SourceStream,
    pub timestamp: OffsetDateTime,
    pub text: String,
    pub line: Option<u64>,
    pub truncated_bytes: u32,
}

impl From<&SourceFrame> for ContextLine {
    fn from(f: &SourceFrame) -> Self {
        Self {
            frame_id: f.frame_id,
            stream: f.stream.clone(),
            timestamp: f.timestamp,
            text: f.text.clone(),
            line: f.line,
            truncated_bytes: f.truncated_bytes,
        }
    }
}

/// Errors emitted by the context-ring manager.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContextError {
    #[error("context ring for probe '{0}' already exists")]
    AlreadyExists(ProbeId),
    #[error("no context ring for probe '{0}'")]
    NotFound(ProbeId),
}

/// Per-probe inner state.
#[derive(Debug)]
struct RingInner {
    config: ContextRingConfig,
    frames: VecDeque<SourceFrame>,
    total_bytes: usize,
    evicted_frames: u64,
}

impl RingInner {
    fn new(config: ContextRingConfig) -> Self {
        Self {
            config,
            frames: VecDeque::with_capacity(64),
            total_bytes: 0,
            evicted_frames: 0,
        }
    }

    fn push(&mut self, frame: SourceFrame) {
        self.total_bytes = self.total_bytes.saturating_add(frame.text.len());
        self.frames.push_back(frame);
        self.evict_until_in_bounds();
    }

    fn evict_until_in_bounds(&mut self) {
        while self.frames.len() > self.config.max_frames || self.total_bytes > self.config.max_bytes
        {
            let Some(front) = self.frames.pop_front() else {
                break;
            };
            self.total_bytes = self.total_bytes.saturating_sub(front.text.len());
            self.evicted_frames = self.evicted_frames.saturating_add(1);
        }
    }

    /// Find the index of `anchor` in the ring, if present.
    fn index_of(&self, anchor: FrameId) -> Option<usize> {
        self.frames.iter().position(|f| f.frame_id == anchor)
    }

    /// Compute the window, returning the response shape. Honors
    /// caller-provided `max_bytes` and the hard [`MAX_WINDOW_BYTES`]
    /// and [`MAX_WINDOW_FRAMES`] caps.
    fn window(&self, req: &ContextWindowRequest) -> ContextWindowResponse {
        let Some(anchor_idx) = self.index_of(req.anchor) else {
            return ContextWindowResponse {
                probe_id: req.probe_id,
                anchor: req.anchor,
                frames: Vec::new(),
                anchor_missing: true,
                truncated_before: false,
                truncated_after: false,
                truncated_bytes: false,
                truncated_frames: false,
                evicted_frames: self.evicted_frames,
            };
        };

        // Desired index range [start, end] inclusive in ring order.
        let before = req.before as usize;
        let after = req.after as usize;
        let want_start = anchor_idx.saturating_sub(before);
        let want_end = anchor_idx.saturating_add(after);
        let truncated_before =
            want_start > anchor_idx.saturating_sub(before) || anchor_idx < before; // before requested more than available
        let truncated_after_initial = want_end >= self.frames.len();
        let end = want_end.min(self.frames.len().saturating_sub(1));

        let cap_bytes = req
            .max_bytes
            .unwrap_or(MAX_WINDOW_BYTES)
            .min(MAX_WINDOW_BYTES);

        let mut out: Vec<ContextLine> = Vec::new();
        let mut bytes_used: usize = 0;
        let mut truncated_bytes = false;
        let mut truncated_frames = false;
        for i in want_start..=end {
            let frame = &self.frames[i];
            if out.len() >= MAX_WINDOW_FRAMES {
                truncated_frames = true;
                break;
            }
            let frame_bytes = frame.text.len();
            if bytes_used.saturating_add(frame_bytes) > cap_bytes && !out.is_empty() {
                truncated_bytes = true;
                break;
            }
            bytes_used = bytes_used.saturating_add(frame_bytes);
            out.push(ContextLine::from(frame));
        }

        ContextWindowResponse {
            probe_id: req.probe_id,
            anchor: req.anchor,
            frames: out,
            anchor_missing: false,
            truncated_before,
            truncated_after: truncated_after_initial,
            truncated_bytes,
            truncated_frames,
            evicted_frames: self.evicted_frames,
        }
    }
}

/// Per-probe context ring manager.
///
/// Holds one [`RingInner`] per `ProbeId`. Concurrency: an outer
/// `RwLock` over the `HashMap`; per-ring `RwLock` for append vs read.
#[derive(Debug, Default)]
pub struct ContextRingManager {
    inner: RwLock<HashMap<ProbeId, Arc<RwLock<RingInner>>>>,
}

impl ContextRingManager {
    /// Construct an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a ring for a probe with the given config.
    pub fn create_ring(
        &self,
        probe_id: ProbeId,
        config: ContextRingConfig,
    ) -> Result<(), ContextError> {
        use std::collections::hash_map::Entry;
        let mut map = self.inner.write();
        match map.entry(probe_id) {
            Entry::Occupied(_) => Err(ContextError::AlreadyExists(probe_id)),
            Entry::Vacant(v) => {
                v.insert(Arc::new(RwLock::new(RingInner::new(config))));
                Ok(())
            }
        }
    }

    /// Create a ring with default config.
    pub fn create_ring_default(&self, probe_id: ProbeId) -> Result<(), ContextError> {
        self.create_ring(probe_id, ContextRingConfig::default())
    }

    /// Drop a ring. Idempotent.
    pub fn drop_ring(&self, probe_id: ProbeId) -> bool {
        self.inner.write().remove(&probe_id).is_some()
    }

    fn cell(&self, probe_id: ProbeId) -> Result<Arc<RwLock<RingInner>>, ContextError> {
        self.inner
            .read()
            .get(&probe_id)
            .cloned()
            .ok_or(ContextError::NotFound(probe_id))
    }

    /// Append a frame to the probe's ring.
    pub fn append_frame(
        &self,
        probe_id: ProbeId,
        frame: SourceFrame,
    ) -> Result<FrameId, ContextError> {
        let id = frame.frame_id;
        let cell = self.cell(probe_id)?;
        let mut inner = cell.write();
        inner.push(frame);
        Ok(id)
    }

    /// Compute a bounded window around an anchor frame.
    ///
    /// Returns a structured "anchor missing" response (not an error)
    /// when the anchor was evicted or never present.
    pub fn window(
        &self,
        request: &ContextWindowRequest,
    ) -> Result<ContextWindowResponse, ContextError> {
        let cell = self.cell(request.probe_id)?;
        let inner = cell.read();
        Ok(inner.window(request))
    }

    /// Resolve a [`SourcePointer`] into a context window. Convenience
    /// for callers that have the pointer from a [`crate::SignalEvent`].
    ///
    /// Returns `NotFound` only when the probe ring itself is missing;
    /// "anchor missing" inside an existing ring is signaled via the
    /// `anchor_missing` flag on the response.
    pub fn window_around_pointer(
        &self,
        probe_id: ProbeId,
        pointer: &SourcePointer,
        before: u32,
        after: u32,
        max_bytes: Option<usize>,
    ) -> Result<ContextWindowResponse, ContextError> {
        let req = ContextWindowRequest {
            probe_id,
            anchor: pointer.frame_id,
            before,
            after,
            max_bytes,
        };
        self.window(&req)
    }

    /// Whether a ring exists for the probe.
    #[must_use]
    pub fn has_ring(&self, probe_id: ProbeId) -> bool {
        self.inner.read().contains_key(&probe_id)
    }

    /// Current frame count for the probe (0 if no ring).
    #[must_use]
    pub fn frame_count(&self, probe_id: ProbeId) -> usize {
        self.inner
            .read()
            .get(&probe_id)
            .map_or(0, |c| c.read().frames.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{FrameId, ProbeId};

    fn frame(probe_id: ProbeId, text: &str, line: u64) -> SourceFrame {
        SourceFrame::new(probe_id, SourceStream::Stderr, text.to_owned()).with_line(line)
    }

    #[test]
    fn context_append_and_exact_window() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        let f0 = frame(pid, "line 0", 1);
        let id_0 = f0.frame_id;
        let f1 = frame(pid, "line 1", 2);
        let id_1 = f1.frame_id;
        let f2 = frame(pid, "line 2", 3);
        let id_2 = f2.frame_id;
        let f3 = frame(pid, "line 3", 4);
        let id_3 = f3.frame_id;
        let f4 = frame(pid, "line 4", 5);
        let id_4 = f4.frame_id;
        for f in [f0, f1, f2, f3, f4] {
            mgr.append_frame(pid, f).unwrap();
        }
        let req = ContextWindowRequest {
            probe_id: pid,
            anchor: id_2,
            before: 1,
            after: 1,
            max_bytes: None,
        };
        let resp = mgr.window(&req).unwrap();
        assert!(!resp.anchor_missing);
        assert!(!resp.truncated_before);
        assert!(!resp.truncated_after);
        assert_eq!(resp.frames.len(), 3);
        assert_eq!(resp.frames[0].frame_id, id_1);
        assert_eq!(resp.frames[1].frame_id, id_2);
        assert_eq!(resp.frames[2].frame_id, id_3);
        let _ = (id_0, id_4); // pinned to avoid unused warnings
    }

    #[test]
    fn context_truncated_before_at_ring_head() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        let mut ids = Vec::new();
        for i in 0..3u64 {
            let f = frame(pid, &format!("l{i}"), i + 1);
            ids.push(f.frame_id);
            mgr.append_frame(pid, f).unwrap();
        }
        // Anchor at head; requesting 3 frames before should report truncation.
        let req = ContextWindowRequest {
            probe_id: pid,
            anchor: ids[0],
            before: 3,
            after: 0,
            max_bytes: None,
        };
        let resp = mgr.window(&req).unwrap();
        assert!(resp.truncated_before);
        assert!(!resp.truncated_after);
        assert_eq!(resp.frames.len(), 1);
        assert_eq!(resp.frames[0].frame_id, ids[0]);
    }

    #[test]
    fn context_truncated_after_at_ring_tail() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        let mut ids = Vec::new();
        for i in 0..3u64 {
            let f = frame(pid, &format!("l{i}"), i + 1);
            ids.push(f.frame_id);
            mgr.append_frame(pid, f).unwrap();
        }
        let req = ContextWindowRequest {
            probe_id: pid,
            anchor: ids[2],
            before: 0,
            after: 5,
            max_bytes: None,
        };
        let resp = mgr.window(&req).unwrap();
        assert!(resp.truncated_after);
        assert!(!resp.truncated_before);
        assert_eq!(resp.frames.len(), 1);
        assert_eq!(resp.frames[0].frame_id, ids[2]);
    }

    #[test]
    fn context_anchor_missing_is_structured() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        for i in 0..3u64 {
            mgr.append_frame(pid, frame(pid, "x", i + 1)).unwrap();
        }
        let resp = mgr
            .window(&ContextWindowRequest {
                probe_id: pid,
                anchor: FrameId::new(),
                before: 2,
                after: 2,
                max_bytes: None,
            })
            .unwrap();
        assert!(resp.anchor_missing);
        assert!(resp.frames.is_empty());
        assert!(!resp.truncated_before);
        assert!(!resp.truncated_after);
    }

    #[test]
    fn context_missing_probe_errors() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        let err = mgr
            .window(&ContextWindowRequest {
                probe_id: pid,
                anchor: FrameId::new(),
                before: 1,
                after: 1,
                max_bytes: None,
            })
            .unwrap_err();
        assert!(matches!(err, ContextError::NotFound(_)));
    }

    #[test]
    fn context_capacity_evicts_oldest_and_bumps_evicted_frames() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring(
            pid,
            ContextRingConfig {
                max_frames: 3,
                max_bytes: usize::MAX,
            },
        )
        .unwrap();
        let mut ids = Vec::new();
        for i in 0..5u64 {
            let f = frame(pid, &format!("l{i}"), i + 1);
            ids.push(f.frame_id);
            mgr.append_frame(pid, f).unwrap();
        }
        // ids[0] and ids[1] were evicted.
        let req = ContextWindowRequest {
            probe_id: pid,
            anchor: ids[0],
            before: 0,
            after: 0,
            max_bytes: None,
        };
        let resp = mgr.window(&req).unwrap();
        assert!(resp.anchor_missing);
        assert_eq!(resp.evicted_frames, 2);
    }

    #[test]
    fn context_byte_cap_truncates_payload() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        // Five small frames; cap response to ~ 1 frame worth of bytes.
        let texts = ["aaaa", "bbbb", "cccc", "dddd", "eeee"];
        let mut ids = Vec::new();
        for t in texts {
            let f = frame(pid, t, 1);
            ids.push(f.frame_id);
            mgr.append_frame(pid, f).unwrap();
        }
        let req = ContextWindowRequest {
            probe_id: pid,
            anchor: ids[2],
            before: 2,
            after: 2,
            max_bytes: Some(5),
        };
        let resp = mgr.window(&req).unwrap();
        // First frame always fits even if oversize alone; subsequent
        // ones are truncated when bytes_used would exceed cap.
        assert!(resp.truncated_bytes);
        assert!(!resp.frames.is_empty());
    }

    #[test]
    fn context_long_line_is_capped_at_append() {
        let pid = ProbeId::new();
        let big = "x".repeat(MAX_FRAME_BYTES + 1234);
        let f = SourceFrame::new(pid, SourceStream::Stdout, big);
        assert_eq!(f.text.len(), MAX_FRAME_BYTES);
        assert_eq!(f.truncated_bytes, 1234);
    }

    #[test]
    fn context_long_line_at_unicode_boundary() {
        let pid = ProbeId::new();
        // Build a string that places a 4-byte char straddling the cap.
        let chunk = "a".repeat(MAX_FRAME_BYTES - 2); // ASCII
        let mut s = String::new();
        s.push_str(&chunk);
        s.push('\u{1F600}'); // 4 bytes (smiley)
        s.push_str("trailing");
        let f = SourceFrame::new(pid, SourceStream::Stdout, s);
        // The truncated text must be valid UTF-8.
        assert!(f.text.is_char_boundary(f.text.len()));
        assert!(f.text.len() <= MAX_FRAME_BYTES);
        assert!(f.truncated_bytes > 0);
    }

    #[test]
    fn context_around_pointer_convenience() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        let f = frame(pid, "the line", 42);
        let fid = f.frame_id;
        mgr.append_frame(pid, f).unwrap();
        let ptr = SourcePointer::new(fid).with_line(42);
        let resp = mgr.window_around_pointer(pid, &ptr, 0, 0, None).unwrap();
        assert!(!resp.anchor_missing);
        assert_eq!(resp.frames.len(), 1);
        assert_eq!(resp.frames[0].line, Some(42));
    }

    #[test]
    fn context_drop_ring_is_idempotent() {
        let mgr = ContextRingManager::new();
        let pid = ProbeId::new();
        mgr.create_ring_default(pid).unwrap();
        assert!(mgr.drop_ring(pid));
        assert!(!mgr.drop_ring(pid));
    }

    #[test]
    fn context_send_sync() {
        fn assert_ss<T: Send + Sync>() {}
        assert_ss::<ContextRingManager>();
    }
}
