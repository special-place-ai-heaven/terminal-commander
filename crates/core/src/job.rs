// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Job manager (TC16).
//!
//! In-memory tracking of process lifecycle: start/running/exited
//! states, exit code or signal, byte/frame counters, and
//! command_failed / command_exited event drafts that the daemon
//! (TC21) hands to the bucket manager.
//!
//! Source-status: live (TC16) for tracking and event-draft
//! generation. Persistence is deferred post-MVP.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::event::{Captures, EventDraft};
use crate::ids::{BucketId, JobId, ProbeId};
use crate::severity::Severity;
use crate::source::{EventSource, SourceStream, SourceType};

/// Default grace window. Mirrors TC15.
pub const DEFAULT_JOB_GRACE: Duration = Duration::from_secs(10);

/// Per-job configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobConfig {
    pub job_id: JobId,
    pub argv: Vec<String>,
    pub bucket_id: BucketId,
    pub probe_id: ProbeId,
    pub grace_secs: u64,
}

impl JobConfig {
    /// Convenience constructor: assign a job_id automatically.
    #[must_use]
    pub fn new(argv: Vec<String>, bucket_id: BucketId, probe_id: ProbeId) -> Self {
        Self {
            job_id: JobId::new(),
            argv,
            bucket_id,
            probe_id,
            grace_secs: DEFAULT_JOB_GRACE.as_secs(),
        }
    }
}

/// Lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Starting,
    Running,
    Exited,
    Cancelled,
    Failed,
}

/// Exit information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobExitInfo {
    /// OS exit code, when available.
    pub exit_code: Option<i32>,
    /// Terminating signal, when available (POSIX).
    pub signal: Option<String>,
    /// Duration from start to exit, in milliseconds.
    pub duration_ms: u64,
}

/// Per-job record kept in memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub config: JobConfig,
    pub state: JobState,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub last_output_at: Option<OffsetDateTime>,
    pub bytes_seen: u64,
    pub frames_seen: u64,
    pub events_emitted: u64,
    pub exit_info: Option<JobExitInfo>,
}

impl JobRecord {
    fn new(config: JobConfig) -> Self {
        Self {
            config,
            state: JobState::Starting,
            started_at: OffsetDateTime::now_utc(),
            ended_at: None,
            last_output_at: None,
            bytes_seen: 0,
            frames_seen: 0,
            events_emitted: 0,
            exit_info: None,
        }
    }
}

/// In-memory job manager. Send + Sync via RwLock.
#[derive(Debug, Default)]
pub struct JobManager {
    inner: RwLock<HashMap<JobId, JobRecord>>,
}

impl JobManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking a new job. Returns its JobId.
    pub fn start(&self, config: JobConfig) -> JobId {
        let id = config.job_id;
        self.inner.write().insert(id, JobRecord::new(config));
        id
    }

    /// Move a job from Starting to Running. Idempotent.
    pub fn mark_running(&self, job_id: JobId) {
        if let Some(rec) = self.inner.write().get_mut(&job_id)
            && rec.state == JobState::Starting
        {
            rec.state = JobState::Running;
        }
    }

    /// Bump byte+frame counters from a probe's per-frame work.
    pub fn record_frame(&self, job_id: JobId, bytes: u64) {
        if let Some(rec) = self.inner.write().get_mut(&job_id) {
            rec.frames_seen = rec.frames_seen.saturating_add(1);
            rec.bytes_seen = rec.bytes_seen.saturating_add(bytes);
            rec.last_output_at = Some(OffsetDateTime::now_utc());
        }
    }

    /// Bump events_emitted.
    pub fn record_event(&self, job_id: JobId) {
        if let Some(rec) = self.inner.write().get_mut(&job_id) {
            rec.events_emitted = rec.events_emitted.saturating_add(1);
        }
    }

    /// Mark a job exited. Returns an `EventDraft` for the bucket
    /// manager: a `command_exited` for code 0, `command_failed` for
    /// any non-zero (or signal).
    #[allow(clippy::needless_pass_by_value)]
    pub fn finish(
        &self,
        job_id: JobId,
        exit_code: Option<i32>,
        signal: Option<String>,
    ) -> Option<EventDraft> {
        let mut g = self.inner.write();
        let rec = g.get_mut(&job_id)?;
        rec.ended_at = Some(OffsetDateTime::now_utc());
        let duration_ms = rec.ended_at.map_or(0u64, |e| {
            u64::try_from((e - rec.started_at).whole_milliseconds().max(0)).unwrap_or(u64::MAX)
        });
        let exit_info = JobExitInfo {
            exit_code,
            signal: signal.clone(),
            duration_ms,
        };
        let nonzero = !matches!(exit_code, Some(0)) || signal.is_some();
        rec.state = if nonzero {
            JobState::Failed
        } else {
            JobState::Exited
        };
        rec.exit_info = Some(exit_info.clone());
        let config = rec.config.clone();
        let started_at = rec.started_at;
        drop(g);
        Some(Self::build_exit_event(
            &config, &exit_info, nonzero, started_at,
        ))
    }

    /// Mark a job cancelled (operator-initiated kill before exit).
    pub fn cancel(&self, job_id: JobId) -> Option<EventDraft> {
        let mut g = self.inner.write();
        let rec = g.get_mut(&job_id)?;
        rec.state = JobState::Cancelled;
        let now = OffsetDateTime::now_utc();
        rec.ended_at = Some(now);
        let duration_ms =
            u64::try_from((now - rec.started_at).whole_milliseconds().max(0)).unwrap_or(u64::MAX);
        let exit_info = JobExitInfo {
            exit_code: None,
            signal: Some("CANCELLED".to_owned()),
            duration_ms,
        };
        rec.exit_info = Some(exit_info.clone());
        let config = rec.config.clone();
        let started_at = rec.started_at;
        drop(g);
        Some(Self::build_exit_event(
            &config, &exit_info, true, started_at,
        ))
    }

    fn build_exit_event(
        config: &JobConfig,
        exit: &JobExitInfo,
        nonzero: bool,
        started_at: OffsetDateTime,
    ) -> EventDraft {
        let mut caps = Captures::new();
        if let Some(c) = exit.exit_code {
            caps.insert("exit_code".to_owned(), c.to_string());
        }
        if let Some(sig) = &exit.signal {
            caps.insert("signal".to_owned(), sig.clone());
        }
        caps.insert("duration_ms".to_owned(), exit.duration_ms.to_string());

        let kind = if nonzero {
            "command_failed"
        } else {
            "command_exited"
        };
        let severity = if nonzero {
            Severity::Critical
        } else {
            Severity::Low
        };
        let command = lifecycle_command_label(&config.argv);
        let summary = if nonzero {
            format!(
                "command failed: {command} (exit={ec:?}, signal={sig:?}, dur_ms={dur})",
                ec = exit.exit_code,
                sig = exit.signal,
                dur = exit.duration_ms,
            )
        } else {
            format!(
                "command exited cleanly: {command} (dur_ms={dur})",
                dur = exit.duration_ms,
            )
        };

        // Lifecycle markers do not point at a stream frame; the
        // TC02 invariant requires a `pointer_unavailable_reason`
        // when severity >= Medium and no pointer is set.
        let pointer_unavailable_reason = if nonzero {
            Some("synthetic command-exit lifecycle event".to_owned())
        } else {
            None
        };

        EventDraft {
            bucket_id: config.bucket_id,
            timestamp: OffsetDateTime::now_utc(),
            severity,
            kind: kind.to_owned(),
            summary,
            rule: None,
            source: EventSource {
                probe_id: config.probe_id,
                source_type: SourceType::Process,
                stream: SourceStream::Meta,
                job_id: Some(config.job_id),
            },
            captures: Some(caps),
            pointer: None,
            pointer_unavailable_reason,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: Some(started_at),
            last_seen: Some(exit.exit_info_timestamp(started_at)),
            suppressed: false,
        }
    }

    /// Snapshot a job record.
    #[must_use]
    pub fn get(&self, job_id: JobId) -> Option<JobRecord> {
        self.inner.read().get(&job_id).cloned()
    }

    /// All known jobs.
    #[must_use]
    pub fn list(&self) -> Vec<JobRecord> {
        self.inner.read().values().cloned().collect()
    }

    /// Remove a job record (e.g. for tests or operator cleanup).
    pub fn forget(&self, job_id: JobId) -> Option<JobRecord> {
        self.inner.write().remove(&job_id)
    }
}

fn lifecycle_command_label(argv: &[String]) -> String {
    let program = argv
        .first()
        .and_then(|arg0| Path::new(arg0).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("<unknown>");
    format!("program={program:?} argc={}", argv.len())
}

impl JobExitInfo {
    /// Compute an effective "last_seen" timestamp for the synthetic
    /// exit event: started_at + duration.
    fn exit_info_timestamp(&self, started_at: OffsetDateTime) -> OffsetDateTime {
        started_at
            + time::Duration::milliseconds(i64::try_from(self.duration_ms).unwrap_or(i64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> JobConfig {
        JobConfig::new(
            vec!["echo".to_owned(), "hi".to_owned()],
            BucketId::new(),
            ProbeId::new(),
        )
    }

    fn secret_cfg() -> JobConfig {
        JobConfig::new(
            vec![
                r"C:\Windows\System32\where.exe".to_owned(),
                "--token=tc_fake_secret_123".to_owned(),
                "wsl.exe".to_owned(),
            ],
            BucketId::new(),
            ProbeId::new(),
        )
    }

    #[test]
    fn job_starts_running_records_frame_and_event() {
        let m = JobManager::new();
        let id = m.start(cfg());
        m.mark_running(id);
        m.record_frame(id, 12);
        m.record_event(id);
        let rec = m.get(id).unwrap();
        assert_eq!(rec.state, JobState::Running);
        assert_eq!(rec.frames_seen, 1);
        assert_eq!(rec.bytes_seen, 12);
        assert_eq!(rec.events_emitted, 1);
        assert!(rec.last_output_at.is_some());
    }

    #[test]
    fn job_zero_exit_emits_command_exited_low_severity() {
        let m = JobManager::new();
        let id = m.start(cfg());
        let draft = m.finish(id, Some(0), None).unwrap();
        assert_eq!(draft.kind, "command_exited");
        assert_eq!(draft.severity, Severity::Low);
        // Low severity: no pointer + no reason is permitted.
        draft.validate().unwrap();
        let rec = m.get(id).unwrap();
        assert_eq!(rec.state, JobState::Exited);
    }

    #[test]
    fn job_nonzero_exit_emits_command_failed_critical() {
        let m = JobManager::new();
        let id = m.start(cfg());
        let draft = m.finish(id, Some(2), None).unwrap();
        assert_eq!(draft.kind, "command_failed");
        assert_eq!(draft.severity, Severity::Critical);
        assert!(draft.pointer_unavailable_reason.is_some());
        draft.validate().unwrap();
        assert_eq!(m.get(id).unwrap().state, JobState::Failed);
    }

    #[test]
    fn job_lifecycle_summaries_do_not_echo_full_argv() {
        let m = JobManager::new();
        let success_id = m.start(secret_cfg());
        let success = m.finish(success_id, Some(0), None).unwrap();
        assert_eq!(success.kind, "command_exited");
        assert!(success.summary.contains("where.exe"));
        assert!(success.summary.contains("argc=3"));
        assert!(!success.summary.contains("tc_fake_secret_123"));
        assert!(!success.summary.contains(r"C:\Windows\System32"));

        let failure_id = m.start(secret_cfg());
        let failure = m.finish(failure_id, Some(2), None).unwrap();
        assert_eq!(failure.kind, "command_failed");
        assert!(failure.summary.contains("where.exe"));
        assert!(failure.summary.contains("argc=3"));
        assert!(!failure.summary.contains("tc_fake_secret_123"));
        assert!(!failure.summary.contains(r"C:\Windows\System32"));
    }

    #[test]
    fn job_cancel_emits_critical_event_and_marks_cancelled() {
        let m = JobManager::new();
        let id = m.start(cfg());
        let draft = m.cancel(id).unwrap();
        assert_eq!(draft.severity, Severity::Critical);
        assert_eq!(
            draft
                .captures
                .as_ref()
                .and_then(|c| c.get("signal"))
                .map(String::as_str),
            Some("CANCELLED")
        );
        assert_eq!(m.get(id).unwrap().state, JobState::Cancelled);
    }

    #[test]
    fn job_list_and_forget() {
        let m = JobManager::new();
        let _ = m.start(cfg());
        let id2 = m.start(cfg());
        assert_eq!(m.list().len(), 2);
        let _ = m.forget(id2);
        assert_eq!(m.list().len(), 1);
    }

    #[test]
    fn job_manager_is_send_sync() {
        fn assert_ss<T: Send + Sync>() {}
        assert_ss::<JobManager>();
    }
}
