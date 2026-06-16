// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Rule definition, validation, and template rendering.
//!
//! Wire shape: `tests/fixtures/contracts/rule-definition.v1.json`.
//!
//! Source-status: live (TC09) for keyword and regex rule kinds and
//! their validation + template renderer. The other nine sifter
//! discriminators are reserved in [`RuleType`] but only validate to
//! [`RuleStatus::Draft`] until their implementing goals (TC10/TC11/
//! TC15/TC19/TC20) wire them.

use indexmap::IndexMap;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::ids::RuleId;
use crate::severity::Severity;
use crate::source::SourceStream;

/// Maximum length of a regex pattern in bytes (source-text cap).
///
/// This bounds the input string only; it does NOT bound the compiled state
/// machine size — a short pattern can still compile to a large DFA. Use
/// [`REGEX_SIZE_LIMIT`] / [`REGEX_DFA_SIZE_LIMIT`] via [`compile_bounded_regex`]
/// for the compiled-size bound.
pub const MAX_PATTERN_BYTES: usize = 4096;

/// Hard cap on a compiled regex program's size in bytes.
///
/// Passed to [`regex::RegexBuilder::size_limit`]. Bounds memory from a hostile
/// or pathological pattern that is short on disk but expands when compiled.
pub const REGEX_SIZE_LIMIT: usize = 65_536;

/// Hard cap on the lazy-DFA cache size in bytes.
///
/// Passed to [`regex::RegexBuilder::dfa_size_limit`].
pub const REGEX_DFA_SIZE_LIMIT: usize = 65_536;

/// Compile a regex with the canonical compiled-size bounds.
///
/// Applies [`REGEX_SIZE_LIMIT`] / [`REGEX_DFA_SIZE_LIMIT`]. All runtime
/// rule-regex compilation (rule validation, the sifter runtime, rule-pack
/// import) must go through this so the bound is applied uniformly.
///
/// # Errors
/// Returns the underlying [`regex::Error`] if the pattern is invalid or exceeds
/// the size limits.
pub fn compile_bounded_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .build()
}

/// Hard cap on the per-rule contribution to a [`regex::RegexSet`]'s budget.
///
/// The set-wide budget is `N * REGEX_SET_PER_RULE_LIMIT`, clamped to a
/// workspace-wide ceiling so one runaway rule cannot drag the whole set above
/// the safe range.
pub const REGEX_SET_PER_RULE_LIMIT: usize = 65_536;

/// Workspace-wide ceiling on the combined [`regex::RegexSet`] program size.
///
/// 16 MiB is the regex crate's default for `RegexSet`; we cap there so a
/// pathological caller with thousands of patterns still gets a bounded
/// program, but the per-rule scaling means a healthy ~50-rule set fits.
pub const REGEX_SET_TOTAL_CEILING: usize = 16 * 1024 * 1024;

/// Compile a [`regex::RegexSet`] with compiled-size bounds that scale with
/// the number of patterns.
///
/// Each individual pattern still gets [`REGEX_SET_PER_RULE_LIMIT`] of headroom;
/// the combined program is allowed to grow proportionally, up to
/// [`REGEX_SET_TOTAL_CEILING`]. A flat per-set cap matching the single-pattern
/// limit caused legitimate multi-rule sets to fail to compile once enough
/// rules were active.
///
/// # Errors
/// Returns the underlying [`regex::Error`] if any pattern is invalid or the
/// combined program exceeds the scaled ceiling.
pub fn compile_bounded_regex_set<I, S>(patterns: I) -> Result<regex::RegexSet, regex::Error>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let pats: Vec<S> = patterns.into_iter().collect();
    let scaled = pats
        .len()
        .saturating_mul(REGEX_SET_PER_RULE_LIMIT)
        .min(REGEX_SET_TOTAL_CEILING);
    // Always leave at least the single-pattern budget so a 0/1-pattern set
    // behaves identically to compile_bounded_regex.
    let combined_limit = scaled.max(REGEX_SIZE_LIMIT);
    regex::RegexSetBuilder::new(pats)
        .size_limit(combined_limit)
        .dfa_size_limit(combined_limit)
        .build()
}

/// Maximum length of a single tag string.
pub const MAX_TAG_BYTES: usize = 64;

/// Maximum number of tags per rule.
pub const MAX_TAGS: usize = 16;

/// Maximum number of examples per rule (kept small to bound
/// per-rule storage).
pub const MAX_EXAMPLES: usize = 16;

/// Maximum length of a rule identifier string.
pub const MAX_RULE_ID_BYTES: usize = 128;

/// Maximum length of a summary template.
pub const MAX_TEMPLATE_BYTES: usize = 512;

/// Hard cap on context-hint frame counts.
pub const MAX_CONTEXT_LINES: u32 = 1024;

/// Reserved `summary_template` keys resolving to the whole matched text.
///
/// The three are synonyms: `${line}`, `${match}`, `${0}`. They are
/// accepted by template validation without appearing in a rule's
/// `captures` list; the sifter runtime injects the (bounded,
/// redaction-scrubbed) matched text under these keys at render time.
pub const RESERVED_MATCH_KEYS: &[&str] = &["line", "match", "0"];

/// The single canonical full-match capture key (TC-E4 / FR-030).
///
/// All three reserved synonyms still render in summary templates, but the
/// sifter STORES the matched text under only this one key plus the rule's
/// named captures -- collapsing the historical `0`/`line`/`match` triple
/// that echoed identical bytes three times. MUST be one of
/// [`RESERVED_MATCH_KEYS`].
pub const CANONICAL_MATCH_KEY: &str = "match";

/// Render-time byte clamp on a substituted reserved full-match value.
///
/// The matched frame text is bounded upstream at `MAX_SIFT_BYTES`
/// (~8 KiB); without this clamp a `${line}` template would let a single
/// rule-match summary balloon to that size. 256 bytes is enough to
/// identify the matched line while keeping the per-event budget tight.
pub const MAX_FULL_MATCH_SUMMARY_BYTES: usize = 256;

/// Whether `name` is a reserved full-match placeholder (see
/// [`RESERVED_MATCH_KEYS`]).
#[must_use]
pub fn is_reserved_match_key(name: &str) -> bool {
    RESERVED_MATCH_KEYS.contains(&name)
}

/// Clamp a full-match value to [`MAX_FULL_MATCH_SUMMARY_BYTES`].
///
/// Truncates at a UTF-8 boundary. Used by the sifter runtime before
/// injecting matched text under a reserved key so `${line}` can never
/// widen the per-event summary budget.
#[must_use]
pub fn clamp_full_match(text: &str) -> String {
    if text.len() <= MAX_FULL_MATCH_SUMMARY_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_FULL_MATCH_SUMMARY_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// The 11 canonical sifter discriminators. Eight of these are
/// reserved-not-implemented at MVP; rules referencing them MUST
/// remain in [`RuleStatus::Draft`] until their implementing goal
/// activates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    Keyword,
    Regex,
    Prompt,
    ExitCode,
    StreamMarker,
    ProgressCollapse,
    Dedupe,
    Threshold,
    Sequence,
    Anchor,
    Custom,
}

impl RuleType {
    /// Whether this discriminator has a live MVP sifter behind it.
    /// Used by validation to enforce that reserved variants stay
    /// in `Draft`.
    #[must_use]
    pub const fn is_mvp_live(self) -> bool {
        matches!(self, Self::Keyword | Self::Regex)
    }
}

/// Rule lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    /// Authored but not yet bound to any probe. Default.
    Draft,
    /// Bound and live; sifter runtime will evaluate against frames.
    Active,
    /// Bound but turned off. Definition retained.
    Disabled,
    /// Superseded by a newer version. Read-only.
    Deprecated,
    /// Soft-deleted. Definition retained so that historical events
    /// referencing this rule id can still resolve.
    Tombstoned,
}

impl RuleStatus {
    /// Whether the runtime should evaluate this rule against frames.
    #[must_use]
    pub const fn is_runtime_eligible(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Hint to the context-window resolver: how many frames before/
/// after the matching frame an event consumer typically wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextHint {
    pub before_lines: u32,
    pub after_lines: u32,
}

/// A worked example. Used by the rule registry browser and by
/// `registry_test` (TC24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleExample {
    /// Stream label (informational; matched against incoming frames
    /// when present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<SourceStream>,
    /// Input text the rule should be evaluated against.
    pub input: String,
    /// Expected outcome shape.
    pub expect: RuleExampleExpect,
}

/// What an example expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuleExampleExpect {
    /// Expected match with captures.
    Match {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        captures: IndexMap<String, String>,
    },
    /// Expected non-match.
    NoMatch {
        #[serde(rename = "match")]
        match_: bool,
    },
}

/// A canonical rule definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDefinition {
    /// Human-readable rule id (also indexed in the registry).
    /// Distinct from [`RuleId`], which is the typed-id wire form.
    pub id: String,
    pub version: u32,
    pub kind: RuleType,
    #[serde(default = "default_status")]
    pub status: RuleStatus,
    pub severity: Severity,
    /// Event kind to emit when the rule matches.
    pub event_kind: String,
    /// Stream the rule applies to. `None` means any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<SourceStream>,
    /// Free-text description, optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// For `keyword` and `regex` kinds: the pattern / keyword set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    /// Names of expected named captures (regex) or keyword tokens.
    #[serde(default)]
    pub captures: Vec<String>,
    /// Summary template using `${name}` placeholders.
    pub summary_template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional per-rule rate limit (events per minute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_min: Option<u32>,
    /// Optional list of capture names whose values should be
    /// redacted before emission (defense-in-depth against secrets).
    #[serde(default)]
    pub redact: Vec<String>,
    #[serde(default)]
    pub context_hint: ContextHint,
    #[serde(default)]
    pub examples: Vec<RuleExample>,
}

const fn default_status() -> RuleStatus {
    RuleStatus::Draft
}

/// Optional typed link from `RuleDefinition.id` (string) to a
/// `RuleId` (typed uuid). Wire form uses the human id; the typed id
/// is registry-internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleHandle {
    pub typed_id: RuleId,
    pub version: u32,
}

/// Errors produced by rule validation and template rendering.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuleError {
    #[error("rule id is empty")]
    EmptyRuleId,
    #[error("rule id '{0}' exceeds the {MAX_RULE_ID_BYTES}-byte limit")]
    RuleIdTooLong(String),
    #[error("rule version must be >= 1, got {0}")]
    InvalidVersion(u32),
    #[error("rule '{0}' has empty event_kind")]
    EmptyEventKind(String),
    #[error("rule '{id}' kind={kind:?} requires a non-empty pattern")]
    MissingPattern { id: String, kind: RuleType },
    #[error("rule '{id}' kind={kind:?} requires a non-empty keywords list")]
    MissingKeywords { id: String, kind: RuleType },
    #[error("rule '{0}' pattern exceeds the {MAX_PATTERN_BYTES}-byte limit")]
    PatternTooLong(String),
    #[error("rule '{id}' pattern contains forbidden construct {feature}: {detail}")]
    ForbiddenPatternFeature {
        id: String,
        feature: &'static str,
        detail: String,
    },
    #[error("rule '{id}' pattern failed to compile: {reason}")]
    PatternCompileFailed { id: String, reason: String },
    #[error("rule '{0}' has empty summary_template")]
    EmptySummaryTemplate(String),
    #[error("rule '{0}' summary_template exceeds the {MAX_TEMPLATE_BYTES}-byte limit")]
    SummaryTemplateTooLong(String),
    #[error(
        "rule '{id}' summary_template references capture '{name}' that is not in `captures` list"
    )]
    SummaryTemplateUnknownCapture { id: String, name: String },
    #[error("rule '{id}' has unbalanced template placeholder near offset {offset}")]
    UnbalancedTemplate { id: String, offset: usize },
    #[error("rule '{0}' carries more than {MAX_TAGS} tags")]
    TooManyTags(String),
    #[error(
        "rule '{id}' tag '{tag}' is empty or exceeds {MAX_TAG_BYTES} bytes or is not lowercase"
    )]
    InvalidTag { id: String, tag: String },
    #[error("rule '{0}' carries more than {MAX_EXAMPLES} examples")]
    TooManyExamples(String),
    #[error(
        "rule '{id}' context_hint exceeds the {MAX_CONTEXT_LINES}-line cap (before={before}, after={after})"
    )]
    ContextHintTooLarge { id: String, before: u32, after: u32 },
    #[error(
        "rule '{id}' kind={kind:?} is reserved-not-implemented at MVP; status must remain Draft, got {status:?}"
    )]
    ReservedKindNotDraft {
        id: String,
        kind: RuleType,
        status: RuleStatus,
    },
}

/// Result of a successful render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSummary(pub String);

/// Reasons a render can fail.
///
/// Distinct from [`RuleError`] because rendering happens at runtime
/// against potentially-absent captures and the caller often wants
/// to fall back instead of treating the situation as a validation
/// failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("missing capture '{0}' while rendering summary")]
    MissingCapture(String),
    #[error("unbalanced ${{...}} placeholder near offset {0}")]
    Unbalanced(usize),
}

impl RuleDefinition {
    /// Validate the rule. Returns `Ok(())` if every field is
    /// well-formed for its `kind`.
    ///
    /// Validation enforces:
    /// - non-empty id (<= 128 bytes);
    /// - version >= 1;
    /// - non-empty event_kind;
    /// - per-kind fields (pattern for `regex`, keywords for `keyword`);
    /// - regex compiles and contains no backreferences/lookaround
    ///   (regex crate already rejects these; we surface a friendly
    ///   error);
    /// - pattern <= 4096 bytes;
    /// - summary template <= 512 bytes, balanced placeholders, and
    ///   every `${name}` references a capture listed in `captures`;
    /// - tags lowercase, <= 64 bytes each, <= 16 entries;
    /// - examples <= 16 entries;
    /// - context_hint fits within MAX_CONTEXT_LINES on each axis;
    /// - reserved (non-MVP) kinds must carry RuleStatus::Draft.
    pub fn validate(&self) -> Result<(), RuleError> {
        // id
        if self.id.is_empty() {
            return Err(RuleError::EmptyRuleId);
        }
        if self.id.len() > MAX_RULE_ID_BYTES {
            return Err(RuleError::RuleIdTooLong(self.id.clone()));
        }

        // version
        if self.version == 0 {
            return Err(RuleError::InvalidVersion(self.version));
        }

        // event_kind
        if self.event_kind.is_empty() {
            return Err(RuleError::EmptyEventKind(self.id.clone()));
        }

        // per-kind body
        match self.kind {
            RuleType::Regex => {
                let pat = self.pattern.as_deref().unwrap_or("");
                if pat.is_empty() {
                    return Err(RuleError::MissingPattern {
                        id: self.id.clone(),
                        kind: self.kind,
                    });
                }
                if pat.len() > MAX_PATTERN_BYTES {
                    return Err(RuleError::PatternTooLong(self.id.clone()));
                }
                check_forbidden_regex_constructs(&self.id, pat)?;
                compile_bounded_regex(pat).map_err(|e| RuleError::PatternCompileFailed {
                    id: self.id.clone(),
                    reason: e.to_string(),
                })?;
            }
            RuleType::Keyword => {
                let kws = self.keywords.as_deref().unwrap_or(&[]);
                if kws.is_empty() || kws.iter().any(String::is_empty) {
                    return Err(RuleError::MissingKeywords {
                        id: self.id.clone(),
                        kind: self.kind,
                    });
                }
            }
            // Reserved variants do not require pattern/keywords at
            // MVP; they must just stay in Draft.
            _ => {}
        }

        // Reserved kinds must stay in Draft until their implementing
        // goal activates them.
        if !self.kind.is_mvp_live() && self.status != RuleStatus::Draft {
            return Err(RuleError::ReservedKindNotDraft {
                id: self.id.clone(),
                kind: self.kind,
                status: self.status,
            });
        }

        // summary template
        if self.summary_template.is_empty() {
            return Err(RuleError::EmptySummaryTemplate(self.id.clone()));
        }
        if self.summary_template.len() > MAX_TEMPLATE_BYTES {
            return Err(RuleError::SummaryTemplateTooLong(self.id.clone()));
        }
        validate_template_against_captures(&self.id, &self.summary_template, &self.captures)?;

        // tags
        if self.tags.len() > MAX_TAGS {
            return Err(RuleError::TooManyTags(self.id.clone()));
        }
        for tag in &self.tags {
            if tag.is_empty() || tag.len() > MAX_TAG_BYTES || tag != &tag.to_lowercase() {
                return Err(RuleError::InvalidTag {
                    id: self.id.clone(),
                    tag: tag.clone(),
                });
            }
        }

        // examples
        if self.examples.len() > MAX_EXAMPLES {
            return Err(RuleError::TooManyExamples(self.id.clone()));
        }

        // context hint
        if self.context_hint.before_lines > MAX_CONTEXT_LINES
            || self.context_hint.after_lines > MAX_CONTEXT_LINES
        {
            return Err(RuleError::ContextHintTooLarge {
                id: self.id.clone(),
                before: self.context_hint.before_lines,
                after: self.context_hint.after_lines,
            });
        }

        Ok(())
    }

    /// Render the summary template against a capture map.
    ///
    /// Returns [`RenderError::MissingCapture`] when a `${name}`
    /// placeholder has no value in `captures`. Returns
    /// [`RenderError::Unbalanced`] when a `${` is not followed by
    /// `}`. Never panics.
    pub fn render_summary(
        &self,
        captures: &IndexMap<String, String>,
    ) -> Result<RenderedSummary, RenderError> {
        render_template(&self.summary_template, captures).map(RenderedSummary)
    }
}

/// Reject the regex constructs that this MVP forbids: backreferences
/// (`\1`-`\9`) and lookaround (`(?=`, `(?!`, `(?<=`, `(?<!`). The
/// `regex` crate would already refuse to compile these, but
/// surfacing a friendly error helps rule authors.
fn check_forbidden_regex_constructs(id: &str, pat: &str) -> Result<(), RuleError> {
    // Lookaround prefixes are unambiguous.
    let lookarounds = ["(?=", "(?!", "(?<=", "(?<!"];
    for la in lookarounds {
        if pat.contains(la) {
            return Err(RuleError::ForbiddenPatternFeature {
                id: id.to_owned(),
                feature: "lookaround",
                detail: format!("pattern contains '{la}'"),
            });
        }
    }
    // Backreferences: \1..\9 unescaped. \\1 is fine (literal).
    let bytes = pat.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            if n.is_ascii_digit() && n != b'0' {
                return Err(RuleError::ForbiddenPatternFeature {
                    id: id.to_owned(),
                    feature: "backreference",
                    detail: format!("pattern contains '\\{}'", n as char),
                });
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    Ok(())
}

/// Validate template placeholders at compile time (rule registration).
/// Returns errors compatible with [`RuleError`]; the runtime renderer
/// returns [`RenderError`] instead.
fn validate_template_against_captures(
    id: &str,
    template: &str,
    captures: &[String],
) -> Result<(), RuleError> {
    let mut i = 0;
    let bytes = template.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let start = i + 2;
            let Some(rel_end) = template[start..].find('}') else {
                return Err(RuleError::UnbalancedTemplate {
                    id: id.to_owned(),
                    offset: i,
                });
            };
            let name = &template[start..start + rel_end];
            // Reserved full-match keys (${line}/${match}/${0}) are
            // always valid: the sifter runtime injects the matched text
            // for them, so they need not appear in `captures`.
            if !is_reserved_match_key(name) && !captures.iter().any(|c| c == name) {
                return Err(RuleError::SummaryTemplateUnknownCapture {
                    id: id.to_owned(),
                    name: name.to_owned(),
                });
            }
            i = start + rel_end + 1;
            continue;
        }
        i += 1;
    }
    Ok(())
}

fn render_template(
    template: &str,
    captures: &IndexMap<String, String>,
) -> Result<String, RenderError> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let start = i + 2;
            let Some(rel_end) = template[start..].find('}') else {
                return Err(RenderError::Unbalanced(i));
            };
            let name = &template[start..start + rel_end];
            let Some(v) = captures.get(name) else {
                return Err(RenderError::MissingCapture(name.to_owned()));
            };
            out.push_str(v);
            i = start + rel_end + 1;
            continue;
        }
        // Push the next char (UTF-8 aware).
        let ch_len = template[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&template[i..i + ch_len]);
        i += ch_len;
    }
    Ok(out)
}

/// Input shape for `registry_test` (TC24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleTestRequest {
    pub rule: RuleDefinition,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<SourceStream>,
}

/// Output shape for `registry_test`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleTestResult {
    pub matched: bool,
    #[serde(default)]
    pub captures: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_bounded_regex_rejects_dfa_bomb_that_unbounded_accepts() {
        // A short pattern whose compiled program is large: nested bounded
        // repetition multiplies the state count well past REGEX_SIZE_LIMIT.
        // Unbounded `Regex::new` accepts it (default 10MB cap); the bounded
        // compiler must reject it. This proves the bound actually bites.
        let bomb = "(a|b|c|d|e|f|g|h){60}{60}";
        assert!(bomb.len() <= MAX_PATTERN_BYTES, "bomb fits the byte cap");
        assert!(
            Regex::new(bomb).is_ok(),
            "unbounded compile accepts the bomb (sanity)"
        );
        assert!(
            compile_bounded_regex(bomb).is_err(),
            "bounded compile must reject a pattern exceeding REGEX_SIZE_LIMIT"
        );
    }

    #[test]
    fn compile_bounded_regex_accepts_ordinary_pattern() {
        assert!(compile_bounded_regex(r"ERROR \d{3}: .*").is_ok());
    }

    #[test]
    fn compile_bounded_regex_set_handles_realistic_active_rule_count() {
        // Regression: a flat per-set 64 KiB cap rejected legitimate sets once
        // ~10+ active rules combined into one program. The seeded
        // wsl/cargo rule packs ship 15 active rules out of the box, and the
        // daemon failed to build any sifter at all because the combined NFA
        // crossed the per-pattern limit. The fixed limit scales with N so
        // realistic active-rule counts compile cleanly.
        let pats: Vec<String> = (0..30)
            .map(|i| format!(r"^EVENT_{i}\s+code=(?P<code>E[0-9]{{4}})\s+msg=(?P<msg>.+)$"))
            .collect();
        let set = compile_bounded_regex_set(&pats).expect(
            "30 ordinary captured-group patterns must compile inside the scaled RegexSet budget",
        );
        assert_eq!(set.len(), 30);
    }

    #[test]
    fn compile_bounded_regex_set_still_caps_pathological_growth() {
        // The scaled limit is bounded by REGEX_SET_TOTAL_CEILING (16 MiB), so a
        // truly pathological per-pattern DFA bomb must still be rejected when
        // it would push the combined program past the ceiling.
        let bomb = "(a|b|c|d|e|f|g|h){60}{60}";
        let bombs: Vec<&str> = std::iter::repeat_n(bomb, 4).collect();
        assert!(
            compile_bounded_regex_set(bombs).is_err(),
            "bounded RegexSet must still reject patterns whose combined program exceeds the ceiling"
        );
    }

    fn k(id: &str) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Draft,
            severity: Severity::Medium,
            event_kind: "test".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["needle".to_owned()]),
            captures: vec![],
            summary_template: "found needle".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    fn r(id: &str, pat: &str) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Regex,
            status: RuleStatus::Draft,
            severity: Severity::High,
            event_kind: "missing_package".to_owned(),
            stream: Some(SourceStream::Stderr),
            description: Some("APT missing package".to_owned()),
            pattern: Some(pat.to_owned()),
            keywords: None,
            captures: vec!["package".to_owned()],
            summary_template: "missing ${package}".to_owned(),
            tags: vec!["apt".to_owned()],
            rate_limit_per_min: Some(30),
            redact: vec![],
            context_hint: ContextHint {
                before_lines: 3,
                after_lines: 1,
            },
            examples: vec![],
        }
    }

    #[test]
    fn rule_valid_keyword() {
        k("kw").validate().unwrap();
    }

    #[test]
    fn rule_valid_regex() {
        r(
            "re",
            "^E: Unable to locate package (?P<package>[A-Za-z0-9._+-]+)$",
        )
        .validate()
        .unwrap();
    }

    #[test]
    fn rule_invalid_empty_id() {
        assert_eq!(k("").validate().unwrap_err(), RuleError::EmptyRuleId);
    }

    #[test]
    fn rule_invalid_zero_version() {
        let mut def = k("kw");
        def.version = 0;
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::InvalidVersion(0)
        ));
    }

    #[test]
    fn rule_invalid_missing_keywords() {
        let mut def = k("kw");
        def.keywords = Some(vec![]);
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::MissingKeywords { .. }
        ));
    }

    #[test]
    fn rule_invalid_missing_pattern() {
        let mut def = r("re", "");
        def.pattern = None;
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::MissingPattern { .. }
        ));
    }

    #[test]
    fn rule_invalid_pattern_too_long() {
        let pat = "a".repeat(MAX_PATTERN_BYTES + 1);
        let def = r("re", &pat);
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::PatternTooLong(_)
        ));
    }

    #[test]
    fn rule_invalid_lookaround_rejected() {
        let def = r("re", r"foo(?=bar)");
        match def.validate().unwrap_err() {
            RuleError::ForbiddenPatternFeature { feature, .. } => {
                assert_eq!(feature, "lookaround");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rule_invalid_backreference_rejected() {
        let def = r("re", r"(foo)\1");
        match def.validate().unwrap_err() {
            RuleError::ForbiddenPatternFeature { feature, .. } => {
                assert_eq!(feature, "backreference");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rule_invalid_regex_compile_failure() {
        // Unclosed group; not a forbidden construct so falls through
        // to compile-error path.
        let def = r("re", "(unclosed");
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::PatternCompileFailed { .. }
        ));
    }

    #[test]
    fn rule_invalid_template_references_unknown_capture() {
        let mut def = r("re", "(?P<package>[A-Za-z]+)");
        def.summary_template = "missing ${ghost}".to_owned();
        match def.validate().unwrap_err() {
            RuleError::SummaryTemplateUnknownCapture { name, .. } => {
                assert_eq!(name, "ghost");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rule_invalid_template_unbalanced() {
        let mut def = k("kw");
        def.summary_template = "broken ${name".to_owned();
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::UnbalancedTemplate { .. }
        ));
    }

    #[test]
    fn rule_invalid_tag_uppercase_rejected() {
        let mut def = k("kw");
        def.tags = vec!["Bad".to_owned()];
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::InvalidTag { .. }
        ));
    }

    #[test]
    fn rule_invalid_context_hint_too_large() {
        let mut def = k("kw");
        def.context_hint.before_lines = MAX_CONTEXT_LINES + 1;
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::ContextHintTooLarge { .. }
        ));
    }

    #[test]
    fn rule_reserved_kind_must_stay_draft() {
        let mut def = k("kw");
        def.kind = RuleType::Threshold;
        def.status = RuleStatus::Active;
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::ReservedKindNotDraft { .. }
        ));
        // But Draft is fine.
        def.status = RuleStatus::Draft;
        def.validate().unwrap();
    }

    #[test]
    fn rule_render_summary_substitutes_named() {
        let def = r("re", "(?P<package>[A-Za-z0-9._+-]+)");
        let mut caps = IndexMap::new();
        caps.insert("package".to_owned(), "libssl-dev".to_owned());
        let out = def.render_summary(&caps).unwrap();
        assert_eq!(out.0, "missing libssl-dev");
    }

    #[test]
    fn rule_render_summary_missing_capture_errors() {
        let def = r("re", "(?P<package>[A-Za-z0-9._+-]+)");
        let caps = IndexMap::new();
        match def.render_summary(&caps).unwrap_err() {
            RenderError::MissingCapture(n) => assert_eq!(n, "package"),
            other @ RenderError::Unbalanced(_) => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rule_render_summary_preserves_utf8_and_non_placeholder_text() {
        let mut def = r("re", "(?P<package>[A-Za-z]+)");
        def.summary_template = "pkg=${package} \u{1F680}".to_owned();
        let mut caps = IndexMap::new();
        caps.insert("package".to_owned(), "x".to_owned());
        let out = def.render_summary(&caps).unwrap();
        assert_eq!(out.0, "pkg=x \u{1F680}");
    }

    // --- TC ergonomics Phase 2 (P-ZERO): reserved full-match keys ---

    #[test]
    fn template_accepts_reserved_match_keys_without_captures() {
        // ${line}/${match}/${0} validate even with an empty captures
        // list — the sifter injects them at render time.
        for tok in ["line", "match", "0"] {
            let mut def = k("kw");
            def.summary_template = format!("saw ${{{tok}}}");
            def.captures = vec![];
            def.validate()
                .unwrap_or_else(|e| panic!("${{{tok}}} must validate: {e}"));
        }
    }

    #[test]
    fn template_still_rejects_unknown_non_reserved_capture() {
        let mut def = k("kw");
        def.summary_template = "saw ${ghost}".to_owned();
        def.captures = vec![];
        assert!(matches!(
            def.validate().unwrap_err(),
            RuleError::SummaryTemplateUnknownCapture { .. }
        ));
    }

    #[test]
    fn is_reserved_match_key_matches_synonyms_only() {
        assert!(is_reserved_match_key("line"));
        assert!(is_reserved_match_key("match"));
        assert!(is_reserved_match_key("0"));
        assert!(!is_reserved_match_key("package"));
        assert!(!is_reserved_match_key("1"));
    }

    #[test]
    fn clamp_full_match_bounds_and_respects_utf8() {
        let short = "abc";
        assert_eq!(clamp_full_match(short), "abc");

        let long = "x".repeat(MAX_FULL_MATCH_SUMMARY_BYTES + 50);
        assert_eq!(clamp_full_match(&long).len(), MAX_FULL_MATCH_SUMMARY_BYTES);

        // Multi-byte char straddling the boundary must not be split.
        let mut s = "a".repeat(MAX_FULL_MATCH_SUMMARY_BYTES - 1);
        s.push('\u{1F680}'); // 4 bytes, crosses the cap
        let out = clamp_full_match(&s);
        assert!(out.len() <= MAX_FULL_MATCH_SUMMARY_BYTES);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn render_summary_resolves_reserved_key_from_injected_value() {
        // The runtime injects the matched text under the reserved key;
        // render_summary then resolves ${line} like any capture.
        let mut def = k("kw");
        def.summary_template = "line: ${line}".to_owned();
        let mut caps = IndexMap::new();
        caps.insert("line".to_owned(), "error: boom".to_owned());
        assert_eq!(def.render_summary(&caps).unwrap().0, "line: error: boom");
    }

    #[test]
    fn rule_fixture_json_round_trip() {
        // Mirror tests/fixtures/contracts/rule-definition.v1.json.
        // The fixture has _meta; production payload does not. We
        // construct the production payload here and round-trip it.
        let def = r(
            "apt-missing-package",
            "^E: Unable to locate package (?P<package>[A-Za-z0-9._+-]+)$",
        );
        def.validate().unwrap();
        let s = serde_json::to_string(&def).unwrap();
        let back: RuleDefinition = serde_json::from_str(&s).unwrap();
        assert_eq!(back, def);
    }

    #[test]
    fn committed_rule_fixture_parses_validates_and_renders() {
        // The canonical wire example MUST be real: deserialize the
        // ACTUAL committed fixture (not a Rust reconstruction), validate
        // it, and prove its summary_template substitutes a capture. This
        // guards the three divergences that previously made the shipped
        // example un-parseable (missing event_kind; context_hint
        // before/after vs before_lines/after_lines; {x} vs ${x}).
        let raw = std::fs::read_to_string("../../tests/fixtures/contracts/rule-definition.v1.json")
            .expect("read rule-definition fixture");
        let mut value: serde_json::Value = serde_json::from_str(&raw).expect("fixture is JSON");
        // The fixture carries a documentation-only _meta envelope; the
        // production payload is the object minus that key.
        value
            .as_object_mut()
            .expect("fixture is an object")
            .remove("_meta");
        let def: RuleDefinition =
            serde_json::from_value(value).expect("fixture must deserialize as RuleDefinition");
        def.validate().expect("fixture rule must validate");
        assert!(!def.event_kind.is_empty(), "event_kind must be present");
        assert_eq!(def.context_hint.before_lines, 3);
        assert_eq!(def.context_hint.after_lines, 1);

        // The summary_template must actually substitute (uses ${package}).
        let mut caps = IndexMap::new();
        caps.insert("package".to_owned(), "libssl-dev".to_owned());
        let rendered = def.render_summary(&caps).expect("render").0;
        assert!(
            rendered.contains("libssl-dev") && !rendered.contains("${"),
            "summary must substitute the capture, got: {rendered}"
        );
    }

    #[test]
    fn rule_status_runtime_eligible_only_for_active() {
        assert!(RuleStatus::Active.is_runtime_eligible());
        for s in [
            RuleStatus::Draft,
            RuleStatus::Disabled,
            RuleStatus::Deprecated,
            RuleStatus::Tombstoned,
        ] {
            assert!(!s.is_runtime_eligible(), "{s:?}");
        }
    }
}
