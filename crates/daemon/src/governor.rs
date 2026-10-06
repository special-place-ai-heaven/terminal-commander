// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Resource governor policy: resolves `[governor]` into per-job
//! [`JobLimits`] and maps the probe's [`GovernorReport`] onto the wire.
//!
//! The probes crate enforces (Job Object, cgroup, rlimit); this module only
//! decides WHAT to ask for and reports what the enforcer said. A disabled
//! governor resolves every start to `JobLimits::default()` and serializes no
//! governor field, so responses stay byte-identical to an ungoverned daemon.

use std::fmt::Write as _;

use terminal_commander_ipc::{
    EXIT_REASON_MEMORY_CEILING, GovernorModeWire, GovernorStatus, JobLimitsSpec, JobPriority,
    LimitsApplied,
};
use terminal_commander_probes::governor::{
    self as probe_governor, GovernorMode, GovernorReport, HostMemory, JobLimits,
};

use terminal_commander_store::AuditEntry;

use crate::config::{DEFAULT_JOB_MEMORY, DEFAULT_JOB_PRIORITY, GovernorSection};
use crate::policy::PolicyProfile;

/// A parsed memory value before host resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySpec {
    Bytes(u64),
    /// 1..=100 percent of host memory.
    Percent(u8),
    Unlimited,
}

/// Parse a memory value: `"24GiB"`, `"512MiB"`, `"123456"`, `"40%"`, `"none"`.
///
/// Units (`KiB`, `MiB`, `GiB`, `TiB`) are binary and case-insensitive. Zero
/// is rejected: a zero ceiling would kill every job at its first allocation.
pub fn parse_memory(raw: &str) -> Result<MemorySpec, String> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("none") {
        return Ok(MemorySpec::Unlimited);
    }
    if let Some(pct) = s.strip_suffix('%') {
        return match pct.trim().parse::<u8>() {
            Ok(p @ 1..=100) => Ok(MemorySpec::Percent(p)),
            _ => Err(format!(
                "memory `{s}`: a percent must be a whole number from 1% to 100%"
            )),
        };
    }
    let lower = s.to_ascii_lowercase();
    let (digits, shift) = [("tib", 40), ("gib", 30), ("mib", 20), ("kib", 10)]
        .iter()
        .find_map(|(unit, shift)| lower.strip_suffix(unit).map(|d| (d.trim(), *shift)))
        .unwrap_or((lower.as_str(), 0));
    let bytes = digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(1u64 << shift))
        .ok_or_else(|| {
            format!(
                "memory `{s}` is not understood: use a size like 512MiB or 24GiB, a byte \
                 count, a percent like 40%, or none"
            )
        })?;
    if bytes == 0 {
        return Err(format!("memory `{s}` must be greater than zero"));
    }
    Ok(MemorySpec::Bytes(bytes))
}

/// Parse `idle` | `below_normal` | `normal`.
pub fn parse_priority(raw: &str) -> Result<JobPriority, String> {
    match raw.trim() {
        "idle" => Ok(JobPriority::Idle),
        "below_normal" => Ok(JobPriority::BelowNormal),
        "normal" => Ok(JobPriority::Normal),
        other => Err(format!(
            "priority `{other}` is not one of idle, below_normal, normal"
        )),
    }
}

/// Bytes for `spec` on this host. `Err` only for a percent with host memory
/// unknown. The percent base is the commit limit when the platform has one
/// (Windows), else physical memory.
fn resolve_memory(spec: MemorySpec, host: Option<HostMemory>) -> Result<Option<u64>, String> {
    match spec {
        MemorySpec::Unlimited => Ok(None),
        MemorySpec::Bytes(b) => Ok(Some(b)),
        MemorySpec::Percent(p) => {
            let host = host.ok_or_else(|| {
                "a percent memory limit needs host memory, which this platform does not report"
                    .to_owned()
            })?;
            let base = host.commit_limit_bytes.unwrap_or(host.total_bytes);
            Ok(Some(
                u64::try_from(u128::from(base) * u128::from(p) / 100).unwrap_or(u64::MAX),
            ))
        }
    }
}

/// One warning per malformed `[governor]` value, in the `config_warnings`
/// style: the key, why it has no effect, and what applies instead.
#[must_use]
pub fn section_warnings(section: &GovernorSection) -> Vec<String> {
    let mut out = Vec::new();
    if let Err(why) = parse_memory(&section.default_job_memory) {
        out.push(format!(
            "`governor.default_job_memory` has no effect ({why}); the built-in \
             {DEFAULT_JOB_MEMORY} applies"
        ));
    }
    if let Err(why) = parse_priority(&section.default_priority) {
        out.push(format!(
            "`governor.default_priority` has no effect ({why}); the built-in \
             {DEFAULT_JOB_PRIORITY} applies"
        ));
    }
    out
}

/// The resolved `[governor]` policy, built once at bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Governor {
    pub enabled: bool,
    pub default_memory_bytes: Option<u64>,
    pub default_priority: JobPriority,
    pub llm_can_raise_limits: bool,
    pub mode_available: GovernorModeWire,
    host: Option<HostMemory>,
    note: Option<String>,
}

/// What one start runs with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedLimits {
    /// Handed to the probe. `JobLimits::default()` when disabled.
    pub limits: JobLimits,
    /// `None` when disabled, so the response field is omitted.
    pub applied: Option<LimitsApplied>,
    /// Axes clamped to the default.
    pub clamped: Vec<String>,
}

impl Governor {
    /// Resolve `section` for `profile` against this host.
    #[must_use]
    pub fn from_section(section: &GovernorSection, profile: PolicyProfile) -> Self {
        Self::resolve_section(section, profile, probe_governor::host_memory())
    }

    fn resolve_section(
        section: &GovernorSection,
        profile: PolicyProfile,
        host: Option<HostMemory>,
    ) -> Self {
        let spec = parse_memory(&section.default_job_memory)
            .or_else(|_| parse_memory(DEFAULT_JOB_MEMORY))
            .unwrap_or(MemorySpec::Unlimited);
        let (default_memory_bytes, note) = match resolve_memory(spec, host) {
            Ok(bytes) => (bytes, None),
            Err(why) => (
                None,
                Some(format!("{why}; jobs get no default memory limit")),
            ),
        };
        let llm_can_raise_limits = section.llm_can_raise_limits.unwrap_or(!matches!(
            profile,
            PolicyProfile::DeveloperLocal
                | PolicyProfile::RepoOnly
                | PolicyProfile::ReadOnlyObserver
        ));
        Self {
            enabled: section.enabled,
            default_memory_bytes,
            default_priority: parse_priority(&section.default_priority)
                .unwrap_or(JobPriority::BelowNormal),
            llm_can_raise_limits,
            mode_available: mode_wire(&probe_governor::available_mode()),
            host,
            note,
        }
    }

    /// Effective limits for one start: the request's value per axis, else the
    /// default. With `llm_can_raise_limits` false a value above the default
    /// (more memory, unlimited memory, or a higher priority) is clamped to the
    /// default and named in `clamped`. `Err` for an unparsable request.
    pub fn resolve(&self, request: Option<&JobLimitsSpec>) -> Result<ResolvedLimits, String> {
        if !self.enabled {
            return Ok(ResolvedLimits::default());
        }
        let mut clamped = Vec::new();
        let mut memory = self.default_memory_bytes;
        let mut priority = self.default_priority;
        if let Some(req) = request {
            if let Some(raw) = req.memory.as_deref() {
                let asked = resolve_memory(parse_memory(raw)?, self.host)?;
                let above = match (asked, self.default_memory_bytes) {
                    (_, None) => false,
                    (None, Some(_)) => true,
                    (Some(a), Some(d)) => a > d,
                };
                if above && !self.llm_can_raise_limits {
                    clamped.push("memory".to_owned());
                } else {
                    memory = asked;
                }
            }
            if let Some(asked) = req.priority {
                if asked > self.default_priority && !self.llm_can_raise_limits {
                    clamped.push("priority".to_owned());
                } else {
                    priority = asked;
                }
            }
        }
        Ok(ResolvedLimits {
            limits: JobLimits {
                memory_bytes: memory,
                priority: Some(priority_probe(priority)),
            },
            applied: Some(LimitsApplied {
                memory_bytes: memory,
                priority: Some(priority),
            }),
            clamped,
        })
    }

    /// The `policy_status` view.
    #[must_use]
    pub fn status(&self) -> GovernorStatus {
        GovernorStatus {
            enabled: self.enabled,
            mode_available: self.mode_available.clone(),
            default_job_memory_bytes: self.default_memory_bytes,
            default_priority: self.default_priority,
            llm_can_raise_limits: self.llm_can_raise_limits,
            note: self.note.clone(),
        }
    }
}

const fn priority_probe(p: JobPriority) -> probe_governor::JobPriority {
    match p {
        JobPriority::Idle => probe_governor::JobPriority::Idle,
        JobPriority::BelowNormal => probe_governor::JobPriority::BelowNormal,
        JobPriority::Normal => probe_governor::JobPriority::Normal,
    }
}

fn mode_wire(mode: &GovernorMode) -> GovernorModeWire {
    match mode {
        GovernorMode::JobObject => GovernorModeWire::JobObject,
        GovernorMode::Cgroup => GovernorModeWire::Cgroup,
        GovernorMode::Rlimit => GovernorModeWire::Rlimit,
        GovernorMode::Unavailable(why) => GovernorModeWire::Unavailable(why.clone()),
    }
}

/// Audit reason for a start whose request was clamped to the defaults.
#[must_use]
pub fn clamp_reason(axes: &[String]) -> String {
    format!(
        "requested limits above the [governor] defaults were clamped ({}): \
         llm_can_raise_limits is false",
        axes.join(", ")
    )
}

fn json_u64(v: Option<u64>) -> String {
    v.map_or_else(|| "null".to_owned(), |n| n.to_string())
}

/// The terminal-status governor fields for one job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GovernorOutcome {
    pub governor: Option<GovernorModeWire>,
    pub peak_memory_bytes: Option<u64>,
    pub exit_reason: Option<String>,
    /// The ceiling the job ran under; audit and persistence only.
    pub memory_limit_bytes: Option<u64>,
}

impl GovernorOutcome {
    /// Map a probe report. An ungoverned job (`mode == None`) maps to all
    /// `None`, so nothing is serialized.
    #[must_use]
    pub fn from_report(report: &GovernorReport) -> Self {
        let Some(mode) = report.mode.as_ref() else {
            return Self::default();
        };
        Self {
            governor: Some(mode_wire(mode)),
            peak_memory_bytes: report.peak_memory_bytes,
            exit_reason: report
                .memory_limit_hit
                .then(|| EXIT_REASON_MEMORY_CEILING.to_owned()),
            memory_limit_bytes: report.memory_limit_bytes,
        }
    }

    /// True when the job was stopped by its memory ceiling.
    #[must_use]
    pub const fn hit_ceiling(&self) -> bool {
        self.exit_reason.is_some()
    }

    /// The `governor_memory_ceiling` audit row for a job its ceiling stopped,
    /// carrying the limit and the peak. `None` otherwise.
    #[must_use]
    pub fn ceiling_audit(&self, job: &str) -> Option<AuditEntry> {
        self.hit_ceiling().then(|| {
            AuditEntry::new("governor_memory_ceiling", job, "info")
                .with_reason("the job was stopped by its memory ceiling")
                .with_metadata_json(format!(
                    "{{\"memory_limit_bytes\":{},\"peak_memory_bytes\":{}}}",
                    json_u64(self.memory_limit_bytes),
                    json_u64(self.peak_memory_bytes)
                ))
        })
    }

    /// Fields appended to the persisted evidence object (numbers, a fixed
    /// mode label, and the probe's reason string, JSON-escaped). Empty for an
    /// ungoverned job so its evidence stays byte-identical.
    #[must_use]
    pub fn evidence_fields(&self) -> String {
        let Some(mode) = self.governor.as_ref() else {
            return String::new();
        };
        let mut out = format!(
            ",\"governor\":{}",
            serde_json::to_string(mode).unwrap_or_else(|_| "null".to_owned())
        );
        if let Some(peak) = self.peak_memory_bytes {
            let _ = write!(out, ",\"peak_memory_bytes\":{peak}");
        }
        if let Some(reason) = &self.exit_reason {
            let _ = write!(out, ",\"exit_reason\":\"{reason}\"");
        }
        out
    }

    /// Read the fields back from a persisted evidence object.
    #[must_use]
    pub fn from_evidence(evidence: Option<&serde_json::Value>) -> Self {
        let Some(v) = evidence else {
            return Self::default();
        };
        Self {
            governor: v
                .get("governor")
                .and_then(|m| serde_json::from_value(m.clone()).ok()),
            peak_memory_bytes: v
                .get("peak_memory_bytes")
                .and_then(serde_json::Value::as_u64),
            exit_reason: v
                .get("exit_reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            memory_limit_bytes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn parse_memory_table() {
        let ok = [
            ("none", MemorySpec::Unlimited),
            ("NONE", MemorySpec::Unlimited),
            ("60%", MemorySpec::Percent(60)),
            ("100%", MemorySpec::Percent(100)),
            ("24GiB", MemorySpec::Bytes(24 * GIB)),
            ("24gib", MemorySpec::Bytes(24 * GIB)),
            ("512MiB", MemorySpec::Bytes(512 << 20)),
            ("64KiB", MemorySpec::Bytes(64 << 10)),
            ("1TiB", MemorySpec::Bytes(1 << 40)),
            ("123456", MemorySpec::Bytes(123_456)),
            (" 2 GiB ", MemorySpec::Bytes(2 * GIB)),
        ];
        for (raw, want) in ok {
            assert_eq!(parse_memory(raw), Ok(want), "{raw}");
        }
        for bad in [
            "",
            "0",
            "0%",
            "101%",
            "-5%",
            "1.5GiB",
            "24GB",
            "lots",
            "%",
            "99999999999TiB",
        ] {
            assert!(parse_memory(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn percent_uses_commit_limit_when_present() {
        let host = HostMemory {
            total_bytes: 100,
            commit_limit_bytes: Some(200),
        };
        assert_eq!(
            resolve_memory(MemorySpec::Percent(50), Some(host)),
            Ok(Some(100))
        );
        let linux = HostMemory {
            total_bytes: 100,
            commit_limit_bytes: None,
        };
        assert_eq!(
            resolve_memory(MemorySpec::Percent(50), Some(linux)),
            Ok(Some(50))
        );
        assert!(resolve_memory(MemorySpec::Percent(50), None).is_err());
    }

    #[test]
    fn percent_default_without_host_memory_is_no_limit_with_note() {
        let g =
            Governor::resolve_section(&GovernorSection::default(), PolicyProfile::FullAccess, None);
        assert_eq!(g.default_memory_bytes, None);
        assert!(g.status().note.is_some());
    }

    #[test]
    fn clamp_only_when_raise_is_denied() {
        let section = GovernorSection {
            default_job_memory: "1GiB".to_owned(),
            ..GovernorSection::default()
        };
        let req = JobLimitsSpec {
            memory: Some("2GiB".to_owned()),
            priority: Some(JobPriority::Normal),
        };
        let open = Governor::resolve_section(&section, PolicyProfile::FullAccess, None);
        let r = open.resolve(Some(&req)).unwrap();
        assert!(r.clamped.is_empty());
        assert_eq!(r.limits.memory_bytes, Some(2 * GIB));

        let hardened = Governor::resolve_section(&section, PolicyProfile::DeveloperLocal, None);
        let r = hardened.resolve(Some(&req)).unwrap();
        assert_eq!(r.clamped, ["memory", "priority"]);
        assert_eq!(r.limits.memory_bytes, Some(GIB));
        assert_eq!(r.applied.unwrap().priority, Some(JobPriority::BelowNormal));
        // Lowering is always allowed.
        let lower = JobLimitsSpec {
            memory: Some("512MiB".to_owned()),
            priority: Some(JobPriority::Idle),
        };
        let r = hardened.resolve(Some(&lower)).unwrap();
        assert!(r.clamped.is_empty());
        assert_eq!(r.limits.memory_bytes, Some(512 << 20));
        // `none` is a raise when a default ceiling exists.
        let none = JobLimitsSpec {
            memory: Some("none".to_owned()),
            priority: None,
        };
        assert_eq!(hardened.resolve(Some(&none)).unwrap().clamped, ["memory"]);
    }

    #[test]
    fn disabled_resolves_to_no_limits() {
        let section = GovernorSection {
            enabled: false,
            ..GovernorSection::default()
        };
        let g = Governor::resolve_section(&section, PolicyProfile::FullAccess, None);
        assert_eq!(
            g.resolve(Some(&JobLimitsSpec::default())).unwrap(),
            ResolvedLimits::default()
        );
    }

    #[test]
    fn malformed_values_warn() {
        let section = GovernorSection {
            default_job_memory: "lots".to_owned(),
            default_priority: "urgent".to_owned(),
            ..GovernorSection::default()
        };
        let w = section_warnings(&section);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("governor.default_job_memory"));
        assert!(w[1].contains("governor.default_priority"));
        assert!(section_warnings(&GovernorSection::default()).is_empty());
    }

    #[test]
    fn outcome_round_trips_through_evidence() {
        let report = GovernorReport {
            mode: Some(GovernorMode::Unavailable("no \"kernel\"".to_owned())),
            memory_limit_bytes: Some(10),
            peak_memory_bytes: Some(12),
            memory_limit_hit: true,
        };
        let o = GovernorOutcome::from_report(&report);
        let json: serde_json::Value =
            serde_json::from_str(&format!("{{\"a\":1{}}}", o.evidence_fields())).unwrap();
        let back = GovernorOutcome::from_evidence(Some(&json));
        assert_eq!(back.governor, o.governor);
        assert_eq!(back.peak_memory_bytes, Some(12));
        assert_eq!(back.exit_reason.as_deref(), Some("memory_ceiling"));
        assert_eq!(
            GovernorOutcome::from_report(&GovernorReport::default()).evidence_fields(),
            ""
        );
    }
}
