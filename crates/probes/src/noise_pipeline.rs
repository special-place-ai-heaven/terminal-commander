// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC11 per-frame noise pipeline for probe hot paths.
//!
//! Orders: progress pre-sift → `SifterRuntime::evaluate` → dedupe
//! (with `password_prompt` bypass) → emit.

use std::sync::Arc;

use parking_lot::Mutex;
use terminal_commander_core::{
    BucketId, EventDraft, EventSource, Severity, SourceFrame, SourcePointer, SourceType,
};
use terminal_commander_sifters::{
    Dedupe, NoisePolicy, ProgressDetector, SifterRuntime, dedupe_bypass_kind,
};
use time::OffsetDateTime;

use crate::process::EventSink;

/// Canonical wire kind for secret prompts (`event-kind.md`).
pub const PASSWORD_PROMPT_KIND: &str = "password_prompt";

/// Per-probe suppression counters mirrored on probe metrics structs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SuppressionMetrics {
    pub frames_suppressed: u64,
    pub frames_suppressed_progress: u64,
    pub frames_suppressed_dedupe: u64,
}

/// Trait implemented by `ProcessProbeMetrics`, `FileProbeMetrics`, and
/// `PtyProbeMetrics`.
pub trait SuppressionCounter {
    fn record_progress_suppressed(&mut self);
    fn record_dedupe_suppressed(&mut self, count: u64);
    fn suppression_snapshot(&self) -> SuppressionMetrics;
}

impl SuppressionCounter for crate::process::ProcessProbeMetrics {
    fn record_progress_suppressed(&mut self) {
        self.frames_suppressed_progress = self.frames_suppressed_progress.saturating_add(1);
        self.frames_suppressed = self.frames_suppressed.saturating_add(1);
    }

    fn record_dedupe_suppressed(&mut self, count: u64) {
        self.frames_suppressed_dedupe = self.frames_suppressed_dedupe.saturating_add(count);
        self.frames_suppressed = self.frames_suppressed.saturating_add(count);
    }

    fn suppression_snapshot(&self) -> SuppressionMetrics {
        SuppressionMetrics {
            frames_suppressed: self.frames_suppressed,
            frames_suppressed_progress: self.frames_suppressed_progress,
            frames_suppressed_dedupe: self.frames_suppressed_dedupe,
        }
    }
}

impl SuppressionCounter for crate::file::FileProbeMetrics {
    fn record_progress_suppressed(&mut self) {
        self.frames_suppressed_progress = self.frames_suppressed_progress.saturating_add(1);
        self.frames_suppressed = self.frames_suppressed.saturating_add(1);
    }

    fn record_dedupe_suppressed(&mut self, count: u64) {
        self.frames_suppressed_dedupe = self.frames_suppressed_dedupe.saturating_add(count);
        self.frames_suppressed = self.frames_suppressed.saturating_add(count);
    }

    fn suppression_snapshot(&self) -> SuppressionMetrics {
        SuppressionMetrics {
            frames_suppressed: self.frames_suppressed,
            frames_suppressed_progress: self.frames_suppressed_progress,
            frames_suppressed_dedupe: self.frames_suppressed_dedupe,
        }
    }
}

// PTY metrics exist on every host with a PTY backend (unix `pty-process`
// and Windows ConPTY), so the suppression counter must too.
#[cfg(any(unix, windows))]
impl SuppressionCounter for crate::pty::PtyProbeMetrics {
    fn record_progress_suppressed(&mut self) {
        self.frames_suppressed_progress = self.frames_suppressed_progress.saturating_add(1);
        self.frames_suppressed = self.frames_suppressed.saturating_add(1);
    }

    fn record_dedupe_suppressed(&mut self, count: u64) {
        self.frames_suppressed_dedupe = self.frames_suppressed_dedupe.saturating_add(count);
        self.frames_suppressed = self.frames_suppressed.saturating_add(count);
    }

    fn suppression_snapshot(&self) -> SuppressionMetrics {
        SuppressionMetrics {
            frames_suppressed: self.frames_suppressed,
            frames_suppressed_progress: self.frames_suppressed_progress,
            frames_suppressed_dedupe: self.frames_suppressed_dedupe,
        }
    }
}

/// Owned per-probe-instance noise state (one dedupe window per probe).
pub struct ProbeNoisePipeline {
    policy: NoisePolicy,
    dedupe: Dedupe,
}

impl ProbeNoisePipeline {
    #[must_use]
    pub fn with_default_policy() -> Self {
        Self::new(NoisePolicy::default())
    }

    #[must_use]
    pub fn new(policy: NoisePolicy) -> Self {
        Self {
            policy,
            dedupe: Dedupe::new(),
        }
    }

    /// Process one frame after the caller appended it to the context ring
    /// and incremented `frames_total`.
    #[allow(clippy::too_many_arguments)]
    pub fn process_frame(
        &mut self,
        frame: &SourceFrame,
        bucket_id: BucketId,
        runtime: &SifterRuntime,
        sink: &dyn EventSink,
        metrics: &mut impl SuppressionCounter,
        events_emitted: &mut u64,
        extra_drafts: impl IntoIterator<Item = EventDraft>,
    ) {
        // M3: evaluate FIRST so a rule (even High/Critical) keyed to a
        // progress-shaped line (`%`, `n/m`, spinner, blank) still fires.
        // A progress frame is suppressed ONLY when the sifter produced
        // zero drafts; a matched rule is never silently dropped.
        let mut drafts = runtime.evaluate(frame, bucket_id);
        drafts.extend(extra_drafts);

        if drafts.is_empty()
            && self.policy.drop_progress_frames
            && ProgressDetector::is_progress_line(&frame.text)
        {
            metrics.record_progress_suppressed();
            return;
        }

        let (bypass, rest): (Vec<_>, Vec<_>) = drafts
            .into_iter()
            .partition(|d| dedupe_bypass_kind(&d.kind));

        let rest_len = rest.len();
        let dedupe_out = self
            .dedupe
            .apply_at(rest, &self.policy, OffsetDateTime::now_utc());
        let dropped =
            u64::try_from(rest_len.saturating_sub(dedupe_out.emit.len())).unwrap_or(u64::MAX);
        if dropped > 0 {
            metrics.record_dedupe_suppressed(dropped);
        }

        for patch in &dedupe_out.patches {
            sink.patch_dedupe_aggregate(bucket_id, patch);
        }

        for draft in bypass.into_iter().chain(dedupe_out.emit) {
            *events_emitted = events_emitted.saturating_add(1);
            if let Some(seq) = sink.emit(draft.clone()) {
                self.dedupe.register_emitted(&draft, seq);
            }
        }
    }
}

/// Build a synthetic `password_prompt` draft for PTY secret-prompt lines
/// (TC11 B0). Does not include typed secret material.
#[must_use]
pub fn password_prompt_draft(frame: &SourceFrame, bucket_id: BucketId) -> EventDraft {
    EventDraft {
        bucket_id,
        timestamp: OffsetDateTime::now_utc(),
        severity: Severity::Critical,
        kind: PASSWORD_PROMPT_KIND.to_owned(),
        summary: "secret password prompt detected".to_owned(),
        rule: None,
        source: EventSource {
            probe_id: frame.probe_id,
            source_type: SourceType::Terminal,
            stream: frame.stream.clone(),
            job_id: None,
        },
        captures: None,
        pointer: frame
            .line
            .map(|n| SourcePointer::new(frame.frame_id).with_line(n)),
        pointer_unavailable_reason: None,
        tags: None,
        frame_truncated_bytes: 0,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

/// Shared noise pipeline across stdout/stderr on a process probe.
pub type SharedProbeNoisePipeline = Arc<Mutex<ProbeNoisePipeline>>;

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{
        Captures, ContextHint, FrameId, ProbeId, RuleDefinition, RuleRef, RuleStatus, RuleType,
        SourceStream,
    };
    use terminal_commander_sifters::SifterRuntime;

    fn keyword_rule(id: &str, event_kind: &str, kw: &str) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::Low,
            event_kind: event_kind.to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec![kw.to_owned()]),
            captures: vec![],
            summary_template: "matched".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    #[derive(Default)]
    struct TestMetrics {
        inner: SuppressionMetrics,
    }

    impl SuppressionCounter for TestMetrics {
        fn record_progress_suppressed(&mut self) {
            self.inner.frames_suppressed_progress += 1;
            self.inner.frames_suppressed += 1;
        }

        fn record_dedupe_suppressed(&mut self, count: u64) {
            self.inner.frames_suppressed_dedupe += count;
            self.inner.frames_suppressed += count;
        }

        fn suppression_snapshot(&self) -> SuppressionMetrics {
            self.inner
        }
    }

    struct VecSink(Arc<Mutex<Vec<EventDraft>>>);

    impl EventSink for VecSink {
        fn emit(&self, draft: EventDraft) -> Option<u64> {
            let mut g = self.0.lock();
            g.push(draft);
            Some(g.len() as u64)
        }

        fn patch_dedupe_aggregate(
            &self,
            _bucket_id: BucketId,
            patch: &terminal_commander_sifters::DedupeAggregatePatch,
        ) {
            let mut g = self.0.lock();
            let Ok(idx) = usize::try_from(patch.seq.saturating_sub(1)) else {
                return;
            };
            if let Some(ev) = g.get_mut(idx) {
                ev.count = patch.count;
                ev.first_seen = Some(patch.first_seen);
                ev.last_seen = Some(patch.last_seen);
            }
        }
    }

    #[test]
    fn progress_line_with_no_matching_rule_is_suppressed() {
        // M3 new contract: a progress-shaped line is evaluated FIRST;
        // it is suppressed as progress ONLY because no rule matched
        // (zero drafts). The keyword rule keys on "WARN", which "45%"
        // does not contain, so evaluation yields nothing and the frame
        // is recorded as a progress suppression.
        let runtime = Arc::new(SifterRuntime::build(&[keyword_rule("r", "k", "WARN")]).unwrap());
        let sink = VecSink(Arc::new(Mutex::new(Vec::new())));
        let probe_id = ProbeId::new();
        let bucket_id = BucketId::new();
        let frame = SourceFrame::new(probe_id, SourceStream::Stdout, "45%".to_owned());
        let mut pipeline = ProbeNoisePipeline::with_default_policy();
        let mut metrics = TestMetrics::default();
        let mut events = 0u64;
        pipeline.process_frame(
            &frame,
            bucket_id,
            &runtime,
            &sink,
            &mut metrics,
            &mut events,
            std::iter::empty::<EventDraft>(),
        );
        assert_eq!(events, 0);
        assert_eq!(metrics.inner.frames_suppressed_progress, 1);
        assert!(sink.0.lock().is_empty());
    }

    #[test]
    fn progress_shaped_line_matching_a_rule_still_fires() {
        // M3: a rule keyed to a progress-shaped line MUST fire — the
        // old pre-filter dropped such frames before evaluation, masking
        // even High/Critical rules. Here a keyword rule keys on a token
        // that appears in a progress-shaped line; the event must emit
        // and NOTHING must be recorded as a progress suppression.
        let runtime = Arc::new(SifterRuntime::build(&[keyword_rule("r", "k", "100%")]).unwrap());
        let sink = VecSink(Arc::new(Mutex::new(Vec::new())));
        let probe_id = ProbeId::new();
        let bucket_id = BucketId::new();
        let frame = SourceFrame::new(probe_id, SourceStream::Stdout, "100%".to_owned());
        let mut pipeline = ProbeNoisePipeline::with_default_policy();
        let mut metrics = TestMetrics::default();
        let mut events = 0u64;
        pipeline.process_frame(
            &frame,
            bucket_id,
            &runtime,
            &sink,
            &mut metrics,
            &mut events,
            std::iter::empty::<EventDraft>(),
        );
        assert_eq!(events, 1, "rule keyed to a progress line must fire");
        assert_eq!(
            metrics.inner.frames_suppressed_progress, 0,
            "a matched progress-shaped frame must NOT be suppressed"
        );
        assert_eq!(sink.0.lock().len(), 1);
    }

    #[test]
    fn dedupe_collapses_repeated_drafts() {
        let runtime = Arc::new(SifterRuntime::build(&[]).unwrap());
        let sink = VecSink(Arc::new(Mutex::new(Vec::new())));
        let probe_id = ProbeId::new();
        let bucket_id = BucketId::new();
        let frame = SourceFrame::new(probe_id, SourceStream::Stdout, "noise".to_owned());
        let mut pipeline = ProbeNoisePipeline::with_default_policy();
        let mut metrics = TestMetrics::default();
        let mut events = 0u64;
        let ts = OffsetDateTime::now_utc();
        let rid = terminal_commander_core::RuleId::new();
        let draft = EventDraft {
            bucket_id,
            timestamp: ts,
            severity: Severity::Low,
            kind: "compile_error".to_owned(),
            summary: "s".to_owned(),
            rule: Some(RuleRef {
                id: rid,
                version: 1,
            }),
            source: EventSource {
                probe_id,
                source_type: SourceType::Process,
                stream: SourceStream::Stdout,
                job_id: None,
            },
            captures: Some({
                let mut c = Captures::new();
                c.insert("pkg".to_owned(), "a".to_owned());
                c
            }),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        };
        pipeline.process_frame(
            &frame,
            bucket_id,
            &runtime,
            &sink,
            &mut metrics,
            &mut events,
            [draft.clone(), draft],
        );
        assert_eq!(events, 1);
        assert_eq!(metrics.inner.frames_suppressed_dedupe, 1);
        assert_eq!(sink.0.lock().len(), 1);
        assert_eq!(sink.0.lock()[0].count, 2);
    }

    #[test]
    fn password_prompt_bypasses_dedupe() {
        let runtime = Arc::new(SifterRuntime::build(&[]).unwrap());
        let sink = VecSink(Arc::new(Mutex::new(Vec::new())));
        let probe_id = ProbeId::new();
        let bucket_id = BucketId::new();
        let frame = SourceFrame::new(
            probe_id,
            SourceStream::Stdout,
            "[sudo] password for dev:".to_owned(),
        );
        let mut pipeline = ProbeNoisePipeline::with_default_policy();
        let mut metrics = TestMetrics::default();
        let mut events = 0u64;
        let d1 = password_prompt_draft(&frame, bucket_id);
        let d2 = password_prompt_draft(&frame, bucket_id);
        pipeline.process_frame(
            &frame,
            bucket_id,
            &runtime,
            &sink,
            &mut metrics,
            &mut events,
            [d1, d2],
        );
        assert_eq!(events, 2);
        assert_eq!(metrics.inner.frames_suppressed_dedupe, 0);
        assert_eq!(sink.0.lock().len(), 2);
    }

    #[test]
    fn sequential_frames_dedupe_across_calls() {
        let runtime = Arc::new(
            SifterRuntime::build(&[keyword_rule("r", "repeat_hit", "REPEAT_ME")]).unwrap(),
        );
        let sink = VecSink(Arc::new(Mutex::new(Vec::new())));
        let probe_id = ProbeId::new();
        let bucket_id = BucketId::new();
        let mut pipeline = ProbeNoisePipeline::with_default_policy();
        let mut metrics = TestMetrics::default();
        let mut events = 0u64;
        for _ in 0..5 {
            let frame = SourceFrame::new(probe_id, SourceStream::Stdout, "REPEAT_ME".to_owned());
            pipeline.process_frame(
                &frame,
                bucket_id,
                &runtime,
                &sink,
                &mut metrics,
                &mut events,
                std::iter::empty::<EventDraft>(),
            );
        }
        assert_eq!(events, 1, "five identical frames collapse to one emit");
        assert_eq!(metrics.inner.frames_suppressed_dedupe, 4);
        assert_eq!(sink.0.lock().len(), 1);
        assert!(
            sink.0.lock()[0].count >= 5,
            "cross-frame collapse must update representative count"
        );
    }

    #[test]
    fn distinct_rules_on_same_frame_stay_separate() {
        let runtime = Arc::new(
            SifterRuntime::build(&[
                keyword_rule("rule-a", "kind_a", "ALPHA"),
                keyword_rule("rule-b", "kind_b", "BETA"),
            ])
            .unwrap(),
        );
        let sink = VecSink(Arc::new(Mutex::new(Vec::new())));
        let probe_id = ProbeId::new();
        let bucket_id = BucketId::new();
        let frame = SourceFrame::new(probe_id, SourceStream::Stdout, "ALPHA then BETA".to_owned());
        let mut pipeline = ProbeNoisePipeline::with_default_policy();
        let mut metrics = TestMetrics::default();
        let mut events = 0u64;
        pipeline.process_frame(
            &frame,
            bucket_id,
            &runtime,
            &sink,
            &mut metrics,
            &mut events,
            std::iter::empty::<EventDraft>(),
        );
        let emitted = sink.0.lock().clone();
        let kinds: Vec<&str> = emitted.iter().map(|d| d.kind.as_str()).collect();
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&"kind_a"));
        assert!(kinds.contains(&"kind_b"));
    }
}
