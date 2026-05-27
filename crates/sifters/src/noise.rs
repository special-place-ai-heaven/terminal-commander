// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Noise suppression, dedupe, and progress classification (TC11).
//!
//! Three responsibilities:
//!
//! 1. [`Dedupe`]: collapse repeated identical events within a
//!    configurable window. Identity is keyed by
//!    `rule_id + canonical(captures)`.
//!
//! 2. [`ProgressDetector`]: classify spinner / percentage / status-
//!    only frames as progress noise so the sifter runtime can skip
//!    them or downgrade their severity.
//!
//! 3. [`NoisePolicy`]: per-runtime policy bundle (severity floor for
//!    suppression, dedupe window, etc.).
//!
//! Source-status: live (TC11). Persistent recurrence aggregation
//! across daemon restarts is deferred to TC12 storage; the
//! columns `count`, `first_seen`, `last_seen`, and `suppressed`
//! are reserved on `SignalEvent` so the wire-form round-trips.

use std::collections::HashMap;
use std::time::Duration;

use time::OffsetDateTime;

use terminal_commander_core::{EventDraft, RuleRef, Severity};

/// Default dedupe window. Repeated identical events within this
/// window are collapsed.
pub const DEFAULT_DEDUPE_WINDOW: Duration = Duration::from_secs(5);

/// Policy bundle controlling noise behavior across a runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoisePolicy {
    /// Severity strictly below this is eligible for dedupe collapse.
    /// `Severity::High` means low/medium/info/debug/trace events
    /// collapse but High/Critical never do.
    pub dedupe_severity_max: Severity,
    /// Window over which identical events collapse.
    pub dedupe_window: Duration,
    /// Whether to classify and drop progress-noise frames before
    /// they reach the sifter runtime.
    pub drop_progress_frames: bool,
}

impl Default for NoisePolicy {
    fn default() -> Self {
        Self {
            dedupe_severity_max: Severity::High,
            dedupe_window: DEFAULT_DEDUPE_WINDOW,
            drop_progress_frames: true,
        }
    }
}

/// State carried between calls to dedupe.
#[derive(Debug, Default)]
pub struct Dedupe {
    /// Map from canonical key -> (first_seen, last_seen, count,
    /// representative event id within the current window).
    state: HashMap<String, DedupeEntry>,
}

#[derive(Debug, Clone)]
struct DedupeEntry {
    first_seen: OffsetDateTime,
    last_seen: OffsetDateTime,
    count: u32,
    representative_index: usize,
}

impl Dedupe {
    /// Construct an empty dedupe state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply dedupe to a vector of drafts under the given policy and
    /// return the kept set. Drafts with severity strictly less than
    /// the policy's `dedupe_severity_max` may collapse; events at or
    /// above never collapse.
    ///
    /// The first occurrence of a key is kept; later occurrences
    /// within the window update the representative's `count`,
    /// `first_seen`, and `last_seen` and are dropped from the output.
    #[must_use]
    pub fn apply(&mut self, drafts: Vec<EventDraft>, policy: &NoisePolicy) -> Vec<EventDraft> {
        self.apply_at(drafts, policy, OffsetDateTime::now_utc())
    }

    /// [`apply`](Self::apply) with an injected clock so the GC cutoff is
    /// deterministic in tests (which build drafts with fixed timestamps).
    /// Production calls `apply`, which passes `OffsetDateTime::now_utc()`.
    #[must_use]
    pub fn apply_at(
        &mut self,
        drafts: Vec<EventDraft>,
        policy: &NoisePolicy,
        now: OffsetDateTime,
    ) -> Vec<EventDraft> {
        let window_secs = i64::try_from(policy.dedupe_window.as_secs()).unwrap_or(i64::MAX);
        let cutoff = now - time::Duration::seconds(window_secs);

        // Garbage-collect expired entries.
        self.state.retain(|_, e| e.last_seen >= cutoff);

        let mut kept: Vec<EventDraft> = Vec::with_capacity(drafts.len());
        for mut d in drafts {
            if d.severity >= policy.dedupe_severity_max {
                // High/Critical: never collapse.
                kept.push(d);
                continue;
            }
            let key = dedupe_key(d.rule.as_ref(), d.captures.as_ref(), &d.kind);
            if let Some(entry) = self.state.get_mut(&key) {
                entry.count = entry.count.saturating_add(1);
                entry.last_seen = d.timestamp;
                // Update the representative draft IN-PLACE (it is
                // already in `kept`).
                if let Some(rep) = kept.get_mut(entry.representative_index) {
                    rep.count = entry.count;
                    rep.first_seen = Some(entry.first_seen);
                    rep.last_seen = Some(entry.last_seen);
                }
                // Drop the duplicate.
                continue;
            }
            // First occurrence in window.
            let representative_index = kept.len();
            self.state.insert(
                key,
                DedupeEntry {
                    first_seen: d.timestamp,
                    last_seen: d.timestamp,
                    count: 1,
                    representative_index,
                },
            );
            d.count = 1;
            d.first_seen = Some(d.timestamp);
            d.last_seen = Some(d.timestamp);
            kept.push(d);
        }
        kept
    }
}

/// Canonical dedupe key: `<rule_id>|<kind>|<sorted captures>`.
fn dedupe_key(
    rule: Option<&RuleRef>,
    captures: Option<&terminal_commander_core::Captures>,
    kind: &str,
) -> String {
    let mut out = String::with_capacity(64);
    if let Some(r) = rule {
        out.push_str(&r.id.to_wire_string());
        out.push('|');
        out.push_str(&r.version.to_string());
    } else {
        out.push_str("<no-rule>");
    }
    out.push('|');
    out.push_str(kind);
    out.push('|');
    if let Some(caps) = captures {
        let mut keys: Vec<&String> = caps.keys().collect();
        keys.sort();
        for k in keys {
            out.push_str(k);
            out.push('=');
            if let Some(v) = caps.get(k) {
                out.push_str(v);
            }
            out.push(';');
        }
    }
    out
}

/// Detector for progress-like frames.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProgressDetector;

impl ProgressDetector {
    /// Classify a frame's text as a progress line. The detector
    /// recognizes:
    /// - lines that are ONLY a percentage (e.g. " 45%"),
    /// - lines that are ONLY a spinner character (e.g. "|", "/",
    ///   "-", "\\", "*"),
    /// - lines that ONLY change a numeric counter (e.g.
    ///   "Compiling ... 17/120"),
    /// - lines that are ONLY whitespace + Carriage Return repeats.
    ///
    /// Returns `true` when the text is judged progress noise.
    #[must_use]
    pub fn is_progress_line(text: &str) -> bool {
        let t = text.trim();
        if t.is_empty() {
            return true;
        }
        // Percentage only (e.g. "45%", "100%", " 5.0%").
        if t.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '%' || c.is_whitespace())
            && t.contains('%')
        {
            return true;
        }
        // Spinner characters.
        if t.len() == 1 && "|/-\\*".contains(t) {
            return true;
        }
        // "n/m" counter (e.g. "17/120").
        if let Some((a, b)) = t.split_once('/')
            && !a.is_empty()
            && !b.is_empty()
            && a.chars().all(|c| c.is_ascii_digit())
            && b.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
        // "Compiling foo (..)" cargo lines: prefix + paren is too
        // noisy to detect generically without false positives; we
        // do NOT flag these here. The dedupe layer handles their
        // repeated emission.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use terminal_commander_core::{
        BucketId, EventDraft, EventSource, FrameId, ProbeId, RuleId, RuleRef, SourcePointer,
        SourceStream, SourceType,
    };

    fn draft(
        rule_id: RuleId,
        severity: Severity,
        captures: &[(&str, &str)],
        ts: OffsetDateTime,
    ) -> EventDraft {
        let mut caps = IndexMap::new();
        for (k, v) in captures {
            caps.insert((*k).to_owned(), (*v).to_owned());
        }
        EventDraft {
            bucket_id: BucketId::new(),
            timestamp: ts,
            severity,
            kind: "k".to_owned(),
            summary: "s".to_owned(),
            rule: Some(RuleRef {
                id: rule_id,
                version: 1,
            }),
            source: EventSource {
                probe_id: ProbeId::new(),
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        }
    }

    #[test]
    fn dedupe_collapses_repeats_below_severity_floor() {
        let mut d = Dedupe::new();
        let pol = NoisePolicy::default();
        let rid = RuleId::new();
        let ts = OffsetDateTime::now_utc();
        let v = vec![
            draft(rid, Severity::Low, &[("pkg", "a")], ts),
            draft(rid, Severity::Low, &[("pkg", "a")], ts),
            draft(rid, Severity::Low, &[("pkg", "a")], ts),
        ];
        let out = d.apply(v, &pol);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].count, 3);
        assert!(out[0].first_seen.is_some());
        assert!(out[0].last_seen.is_some());
    }

    #[test]
    fn dedupe_keeps_distinct_captures_separate() {
        let mut d = Dedupe::new();
        let pol = NoisePolicy::default();
        let rid = RuleId::new();
        let ts = OffsetDateTime::now_utc();
        let v = vec![
            draft(rid, Severity::Low, &[("pkg", "a")], ts),
            draft(rid, Severity::Low, &[("pkg", "b")], ts),
            draft(rid, Severity::Low, &[("pkg", "a")], ts),
        ];
        let out = d.apply(v, &pol);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn dedupe_keeps_high_and_critical_distinct() {
        let mut d = Dedupe::new();
        let pol = NoisePolicy::default();
        let rid = RuleId::new();
        let ts = OffsetDateTime::now_utc();
        let v = vec![
            draft(rid, Severity::High, &[("pkg", "a")], ts),
            draft(rid, Severity::High, &[("pkg", "a")], ts),
            draft(rid, Severity::Critical, &[("pkg", "a")], ts),
        ];
        let out = d.apply(v, &pol);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn dedupe_keeps_different_rules_separate() {
        let mut d = Dedupe::new();
        let pol = NoisePolicy::default();
        let ts = OffsetDateTime::now_utc();
        let v = vec![
            draft(RuleId::new(), Severity::Low, &[("k", "x")], ts),
            draft(RuleId::new(), Severity::Low, &[("k", "x")], ts),
        ];
        let out = d.apply(v, &pol);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn progress_detector_recognizes_percentage_only() {
        assert!(ProgressDetector::is_progress_line(" 45%"));
        assert!(ProgressDetector::is_progress_line("100% "));
        assert!(!ProgressDetector::is_progress_line("error: 45% chance"));
    }

    #[test]
    fn progress_detector_recognizes_spinner_only() {
        for ch in ["|", "/", "-", "\\", "*"] {
            assert!(ProgressDetector::is_progress_line(ch), "spinner {ch}");
        }
        assert!(!ProgressDetector::is_progress_line("//"));
    }

    #[test]
    fn progress_detector_recognizes_counter_only() {
        assert!(ProgressDetector::is_progress_line("17/120"));
        assert!(ProgressDetector::is_progress_line(" 1/3 "));
        assert!(!ProgressDetector::is_progress_line("a/b"));
    }

    #[test]
    fn progress_detector_does_not_flag_real_messages() {
        assert!(!ProgressDetector::is_progress_line("Compiling foo v0.1"));
        assert!(!ProgressDetector::is_progress_line(
            "warning: unused variable"
        ));
    }
}
