// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! [`SignalEvent`] is the canonical wire shape for a single signal
//! event produced by a probe-sifter pair.
//!
//! Wire shape: `tests/fixtures/contracts/event.signal.v1.json`.
//!
//! Source-status: live (TC06). Persistence lands in TC12; sifter
//! emission in TC10/TC11; probes in TC15-TC20.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::serde::rfc3339;

use crate::error::CoreError;
use crate::ids::{BucketId, EventId, RuleId};
use crate::pointer::SourcePointer;
use crate::severity::Severity;
use crate::source::EventSource;

/// Insertion-ordered string-to-string capture map.
///
/// Sifter rules emit named captures (e.g. `package` -> `libssl-dev`).
/// `IndexMap` preserves regex named-group order so summary templates
/// render deterministically.
pub type Captures = IndexMap<String, String>;

/// Reference to the rule that produced an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRef {
    pub id: RuleId,
    pub version: u32,
}

/// Canonical signal event. One row in a bucket.
///
/// # Invariants
///
/// Every event with `severity >= Severity::Medium` MUST carry either
/// a `pointer` OR a `pointer_unavailable_reason`. [`Self::validate`]
/// enforces this; use it on every freshly built `SignalEvent` before
/// emission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalEvent {
    pub event_id: EventId,
    pub bucket_id: BucketId,
    /// Monotonic per-bucket sequence number. Persists as SQLite
    /// INTEGER (signed 64-bit). The conversion site (u64 -> i64) is
    /// in `terminal-commander-store` (TC12) and asserts no value
    /// crosses `i64::MAX` (an unreachable bound at MVP rates).
    pub seq: u64,
    /// Event timestamp in RFC 3339 form on the wire.
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub severity: Severity,
    /// Open-string kind (see `docs/contracts/enums/event-kind.md`).
    pub kind: String,
    /// Short, human-readable summary. Single-line.
    pub summary: String,
    /// Optional rule reference (sifter-produced events carry this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<RuleRef>,
    pub source: EventSource,
    /// Optional named captures from the rule. Empty map serializes
    /// as `{}` if present; absent map elides the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captures: Option<Captures>,
    /// Bounded source pointer back to the originating frames. MUST
    /// be present for `severity >= Medium` unless
    /// `pointer_unavailable_reason` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<SourcePointer>,
    /// Explanation when no pointer can be produced (e.g. the event
    /// was synthesized from a probe-lifecycle marker, not a stream
    /// frame). Required by TC02 invariant for high-severity events
    /// that lack a pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_unavailable_reason: Option<String>,
    /// Optional free-form tags for downstream filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl SignalEvent {
    /// Validate the TC02 invariant: every event with severity >=
    /// `Medium` must have either `pointer` or
    /// `pointer_unavailable_reason`. Returns `Err` otherwise.
    pub fn validate(&self) -> crate::Result<()> {
        if self.severity >= Severity::Medium
            && self.pointer.is_none()
            && self.pointer_unavailable_reason.is_none()
        {
            return Err(CoreError::PointerInvariantViolation {
                event_id: self.event_id.to_wire_string(),
                severity: self.severity.as_str().to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{BucketId, EventId, FrameId, ProbeId};
    use crate::pointer::SourcePointer;
    use crate::source::{SourceStream, SourceType};
    use time::macros::datetime;

    fn fixture_event_with_pointer() -> SignalEvent {
        let mut caps = Captures::new();
        caps.insert("package".to_owned(), "libssl-dev".to_owned());
        SignalEvent {
            event_id: EventId::new(),
            bucket_id: BucketId::new(),
            seq: 1842,
            timestamp: datetime!(2026-05-20 20:11:34.218 +02:00),
            severity: Severity::High,
            kind: "missing_package".to_owned(),
            summary: "APT could not locate package libssl-dev".to_owned(),
            rule: None,
            source: EventSource {
                probe_id: ProbeId::new(),
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(318)),
            pointer_unavailable_reason: None,
            tags: Some(vec!["packaging".to_owned(), "apt".to_owned()]),
        }
    }

    #[test]
    fn round_trip_signal_event() {
        let ev = fixture_event_with_pointer();
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"kind\":\"missing_package\""));
        assert!(json.contains("\"severity\":\"high\""));
        let back: SignalEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn high_severity_without_pointer_or_reason_fails_validate() {
        let mut ev = fixture_event_with_pointer();
        ev.pointer = None;
        ev.pointer_unavailable_reason = None;
        match ev.validate().unwrap_err() {
            CoreError::PointerInvariantViolation { severity, .. } => {
                assert_eq!(severity, "high");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn high_severity_with_reason_only_passes_validate() {
        let mut ev = fixture_event_with_pointer();
        ev.pointer = None;
        ev.pointer_unavailable_reason = Some("synthesized from probe lifecycle marker".to_owned());
        ev.validate().unwrap();
    }

    #[test]
    fn low_severity_without_pointer_passes_validate() {
        let mut ev = fixture_event_with_pointer();
        ev.severity = Severity::Low;
        ev.pointer = None;
        ev.pointer_unavailable_reason = None;
        ev.validate().unwrap();
    }

    #[test]
    fn captures_preserve_insertion_order() {
        let mut caps = Captures::new();
        caps.insert("first".to_owned(), "1".to_owned());
        caps.insert("second".to_owned(), "2".to_owned());
        caps.insert("third".to_owned(), "3".to_owned());
        let keys: Vec<_> = caps.keys().cloned().collect();
        assert_eq!(keys, vec!["first", "second", "third"]);
        let json = serde_json::to_string(&caps).unwrap();
        // IndexMap with serde feature preserves order in JSON output.
        assert!(json.find("first").unwrap() < json.find("second").unwrap());
        assert!(json.find("second").unwrap() < json.find("third").unwrap());
    }
}
