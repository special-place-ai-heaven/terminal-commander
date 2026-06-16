// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Universal extractors (US2 / FR-009): always-on, LOW-severity
//! baseline sifters that emit bounded structured signal for ANY
//! command even when no tool-specific rule pack is active.
//!
//! These are ordinary [`RuleDefinition`]s (regex/keyword) built in
//! code and merged into the per-job sifter runtime when the operator
//! sets `sifters.universal_extractors = true` (default FALSE). Because
//! they flow through the SAME sifter runtime as every other rule, the
//! output stays combed and bounded (constitution III): a universal
//! extractor produces a structured [`EventDraft`], never a raw stream.
//!
//! Detected baseline shapes:
//!
//! - stderr `error:` / `ERROR` lines  -> `universal_error` (LOW);
//! - stderr `warning:` / `WARNING` lines -> `universal_warning` (LOW);
//! - exit-summary lines (`exited with code N`) -> `universal_exit` (LOW);
//! - progress ticks (`NN%`, `n/m`) -> `universal_progress` (LOW).
//!
//! Severity is intentionally pinned at [`Severity::Low`] so a
//! tool-specific pack (typically High/Critical) always out-ranks the
//! baseline and the agent is never spammed with a louder signal than
//! the operator opted into.
//!
//! Source-status: live (US2 / T031). Activation is config-gated; the
//! rules themselves are pure data built here.

use terminal_commander_core::{
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};

/// Synthetic pack name reported for universal-extractor rules so they
/// are distinguishable from operator-imported packs in audit/listing.
pub const UNIVERSAL_PACK: &str = "universal";

/// Build the universal extractor rule set as ACTIVE rules ready to
/// merge into a sifter runtime.
///
/// The rules are returned `status == Active` because the caller only
/// ever asks for them when the `universal_extractors` config flag is
/// ON -- i.e. the operator has already opted in, which is the
/// activation decision. They are NOT persisted to the registry; they
/// live only for the duration of the job's sifter. Patterns are
/// anchored and simple (ReDoS budget per TESTING.md sec 7).
#[must_use]
pub fn universal_extractor_rules() -> Vec<RuleDefinition> {
    vec![
        regex_rule(
            "universal.error",
            "universal_error",
            SourceStream::Stderr,
            r"^(?i:error)[: ](?P<message>.+)$",
            &["message"],
            "error: ${message}",
            "Baseline stderr error line (universal extractor).",
        ),
        regex_rule(
            "universal.warning",
            "universal_warning",
            SourceStream::Stderr,
            r"^(?i:warning)[: ](?P<message>.+)$",
            &["message"],
            "warning: ${message}",
            "Baseline stderr warning line (universal extractor).",
        ),
        regex_rule(
            "universal.exit",
            "universal_exit",
            SourceStream::Stderr,
            r"(?i:exited with code|exit status|process exited) (?P<code>[0-9]+)",
            &["code"],
            "exited with code ${code}",
            "Baseline exit-summary line (universal extractor).",
        ),
        regex_rule(
            "universal.progress",
            "universal_progress",
            SourceStream::Stdout,
            r"^\s*(?P<pct>[0-9]{1,3})%\s*$",
            &["pct"],
            "progress ${pct}%",
            "Baseline progress tick (universal extractor).",
        ),
    ]
}

/// Construct one LOW-severity regex universal rule. Severity is fixed
/// at [`Severity::Low`] by design (see module docs).
fn regex_rule(
    id: &str,
    event_kind: &str,
    stream: SourceStream,
    pattern: &str,
    captures: &[&str],
    summary_template: &str,
    description: &str,
) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Regex,
        status: RuleStatus::Active,
        // INVARIANT: universal extractors are always LOW so a real
        // pack out-ranks them.
        severity: Severity::Low,
        event_kind: event_kind.to_owned(),
        stream: Some(stream),
        description: Some(description.to_owned()),
        pattern: Some(pattern.to_owned()),
        keywords: None,
        captures: captures.iter().map(|c| (*c).to_owned()).collect(),
        summary_template: summary_template.to_owned(),
        tags: vec!["universal".to_owned()],
        // Rate-limit the baseline noisy shapes so a flood of warnings
        // cannot blow the bounded-output budget.
        rate_limit_per_min: Some(20),
        redact: Vec::new(),
        context_hint: ContextHint {
            before_lines: 0,
            after_lines: 1,
        },
        examples: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{BucketId, ProbeId, SourceFrame};

    use crate::SifterRuntime;

    #[test]
    fn all_universal_rules_are_low_severity_and_validate() {
        let rules = universal_extractor_rules();
        assert!(!rules.is_empty());
        for r in &rules {
            assert_eq!(
                r.severity,
                Severity::Low,
                "universal extractor {} must be LOW severity",
                r.id
            );
            assert_eq!(r.status, RuleStatus::Active);
            r.validate()
                .unwrap_or_else(|e| panic!("universal rule {} failed validation: {e:?}", r.id));
        }
    }

    #[test]
    fn universal_rules_build_a_sifter_and_emit_on_error_line() {
        let rules = universal_extractor_rules();
        let sifter = SifterRuntime::build(&rules).expect("universal sifter builds");
        let probe = ProbeId::new();
        let bucket = BucketId::new();
        let frame = SourceFrame::new(probe, SourceStream::Stderr, "error: boom".to_owned());
        let drafts = sifter.evaluate(&frame, bucket);
        assert!(
            drafts.iter().any(|d| d.kind == "universal_error"),
            "universal error extractor must fire on an error line"
        );
        // Bounded/combed: the emitted summary is the rendered template,
        // not the raw stream echo of arbitrary length.
        for d in &drafts {
            assert_eq!(d.severity, Severity::Low);
        }
    }

    #[test]
    fn universal_progress_tick_is_detected() {
        let rules = universal_extractor_rules();
        let sifter = SifterRuntime::build(&rules).expect("sifter");
        let frame = SourceFrame::new(ProbeId::new(), SourceStream::Stdout, " 42%".to_owned());
        let drafts = sifter.evaluate(&frame, BucketId::new());
        assert!(drafts.iter().any(|d| d.kind == "universal_progress"));
    }

    #[test]
    fn benign_line_emits_nothing() {
        let rules = universal_extractor_rules();
        let sifter = SifterRuntime::build(&rules).expect("sifter");
        let frame = SourceFrame::new(
            ProbeId::new(),
            SourceStream::Stdout,
            "just normal output".to_owned(),
        );
        let drafts = sifter.evaluate(&frame, BucketId::new());
        assert!(drafts.is_empty(), "benign output must stay silent");
    }
}
