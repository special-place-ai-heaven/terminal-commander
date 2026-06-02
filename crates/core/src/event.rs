// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
    /// Number of underlying matches collapsed into this event.
    /// Defaults to `1` (single occurrence; omitted from the wire
    /// form). Set by TC11 dedupe.
    #[serde(default = "one_u32", skip_serializing_if = "is_one_u32")]
    pub count: u32,
    /// First time the underlying pattern was seen in the collapse
    /// window. `None` when this event is the only occurrence.
    #[serde(default, with = "rfc3339_opt", skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<OffsetDateTime>,
    /// Last time the underlying pattern was seen in the collapse
    /// window. `None` when this event is the only occurrence.
    #[serde(default, with = "rfc3339_opt", skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<OffsetDateTime>,
    /// True when this event was emitted in lieu of an emission
    /// blocked by suppression (progress noise classification).
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppressed: bool,
}

const fn one_u32() -> u32 {
    1
}

// Serde requires these helpers to take `&T`.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_one_u32(n: &u32) -> bool {
    *n == 1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(b: &bool) -> bool {
    !*b
}

mod rfc3339_opt {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    // Serde-required signature.
    #[allow(clippy::ref_option, unreachable_pub)]
    pub fn serialize<S: Serializer>(t: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            None => s.serialize_none(),
            Some(t) => s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
        }
    }
    #[allow(unreachable_pub)]
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        let opt = Option::<String>::deserialize(d)?;
        opt.map(|s| OffsetDateTime::parse(&s, &Rfc3339).map_err(serde::de::Error::custom))
            .transpose()
    }
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

/// A signal event without the `event_id` and `seq` fields that the
/// [`crate::bucket::BucketManager`] assigns at `append` time.
///
/// Sifters produce `EventDraft`s; the daemon route that hands them
/// to a bucket converts them into [`SignalEvent`] by minting the
/// `event_id` and letting the manager assign the per-bucket `seq`.
///
/// Source-status: live (TC10). Used by `terminal-commander-sifters`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventDraft {
    pub bucket_id: BucketId,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub severity: Severity,
    pub kind: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<RuleRef>,
    pub source: EventSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captures: Option<Captures>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<crate::pointer::SourcePointer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_unavailable_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Number of bytes truncated from the incoming frame BEFORE
    /// sifter evaluation (defense-in-depth cap; TC10).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub frame_truncated_bytes: u32,
    /// See [`SignalEvent::count`]. Set by TC11 dedupe before
    /// promotion.
    #[serde(default = "one_u32", skip_serializing_if = "is_one_u32")]
    pub count: u32,
    #[serde(default, with = "rfc3339_opt", skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<OffsetDateTime>,
    #[serde(default, with = "rfc3339_opt", skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<OffsetDateTime>,
    /// True when emitted in place of a progress-noise burst.
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppressed: bool,
}

// Signature is dictated by serde's `skip_serializing_if`, which
// requires `fn(&T) -> bool`.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

impl EventDraft {
    /// Promote a draft into a full [`SignalEvent`] by minting an
    /// [`EventId`]. The bucket manager assigns `seq` on `append`.
    #[must_use]
    pub fn into_signal_event(self, seq: u64) -> SignalEvent {
        SignalEvent {
            event_id: EventId::new(),
            bucket_id: self.bucket_id,
            seq,
            timestamp: self.timestamp,
            severity: self.severity,
            kind: self.kind,
            summary: self.summary,
            rule: self.rule,
            source: self.source,
            captures: self.captures,
            pointer: self.pointer,
            pointer_unavailable_reason: self.pointer_unavailable_reason,
            tags: self.tags,
            count: self.count,
            first_seen: self.first_seen,
            last_seen: self.last_seen,
            suppressed: self.suppressed,
        }
    }

    /// Run the TC02 invariant check on a draft (mirrors
    /// [`SignalEvent::validate`]).
    pub fn validate(&self) -> crate::Result<()> {
        if self.severity >= Severity::Medium
            && self.pointer.is_none()
            && self.pointer_unavailable_reason.is_none()
        {
            return Err(CoreError::PointerInvariantViolation {
                event_id: "<draft>".to_owned(),
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
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
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
