// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Pure-Rust rule SUGGESTION heuristics (US2 / FR-007, FR-008).
//!
//! Given a bounded set of raw output sample lines, [`suggest_rules`]
//! produces candidate [`RuleDefinition`] DRAFTS using deterministic
//! line-shape heuristics (no ML, no I/O, no clock). The output is a
//! PROPOSAL only:
//!
//! - every proposed rule has `status = Draft`;
//! - nothing is persisted and nothing is activated;
//! - the caller MUST run the explicit `registry_test` ->
//!   `registry_upsert` -> `registry_activate` loop to make a rule live
//!   (constitution VII: suggest-never-auto-activate).
//!
//! Heuristics detected, in priority order:
//!
//! 1. `error[Ennnn]: ...` coded-error prefixes (rustc/TS-style);
//! 2. `error: ...` / `ERROR ...` plain error prefixes;
//! 3. `warning: ...` / `WARNING ...` warning prefixes;
//! 4. `FAILED` / `FAIL` test-style failure tokens;
//! 5. `path/to/file:line[:col]:` file:line locators;
//! 6. exit-summary lines (`exited with code N`, `Error: process ...`).
//!
//! Empty or low-signal input yields an EMPTY proposal set plus an
//! explanation string, never a junk rule (FR-007 acceptance #1).
//!
//! Source-status: live (US2 / T028). Pure function; fully unit-tested.

use regex::Regex;
use terminal_commander_core::{
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};

/// Hard cap on the number of sample lines the heuristics scan.
///
/// Lines beyond this are ignored so a hostile or huge sample set
/// cannot blow the bounded-output guarantee. Samples are pre-capped at
/// the IPC layer too; this is defense-in-depth.
pub const MAX_SUGGEST_SAMPLES: usize = 200;

/// Hard cap on per-line bytes the heuristics inspect.
pub const MAX_SUGGEST_LINE_BYTES: usize = 4096;

/// Default cap on the number of proposed rules returned. The IPC
/// layer may lower this via `max_rules`; it never raises it.
pub const DEFAULT_MAX_PROPOSED_RULES: usize = 8;

/// Confidence label for the heuristic suggester. There is exactly one
/// value: the suggestions are deterministic heuristics, never a
/// statistical / ML score, so the label is a constant honest claim.
pub const SUGGEST_CONFIDENCE: &str = "heuristic";

/// Outcome of [`suggest_rules`]: a (possibly empty) set of DRAFT rule
/// proposals plus a human-readable explanation of what was (or was
/// not) detected.
///
/// This type carries no activation state and no persistence handle by
/// design: a suggestion can never become live without the explicit
/// downstream loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionSet {
    /// Proposed DRAFT rules. Always `status == Draft`.
    pub proposed_rules: Vec<RuleDefinition>,
    /// Why these (or no) rules were proposed. Bounded plain text.
    pub explanation: String,
}

impl SuggestionSet {
    /// Empty proposal with an explanation. Used for empty / low-signal
    /// input so the caller gets honest "nothing matched" feedback
    /// rather than a junk rule.
    fn empty(explanation: impl Into<String>) -> Self {
        Self {
            proposed_rules: Vec::new(),
            explanation: explanation.into(),
        }
    }
}

/// A single heuristic: a compiled detector plus the metadata used to
/// turn a hit into a proposed [`RuleDefinition`].
struct Heuristic {
    /// Stable id suffix; the proposed rule id is `suggest.<suffix>`.
    id_suffix: &'static str,
    /// Regex that both DETECTS a line and is REUSED as the proposed
    /// rule's pattern (so the suggestion is directly testable).
    detector: Regex,
    severity: Severity,
    event_kind: &'static str,
    stream: SourceStream,
    summary_template: &'static str,
    /// Named captures the template + rule declare.
    captures: &'static [&'static str],
    description: &'static str,
}

/// Build the ordered heuristic table. Patterns are anchored and
/// simple so each compiles well under the ReDoS budget (TESTING.md
/// sec 7: compile < 50ms, run < 10ms). Compiled once per call; the
/// set is tiny so this is cheap and keeps the function pure.
fn heuristics() -> Vec<Heuristic> {
    // Each `expect` is over a hard-coded literal pattern authored in
    // this file; a failure is a compile-time-authoring bug surfaced
    // immediately by the unit tests, never runtime/user input.
    vec![
        Heuristic {
            id_suffix: "coded-error",
            detector: Regex::new(r"^(?P<code>[A-Z]+[0-9]{2,5}): (?P<message>.+)$")
                .expect("coded-error pattern compiles"),
            severity: Severity::High,
            event_kind: "compile_error",
            stream: SourceStream::Stderr,
            summary_template: "${code}: ${message}",
            captures: &["code", "message"],
            description: "Coded diagnostic prefix (e.g. E0432, TS2304).",
        },
        Heuristic {
            id_suffix: "error-prefix",
            detector: Regex::new(r"^(?i:error)[: ](?P<message>.+)$")
                .expect("error-prefix pattern compiles"),
            severity: Severity::High,
            event_kind: "error",
            stream: SourceStream::Stderr,
            summary_template: "error: ${message}",
            captures: &["message"],
            description: "Plain `error:`/`ERROR ` line prefix.",
        },
        Heuristic {
            id_suffix: "warning-prefix",
            detector: Regex::new(r"^(?i:warning)[: ](?P<message>.+)$")
                .expect("warning-prefix pattern compiles"),
            severity: Severity::Low,
            event_kind: "warning",
            stream: SourceStream::Stderr,
            summary_template: "warning: ${message}",
            captures: &["message"],
            description: "Plain `warning:`/`WARNING ` line prefix.",
        },
        Heuristic {
            id_suffix: "failed-token",
            detector: Regex::new(r"(?P<token>FAILED|FAIL)\b(?P<rest>.*)$")
                .expect("failed-token pattern compiles"),
            severity: Severity::High,
            event_kind: "test_failed",
            stream: SourceStream::Stdout,
            summary_template: "failure: ${token}${rest}",
            captures: &["token", "rest"],
            description: "A `FAILED`/`FAIL` test-style failure token.",
        },
        Heuristic {
            id_suffix: "file-line",
            detector: Regex::new(
                r"^(?P<file>[A-Za-z0-9_./-]+):(?P<line>[0-9]+)(?::(?P<col>[0-9]+))?[: ]",
            )
            .expect("file-line pattern compiles"),
            severity: Severity::Medium,
            event_kind: "source_location",
            stream: SourceStream::Stderr,
            summary_template: "at ${file}:${line}",
            captures: &["file", "line", "col"],
            description: "A `file:line[:col]` source locator prefix.",
        },
        Heuristic {
            id_suffix: "exit-summary",
            detector: Regex::new(
                r"(?i:exited with code|exit status|process exited) (?P<code>[0-9]+)",
            )
            .expect("exit-summary pattern compiles"),
            severity: Severity::Medium,
            event_kind: "command_failed",
            stream: SourceStream::Stderr,
            summary_template: "exited with code ${code}",
            captures: &["code"],
            description: "An exit-code summary line.",
        },
    ]
}

/// Run the suggestion heuristics over `samples` and return a bounded
/// set of DRAFT proposals. Pure: no I/O, no persistence, no clock,
/// no activation.
///
/// `max_rules` caps the number of proposals; `None` uses
/// [`DEFAULT_MAX_PROPOSED_RULES`]. The cap is clamped to
/// `DEFAULT_MAX_PROPOSED_RULES` so a caller can never request an
/// unbounded set.
#[must_use]
pub fn suggest_rules(samples: &[String], max_rules: Option<usize>) -> SuggestionSet {
    let cap = max_rules
        .unwrap_or(DEFAULT_MAX_PROPOSED_RULES)
        .clamp(1, DEFAULT_MAX_PROPOSED_RULES);

    // Filter to non-empty lines within the byte budget.
    let lines: Vec<&str> = samples
        .iter()
        .take(MAX_SUGGEST_SAMPLES)
        .map(|s| {
            let trimmed = s.trim_end_matches(['\n', '\r']);
            if trimmed.len() > MAX_SUGGEST_LINE_BYTES {
                let mut end = MAX_SUGGEST_LINE_BYTES;
                while !trimmed.is_char_boundary(end) {
                    end -= 1;
                }
                &trimmed[..end]
            } else {
                trimmed
            }
        })
        .filter(|l| !l.trim().is_empty())
        .collect();

    if lines.is_empty() {
        return SuggestionSet::empty("no samples provided (all lines empty); nothing to suggest");
    }

    let table = heuristics();
    let mut proposed: Vec<RuleDefinition> = Vec::new();
    let mut matched_suffixes: Vec<&'static str> = Vec::new();

    for h in &table {
        if proposed.len() >= cap {
            break;
        }
        // A heuristic fires if it detects ANY of the sample lines. We
        // propose ONE rule per heuristic (not one per line) so the
        // proposal set stays bounded and de-duplicated.
        let fired = lines.iter().any(|l| h.detector.is_match(l));
        if fired {
            proposed.push(build_proposal(h));
            matched_suffixes.push(h.id_suffix);
        }
    }

    if proposed.is_empty() {
        return SuggestionSet::empty(format!(
            "scanned {} line(s); no recognizable error/warning/failure/locator \
             shapes detected. Define a rule manually via registry_upsert, or \
             capture more output first.",
            lines.len()
        ));
    }

    let explanation = format!(
        "scanned {} line(s); detected {} candidate shape(s): {}. \
         These are DRAFT proposals only -- test then explicitly activate.",
        lines.len(),
        proposed.len(),
        matched_suffixes.join(", ")
    );

    SuggestionSet {
        proposed_rules: proposed,
        explanation,
    }
}

/// Turn a fired heuristic into a DRAFT [`RuleDefinition`]. The
/// proposal reuses the detector pattern so it is directly testable
/// via `registry_test`, and is ALWAYS `status == Draft`.
fn build_proposal(h: &Heuristic) -> RuleDefinition {
    RuleDefinition {
        id: format!("suggest.{}", h.id_suffix),
        version: 1,
        kind: RuleType::Regex,
        // INVARIANT (FR-008): a proposal is never live. Draft only.
        status: RuleStatus::Draft,
        severity: h.severity,
        event_kind: h.event_kind.to_owned(),
        stream: Some(h.stream.clone()),
        description: Some(h.description.to_owned()),
        pattern: Some(h.detector.as_str().to_owned()),
        keywords: None,
        captures: h.captures.iter().map(|c| (*c).to_owned()).collect(),
        summary_template: h.summary_template.to_owned(),
        tags: vec!["suggested".to_owned()],
        rate_limit_per_min: Some(30),
        redact: Vec::new(),
        context_hint: ContextHint {
            before_lines: 0,
            after_lines: 2,
        },
        examples: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn empty_input_yields_empty_proposal_with_explanation() {
        let set = suggest_rules(&[], None);
        assert!(set.proposed_rules.is_empty());
        assert!(!set.explanation.is_empty());
        assert!(set.explanation.contains("no samples"));
    }

    #[test]
    fn all_blank_lines_yield_empty_proposal_not_a_junk_rule() {
        let set = suggest_rules(&s(&["", "   ", "\t", "\n"]), None);
        assert!(
            set.proposed_rules.is_empty(),
            "blank-only input must NOT fabricate a rule"
        );
    }

    #[test]
    fn low_signal_text_yields_empty_proposal_with_explanation() {
        let set = suggest_rules(
            &s(&["hello world", "the quick brown fox", "just some prose"]),
            None,
        );
        assert!(
            set.proposed_rules.is_empty(),
            "prose with no error/warning shape must propose nothing"
        );
        assert!(set.explanation.contains("no recognizable"));
    }

    #[test]
    fn detects_coded_error_prefix() {
        let set = suggest_rules(&s(&["E0432: unresolved import `crate::missing`"]), None);
        let r = set
            .proposed_rules
            .iter()
            .find(|r| r.id == "suggest.coded-error")
            .expect("coded-error proposal");
        assert_eq!(r.kind, RuleType::Regex);
        assert_eq!(r.severity, Severity::High);
        assert!(r.captures.contains(&"code".to_owned()));
    }

    #[test]
    fn detects_plain_error_prefix() {
        let set = suggest_rules(&s(&["error: could not compile foo"]), None);
        assert!(
            set.proposed_rules
                .iter()
                .any(|r| r.id == "suggest.error-prefix"),
            "expected error-prefix proposal"
        );
    }

    #[test]
    fn detects_warning_prefix() {
        let set = suggest_rules(&s(&["warning: unused variable `x`"]), None);
        let r = set
            .proposed_rules
            .iter()
            .find(|r| r.id == "suggest.warning-prefix")
            .expect("warning-prefix proposal");
        // Universal/warning shapes are LOW severity by design.
        assert_eq!(r.severity, Severity::Low);
    }

    #[test]
    fn detects_failed_token() {
        let set = suggest_rules(&s(&["test result: FAILED. 1 passed; 2 failed"]), None);
        assert!(
            set.proposed_rules
                .iter()
                .any(|r| r.id == "suggest.failed-token"),
            "expected failed-token proposal"
        );
    }

    #[test]
    fn detects_file_line_locator() {
        let set = suggest_rules(&s(&["src/main.rs:42:7: error here"]), None);
        let r = set
            .proposed_rules
            .iter()
            .find(|r| r.id == "suggest.file-line")
            .expect("file-line proposal");
        assert!(r.captures.contains(&"line".to_owned()));
    }

    #[test]
    fn detects_exit_summary() {
        let set = suggest_rules(&s(&["process exited with code 137"]), None);
        assert!(
            set.proposed_rules
                .iter()
                .any(|r| r.id == "suggest.exit-summary"),
            "expected exit-summary proposal"
        );
    }

    #[test]
    fn every_proposed_rule_is_draft_and_validates() {
        // FR-008: no proposal may carry an active status, and every
        // proposal must be a well-formed (validatable) rule so the
        // downstream test->upsert->activate loop accepts it verbatim.
        let set = suggest_rules(
            &s(&[
                "E0001: bad",
                "error: boom",
                "warning: meh",
                "FAILED something",
                "src/x.rs:1:1: y",
                "process exited with code 2",
            ]),
            None,
        );
        assert!(!set.proposed_rules.is_empty());
        for r in &set.proposed_rules {
            assert_eq!(
                r.status,
                RuleStatus::Draft,
                "proposal {} must be Draft (suggest never activates)",
                r.id
            );
            r.validate()
                .unwrap_or_else(|e| panic!("proposal {} failed validation: {e:?}", r.id));
        }
    }

    #[test]
    fn max_rules_cap_is_honored_and_clamped() {
        let many = s(&[
            "E0001: bad",
            "error: boom",
            "warning: meh",
            "FAILED something",
            "src/x.rs:1:1: y",
            "process exited with code 2",
        ]);
        // Request only 2.
        let set = suggest_rules(&many, Some(2));
        assert_eq!(set.proposed_rules.len(), 2);
        // Request an absurd number -> clamped to the default cap, and
        // bounded by however many heuristics actually fired.
        let set = suggest_rules(&many, Some(9999));
        assert!(set.proposed_rules.len() <= DEFAULT_MAX_PROPOSED_RULES);
    }

    #[test]
    fn one_proposal_per_heuristic_not_per_line() {
        // Three error lines must collapse to a SINGLE error proposal.
        let set = suggest_rules(&s(&["error: a", "error: b", "error: c"]), None);
        let count = set
            .proposed_rules
            .iter()
            .filter(|r| r.id == "suggest.error-prefix")
            .count();
        assert_eq!(count, 1, "must dedupe to one proposal per heuristic");
    }

    #[test]
    fn proposed_patterns_compile_under_bounded_regex() {
        // Each proposal pattern must pass the same bounded compile the
        // registry import path uses (ReDoS budget).
        let set = suggest_rules(&s(&["error: x", "warning: y", "E1: z"]), None);
        for r in &set.proposed_rules {
            let pat = r.pattern.as_deref().expect("regex proposal has a pattern");
            terminal_commander_core::compile_bounded_regex(pat).unwrap_or_else(|e| {
                panic!("proposal {} pattern failed bounded compile: {e}", r.id)
            });
        }
    }
}
