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
    EXIT_REASON_HOST_CEILING, EXIT_REASON_MEMORY_CEILING, GovernorModeWire, GovernorStatus,
    JobLimitsSpec, JobPriority, LimitsApplied,
};
use terminal_commander_probes::governor::{
    self as probe_governor, GovernorMode, GovernorReport, HostMemory, JobLimits,
};

use terminal_commander_store::AuditEntry;

use crate::config::{
    DEFAULT_HOST_CEILING, DEFAULT_JOB_MEMORY, DEFAULT_JOB_PRIORITY, GovernorSection,
};
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
    if let Err(why) = parse_memory(&section.host_ceiling) {
        out.push(format!(
            "`governor.host_ceiling` has no effect ({why}); the built-in \
             {DEFAULT_HOST_CEILING} applies"
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
    /// The configured default ceiling, clamped to the host ceiling.
    pub default_memory_bytes: Option<u64>,
    pub default_priority: JobPriority,
    pub llm_can_raise_limits: bool,
    pub mode_available: GovernorModeWire,
    /// Resolved `host_ceiling`; `None` for `"none"` or an unresolvable percent.
    pub host_ceiling_bytes: Option<u64>,
    /// Set by [`Self::install_host_ceiling`]; `None` until then.
    pub host_ceiling_mode: Option<GovernorModeWire>,
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
    /// Axes clamped to the default or the host ceiling.
    pub clamped: Vec<String>,
    /// One sentence per clamped axis: what was asked and why it was cut.
    clamp_reasons: Vec<String>,
}

impl ResolvedLimits {
    /// Audit reason and metadata (`requested` and `applied` values) for a
    /// start whose request was clamped; `None` when nothing was.
    #[must_use]
    pub fn clamp_audit(&self, request: Option<&JobLimitsSpec>) -> Option<(String, String)> {
        if self.clamped.is_empty() {
            return None;
        }
        let meta = serde_json::json!({
            "axes": self.clamped,
            "requested": request,
            "applied": self.applied,
        });
        Some((self.clamp_reasons.join("; "), meta.to_string()))
    }
}

/// Status note when the host can only enforce per-process `RLIMIT_DATA`.
pub const RLIMIT_DEFAULT_NOTE: &str =
    "default memory limit not applied under rlimit; pass limits.memory to opt in";

impl Governor {
    /// Resolve `section` for `profile` against this host. Pure: installs
    /// nothing (see [`Self::install_host_ceiling`]) and, when the section is
    /// disabled, probes no kernel mechanism.
    #[must_use]
    pub fn from_section(section: &GovernorSection, profile: PolicyProfile) -> Self {
        let mode = if section.enabled {
            mode_wire(&probe_governor::available_mode())
        } else {
            GovernorModeWire::Unavailable("governor disabled".to_owned())
        };
        Self::resolve_section(section, profile, probe_governor::host_memory(), mode)
    }

    fn resolve_section(
        section: &GovernorSection,
        profile: PolicyProfile,
        host: Option<HostMemory>,
        mode_available: GovernorModeWire,
    ) -> Self {
        let mut notes = Vec::new();
        let mut resolve_or_note = |raw: &str, builtin: &str, what: &str| {
            let spec = parse_memory(raw)
                .or_else(|_| parse_memory(builtin))
                .unwrap_or(MemorySpec::Unlimited);
            resolve_memory(spec, host).unwrap_or_else(|why| {
                notes.push(format!("{why}; {what}"));
                None
            })
        };
        let default_memory = resolve_or_note(
            &section.default_job_memory,
            DEFAULT_JOB_MEMORY,
            "jobs get no default memory limit",
        );
        let host_ceiling_bytes = resolve_or_note(
            &section.host_ceiling,
            DEFAULT_HOST_CEILING,
            "no host ceiling is installed",
        );
        // The default never exceeds the ceiling every job also joins.
        let default_memory_bytes = match (default_memory, host_ceiling_bytes) {
            (Some(d), Some(c)) => Some(d.min(c)),
            (d, _) => d,
        };
        if section.enabled && mode_available == GovernorModeWire::Rlimit {
            notes.push(RLIMIT_DEFAULT_NOTE.to_owned());
        }
        // Allow-list: only the two trust-inheriting profiles may raise.
        let llm_can_raise_limits = section.llm_can_raise_limits.unwrap_or(matches!(
            profile,
            PolicyProfile::FullAccess | PolicyProfile::AdminDebug
        ));
        Self {
            enabled: section.enabled,
            default_memory_bytes,
            default_priority: parse_priority(&section.default_priority)
                .unwrap_or(JobPriority::BelowNormal),
            llm_can_raise_limits,
            mode_available,
            host_ceiling_bytes,
            host_ceiling_mode: None,
            host,
            note: (!notes.is_empty()).then(|| notes.join("; ")),
        }
    }

    /// Daemon boot: sweep cgroup dirs a previous daemon left behind, then
    /// install the host ceiling (once per process) and record how it is
    /// enforced. No-op when disabled; no install when no ceiling is set.
    pub fn install_host_ceiling(&mut self) {
        if !self.enabled {
            return;
        }
        probe_governor::sweep_stale_job_dirs();
        let Some(limit) = self.host_ceiling_bytes else {
            return;
        };
        self.host_ceiling_mode = Some(match probe_governor::install_host_ceiling(limit) {
            Ok(mode) => mode_wire(&mode),
            // A second daemon state in one process (an embedder, a test)
            // shares the ceiling the first installed when the limit matches.
            Err(_) if probe_governor::host_ceiling() == Some(limit) => self.mode_available.clone(),
            Err(why) => GovernorModeWire::Unavailable(why),
        });
    }

    /// True when the profile default memory applies: never under `rlimit`,
    /// whose per-process `RLIMIT_DATA` breaks sanitizers and large
    /// reservations and cannot be raised back by the job.
    fn default_memory_applies(&self) -> bool {
        self.mode_available != GovernorModeWire::Rlimit
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
        let mut clamp_reasons = Vec::new();
        let mut memory = self
            .default_memory_bytes
            .filter(|_| self.default_memory_applies());
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
                    clamp_reasons.push(format!(
                        "memory `{raw}` is above the [governor] default and \
                         llm_can_raise_limits is false"
                    ));
                    memory = self.default_memory_bytes;
                } else if let (Some(a), Some(c)) = (asked, self.host_ceiling_bytes)
                    && a > c
                {
                    clamped.push("memory".to_owned());
                    clamp_reasons.push(format!(
                        "memory `{raw}` is above the host ceiling of {c} bytes"
                    ));
                    memory = Some(c);
                } else {
                    memory = asked;
                }
            }
            if let Some(asked) = req.priority {
                if asked > self.default_priority && !self.llm_can_raise_limits {
                    clamped.push("priority".to_owned());
                    clamp_reasons.push(
                        "priority is above the [governor] default and \
                         llm_can_raise_limits is false"
                            .to_owned(),
                    );
                } else {
                    priority = asked;
                }
            }
        }
        Ok(ResolvedLimits {
            limits: JobLimits {
                memory_bytes: memory,
                priority: Some(priority_probe(priority)),
                join_host_ceiling: true,
            },
            applied: Some(LimitsApplied {
                memory_bytes: memory,
                priority: Some(priority),
            }),
            clamped,
            clamp_reasons,
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
            host_ceiling_bytes: self.host_ceiling_mode.as_ref().and(self.host_ceiling_bytes),
            host_ceiling_mode: self.host_ceiling_mode.clone(),
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
    /// The limits the job runs with (start response value), kept through
    /// exit, stop, and persistence so every status shows them.
    pub limits_applied: Option<LimitsApplied>,
    /// `Some(false)` when a host ceiling is installed but the job failed to
    /// join it; `None` otherwise (joined, no ceiling, or ungoverned).
    pub host_ceiling_joined: Option<bool>,
}

/// `governor_unavailable` audit reason for a job that failed to join the
/// installed host ceiling.
pub const HOST_CEILING_JOIN_FAILED: &str = "host_ceiling_join_failed: the job did not join      the daemon-wide host ceiling; only its own limit bounds it";

impl GovernorOutcome {
    /// Map a probe report. An ungoverned job (`mode == None`) maps to all
    /// `None`, so nothing is serialized.
    #[must_use]
    pub fn from_report(report: &GovernorReport) -> Self {
        Self::map_report(report, probe_governor::host_ceiling().is_some())
    }

    /// [`Self::from_report`] with the host-ceiling install state passed in.
    fn map_report(report: &GovernorReport, ceiling_installed: bool) -> Self {
        let Some(mode) = report.mode.as_ref() else {
            return Self::default();
        };
        Self {
            governor: Some(mode_wire(mode)),
            peak_memory_bytes: report.peak_memory_bytes,
            // The job's own limit takes precedence over the host ceiling.
            exit_reason: if report.memory_limit_hit {
                Some(EXIT_REASON_MEMORY_CEILING.to_owned())
            } else {
                report
                    .host_ceiling_hit
                    .then(|| EXIT_REASON_HOST_CEILING.to_owned())
            },
            memory_limit_bytes: report.memory_limit_bytes,
            limits_applied: None,
            host_ceiling_joined: (ceiling_installed && !report.host_ceiling_joined)
                .then_some(false),
        }
    }

    /// The spawn-time view: mode, limit and applied limits only. Peak and
    /// exit reason exist only after exit, so a running status never shows
    /// them. Ungoverned (`mode == None`) stays all `None`.
    #[must_use]
    pub fn at_spawn(report: &GovernorReport, limits_applied: Option<LimitsApplied>) -> Self {
        let base = Self::from_report(report);
        Self {
            peak_memory_bytes: None,
            exit_reason: None,
            limits_applied: base.governor.as_ref().and(limits_applied),
            ..base
        }
    }

    /// Replace the spawn-time view with the final report, keeping the
    /// applied limits.
    pub fn finish(&mut self, report: &GovernorReport) {
        *self = Self {
            limits_applied: self.limits_applied,
            ..Self::from_report(report)
        };
    }

    /// Audit reason for a governed start the enforcer could not govern
    /// (`governor_unavailable` row); `None` when it is enforced.
    #[must_use]
    pub fn unavailable_reason(&self) -> Option<String> {
        match &self.governor {
            Some(GovernorModeWire::Unavailable(why)) => {
                Some(format!("the job runs ungoverned: {why}"))
            }
            _ if self.host_ceiling_joined == Some(false) => {
                Some(HOST_CEILING_JOIN_FAILED.to_owned())
            }
            _ => None,
        }
    }

    /// True when the job was stopped by its own limit or the host ceiling.
    #[must_use]
    pub const fn hit_ceiling(&self) -> bool {
        self.exit_reason.is_some()
    }

    /// The `governor_memory_ceiling` audit row for a job its ceiling stopped,
    /// carrying the limit and the peak. `None` otherwise.
    #[must_use]
    pub fn ceiling_audit(&self, job: &str) -> Option<AuditEntry> {
        let (action, reason, limit) = match self.exit_reason.as_deref()? {
            EXIT_REASON_HOST_CEILING => (
                "governor_host_ceiling",
                "the job was stopped by the daemon-wide host ceiling",
                probe_governor::host_ceiling(),
            ),
            _ => (
                "governor_memory_ceiling",
                "the job was stopped by its memory ceiling",
                self.memory_limit_bytes,
            ),
        };
        Some(
            AuditEntry::new(action, job, "info")
                .with_reason(reason)
                .with_metadata_json(format!(
                    "{{\"memory_limit_bytes\":{},\"peak_memory_bytes\":{}}}",
                    json_u64(limit),
                    json_u64(self.peak_memory_bytes)
                )),
        )
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
        if self.host_ceiling_joined == Some(false) {
            out.push_str(",\"host_ceiling_joined\":false");
        }
        if let Some(limits) = &self.limits_applied {
            let _ = write!(
                out,
                ",\"limits_applied\":{}",
                serde_json::to_string(limits).unwrap_or_else(|_| "null".to_owned())
            );
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
            limits_applied: v
                .get("limits_applied")
                .and_then(|l| serde_json::from_value(l.clone()).ok()),
            host_ceiling_joined: v
                .get("host_ceiling_joined")
                .and_then(serde_json::Value::as_bool),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;
    const JO: GovernorModeWire = GovernorModeWire::JobObject;

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
        let g = Governor::resolve_section(
            &GovernorSection::default(),
            PolicyProfile::FullAccess,
            None,
            JO,
        );
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
        let open = Governor::resolve_section(&section, PolicyProfile::FullAccess, None, JO);
        let r = open.resolve(Some(&req)).unwrap();
        assert!(r.clamped.is_empty());
        assert_eq!(r.limits.memory_bytes, Some(2 * GIB));

        let hardened = Governor::resolve_section(&section, PolicyProfile::DeveloperLocal, None, JO);
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
    fn host_ceiling_caps_the_default_and_a_request() {
        let section = GovernorSection {
            default_job_memory: "2GiB".to_owned(),
            host_ceiling: "1GiB".to_owned(),
            ..GovernorSection::default()
        };
        let g = Governor::resolve_section(&section, PolicyProfile::FullAccess, None, JO);
        assert_eq!(
            g.default_memory_bytes,
            Some(GIB),
            "default clamped to ceiling"
        );
        let r = g.resolve(None).unwrap();
        assert!(r.limits.join_host_ceiling);
        assert_eq!(r.limits.memory_bytes, Some(GIB));
        let big = JobLimitsSpec {
            memory: Some("4GiB".to_owned()),
            priority: None,
        };
        let r = g.resolve(Some(&big)).unwrap();
        assert_eq!(r.clamped, ["memory"]);
        assert_eq!(r.limits.memory_bytes, Some(GIB));
        let (reason, meta) = r.clamp_audit(Some(&big)).unwrap();
        assert!(reason.contains("host ceiling"), "{reason}");
        let meta: serde_json::Value = serde_json::from_str(&meta).unwrap();
        assert_eq!(meta["requested"]["memory"], "4GiB");
        assert_eq!(meta["applied"]["memory_bytes"], GIB);
        // `none` keeps no per-job limit; the job still joins the ceiling.
        let none = JobLimitsSpec {
            memory: Some("none".to_owned()),
            priority: None,
        };
        let r = g.resolve(Some(&none)).unwrap();
        assert!(r.clamped.is_empty());
        assert_eq!(r.limits.memory_bytes, None);
        assert!(r.limits.join_host_ceiling);
        assert!(g.resolve(None).unwrap().clamp_audit(None).is_none());
    }

    #[test]
    fn rlimit_skips_the_default_but_honours_an_explicit_request() {
        let section = GovernorSection {
            default_job_memory: "1GiB".to_owned(),
            ..GovernorSection::default()
        };
        let g = Governor::resolve_section(
            &section,
            PolicyProfile::FullAccess,
            None,
            GovernorModeWire::Rlimit,
        );
        let r = g.resolve(None).unwrap();
        assert_eq!(r.limits.memory_bytes, None);
        assert_eq!(r.applied.unwrap().priority, Some(JobPriority::BelowNormal));
        assert!(g.status().note.unwrap().contains(RLIMIT_DEFAULT_NOTE));
        let ask = JobLimitsSpec {
            memory: Some("512MiB".to_owned()),
            priority: None,
        };
        assert_eq!(
            g.resolve(Some(&ask)).unwrap().limits.memory_bytes,
            Some(512 << 20)
        );
    }

    #[test]
    fn llm_can_raise_limits_is_an_allow_list() {
        for (profile, want) in [
            (PolicyProfile::FullAccess, true),
            (PolicyProfile::AdminDebug, true),
            (PolicyProfile::DeveloperLocal, false),
            (PolicyProfile::RepoOnly, false),
            (PolicyProfile::ReadOnlyObserver, false),
        ] {
            let g = Governor::resolve_section(&GovernorSection::default(), profile, None, JO);
            assert_eq!(g.llm_can_raise_limits, want, "{profile:?}");
        }
    }

    #[test]
    fn unavailable_outcome_names_the_reason() {
        let report = GovernorReport {
            mode: Some(GovernorMode::Unavailable("no kernel primitive".to_owned())),
            ..GovernorReport::default()
        };
        let o = GovernorOutcome::at_spawn(&report, None);
        assert!(
            o.unavailable_reason()
                .unwrap()
                .contains("no kernel primitive")
        );
        let ok = GovernorReport {
            mode: Some(GovernorMode::JobObject),
            peak_memory_bytes: Some(5),
            memory_limit_hit: true,
            // Joined, so the result does not depend on whether this test
            // process has a host ceiling installed.
            host_ceiling_joined: true,
            ..GovernorReport::default()
        };
        let applied = Some(LimitsApplied {
            memory_bytes: Some(7),
            priority: None,
        });
        let mut o = GovernorOutcome::at_spawn(&ok, applied);
        assert!(o.unavailable_reason().is_none());
        assert_eq!((o.peak_memory_bytes, &o.exit_reason), (None, &None));
        o.finish(&ok);
        assert_eq!(o.peak_memory_bytes, Some(5));
        assert_eq!(o.limits_applied, applied, "limits survive the final report");
        assert_eq!(
            GovernorOutcome::at_spawn(&GovernorReport::default(), applied),
            GovernorOutcome::default(),
            "ungoverned stays empty"
        );
    }

    #[test]
    fn disabled_resolves_to_no_limits() {
        let section = GovernorSection {
            enabled: false,
            ..GovernorSection::default()
        };
        let g = Governor::resolve_section(&section, PolicyProfile::FullAccess, None, JO);
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
    fn failed_host_join_is_surfaced_and_audited() {
        let report = |host_ceiling_joined| GovernorReport {
            mode: Some(GovernorMode::JobObject),
            host_ceiling_joined,
            ..GovernorReport::default()
        };
        let failed = GovernorOutcome::map_report(&report(false), true);
        assert_eq!(failed.host_ceiling_joined, Some(false));
        assert!(
            failed
                .unavailable_reason()
                .unwrap()
                .starts_with("host_ceiling_join_failed")
        );
        let json: serde_json::Value =
            serde_json::from_str(&format!("{{\"a\":1{}}}", failed.evidence_fields())).unwrap();
        assert_eq!(
            GovernorOutcome::from_evidence(Some(&json)).host_ceiling_joined,
            Some(false)
        );
        for (joined, installed) in [(true, true), (false, false), (true, false)] {
            let o = GovernorOutcome::map_report(&report(joined), installed);
            assert_eq!(o.host_ceiling_joined, None, "{joined} {installed}");
            assert!(o.unavailable_reason().is_none());
        }
        let ungoverned = GovernorOutcome::map_report(&GovernorReport::default(), true);
        assert_eq!(ungoverned.host_ceiling_joined, None);
    }

    #[test]
    fn own_limit_takes_precedence_over_the_host_ceiling() {
        let base = GovernorReport {
            mode: Some(GovernorMode::JobObject),
            ..GovernorReport::default()
        };
        let reason = |memory_limit_hit, host_ceiling_hit| {
            GovernorOutcome::from_report(&GovernorReport {
                memory_limit_hit,
                host_ceiling_hit,
                ..base.clone()
            })
            .exit_reason
        };
        assert_eq!(reason(true, true).as_deref(), Some("memory_ceiling"));
        assert_eq!(reason(false, true).as_deref(), Some("host_ceiling"));
        assert_eq!(reason(false, false), None);
        let o = GovernorOutcome::from_report(&GovernorReport {
            host_ceiling_hit: true,
            ..base
        });
        assert_eq!(
            o.ceiling_audit("job").unwrap().action,
            "governor_host_ceiling"
        );
    }

    #[test]
    fn outcome_round_trips_through_evidence() {
        let report = GovernorReport {
            mode: Some(GovernorMode::Unavailable("no \"kernel\"".to_owned())),
            memory_limit_bytes: Some(10),
            peak_memory_bytes: Some(12),
            memory_limit_hit: true,
            ..GovernorReport::default()
        };
        let mut o = GovernorOutcome::from_report(&report);
        o.limits_applied = Some(LimitsApplied {
            memory_bytes: Some(10),
            priority: Some(JobPriority::Idle),
        });
        let json: serde_json::Value =
            serde_json::from_str(&format!("{{\"a\":1{}}}", o.evidence_fields())).unwrap();
        let back = GovernorOutcome::from_evidence(Some(&json));
        assert_eq!(back.governor, o.governor);
        assert_eq!(back.peak_memory_bytes, Some(12));
        assert_eq!(back.exit_reason.as_deref(), Some("memory_ceiling"));
        assert_eq!(back.limits_applied, o.limits_applied);
        assert_eq!(
            GovernorOutcome::from_report(&GovernorReport::default()).evidence_fields(),
            ""
        );
    }
}
