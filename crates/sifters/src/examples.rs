// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Evaluate a rule's own `examples` through the same [`SifterRuntime`] that
//! scores live frames and `registry_test` samples, so an example that
//! contradicts its rule is caught instead of sitting unchecked.

use terminal_commander_core::{
    BucketId, ProbeId, RuleDefinition, RuleExampleExpect, RuleStatus, SourceFrame, SourceStream,
};

use crate::{SifterError, SifterRuntime};

/// Longest slice of an example's input quoted back in a failure reason.
const REASON_INPUT_CHARS: usize = 80;

/// Result of evaluating one example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleOutcome {
    /// Position of the example in the rule's `examples` list (0-based).
    pub index: usize,
    /// `None` when the example holds; otherwise why it does not.
    pub failure: Option<String>,
}

impl ExampleOutcome {
    /// Whether the example held against its rule.
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.failure.is_none()
    }
}

/// Evaluate every example of `def` against `def` itself.
///
/// The rule is scored as if active (a draft rule's examples are still checkable). An example
/// without its own `stream` runs on the rule's stream, so a stream filter
/// cannot make a correct example fail.
pub fn evaluate_examples(def: &RuleDefinition) -> Result<Vec<ExampleOutcome>, SifterError> {
    let mut rule = def.clone();
    rule.status = RuleStatus::Active;
    let mut out = Vec::with_capacity(rule.examples.len());
    for (index, ex) in rule.examples.iter().enumerate() {
        // Fresh runtime per example: per-rule rate limits must not bleed
        // from one example into the next.
        let sifter = SifterRuntime::build(std::slice::from_ref(&rule))?;
        let stream = ex
            .stream
            .clone()
            .or_else(|| rule.stream.clone())
            .unwrap_or(SourceStream::Stdout);
        let frame = SourceFrame::new(ProbeId::new(), stream, ex.input.clone());
        let drafts = sifter.evaluate(&frame, BucketId::new());
        let failure = match (&ex.expect, drafts.first()) {
            (RuleExampleExpect::NoMatch { .. }, None) => None,
            (RuleExampleExpect::NoMatch { .. }, Some(d)) => Some(format!(
                "expected no match for {}, but the rule matched (kind '{}')",
                quote(&ex.input),
                d.kind
            )),
            (RuleExampleExpect::Match { .. }, None) => Some(format!(
                "expected a match for {}, got none",
                quote(&ex.input)
            )),
            (RuleExampleExpect::Match { kind, captures }, Some(d)) => {
                match_failure(kind.as_deref(), captures, d)
            }
        };
        out.push(ExampleOutcome { index, failure });
    }
    Ok(out)
}

/// Why a produced draft does not meet a positive expectation, if it does not.
fn match_failure(
    want_kind: Option<&str>,
    want_captures: &indexmap::IndexMap<String, String>,
    draft: &terminal_commander_core::EventDraft,
) -> Option<String> {
    if let Some(want) = want_kind
        && want != draft.kind
    {
        return Some(format!("expected kind '{want}', got '{}'", draft.kind));
    }
    want_captures.iter().find_map(|(name, want)| {
        match draft.captures.as_ref().and_then(|c| c.get(name)) {
            None => Some(format!("expected capture '{name}', but it is missing")),
            Some(got) if got != want => {
                Some(format!("capture '{name}': expected '{want}', got '{got}'"))
            }
            Some(_) => None,
        }
    })
}

fn quote(input: &str) -> String {
    let head: String = input.chars().take(REASON_INPUT_CHARS).collect();
    if head.len() < input.len() {
        format!("{head:?}...")
    } else {
        format!("{head:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{ContextHint, RuleExample, RuleType, Severity};

    fn rule(examples: Vec<RuleExample>) -> RuleDefinition {
        RuleDefinition {
            id: "t.missing-pkg".to_owned(),
            version: 1,
            kind: RuleType::Regex,
            status: RuleStatus::Draft,
            severity: Severity::High,
            event_kind: "missing_package".to_owned(),
            stream: Some(SourceStream::Stderr),
            description: None,
            pattern: Some(r"^E: Unable to locate package (?P<package>\S+)$".to_owned()),
            keywords: None,
            captures: vec!["package".to_owned()],
            summary_template: "missing ${package}".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples,
        }
    }

    fn positive(input: &str, kind: Option<&str>, caps: &[(&str, &str)]) -> RuleExample {
        RuleExample {
            stream: None,
            input: input.to_owned(),
            expect: RuleExampleExpect::Match {
                kind: kind.map(str::to_owned),
                captures: caps
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            },
        }
    }

    fn negative(input: &str) -> RuleExample {
        RuleExample {
            stream: None,
            input: input.to_owned(),
            expect: RuleExampleExpect::NoMatch { match_: false },
        }
    }

    const HIT: &str = "E: Unable to locate package libssl-dev";

    #[test]
    fn no_examples_evaluates_zero() {
        assert!(evaluate_examples(&rule(vec![])).unwrap().is_empty());
    }

    #[test]
    fn positive_example_with_kind_and_capture_passes() {
        let r = rule(vec![positive(
            HIT,
            Some("missing_package"),
            &[("package", "libssl-dev")],
        )]);
        let out = evaluate_examples(&r).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].passed(), "{out:?}");
    }

    #[test]
    fn wrong_kind_fails_naming_both_kinds() {
        let out =
            evaluate_examples(&rule(vec![positive(HIT, Some("compile_error"), &[])])).unwrap();
        let why = out[0].failure.as_deref().unwrap();
        assert!(
            why.contains("'compile_error'") && why.contains("'missing_package'"),
            "{why}"
        );
    }

    #[test]
    fn wrong_and_missing_captures_fail_with_the_capture_name() {
        let wrong =
            evaluate_examples(&rule(vec![positive(HIT, None, &[("package", "zlib")])])).unwrap();
        let why = wrong[0].failure.as_deref().unwrap();
        assert!(
            why.contains("'package'") && why.contains("'zlib'") && why.contains("'libssl-dev'"),
            "{why}"
        );
        let missing =
            evaluate_examples(&rule(vec![positive(HIT, None, &[("nope", "x")])])).unwrap();
        let why = missing[0].failure.as_deref().unwrap();
        assert!(why.contains("'nope'") && why.contains("missing"), "{why}");
    }

    #[test]
    fn positive_example_that_does_not_match_fails() {
        let out = evaluate_examples(&rule(vec![positive("all good", None, &[])])).unwrap();
        assert!(out[0].failure.as_deref().unwrap().contains("got none"));
    }

    #[test]
    fn negative_example_passes_when_rule_does_not_match() {
        let out = evaluate_examples(&rule(vec![negative("all good")])).unwrap();
        assert!(out[0].passed(), "{out:?}");
    }

    #[test]
    fn negative_example_fails_when_rule_matches() {
        let out = evaluate_examples(&rule(vec![negative(HIT)])).unwrap();
        let why = out[0].failure.as_deref().unwrap();
        assert!(
            why.contains("expected no match") && why.contains("missing_package"),
            "{why}"
        );
    }

    #[test]
    fn indexes_follow_example_order() {
        let out = evaluate_examples(&rule(vec![
            positive(HIT, None, &[]),
            negative(HIT),
            negative("fine"),
        ]))
        .unwrap();
        let shape: Vec<(usize, bool)> = out.iter().map(|o| (o.index, o.passed())).collect();
        assert_eq!(shape, vec![(0, true), (1, false), (2, true)]);
    }

    #[test]
    fn example_without_stream_runs_on_the_rules_stream() {
        // The rule only fires on stderr; the example names no stream.
        let out = evaluate_examples(&rule(vec![positive(HIT, None, &[])])).unwrap();
        assert!(out[0].passed());
    }
}
