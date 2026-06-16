// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Keyword + regex sifter runtime for Terminal Commander.
//!
//! A [`SifterRuntime`] is built from a set of active [`RuleDefinition`]s
//! and evaluates them against [`SourceFrame`]s. Each match produces
//! an [`EventDraft`] with the rule's severity, kind, summary
//! (rendered from named captures), and a [`SourcePointer`] back to
//! the matching frame.
//!
//! Two scanners are used:
//! - `aho_corasick::AhoCorasick` for the union of all keyword rule
//!   tokens (single O(n) pass per frame);
//! - `regex::RegexSet` to pre-filter candidate regex rules, then
//!   `regex::Regex::captures` on the small candidate subset to
//!   extract named groups.
//!
//! Frames are capped at [`MAX_SIFT_BYTES`] (matches
//! `terminal_commander_core::context::MAX_FRAME_BYTES`) BEFORE
//! evaluation; any drop is recorded in `EventDraft::frame_truncated_bytes`.
//!
//! Source-status: live (TC10/TC11). Persistence in TC12; daemon
//! activation in TC13/TC21.

pub mod noise;
pub use noise::{
    DEFAULT_DEDUPE_WINDOW, Dedupe, DedupeAggregatePatch, DedupeApplyResult, NoisePolicy,
    ProgressDetector, dedupe_bypass_kind,
};

use std::sync::Arc;

use aho_corasick::AhoCorasick;
use indexmap::IndexMap;
use parking_lot::RwLock;
use regex::{Regex, RegexSet};
use terminal_commander_core::{
    BucketId, Captures, EventDraft, EventSource, RuleDefinition, RuleRef, RuleType, Severity,
    SourceFrame, SourcePointer, SourceStream, compile_bounded_regex, compile_bounded_regex_set,
};

/// Hard cap on per-frame text length passed to the sifter.
///
/// Mirrors `terminal_commander_core::context::MAX_FRAME_BYTES`.
/// Bytes beyond this are dropped before evaluation; the loss is
/// recorded in [`EventDraft::frame_truncated_bytes`].
pub const MAX_SIFT_BYTES: usize = 8192;

/// Errors produced while building a [`SifterRuntime`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SifterError {
    #[error("rule '{0}' is not runtime-eligible (status != Active)")]
    NotActive(String),
    #[error("rule '{0}' kind is not implemented at MVP")]
    KindNotImplemented(String),
    #[error("rule '{id}' regex failed to compile: {reason}")]
    RegexCompile { id: String, reason: String },
    #[error("rule '{id}' keyword list is empty")]
    EmptyKeywords { id: String },
    #[error("rule '{id}' pattern is missing")]
    MissingPattern { id: String },
    #[error("rule '{0}' validation failed: {1}")]
    Invalid(String, String),
}

/// Per-rule metadata kept in the runtime for fast lookups.
#[derive(Debug, Clone)]
struct RegexRule {
    def: RuleDefinition,
    compiled: Regex,
}

#[derive(Debug, Clone)]
struct KeywordRule {
    def: RuleDefinition,
    /// Sorted list of keyword tokens for this rule (so we can group
    /// AhoCorasick patterns back into their owning rule).
    keywords: Vec<String>,
}

/// Immutable, evaluatable snapshot of a built rule set. Owned and
/// hot-swapped by [`SifterRuntime`] so callers holding an
/// `Arc<SifterRuntime>` can keep evaluating while the rule set is
/// rebuilt under the hood (TC42b).
#[derive(Debug)]
struct SifterRuntimeInner {
    keyword_rules: Vec<KeywordRule>,
    /// AhoCorasick over the union of all keyword tokens. For each
    /// match we look up the owning rule via `kw_pattern_to_rule`.
    keyword_ac: Option<AhoCorasick>,
    kw_pattern_to_rule: Vec<usize>,
    regex_rules: Vec<RegexRule>,
    /// RegexSet over the patterns of `regex_rules`.
    regex_set: Option<RegexSet>,
}

/// A built, ready-to-evaluate set of keyword + regex rules.
///
/// The outer type holds an atomic-swap container; readers
/// (`evaluate`, `rule_count`) take a brief read lock and clone the
/// inner `Arc` so the lock is held only long enough to grab a
/// snapshot. Writers (`rebuild`) build the new inner outside the
/// lock then swap it in. This is how TC42b makes
/// `registry_activate` / `registry_deactivate` affect already-
/// running command streams without restarting the probe.
pub struct SifterRuntime {
    inner: RwLock<Arc<SifterRuntimeInner>>,
}

impl std::fmt::Debug for SifterRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let snap = self.inner.read().clone();
        f.debug_struct("SifterRuntime")
            .field(
                "rule_count",
                &(snap.keyword_rules.len() + snap.regex_rules.len()),
            )
            .finish_non_exhaustive()
    }
}

impl SifterRuntime {
    /// Build a runtime from a list of [`RuleDefinition`]s.
    ///
    /// Rules MUST be Active and of one of the MVP-live kinds
    /// (Keyword, Regex). Otherwise [`SifterError`] is returned.
    pub fn build(rules: &[RuleDefinition]) -> Result<Self, SifterError> {
        let inner = build_inner(rules)?;
        Ok(Self {
            inner: RwLock::new(Arc::new(inner)),
        })
    }

    /// Atomically replace the active rule set. Builds the new
    /// compiled state outside the lock then swaps it in. Returns the
    /// previous rule count + the new rule count so the caller can
    /// audit the effect.
    ///
    /// On error the runtime is left unchanged.
    pub fn rebuild(&self, rules: &[RuleDefinition]) -> Result<RebindReport, SifterError> {
        let new_inner = build_inner(rules)?;
        let new_count = new_inner.keyword_rules.len() + new_inner.regex_rules.len();
        let mut g = self.inner.write();
        let old_count = g.keyword_rules.len() + g.regex_rules.len();
        *g = Arc::new(new_inner);
        Ok(RebindReport {
            old_rule_count: old_count,
            new_rule_count: new_count,
        })
    }

    /// Number of active rules (kw + regex) in the current snapshot.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        let snap = self.inner.read();
        snap.keyword_rules.len() + snap.regex_rules.len()
    }

    /// Evaluate the runtime against one frame. Returns a vector of
    /// [`EventDraft`]s ready to be promoted into [`SignalEvent`]s
    /// by the bucket manager. Order: keyword matches first (in rule
    /// order), then regex matches.
    pub fn evaluate(&self, frame: &SourceFrame, bucket_id: BucketId) -> Vec<EventDraft> {
        // Clone the Arc out so the lock is released before the
        // (potentially expensive) evaluation runs. A rebuild that
        // races with us simply means the next frame sees the new
        // rule set; frames in flight finish against the snapshot
        // they captured. This is the TC42b invariant: no fake
        // historical matches, no missed future matches.
        let snap = self.inner.read().clone();
        snap.evaluate(frame, bucket_id)
    }
}

/// Outcome of a [`SifterRuntime::rebuild`] call. Bounded counters
/// only — never raw stream content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebindReport {
    pub old_rule_count: usize,
    pub new_rule_count: usize,
}

fn build_inner(rules: &[RuleDefinition]) -> Result<SifterRuntimeInner, SifterError> {
    let mut keyword_rules: Vec<KeywordRule> = Vec::new();
    let mut kw_patterns: Vec<String> = Vec::new();
    let mut kw_pattern_to_rule: Vec<usize> = Vec::new();
    let mut regex_rules: Vec<RegexRule> = Vec::new();

    for def in rules {
        // Validation already covers most things, but call it
        // here too so the runtime is a safe entry point.
        def.validate()
            .map_err(|e| SifterError::Invalid(def.id.clone(), e.to_string()))?;
        if !def.status.is_runtime_eligible() {
            return Err(SifterError::NotActive(def.id.clone()));
        }
        match def.kind {
            RuleType::Keyword => {
                let kws = def
                    .keywords
                    .as_deref()
                    .ok_or_else(|| SifterError::EmptyKeywords { id: def.id.clone() })?;
                if kws.is_empty() {
                    return Err(SifterError::EmptyKeywords { id: def.id.clone() });
                }
                let rule_idx = keyword_rules.len();
                let kws_sorted = {
                    let mut v: Vec<String> = kws.to_vec();
                    v.sort();
                    v
                };
                for kw in &kws_sorted {
                    kw_patterns.push(kw.clone());
                    kw_pattern_to_rule.push(rule_idx);
                }
                keyword_rules.push(KeywordRule {
                    def: def.clone(),
                    keywords: kws_sorted,
                });
            }
            RuleType::Regex => {
                let pat = def
                    .pattern
                    .as_deref()
                    .ok_or_else(|| SifterError::MissingPattern { id: def.id.clone() })?;
                let compiled =
                    compile_bounded_regex(pat).map_err(|e| SifterError::RegexCompile {
                        id: def.id.clone(),
                        reason: e.to_string(),
                    })?;
                regex_rules.push(RegexRule {
                    def: def.clone(),
                    compiled,
                });
            }
            other => {
                return Err(SifterError::KindNotImplemented(format!(
                    "{}/{other:?}",
                    def.id
                )));
            }
        }
    }

    let keyword_ac = if kw_patterns.is_empty() {
        None
    } else {
        Some(
            AhoCorasick::new(&kw_patterns).map_err(|e| SifterError::RegexCompile {
                id: "<aho>".to_owned(),
                reason: e.to_string(),
            })?,
        )
    };

    let regex_set = if regex_rules.is_empty() {
        None
    } else {
        let pats: Vec<&str> = regex_rules
            .iter()
            .map(|r| r.def.pattern.as_deref().unwrap_or(""))
            .collect();
        Some(
            compile_bounded_regex_set(pats).map_err(|e| SifterError::RegexCompile {
                id: "<set>".to_owned(),
                reason: e.to_string(),
            })?,
        )
    };

    Ok(SifterRuntimeInner {
        keyword_rules,
        keyword_ac,
        kw_pattern_to_rule,
        regex_rules,
        regex_set,
    })
}

impl SifterRuntimeInner {
    fn evaluate(&self, frame: &SourceFrame, bucket_id: BucketId) -> Vec<EventDraft> {
        let (text, truncated_bytes) = cap_text(&frame.text);
        let mut out = Vec::new();

        // Stream filter: rule.stream Some(s) means only frames with
        // a matching stream qualify.
        let frame_stream = &frame.stream;

        // Keyword pass.
        if let Some(ac) = &self.keyword_ac {
            // Track which rules already fired; one event per rule
            // per frame (keyword rules don't dedupe captures by
            // token at this layer).
            let mut fired = vec![false; self.keyword_rules.len()];
            for mat in ac.find_iter(text.as_bytes()) {
                let pat_idx = mat.pattern().as_usize();
                let rule_idx = self.kw_pattern_to_rule[pat_idx];
                if fired[rule_idx] {
                    continue;
                }
                let rule = &self.keyword_rules[rule_idx];
                if !stream_matches(rule.def.stream.as_ref(), frame_stream) {
                    continue;
                }
                fired[rule_idx] = true;
                let kw_token = &rule.keywords
                    [match_index_for_pattern(pat_idx, rule_idx, &self.kw_pattern_to_rule)];
                let mut captures = IndexMap::new();
                captures.insert("keyword".to_owned(), kw_token.clone());
                let injected = inject_full_match(&mut captures, text, &rule.def.redact);
                let draft = build_draft(
                    &rule.def,
                    frame,
                    bucket_id,
                    captures,
                    &injected,
                    truncated_bytes,
                );
                if let Some(d) = draft {
                    out.push(d);
                }
            }
        }

        // Regex pass.
        if let Some(set) = &self.regex_set {
            for hit in set.matches(text) {
                let rule = &self.regex_rules[hit];
                if !stream_matches(rule.def.stream.as_ref(), frame_stream) {
                    continue;
                }
                let Some(caps) = rule.compiled.captures(text) else {
                    continue;
                };
                let mut named = IndexMap::new();
                for name in rule.compiled.capture_names().flatten() {
                    if let Some(m) = caps.name(name) {
                        named.insert(name.to_owned(), m.as_str().to_owned());
                    }
                }
                // Honor `redact`: replace captured value with "<redacted>"
                // before emitting (defense-in-depth).
                for r in &rule.def.redact {
                    if let Some(slot) = named.get_mut(r) {
                        slot.clear();
                        slot.push_str("<redacted>");
                    }
                }
                let injected = inject_full_match(&mut named, text, &rule.def.redact);
                let draft = build_draft(
                    &rule.def,
                    frame,
                    bucket_id,
                    named,
                    &injected,
                    truncated_bytes,
                );
                if let Some(d) = draft {
                    out.push(d);
                }
            }
        }

        out
    }
}

/// Truncate text at the last UTF-8 boundary that fits in
/// [`MAX_SIFT_BYTES`]; return the kept slice and the byte count
/// dropped.
fn cap_text(text: &str) -> (&str, u32) {
    if text.len() <= MAX_SIFT_BYTES {
        return (text, 0);
    }
    let mut end = MAX_SIFT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let dropped = u32::try_from(text.len() - end).unwrap_or(u32::MAX);
    (&text[..end], dropped)
}

/// Inject the matched text under the reserved full-match keys
/// (`line`/`match`/`0`) so a `${line}`/`${match}`/`${0}` summary
/// template renders the matched line (TC ergonomics Phase 2, P-ZERO).
///
/// Returns the reserved keys this call actually INJECTED (those not
/// already present as real named captures). The caller renders the
/// summary against the full map, then collapses the injected synonyms to
/// the single [`terminal_commander_core::CANONICAL_MATCH_KEY`] for storage
/// (TC-E4): the historical `0`/`line`/`match` triple echoed identical
/// bytes three times in every stored event. A real named capture that
/// collides with a reserved key is NOT in the returned set and is never
/// collapsed.
///
/// Two safety guarantees, both required:
/// 1. **Bounded** — the value is clamped to
///    [`terminal_commander_core::MAX_FULL_MATCH_SUMMARY_BYTES`] so a
///    full-match echo cannot widen the per-event summary budget to the
///    ~8 KiB sift cap.
/// 2. **Redaction-honoring** — any already-redacted capture value in
///    `captures` (i.e. a slot holding `<redacted>`) corresponds to a
///    secret the rule author asked to hide; the raw line would re-leak
///    it. We therefore refuse to inject a full match for any rule that
///    declares `redact`, replacing it with a notice instead — the safe
///    default (a rule that redacts cannot also echo the whole line).
fn inject_full_match(captures: &mut Captures, text: &str, redact: &[String]) -> Vec<&'static str> {
    let value = if redact.is_empty() {
        terminal_commander_core::clamp_full_match(text)
    } else {
        // A rule that declares redaction must not echo the raw line:
        // the full match would bypass the per-capture redaction. Refuse
        // and say so, rather than risk leaking a secret.
        "<full match suppressed: rule declares redact>".to_owned()
    };
    let mut injected = Vec::new();
    for key in terminal_commander_core::RESERVED_MATCH_KEYS {
        // Do not clobber a real named capture that happens to collide
        // with a reserved key (named captures win; reserved keys are a
        // fallback for rules without them). Only keys we actually insert
        // are returned for collapse -- a real named capture stays.
        if !captures.contains_key(*key) {
            captures.insert((*key).to_owned(), value.clone());
            injected.push(*key);
        }
    }
    injected
}

/// Collapse the injected full-match reserved synonyms to the single
/// canonical key (TC-E4 / FR-030). Called AFTER the summary is rendered
/// (rendering needs all synonyms present), so the STORED event carries
/// the matched text once under
/// [`terminal_commander_core::CANONICAL_MATCH_KEY`] plus the rule's named
/// captures -- not the redundant `0`/`line`/`match` triple. Only keys this
/// run injected are removed; a real named capture colliding with a
/// reserved name is never touched.
fn collapse_full_match_captures(captures: &mut Captures, injected: &[&'static str]) {
    for key in injected {
        if *key != terminal_commander_core::CANONICAL_MATCH_KEY {
            captures.shift_remove(*key);
        }
    }
}

/// Whether a rule with `Some(rs)` matches a frame's stream. A rule
/// with `None` matches any stream.
fn stream_matches(rule_stream: Option<&SourceStream>, frame_stream: &SourceStream) -> bool {
    rule_stream.is_none_or(|rs| rs == frame_stream)
}

/// Given a (flat pattern index, owning rule index), return the
/// per-rule local index of the keyword.
fn match_index_for_pattern(pat_idx: usize, rule_idx: usize, map: &[usize]) -> usize {
    // Count how many earlier patterns belong to the same rule.
    map[..pat_idx].iter().filter(|&&r| r == rule_idx).count()
}

/// UUID v5 namespace for [`stable_rule_id`] (registry rule id → [`RuleId`]).
///
/// Name input is **only** the registry `id` string; [`RuleRef::version`] remains
/// in the dedupe key (`rule_id|version|kind|captures`).
///
/// **Frozen:** never rotate or replace this UUID, even if registry rule-id format
/// changes. Doing so would change every derived [`RuleId`] and break persisted
/// dedupe expectations.
pub const STABLE_RULE_ID_NAMESPACE: uuid::Uuid =
    uuid::uuid!("a3b5c7d9-e1f2-4a6b-8c0d-1e2f3a4b5c6d");

/// Deterministic [`RuleId`] from a registry rule id string so TC11
/// dedupe keys remain stable across frames from the same rule.
fn stable_rule_id(registry_id: &str) -> terminal_commander_core::RuleId {
    use terminal_commander_core::RuleId;
    use uuid::Uuid;
    RuleId::from_uuid(Uuid::new_v5(
        &STABLE_RULE_ID_NAMESPACE,
        registry_id.as_bytes(),
    ))
}

/// Build an [`EventDraft`] from a rule match. Returns `None` if the
/// rendered summary fails (missing capture in template, etc.).
///
/// `injected` lists the reserved full-match synonyms `inject_full_match`
/// added to `captures` for this match. The summary is rendered against the
/// FULL map (so `${line}`/`${match}`/`${0}` all resolve), then the
/// redundant synonyms are collapsed to the single canonical key for
/// STORAGE (TC-E4): the stored event carries the matched text once, not
/// the historical `0`/`line`/`match` triple.
fn build_draft(
    def: &RuleDefinition,
    frame: &SourceFrame,
    bucket_id: BucketId,
    mut captures: Captures,
    injected: &[&'static str],
    frame_truncated_bytes: u32,
) -> Option<EventDraft> {
    // Try to render the summary; on missing capture, fall back to
    // the raw template (the runtime never panics). Render BEFORE the
    // collapse so every reserved synonym still resolves.
    let summary = match def.render_summary(&captures) {
        Ok(s) => s.0,
        Err(_) => def.summary_template.clone(),
    };

    // TC-E4: collapse the injected full-match synonyms to one canonical
    // key now that the summary is rendered. Real named captures (never in
    // `injected`) are untouched.
    collapse_full_match_captures(&mut captures, injected);

    let pointer = SourcePointer {
        frame_id: frame.frame_id,
        line: frame.line,
        byte_start: frame.byte_offset,
        byte_end: None,
        stream: Some(frame.stream.clone()),
        context_available: true,
    };

    let source = EventSource {
        probe_id: frame.probe_id,
        // We don't know the SourceType here; we set it to Process
        // as a placeholder. The probe layer (TC15+) will set the
        // correct type via its own EventDraft construction or by
        // patching the draft before promotion.
        source_type: terminal_commander_core::SourceType::Process,
        stream: frame.stream.clone(),
        job_id: None,
    };

    let rule_ref = RuleRef {
        id: stable_rule_id(&def.id),
        version: def.version,
    };

    // Severity below Medium does not require a pointer, but we
    // include one anyway (we have one).
    let _ = def.severity == Severity::Trace; // silence dead-code on rank

    let draft = EventDraft {
        bucket_id,
        timestamp: time::OffsetDateTime::now_utc(),
        severity: def.severity,
        kind: def.event_kind.clone(),
        summary,
        rule: Some(rule_ref),
        source,
        captures: Some(captures),
        pointer: Some(pointer),
        pointer_unavailable_reason: None,
        tags: if def.tags.is_empty() {
            None
        } else {
            Some(def.tags.clone())
        },
        frame_truncated_bytes,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    };
    // Drafts are always self-consistent here; validate as a sanity check.
    if draft.validate().is_err() {
        return None;
    }
    Some(draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{BucketId, ProbeId, RuleStatus};

    fn frame(text: &str, stream: SourceStream) -> SourceFrame {
        SourceFrame::new(ProbeId::new(), stream, text.to_owned()).with_line(1)
    }

    fn kw_rule(id: &str, kws: &[&str], stream: Option<SourceStream>) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::Medium,
            event_kind: "kw_match".to_owned(),
            stream,
            description: None,
            pattern: None,
            keywords: Some(kws.iter().map(|s| (*s).to_owned()).collect()),
            captures: vec![],
            summary_template: "matched a keyword".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: terminal_commander_core::ContextHint::default(),
            examples: vec![],
        }
    }

    fn rx_rule(id: &str, pat: &str, kind: &str) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Regex,
            status: RuleStatus::Active,
            severity: Severity::High,
            event_kind: kind.to_owned(),
            stream: Some(SourceStream::Stderr),
            description: None,
            pattern: Some(pat.to_owned()),
            keywords: None,
            captures: vec!["package".to_owned()],
            summary_template: "missing ${package}".to_owned(),
            tags: vec!["apt".to_owned()],
            rate_limit_per_min: Some(30),
            redact: vec![],
            context_hint: terminal_commander_core::ContextHint::default(),
            examples: vec![],
        }
    }

    #[test]
    fn build_empty_runtime() {
        let rt = SifterRuntime::build(&[]).unwrap();
        assert_eq!(rt.rule_count(), 0);
    }

    #[test]
    fn build_rejects_draft_rule() {
        let mut def = kw_rule("kw", &["needle"], None);
        def.status = RuleStatus::Draft;
        let err = SifterRuntime::build(std::slice::from_ref(&def)).unwrap_err();
        assert!(matches!(err, SifterError::NotActive(_)));
    }

    #[test]
    fn build_rejects_reserved_kind() {
        let mut def = kw_rule("kw", &["needle"], None);
        def.kind = RuleType::Threshold;
        // Validation refuses Threshold + Active.
        let err = SifterRuntime::build(std::slice::from_ref(&def)).unwrap_err();
        match err {
            SifterError::Invalid(id, _) => assert_eq!(id, "kw"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    // --- TC ergonomics Phase 2 (P-ZERO): ${line}/${match}/${0} ---

    #[test]
    fn keyword_full_match_template_echoes_line() {
        let mut def = kw_rule("kw", &["error"], None);
        def.summary_template = "${line}".to_owned();
        let rt = SifterRuntime::build(&[def]).unwrap();
        let drafts = rt.evaluate(
            &frame("some error occurred", SourceStream::Stdout),
            BucketId::new(),
        );
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].summary, "some error occurred");
    }

    #[test]
    fn regex_full_match_template_echoes_line() {
        let mut def = rx_rule("re", "(?P<package>[a-z]+)-dev", "missing");
        def.summary_template = "${0}".to_owned();
        let rt = SifterRuntime::build(&[def]).unwrap();
        let drafts = rt.evaluate(
            &frame("E: package libssl-dev missing", SourceStream::Stderr),
            BucketId::new(),
        );
        assert_eq!(drafts.len(), 1);
        // All three reserved keys (line/match/0) resolve to the whole
        // matched LINE — consistent across keyword and regex rules, and
        // the bounded, redaction-aware behavior is identical regardless
        // of which synonym the author uses.
        assert_eq!(drafts[0].summary, "E: package libssl-dev missing");
    }

    #[test]
    fn full_match_summary_is_byte_bounded() {
        let mut def = kw_rule("kw", &["needle"], None);
        def.summary_template = "${line}".to_owned();
        let rt = SifterRuntime::build(&[def]).unwrap();
        // A line far larger than the clamp.
        let big = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
        let drafts = rt.evaluate(&frame(&big, SourceStream::Stdout), BucketId::new());
        assert_eq!(drafts.len(), 1);
        assert!(
            drafts[0].summary.len() <= terminal_commander_core::MAX_FULL_MATCH_SUMMARY_BYTES,
            "summary must be clamped, got {} bytes",
            drafts[0].summary.len()
        );
    }

    #[test]
    fn full_match_suppressed_when_rule_declares_redact() {
        // A rule that redacts a capture must NOT re-leak the raw line
        // via ${line}; the injected value is a suppression notice.
        let mut def = rx_rule("re", "token=(?P<secret>[A-Za-z0-9]+)", "leak");
        def.captures = vec!["secret".to_owned()];
        def.redact = vec!["secret".to_owned()];
        def.summary_template = "${line}".to_owned();
        let rt = SifterRuntime::build(&[def]).unwrap();
        let drafts = rt.evaluate(
            &frame("auth token=abc123XYZ accepted", SourceStream::Stderr),
            BucketId::new(),
        );
        assert_eq!(drafts.len(), 1);
        assert!(
            !drafts[0].summary.contains("abc123XYZ"),
            "redacted secret must not appear via full-match echo: {}",
            drafts[0].summary
        );
        assert!(drafts[0].summary.contains("suppressed"));
    }

    #[test]
    fn named_capture_wins_over_reserved_key_collision() {
        // A real named capture called "line" takes precedence over the
        // injected reserved key.
        let mut def = rx_rule("re", "(?P<line>[A-Z]+)", "tag");
        def.captures = vec!["line".to_owned()];
        def.summary_template = "${line}".to_owned();
        let rt = SifterRuntime::build(&[def]).unwrap();
        let drafts = rt.evaluate(&frame("xx ABC yy", SourceStream::Stderr), BucketId::new());
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].summary, "ABC");
    }

    #[test]
    fn keyword_rule_matches_in_haystack() {
        let rt = SifterRuntime::build(&[kw_rule("kw", &["error", "FAILED"], None)]).unwrap();
        let f = frame("some error occurred", SourceStream::Stdout);
        let bid = BucketId::new();
        let drafts = rt.evaluate(&f, bid);
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert_eq!(d.kind, "kw_match");
        assert_eq!(d.severity, Severity::Medium);
        assert_eq!(
            d.captures
                .as_ref()
                .unwrap()
                .get("keyword")
                .map(String::as_str),
            Some("error")
        );
        assert!(d.pointer.is_some());
        assert_eq!(d.bucket_id, bid);
    }

    #[test]
    fn keyword_rule_one_event_per_rule_per_frame() {
        // Even if both keywords match, we emit one event per rule.
        let rt = SifterRuntime::build(&[kw_rule("kw", &["error", "FAILED"], None)]).unwrap();
        let f = frame("error: FAILED to do thing", SourceStream::Stdout);
        let drafts = rt.evaluate(&f, BucketId::new());
        assert_eq!(drafts.len(), 1);
    }

    #[test]
    fn keyword_rule_stream_filter_excludes_non_match() {
        let rt =
            SifterRuntime::build(&[kw_rule("kw", &["error"], Some(SourceStream::Stderr))]).unwrap();
        let stdout_frame = frame("error here", SourceStream::Stdout);
        assert!(rt.evaluate(&stdout_frame, BucketId::new()).is_empty());
        let stderr_frame = frame("error here", SourceStream::Stderr);
        assert_eq!(rt.evaluate(&stderr_frame, BucketId::new()).len(), 1);
    }

    #[test]
    fn no_match_keyword_returns_empty() {
        let rt = SifterRuntime::build(&[kw_rule("kw", &["zebra"], None)]).unwrap();
        let f = frame("nothing of note here", SourceStream::Stdout);
        assert!(rt.evaluate(&f, BucketId::new()).is_empty());
    }

    #[test]
    fn regex_rule_apt_missing_package_captures_name() {
        let rt = SifterRuntime::build(&[rx_rule(
            "apt-missing",
            r"^E: Unable to locate package (?P<package>[A-Za-z0-9._+-]+)$",
            "missing_package",
        )])
        .unwrap();
        let f = frame(
            "E: Unable to locate package libssl-dev",
            SourceStream::Stderr,
        );
        let drafts = rt.evaluate(&f, BucketId::new());
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert_eq!(d.kind, "missing_package");
        assert_eq!(d.severity, Severity::High);
        assert_eq!(d.summary, "missing libssl-dev");
        assert_eq!(
            d.captures
                .as_ref()
                .unwrap()
                .get("package")
                .map(String::as_str),
            Some("libssl-dev")
        );
        assert!(d.pointer.is_some());
    }

    #[test]
    fn regex_rule_gcc_missing_header_captures() {
        let mut rule = rx_rule(
            "gcc-missing-header",
            r"fatal error: (?P<header>[^:]+): No such file or directory",
            "missing_header",
        );
        rule.captures = vec!["header".to_owned()];
        rule.summary_template = "missing ${header}".to_owned();
        let rt = SifterRuntime::build(&[rule]).unwrap();
        let f = frame(
            "src/foo.c:1:10: fatal error: openssl/ssl.h: No such file or directory",
            SourceStream::Stderr,
        );
        let drafts = rt.evaluate(&f, BucketId::new());
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].summary, "missing openssl/ssl.h");
    }

    #[test]
    fn regex_rule_no_match_silent() {
        let rt = SifterRuntime::build(&[rx_rule(
            "apt-missing",
            r"^E: Unable to locate package (?P<package>\S+)$",
            "missing_package",
        )])
        .unwrap();
        let f = frame("ordinary noise line", SourceStream::Stderr);
        assert!(rt.evaluate(&f, BucketId::new()).is_empty());
    }

    #[test]
    fn frame_oversize_text_is_capped_and_recorded() {
        // Build a rule whose pattern appears near the END of the
        // frame, beyond the cap. After capping, the regex shouldn't
        // match — but the truncated count should be non-zero.
        let rt = SifterRuntime::build(&[rx_rule("needle", r"NEEDLE_(?P<package>\w+)", "found")])
            .unwrap();
        let mut big = "a".repeat(MAX_SIFT_BYTES + 1024);
        big.push_str("NEEDLE_x");
        let f = SourceFrame::new(ProbeId::new(), SourceStream::Stderr, big);
        let drafts = rt.evaluate(&f, BucketId::new());
        // SourceFrame::new already capped the text at
        // MAX_FRAME_BYTES (8192) so by the time we get here the
        // "NEEDLE_x" is already gone. The sifter doesn't see it.
        assert!(drafts.is_empty());
        // The frame itself carries the truncated_bytes record.
        assert!(f.truncated_bytes > 0);
    }

    #[test]
    fn regex_rule_redact_replaces_captured_value() {
        let mut rule = rx_rule("secret", r"token=(?P<token>[A-Za-z0-9_-]+)", "secret_leak");
        rule.captures = vec!["token".to_owned()];
        rule.summary_template = "leaked ${token}".to_owned();
        rule.redact = vec!["token".to_owned()];
        let rt = SifterRuntime::build(&[rule]).unwrap();
        let f = frame("token=abcdef123 trailing", SourceStream::Stderr);
        let drafts = rt.evaluate(&f, BucketId::new());
        assert_eq!(drafts.len(), 1);
        assert_eq!(
            drafts[0]
                .captures
                .as_ref()
                .unwrap()
                .get("token")
                .map(String::as_str),
            Some("<redacted>")
        );
        assert_eq!(drafts[0].summary, "leaked <redacted>");
    }

    #[test]
    fn keyword_and_regex_rules_compose_on_one_frame() {
        let mut rules = vec![
            kw_rule("kw", &["error"], None),
            rx_rule(
                "apt-missing",
                r"^E: Unable to locate package (?P<package>\S+)$",
                "missing_package",
            ),
        ];
        rules[1].stream = Some(SourceStream::Stderr);
        let rt = SifterRuntime::build(&rules).unwrap();
        let f = frame(
            "E: Unable to locate package libssl-dev",
            SourceStream::Stderr,
        );
        let drafts = rt.evaluate(&f, BucketId::new());
        // The keyword rule does not match because the haystack does
        // not contain "error". So only the regex fires.
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, "missing_package");
    }
}
