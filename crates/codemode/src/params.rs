// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Strict serde input structs, `serde_ignored` keyword-argument validation,
//! and inline `RuleInput` compilation matching `crates/mcp/src/tools.rs`.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use terminal_commander_core::ids::{
    BucketIdKind, EventIdKind, JobIdKind, ProbeIdKind, SessionIdKind, TypedId, TypedIdKind,
};
use terminal_commander_core::{
    ActivationScope, BucketConfig, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
    SourceStream, WakeMode,
};
use terminal_commander_ipc::{
    BucketEventsSinceParams, BucketWaitParams, CommandStartParams, EventContextParams,
    JobLimitsSpec, JobPriority, ReceiptShape, ShellExecParams, SubscriptionPredicate,
    SubscriptionSourceSel,
};

use super::backend::CodeBackendError;
use super::catalog::TcCodeOp;

/// Strict zero-parameter input struct for 0-arg operations (`health`, `policy_status`, etc.).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmptyParams {}

/// Strict decoder for a `TcCodeOp`'s keyword argument map.
///
/// Uses `serde_ignored::deserialize` so any unconsumed keyword argument
/// (or any keyword argument passed to a zero-parameter op) raises a
/// Python `TypeError` (`CodeBackendError::type_error`) naming the offending
/// keyword and the 1-line `def <op>(...)` signature. Missing required fields
/// or wrong argument types also map to `TypeError` with the signature hint.
pub fn decode_op_kwargs<T: DeserializeOwned>(
    op: TcCodeOp,
    kwargs: Map<String, Value>,
) -> Result<T, CodeBackendError> {
    let mut unused: Vec<String> = Vec::new();
    let res = serde_ignored::deserialize::<_, _, T>(Value::Object(kwargs), |path| {
        let p = path.to_string();
        if !p.is_empty() {
            unused.push(p);
        }
    });
    match res {
        Ok(val) => {
            if let Some(first) = unused.first() {
                return Err(CodeBackendError::type_error(format!(
                    "{op}(): unexpected keyword argument '{first}'; signature: {}",
                    op.signature_one_liner()
                )));
            }
            Ok(val)
        }
        Err(serde_err) => {
            if let Some(first) = unused.first() {
                return Err(CodeBackendError::type_error(format!(
                    "{op}(): unexpected keyword argument '{first}'; signature: {}",
                    op.signature_one_liner()
                )));
            }
            let raw_msg = serde_err.to_string();
            if let Some(unknown) = extract_unknown_field(&raw_msg) {
                return Err(CodeBackendError::type_error(format!(
                    "{op}(): unexpected keyword argument '{unknown}'; signature: {}",
                    op.signature_one_liner()
                )));
            }
            Err(CodeBackendError::type_error(format!(
                "{op}(): {raw_msg}; signature: {}",
                op.signature_one_liner()
            )))
        }
    }
}

fn extract_unknown_field(msg: &str) -> Option<&str> {
    let rest = msg.strip_prefix("unknown field `")?;
    rest.split('`').next()
}

pub fn parse_id<K: TypedIdKind>(field: &str, raw: &str) -> Result<TypedId<K>, String> {
    TypedId::<K>::parse_wire(raw).map_err(|e| format!("{field}: {e}"))
}

pub fn parse_severity_filter(s: &str) -> Result<Severity, String> {
    match s {
        "trace" => Ok(Severity::Trace),
        "debug" => Ok(Severity::Debug),
        "info" => Ok(Severity::Info),
        "low" => Ok(Severity::Low),
        "medium" | "warn" | "warning" => Ok(Severity::Medium),
        "high" | "error" | "err" => Ok(Severity::High),
        "critical" | "fatal" => Ok(Severity::Critical),
        other => Err(format!(
            "severity_min '{other}' is not one of trace|debug|info|low|medium|high|critical"
        )),
    }
}

pub fn parse_source_stream(name: &str) -> Result<SourceStream, String> {
    match name {
        "stdout" => Ok(SourceStream::Stdout),
        "stderr" => Ok(SourceStream::Stderr),
        "meta" => Ok(SourceStream::Meta),
        other => Err(format!(
            "stream '{other}' must be one of stdout|stderr|meta"
        )),
    }
}

const fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "a map",
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
}

fn deserialize_env<'de, D>(deserializer: D) -> Result<Vec<EnvEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let items = Vec::<EnvEntry>::deserialize(deserializer).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("invalid type: map") {
            D::Error::custom(
                "env must be an ARRAY of {\"key\":\"NAME\",\"value\":\"VAL\"} objects, not a map; \
                 e.g. [{\"key\":\"FOO\",\"value\":\"bar\"}]",
            )
        } else {
            e
        }
    })?;
    Ok(items)
}

fn deserialize_argv<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    Vec::<String>::deserialize(deserializer).map_err(|e| {
        D::Error::custom(format!(
            "argv must be an array of strings, e.g. [\"cargo\",\"test\"]; {e}"
        ))
    })
}

fn opt_u64_from_value<E: serde::de::Error>(value: Option<LenientScalar>) -> Result<Option<u64>, E> {
    match value {
        None => Ok(None),
        Some(LenientScalar::Uint(u)) => Ok(Some(u)),
        Some(LenientScalar::Int(i)) => u64::try_from(i)
            .map(Some)
            .map_err(|_| E::custom(format!("expected an unsigned integer (e.g. 5000); got {i}"))),
        Some(LenientScalar::Str(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            t.parse::<u64>().map(Some).map_err(|_| {
                E::custom(format!(
                    "expected an unsigned integer as a number or numeric string (e.g. 5000); got \"{s}\""
                ))
            })
        }
        Some(LenientScalar::Bool(b)) => Err(E::custom(format!(
            "expected an unsigned integer; got boolean {b}"
        ))),
        Some(LenientScalar::Float(f)) => Err(E::custom(format!(
            "expected an unsigned integer; got float {f}"
        ))),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LenientScalar {
    Bool(bool),
    Uint(u64),
    Int(i64),
    Float(f64),
    Str(String),
}

fn opt_uint_narrow<T, E>(value: Option<LenientScalar>) -> Result<Option<T>, E>
where
    T: TryFrom<u64>,
    E: serde::de::Error,
{
    opt_u64_from_value::<E>(value)?.map_or_else(
        || Ok(None),
        |v| {
            T::try_from(v)
                .map(Some)
                .map_err(|_| E::custom(format!("{v} exceeds this parameter's accepted range")))
        },
    )
}

fn de_opt_u64_lenient<'de, D>(de: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_u64_from_value(Option::<LenientScalar>::deserialize(de)?)
}

fn de_opt_u32_lenient<'de, D>(de: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_uint_narrow(Option::<LenientScalar>::deserialize(de)?)
}

fn de_opt_u16_lenient<'de, D>(de: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_uint_narrow(Option::<LenientScalar>::deserialize(de)?)
}

fn de_opt_usize_lenient<'de, D>(de: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    opt_uint_narrow(Option::<LenientScalar>::deserialize(de)?)
}

fn de_opt_bool_lenient<'de, D>(de: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    match Option::<LenientScalar>::deserialize(de)? {
        None => Ok(None),
        Some(LenientScalar::Bool(b)) => Ok(Some(b)),
        Some(LenientScalar::Str(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            "" => Ok(None),
            other => Err(D::Error::custom(format!(
                "expected a boolean (true/false); got \"{other}\""
            ))),
        },
        Some(other) => Err(D::Error::custom(match other {
            LenientScalar::Uint(_) | LenientScalar::Int(_) | LenientScalar::Float(_) => {
                "expected a boolean; got a number"
            }
            _ => "expected a boolean",
        })),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RulesWire {
    List(Vec<RuleInput>),
    JsonStr(String),
}

fn de_opt_rules_lenient<'de, D>(de: D) -> Result<Option<Vec<RuleInput>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    match Option::<RulesWire>::deserialize(de)? {
        None => Ok(None),
        Some(RulesWire::List(v)) => Ok(Some(v)),
        Some(RulesWire::JsonStr(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            serde_json::from_str::<Vec<RuleInput>>(t)
                .map(Some)
                .map_err(|e| {
                    D::Error::custom(format!(
                        "rules arrived as a string; it must contain a JSON array of rule objects \
                         (minimal: [{{\"pattern\": \"ERROR\"}}]); parse failed: {e}"
                    ))
                })
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ScopeWire {
    Obj(ActivationScopeInput),
    JsonStr(String),
}

fn de_scope_lenient<'de, D>(de: D) -> Result<ActivationScopeInput, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    match ScopeWire::deserialize(de)? {
        ScopeWire::Obj(s) => Ok(s),
        ScopeWire::JsonStr(raw) => serde_json::from_str::<ActivationScopeInput>(raw.trim())
            .map_err(|e| {
                D::Error::custom(format!(
                    "scope arrived as a string; expected JSON object like {{\"kind\":\"global\"}}: {e}"
                ))
            }),
    }
}

fn de_opt_scope_lenient<'de, D>(de: D) -> Result<Option<ActivationScopeInput>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    match Option::<ScopeWire>::deserialize(de)? {
        None => Ok(None),
        Some(ScopeWire::Obj(s)) => Ok(Some(s)),
        Some(ScopeWire::JsonStr(raw)) => {
            let t = raw.trim();
            if t.is_empty() {
                return Ok(None);
            }
            serde_json::from_str::<ActivationScopeInput>(t)
                .map(Some)
                .map_err(|e| {
                    D::Error::custom(format!(
                        "scope arrived as a string; expected JSON object like {{\"kind\":\"global\"}}: {e}"
                    ))
                })
        }
    }
}

/// Inline comb rule shorthand matching `crates/mcp/src/tools.rs::RuleInput`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub summary_template: Option<String>,
    #[serde(default)]
    pub event_kind: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
    #[serde(default)]
    pub captures: Vec<String>,
    #[serde(default)]
    pub stream: Option<String>,
    #[serde(default)]
    pub redact: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl RuleInput {
    pub fn finalize(self, index: usize) -> Result<RuleDefinition, String> {
        let has_pattern = self.pattern.as_ref().is_some_and(|p| !p.is_empty());
        let has_keywords = self.keywords.as_ref().is_some_and(|k| !k.is_empty());
        let inferred_event_kind = self
            .kind
            .as_deref()
            .filter(|kind| !matches!(*kind, "keyword" | "regex"))
            .map(str::to_owned);

        let kind = match self.kind.as_deref() {
            Some("keyword") => RuleType::Keyword,
            Some("regex") => RuleType::Regex,
            Some(_) | None => {
                if has_pattern && has_keywords {
                    return Err(
                        "rule has both `pattern` and `keywords`; set `kind` to disambiguate \
                         (example: {\"pattern\": \"ERROR\"})"
                            .to_owned(),
                    );
                } else if has_pattern {
                    RuleType::Regex
                } else if has_keywords {
                    RuleType::Keyword
                } else {
                    return Err(
                        "rule needs a matcher: provide `pattern` (regex) or `keywords` \
                         (example: {\"pattern\": \"ERROR\"})"
                            .to_owned(),
                    );
                }
            }
        };

        let severity = match self.severity.as_deref() {
            None => Severity::Info,
            Some("error" | "err") => Severity::High,
            Some("warn" | "warning") => Severity::Medium,
            Some("fatal") => Severity::Critical,
            Some(s) => parse_severity_filter(s).map_err(|_| {
                format!(
                    "severity '{s}' is not valid; one of: trace, debug, info, low, medium, \
                     high, critical (aliases: error->high, warn/warning->medium, fatal->critical)"
                )
            })?,
        };

        let stream = match self.stream.as_deref() {
            None => None,
            Some("stdout") => Some(SourceStream::Stdout),
            Some("stderr") => Some(SourceStream::Stderr),
            Some(other) => {
                return Err(format!(
                    "stream '{other}' is not valid; one of: stdout, stderr (or omit for any)"
                ));
            }
        };

        let mut def = RuleDefinition {
            id: self.id.unwrap_or_else(|| {
                let mut h = std::hash::DefaultHasher::new();
                self.pattern.hash(&mut h);
                self.keywords.hash(&mut h);
                format!("inline-{index}-{:08x}", h.finish() & 0xffff_ffff)
            }),
            version: self.version.unwrap_or(1),
            kind,
            status: RuleStatus::Active,
            severity,
            event_kind: self
                .event_kind
                .or(inferred_event_kind)
                .unwrap_or_else(|| "match".to_owned()),
            stream,
            description: None,
            pattern: self.pattern,
            keywords: self.keywords,
            captures: self.captures,
            summary_template: self
                .summary_template
                .unwrap_or_else(|| "${line}".to_owned()),
            tags: self.tags,
            rate_limit_per_min: None,
            redact: self.redact,
            context_hint: ContextHint::default(),
            examples: vec![],
        };
        normalize_keyword_pattern(&mut def);
        Ok(def)
    }
}

pub fn normalize_keyword_pattern(def: &mut RuleDefinition) {
    if def.kind == RuleType::Keyword
        && def.keywords.as_deref().unwrap_or(&[]).is_empty()
        && def.pattern.as_deref().is_some_and(|p| !p.is_empty())
    {
        def.keywords = def.pattern.take().map(|p| vec![p]);
    }
}

pub fn parse_bucket_and_rules(
    bucket_config_json: Option<String>,
    rules: Option<Vec<RuleInput>>,
    rules_json: Option<String>,
) -> Result<(Option<BucketConfig>, Vec<RuleDefinition>), CodeBackendError> {
    let bucket_config = bucket_config_json
        .map(|raw| {
            serde_json::from_str::<BucketConfig>(&raw)
                .map_err(|e| CodeBackendError::invalid_args(format!("bucket_config_json: {e}")))
        })
        .transpose()?;

    let has_typed = rules.as_ref().is_some_and(|v| !v.is_empty());
    let has_json = rules_json.as_ref().is_some_and(|s| !s.is_empty());
    if has_typed && has_json {
        return Err(CodeBackendError::invalid_args(
            "provide rules via `rules` (typed) OR `rules_json` (string), not both",
        ));
    }

    let resolved = if has_typed {
        rules
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                r.finalize(i)
                    .map_err(|e| CodeBackendError::invalid_args(format!("rules[{i}]: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        rules_json
            .map(|raw| {
                serde_json::from_str::<Vec<RuleDefinition>>(&raw)
                    .map_err(|e| CodeBackendError::invalid_args(format!("rules_json: {e}")))
            })
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|mut def| {
                if !def.status.is_runtime_eligible() {
                    def.status = RuleStatus::Active;
                }
                normalize_keyword_pattern(&mut def);
                def
            })
            .collect()
    };
    Ok((bucket_config, resolved))
}

fn fresh_dedup_nonce() -> String {
    static NONCE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("tc-code-{}-{}", std::process::id(), seq)
}

const fn default_true() -> bool {
    true
}

fn receipt_shape(head: Option<u32>, tail: Option<u32>) -> Option<ReceiptShape> {
    if head.is_none() && tail.is_none() {
        return None;
    }
    let default = ReceiptShape::default();
    Some(ReceiptShape {
        head_lines: head.unwrap_or(default.head_lines),
        tail_lines: tail.unwrap_or(default.tail_lines),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobLimitsInput {
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub priority: Option<JobPriorityInput>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPriorityInput {
    Idle,
    BelowNormal,
    Normal,
}

impl JobLimitsInput {
    #[must_use]
    pub fn into_ipc(self) -> JobLimitsSpec {
        JobLimitsSpec {
            memory: self.memory,
            priority: self.priority.map(|p| match p {
                JobPriorityInput::Idle => JobPriority::Idle,
                JobPriorityInput::BelowNormal => JobPriority::BelowNormal,
                JobPriorityInput::Normal => JobPriority::Normal,
            }),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationScopeInput {
    pub kind: String,
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub probe_id: Option<String>,
}

impl ActivationScopeInput {
    pub fn into_ipc_scope(self) -> Result<ActivationScope, CodeBackendError> {
        match self.kind.as_str() {
            "global" => Ok(ActivationScope::Global),
            "bucket" => {
                let s = self.bucket_id.ok_or_else(|| {
                    CodeBackendError::invalid_args("scope.kind=bucket requires scope.bucket_id")
                })?;
                let bucket_id = parse_id::<BucketIdKind>("scope.bucket_id", &s)
                    .map_err(CodeBackendError::invalid_args)?;
                Ok(ActivationScope::Bucket { bucket_id })
            }
            "job" => {
                let s = self.job_id.ok_or_else(|| {
                    CodeBackendError::invalid_args("scope.kind=job requires scope.job_id")
                })?;
                let job_id = parse_id::<JobIdKind>("scope.job_id", &s)
                    .map_err(CodeBackendError::invalid_args)?;
                Ok(ActivationScope::Job { job_id })
            }
            "probe" => {
                let s = self.probe_id.ok_or_else(|| {
                    CodeBackendError::invalid_args("scope.kind=probe requires scope.probe_id")
                })?;
                let probe_id = parse_id::<ProbeIdKind>("scope.probe_id", &s)
                    .map_err(CodeBackendError::invalid_args)?;
                Ok(ActivationScope::Probe { probe_id })
            }
            other => Err(CodeBackendError::invalid_args(format!(
                "scope.kind '{other}' is not one of global|bucket|job|probe"
            ))),
        }
    }
}

// -------------------------------------------------------------------------
// Per-Op Parameter Structs (60 ops)
// -------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverDetailInput {
    #[default]
    Summary,
    Full,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SystemDiscoverInput {
    #[serde(default)]
    pub detail: DiscoverDetailInput,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuditSinceInput {
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub cursor: Option<u64>,
    #[serde(default)]
    pub action_filter: Option<String>,
    #[serde(default)]
    pub decision_filter: Option<String>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListLimitInput {
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProbeStatusInput {
    pub probe_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetProbeInput {
    pub target_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandStartInput {
    #[serde(deserialize_with = "deserialize_argv")]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "deserialize_env")]
    pub env: Vec<EnvEntry>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub grace_ms: Option<u64>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    pub rules: Option<Vec<RuleInput>>,
    #[serde(default)]
    pub rules_json: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub receipt_head_lines: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub receipt_tail_lines: Option<u32>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub limits: Option<JobLimitsInput>,
}

impl CommandStartInput {
    pub fn into_ipc(self) -> Result<(CommandStartParams, Option<String>), CodeBackendError> {
        let target_id = self.target_id;
        let cwd = self.cwd.map(PathBuf::from);
        let env: Vec<(String, String)> = self.env.into_iter().map(|e| (e.key, e.value)).collect();
        let (bucket_config, rules) =
            parse_bucket_and_rules(self.bucket_config_json, self.rules, self.rules_json)?;
        Ok((
            CommandStartParams {
                environment: None,
                argv: self.argv,
                cwd,
                env,
                bucket_config,
                rules,
                grace_ms: self.grace_ms,
                tag: self.tag,
                strip_ansi: self.strip_ansi,
                dedup_nonce: Some(fresh_dedup_nonce()),
                receipt_shape: receipt_shape(self.receipt_head_lines, self.receipt_tail_lines),
                limits: self.limits.map(JobLimitsInput::into_ipc),
            },
            target_id,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunAndWatchInput {
    #[serde(deserialize_with = "deserialize_argv")]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "deserialize_env")]
    pub env: Vec<EnvEntry>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub grace_ms: Option<u64>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    pub rules: Option<Vec<RuleInput>>,
    #[serde(default)]
    pub rules_json: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub wait_ms: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub max_signals: Option<usize>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub receipt_head_lines: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub receipt_tail_lines: Option<u32>,
    #[serde(default)]
    pub compact: bool,
    #[serde(default)]
    pub wait_until: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub limits: Option<JobLimitsInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandJobRefInput {
    pub job_id: String,
    #[serde(default)]
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShellExecInput {
    pub shell_line: String,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "deserialize_env")]
    pub env: Vec<EnvEntry>,
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    pub rules: Option<Vec<RuleInput>>,
    #[serde(default)]
    pub rules_json: Option<String>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub receipt_head_lines: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub receipt_tail_lines: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub wait_ms: Option<u64>,
    #[serde(default)]
    pub limits: Option<JobLimitsInput>,
}

impl ShellExecInput {
    pub fn into_ipc(self) -> Result<(ShellExecParams, String), CodeBackendError> {
        let _ = self.wait_ms;
        let shell_line = self.shell_line.clone();
        let cwd = self.cwd.map(PathBuf::from);
        let env: Vec<(String, String)> = self.env.into_iter().map(|e| (e.key, e.value)).collect();
        let (bucket_config, rules) =
            parse_bucket_and_rules(self.bucket_config_json, self.rules, self.rules_json)?;
        Ok((
            ShellExecParams {
                shell_line: self.shell_line,
                shell: self.shell,
                cwd,
                env,
                rules,
                bucket_config,
                tag: self.tag,
                receipt_shape: receipt_shape(self.receipt_head_lines, self.receipt_tail_lines),
                limits: self.limits.map(JobLimitsInput::into_ipc),
            },
            shell_line,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandOutputTailInput {
    pub job_id: String,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub max_lines: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub max_bytes: Option<u32>,
    #[serde(default)]
    pub strip_ansi: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BucketEventsSinceInput {
    pub bucket_id: String,
    pub cursor: u64,
    #[serde(default)]
    pub severity_min: Option<String>,
    #[serde(default)]
    pub kind_filter: Option<String>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub limit: Option<usize>,
    #[serde(default)]
    pub compact: bool,
}

impl BucketEventsSinceInput {
    pub fn into_ipc(self) -> Result<(BucketEventsSinceParams, bool), CodeBackendError> {
        let bucket_id = parse_id::<BucketIdKind>("bucket_id", &self.bucket_id)
            .map_err(CodeBackendError::invalid_args)?;
        let severity_min = self
            .severity_min
            .as_deref()
            .map(parse_severity_filter)
            .transpose()
            .map_err(CodeBackendError::invalid_args)?;
        Ok((
            BucketEventsSinceParams {
                bucket_id,
                cursor: self.cursor,
                severity_min,
                kind_filter: self.kind_filter,
                limit: self.limit,
            },
            self.compact,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BucketWaitInput {
    pub bucket_id: String,
    pub cursor: u64,
    #[serde(default)]
    pub severity_min: Option<String>,
    #[serde(default)]
    pub kind_filter: Option<String>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub limit: Option<usize>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub max_signals: Option<usize>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub wake_mode: Option<String>,
    #[serde(default)]
    pub compact: bool,
}

impl BucketWaitInput {
    pub fn into_ipc(
        self,
        remaining_wall_ms: u64,
    ) -> Result<(BucketWaitParams, bool), CodeBackendError> {
        let bucket_id = parse_id::<BucketIdKind>("bucket_id", &self.bucket_id)
            .map_err(CodeBackendError::invalid_args)?;
        let severity_min = self
            .severity_min
            .as_deref()
            .map(parse_severity_filter)
            .transpose()
            .map_err(CodeBackendError::invalid_args)?;
        let max_signals = self
            .max_signals
            .map(|cap| cap.min(terminal_commander_ipc::compound::RUN_AND_WATCH_MAX_SIGNALS));
        let limit = match (self.limit, max_signals) {
            (Some(l), Some(m)) => Some(l.min(m)),
            (l, m) => l.or(m),
        };
        let timeout_ms = Some(
            self.timeout_ms
                .unwrap_or(terminal_commander_ipc::DEFAULT_BUCKET_WAIT_MS)
                .min(remaining_wall_ms),
        );
        let wake_mode = match self.wake_mode.as_deref() {
            None => None,
            Some(m) if m.eq_ignore_ascii_case("any") => Some(WakeMode::Any),
            Some(m) if m.eq_ignore_ascii_case("match") => Some(WakeMode::Match),
            Some(other) => {
                return Err(CodeBackendError::invalid_args(format!(
                    "wake_mode must be \"any\" or \"match\", got \"{other}\""
                )));
            }
        };
        Ok((
            BucketWaitParams {
                bucket_id,
                cursor: self.cursor,
                severity_min,
                kind_filter: self.kind_filter,
                limit,
                timeout_ms,
                wake_mode,
            },
            self.compact,
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BucketSummaryInput {
    pub bucket_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventContextInput {
    pub event_id: String,
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub before: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub after: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub max_bytes: Option<usize>,
}

impl EventContextInput {
    pub fn into_ipc(self) -> Result<EventContextParams, CodeBackendError> {
        let bucket_id = self
            .bucket_id
            .as_deref()
            .map(|b| parse_id::<BucketIdKind>("bucket_id", b))
            .transpose()
            .map_err(CodeBackendError::invalid_args)?;
        let event_id = parse_id::<EventIdKind>("event_id", &self.event_id)
            .map_err(CodeBackendError::invalid_args)?;
        Ok(EventContextParams {
            bucket_id,
            event_id,
            before: self.before,
            after: self.after,
            max_bytes: self.max_bytes,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistrySearchInput {
    pub query: String,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryGetInput {
    pub rule_id: String,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryUpsertInput {
    pub definition_json: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryTestSampleInput {
    pub text: String,
    #[serde(default)]
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryTestInput {
    pub rule_id: String,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
    pub samples: Vec<RegistryTestSampleInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryActivateInput {
    pub rule_id: String,
    #[serde(deserialize_with = "de_scope_lenient")]
    pub scope: ActivationScopeInput,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryImportPackInput {
    pub pack: String,
    #[serde(default)]
    pub activate: bool,
    #[serde(default, deserialize_with = "de_opt_scope_lenient")]
    pub scope: Option<ActivationScopeInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryDeactivateInput {
    #[serde(deserialize_with = "de_scope_lenient")]
    pub scope: ActivationScopeInput,
    #[serde(default)]
    pub rule_id: Option<String>,
    #[serde(default)]
    pub rule_ids: Option<Vec<String>>,
    #[serde(default)]
    pub pack: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistrySuggestInput {
    #[serde(alias = "samples")]
    pub sample_lines: Vec<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub max_rules: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeSearchInput {
    pub query: String,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeGetInput {
    pub recipe_id: String,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeUpsertInput {
    pub definition_json: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RecipeTestInput {
    #[serde(default)]
    pub definition_json: Option<String>,
    #[serde(default)]
    pub recipe_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
    #[serde(default)]
    pub fills: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub expect_argv0: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeActivateInput {
    pub recipe_id: String,
    #[serde(deserialize_with = "de_scope_lenient")]
    pub scope: ActivationScopeInput,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeDeactivateInput {
    pub recipe_id: String,
    #[serde(deserialize_with = "de_scope_lenient")]
    pub scope: ActivationScopeInput,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeRunInput {
    pub recipe_id: String,
    #[serde(deserialize_with = "de_scope_lenient")]
    pub scope: ActivationScopeInput,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub version: Option<u32>,
    #[serde(default)]
    pub fills: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub limits: Option<JobLimitsInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileReadWindowInput {
    pub path: String,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub start_line: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub max_lines: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileSearchInput {
    pub path: String,
    pub query: String,
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    pub case_insensitive: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_u32_lenient")]
    pub max_matches: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub max_snippet_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileWriteInput {
    pub path: String,
    pub content: String,
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    pub create_dirs: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    pub append: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileWatchStartInput {
    pub path: String,
    #[serde(default, deserialize_with = "de_opt_bool_lenient")]
    pub follow_from_beginning: Option<bool>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    pub rules: Option<Vec<RuleInput>>,
    #[serde(default)]
    pub rules_json: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileWatchStopInput {
    pub watch_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PtyCommandStartInput {
    #[serde(deserialize_with = "deserialize_argv")]
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "deserialize_env")]
    pub env: Vec<EnvEntry>,
    #[serde(default, deserialize_with = "de_opt_u16_lenient")]
    pub rows: Option<u16>,
    #[serde(default, deserialize_with = "de_opt_u16_lenient")]
    pub cols: Option<u16>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    pub rules: Option<Vec<RuleInput>>,
    #[serde(default)]
    pub rules_json: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub limits: Option<JobLimitsInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PtyCommandWriteStdinInput {
    pub job_id: String,
    pub bytes: String,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub cursor: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PtyJobRefInput {
    pub job_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShellSessionStartInput {
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "deserialize_env")]
    pub env: Vec<EnvEntry>,
    #[serde(default, deserialize_with = "de_opt_rules_lenient")]
    pub rules: Option<Vec<RuleInput>>,
    #[serde(default)]
    pub rules_json: Option<String>,
    #[serde(default)]
    pub bucket_config_json: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShellSessionExecInput {
    pub session_id: String,
    pub line: String,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub cursor: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShellSessionRefInput {
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceSnapshotCreateInput {
    pub session_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceSnapshotApplyInput {
    pub snapshot_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SubscriptionSourceSelInput {
    #[default]
    All,
    Jobs {
        jobs: Vec<String>,
    },
    Buckets {
        buckets: Vec<String>,
    },
    Probes {
        probes: Vec<String>,
    },
}

impl SubscriptionSourceSelInput {
    pub fn into_wire(self) -> Result<SubscriptionSourceSel, CodeBackendError> {
        match self {
            Self::All => Ok(SubscriptionSourceSel::All),
            Self::Jobs { jobs } => {
                let parsed = jobs
                    .iter()
                    .map(|s| parse_id::<JobIdKind>("sources.jobs", s))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CodeBackendError::invalid_args)?;
                Ok(SubscriptionSourceSel::Jobs { jobs: parsed })
            }
            Self::Buckets { buckets } => {
                let parsed = buckets
                    .iter()
                    .map(|s| parse_id::<BucketIdKind>("sources.buckets", s))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CodeBackendError::invalid_args)?;
                Ok(SubscriptionSourceSel::Buckets { buckets: parsed })
            }
            Self::Probes { probes } => {
                let parsed = probes
                    .iter()
                    .map(|s| parse_id::<ProbeIdKind>("sources.probes", s))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CodeBackendError::invalid_args)?;
                Ok(SubscriptionSourceSel::Probes { probes: parsed })
            }
        }
    }
}

fn de_opt_source_sel<'de, D>(de: D) -> Result<SubscriptionSourceSelInput, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<SubscriptionSourceSelInput>::deserialize(de)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SubscriptionOpenInput {
    #[serde(default)]
    pub severity_min: Option<String>,
    #[serde(default)]
    pub kind: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_source_sel")]
    pub sources: SubscriptionSourceSelInput,
    #[serde(default)]
    pub tag: Option<String>,
}

impl SubscriptionOpenInput {
    pub fn into_predicate(self) -> Result<SubscriptionPredicate, CodeBackendError> {
        let severity_min = self
            .severity_min
            .as_deref()
            .map(parse_severity_filter)
            .transpose()
            .map_err(CodeBackendError::invalid_args)?;
        let sources = self.sources.into_wire()?;
        Ok(SubscriptionPredicate {
            severity_min,
            kind: self.kind,
            sources,
            tag: self.tag,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionPullInput {
    pub sub_id: String,
    #[serde(default, deserialize_with = "de_opt_usize_lenient")]
    pub max: Option<usize>,
    #[serde(default, deserialize_with = "de_opt_u64_lenient")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionCloseInput {
    pub sub_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionSeekInput {
    pub sub_id: String,
    pub bucket_id: String,
    pub seq: u64,
}

/// Validates parameter decoding for `op` without contacting a daemon.
/// Used by contract tests to guarantee zero drift between `catalog.rs` `ParamSpec`
/// and `params.rs` serde structs across all 60 operations.
#[allow(clippy::too_many_lines)]
pub fn validate_kwargs_schema_only(
    op: TcCodeOp,
    kwargs: Map<String, Value>,
) -> Result<(), CodeBackendError> {
    let _ = json_type_name(&Value::Null);
    let _ = parse_id::<SessionIdKind>;
    match op {
        TcCodeOp::SystemDiscover => {
            let _: SystemDiscoverInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::Health
        | TcCodeOp::PolicyStatus
        | TcCodeOp::SelfCheck
        | TcCodeOp::TargetList
        | TcCodeOp::FileWatchList
        | TcCodeOp::PtyCommandList
        | TcCodeOp::ShellSessionList => {
            let _: EmptyParams = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::AuditSince => {
            let _: AuditSinceInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RuntimeState
        | TcCodeOp::ProbeList
        | TcCodeOp::RegistryListActive
        | TcCodeOp::RecipeListActive
        | TcCodeOp::SubscriptionList => {
            let _: ListLimitInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::ProbeStatus => {
            let _: ProbeStatusInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::TargetProbe => {
            let _: TargetProbeInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::CommandStartCombed => {
            let _: CommandStartInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RunAndWatch => {
            let _: RunAndWatchInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::CommandStatus | TcCodeOp::CommandStop => {
            let _: CommandJobRefInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::ShellExec => {
            let _: ShellExecInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::CommandOutputTail => {
            let _: CommandOutputTailInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::BucketEventsSince => {
            let _: BucketEventsSinceInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::BucketWait => {
            let _: BucketWaitInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::BucketSummary => {
            let _: BucketSummaryInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::EventContext => {
            let _: EventContextInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistrySearch => {
            let _: RegistrySearchInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistryGet => {
            let _: RegistryGetInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistryUpsert => {
            let _: RegistryUpsertInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistryTest => {
            let _: RegistryTestInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistryActivate => {
            let _: RegistryActivateInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistryImportPack => {
            let _: RegistryImportPackInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistryDeactivate => {
            let _: RegistryDeactivateInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RegistrySuggestFromSamples => {
            let _: RegistrySuggestInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeSearch => {
            let _: RecipeSearchInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeGet => {
            let _: RecipeGetInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeUpsert => {
            let _: RecipeUpsertInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeTest => {
            let _: RecipeTestInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeActivate => {
            let _: RecipeActivateInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeDeactivate => {
            let _: RecipeDeactivateInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::RecipeRun => {
            let _: RecipeRunInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::FileReadWindow => {
            let _: FileReadWindowInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::FileSearch => {
            let _: FileSearchInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::FileWrite => {
            let _: FileWriteInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::FileWatchStart => {
            let _: FileWatchStartInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::FileWatchStop => {
            let _: FileWatchStopInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::PtyCommandStart => {
            let _: PtyCommandStartInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::PtyCommandWriteStdin => {
            let _: PtyCommandWriteStdinInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::PtyCommandStop | TcCodeOp::CredentialRequest => {
            let _: PtyJobRefInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::ShellSessionStart => {
            let _: ShellSessionStartInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::ShellSessionExec => {
            let _: ShellSessionExecInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::ShellSessionStatus | TcCodeOp::ShellSessionStop => {
            let _: ShellSessionRefInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::WorkspaceSnapshotCreate => {
            let _: WorkspaceSnapshotCreateInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::WorkspaceSnapshotApply => {
            let _: WorkspaceSnapshotApplyInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::SubscriptionOpen => {
            let _: SubscriptionOpenInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::SubscriptionPull => {
            let _: SubscriptionPullInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::SubscriptionClose => {
            let _: SubscriptionCloseInput = decode_op_kwargs(op, kwargs)?;
        }
        TcCodeOp::SubscriptionSeek => {
            let _: SubscriptionSeekInput = decode_op_kwargs(op, kwargs)?;
        }
    }
    Ok(())
}
