// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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

/// In-place bucket update when a cross-frame dedupe collapse occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupeAggregatePatch {
    pub seq: u64,
    pub count: u32,
    pub first_seen: OffsetDateTime,
    pub last_seen: OffsetDateTime,
}

/// Output of [`Dedupe::apply_at`]: drafts to append plus bucket patches.
#[derive(Debug, Default)]
pub struct DedupeApplyResult {
    pub emit: Vec<EventDraft>,
    pub patches: Vec<DedupeAggregatePatch>,
}

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
    /// Bucket seq assigned when the representative was first appended.
    representative_seq: Option<u64>,
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
            .emit
    }

    /// Record the bucket seq of a representative event after append.
    pub fn register_emitted(&mut self, draft: &EventDraft, seq: u64) {
        let key = dedupe_key(draft.rule.as_ref(), draft.captures.as_ref(), &draft.kind);
        if let Some(entry) = self.state.get_mut(&key) {
            entry.representative_seq = Some(seq);
        }
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
    ) -> DedupeApplyResult {
        let window_secs = i64::try_from(policy.dedupe_window.as_secs()).unwrap_or(i64::MAX);
        let cutoff = now - time::Duration::seconds(window_secs);

        // Garbage-collect expired entries.
        self.state.retain(|_, e| e.last_seen >= cutoff);

        let mut emit: Vec<EventDraft> = Vec::with_capacity(drafts.len());
        let mut patches: Vec<DedupeAggregatePatch> = Vec::new();
        // Key -> index of its representative draft IN THIS BATCH's `emit`
        // vec. A `DedupeEntry` persists across `apply_at` calls, so an index
        // captured in a prior batch would address an unrelated draft in this
        // batch's fresh `emit` vec -- the source of a cross-batch corruption
        // where one event was emitted with another's count/timestamps.
        // Tracking the index per batch confines the in-`emit` collapse patch
        // to same-batch representatives; a cross-batch recurrence is
        // reflected only through the persisted `representative_seq` bucket
        // patch below.
        let mut batch_index: HashMap<String, usize> = HashMap::new();
        for mut d in drafts {
            if d.severity >= policy.dedupe_severity_max {
                // High/Critical: never collapse.
                emit.push(d);
                continue;
            }
            let key = dedupe_key(d.rule.as_ref(), d.captures.as_ref(), &d.kind);
            if let Some(entry) = self.state.get_mut(&key) {
                entry.count = entry.count.saturating_add(1);
                entry.last_seen = d.timestamp;
                // Same-batch collapse only: patch the representative draft if
                // it was emitted in THIS batch. A carried-over entry has no
                // batch_index entry, so its prior-batch index is never used
                // to index this batch's emit vec.
                if let Some(&idx) = batch_index.get(&key)
                    && let Some(rep) = emit.get_mut(idx)
                {
                    rep.count = entry.count;
                    rep.first_seen = Some(entry.first_seen);
                    rep.last_seen = Some(entry.last_seen);
                }
                // Patch the already-appended bucket row when its seq is known.
                if let Some(seq) = entry.representative_seq {
                    patches.push(DedupeAggregatePatch {
                        seq,
                        count: entry.count,
                        first_seen: entry.first_seen,
                        last_seen: entry.last_seen,
                    });
                }
                continue;
            }
            // First occurrence in window.
            let representative_index = emit.len();
            batch_index.insert(key.clone(), representative_index);
            self.state.insert(
                key,
                DedupeEntry {
                    first_seen: d.timestamp,
                    last_seen: d.timestamp,
                    count: 1,
                    representative_seq: None,
                },
            );
            d.count = 1;
            d.first_seen = Some(d.timestamp);
            d.last_seen = Some(d.timestamp);
            emit.push(d);
        }
        DedupeApplyResult { emit, patches }
    }
}

/// Returns true when a draft must never pass through dedupe collapse.
#[must_use]
pub fn dedupe_bypass_kind(kind: &str) -> bool {
    kind == "password_prompt"
}

/// Canonical dedupe key:
/// `<rule_id>|<version>|<kind>|<sorted captures>` (or
/// `<no-rule>|<kind>|<sorted captures>` when no rule matched).
///
/// The `version` segment is load-bearing: a rule version bump
/// intentionally splits dedupe buckets so a redefined rule never
/// collapses against events from its prior version. Do not drop it.
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
    fn cross_batch_repeat_does_not_corrupt_an_unrelated_draft() {
        // Regression: a DedupeEntry carried across apply_at calls used to
        // index THIS batch's emit vec with a PRIOR batch's representative
        // index, overwriting an unrelated draft's count/timestamps. Here K1
        // (pkg=a) is the representative from batch A; in batch B a fresh key
        // K2 (pkg=b) lands at emit index 0 and a K1 repeat follows. K1's
        // repeat must NOT touch K2.
        let mut d = Dedupe::new();
        let pol = NoisePolicy::default();
        let rid = RuleId::new();
        let ts = OffsetDateTime::now_utc();

        // Batch A: K1 becomes the representative; register its bucket seq so
        // a cross-batch patch has a target.
        let a = d.apply_at(
            vec![draft(rid, Severity::Low, &[("pkg", "a")], ts)],
            &pol,
            ts,
        );
        assert_eq!(a.emit.len(), 1);
        d.register_emitted(&a.emit[0], 4242);

        // Batch B: a NEW key K2 is the first occurrence (emit index 0),
        // followed by a repeat of K1. With the old stale-index bug, K1's
        // repeat wrote K1's aggregate onto emit[0] == the K2 draft.
        let b = d.apply_at(
            vec![
                draft(rid, Severity::Low, &[("pkg", "b")], ts),
                draft(rid, Severity::Low, &[("pkg", "a")], ts),
            ],
            &pol,
            ts,
        );

        // Only the fresh K2 draft is emitted, and it is UNCORRUPTED: count 1
        // (it must not inherit K1's aggregated count of 2).
        assert_eq!(b.emit.len(), 1, "only the fresh K2 draft is emitted");
        assert_eq!(
            b.emit[0].count, 1,
            "K2 must not inherit K1's aggregated count"
        );
        // K1's cross-batch recurrence is reflected as a patch on K1's own seq.
        assert!(
            b.patches.iter().any(|p| p.seq == 4242 && p.count == 2),
            "K1 recurrence must patch its own bucket row (seq 4242) to count 2"
        );
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
    fn dedupe_bypass_kind_password_prompt() {
        assert!(super::dedupe_bypass_kind("password_prompt"));
        assert!(!super::dedupe_bypass_kind("compile_error"));
    }

    #[test]
    fn progress_detector_does_not_flag_real_messages() {
        assert!(!ProgressDetector::is_progress_line("Compiling foo v0.1"));
        assert!(!ProgressDetector::is_progress_line(
            "warning: unused variable"
        ));
    }
}
