// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! [`SourcePointer`] gives a bounded handle back to the underlying
//! stream frames that produced a [`SignalEvent`].
//!
//! Wire shape lives in `tests/fixtures/contracts/source-pointer.v1.json`.
//!
//! Source-status: live (TC06). Concrete `event_context` retrieval
//! lands in TC08.
//!
//! [`SignalEvent`]: crate::event::SignalEvent

use serde::{Deserialize, Serialize};

use crate::ids::FrameId;
use crate::source::SourceStream;

/// A bounded back-pointer to the frame that produced an event.
///
/// The TC02 invariant requires every signal event with severity
/// >= `Medium` to carry either a `SourcePointer` or a
/// `pointer_unavailable_reason` (see [`SignalEvent`]). The pointer
/// itself is small; raw frame text is retrieved on demand via the
/// `event_context` MCP tool (TC08).
///
/// [`SignalEvent`]: crate::event::SignalEvent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePointer {
    /// Frame identifier inside the probe's context ring.
    pub frame_id: FrameId,
    /// Best-effort line number within the probe's stream. Lines are
    /// 1-based.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Optional byte offsets into the probe's spool, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_end: Option<u64>,
    /// Stream the frame came from (stdout/stderr/...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<SourceStream>,
    /// Whether `event_context(event_id)` is still expected to
    /// succeed for this pointer. False if the context ring has
    /// evicted the relevant frames.
    pub context_available: bool,
}

impl SourcePointer {
    /// Construct a pointer for a frame, defaulting line/byte
    /// information to `None` and context availability to `true`.
    #[must_use]
    pub const fn new(frame_id: FrameId) -> Self {
        Self {
            frame_id,
            line: None,
            byte_start: None,
            byte_end: None,
            stream: None,
            context_available: true,
        }
    }

    /// Set the 1-based line number.
    #[must_use]
    pub const fn with_line(mut self, line: u64) -> Self {
        self.line = Some(line);
        self
    }

    /// Set a stream tag.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // SourceStream is non-Copy.
    pub fn with_stream(mut self, stream: SourceStream) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Mark the context as no longer available (ring evicted the
    /// frames). Used by TC07/TC08 when emitting after eviction.
    #[must_use]
    pub const fn evicted(mut self) -> Self {
        self.context_available = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FrameId;

    #[test]
    fn minimal_pointer_round_trips() {
        let p = SourcePointer::new(FrameId::new());
        let j = serde_json::to_value(&p).unwrap();
        assert!(j["frame_id"].as_str().unwrap().starts_with("frm_"));
        assert_eq!(j["context_available"], true);
        assert!(j.get("line").is_none());
        let back: SourcePointer = serde_json::from_value(j).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn pointer_with_line_and_stream() {
        let p = SourcePointer::new(FrameId::new())
            .with_line(318)
            .with_stream(SourceStream::Stderr);
        assert_eq!(p.line, Some(318));
        assert_eq!(p.stream, Some(SourceStream::Stderr));
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["line"], 318);
        assert_eq!(j["stream"], "stderr");
    }

    #[test]
    fn evicted_pointer_sets_flag() {
        let p = SourcePointer::new(FrameId::new()).evicted();
        assert!(!p.context_available);
    }
}
