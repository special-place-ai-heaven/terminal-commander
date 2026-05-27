// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon local API router (TC21).
//!
//! Wires the in-memory bucket manager, context rings, job manager,
//! sifter runtime, and an [`AuditSink`] into a single `Router`
//! exposing the typed operations that the MCP server (TC23) and admin
//! CLI (TC25) call.
//!
//! Source-status:
//! - Router itself: live (TC21) for the in-process API. The UDS /
//!   named-pipe transport shipped in TC37 and lives in `ipc/server.rs`
//!   (this module remains the in-process layer it dispatches into).
//! - Audit emission: live (TC35). Backed by [`AuditSink`]. By default
//!   `Router::new` uses an [`InMemoryAudit`] sink for library smoke
//!   and unit tests; production callers use `Router::with_sink` with
//!   a [`PersistentAudit`] backed by an `EventStore`.

use std::sync::Arc;

use terminal_commander_core::{
    BucketConfig, BucketId, BucketManager, BucketReadRequest, BucketReadResponse, BucketSummary,
    BucketWaitRequest, BucketWaitResponse, ContextRingManager, ContextWindowRequest,
    ContextWindowResponse, EventDraft, JobConfig, JobId, JobManager, JobRecord, ProbeId,
    SignalEvent,
};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::AuditEntry;

use crate::audit::{AuditSink, InMemoryAudit};

/// Daemon router. Holds Arc-shared subsystems and the audit sink.
#[derive(Debug)]
pub struct Router {
    pub buckets: Arc<BucketManager>,
    pub rings: Arc<ContextRingManager>,
    pub jobs: Arc<JobManager>,
    pub sifter: Arc<SifterRuntime>,
    pub audit: Arc<dyn AuditSink>,
}

impl Router {
    /// Construct a router with the default in-memory audit sink.
    /// Convenient for library smoke and unit tests; production code
    /// should call [`Router::with_sink`] with a persistent sink.
    #[must_use]
    pub fn new(
        buckets: Arc<BucketManager>,
        rings: Arc<ContextRingManager>,
        jobs: Arc<JobManager>,
        sifter: Arc<SifterRuntime>,
    ) -> Self {
        Self::with_sink(buckets, rings, jobs, sifter, Arc::new(InMemoryAudit::new()))
    }

    /// Construct a router with an explicit audit sink. Production
    /// callers pass a `PersistentAudit` backed by an `EventStore`.
    #[must_use]
    pub fn with_sink(
        buckets: Arc<BucketManager>,
        rings: Arc<ContextRingManager>,
        jobs: Arc<JobManager>,
        sifter: Arc<SifterRuntime>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        Self {
            buckets,
            rings,
            jobs,
            sifter,
            audit,
        }
    }

    /// Best-effort audit. Errors are not propagated to callers; the
    /// router would otherwise have to refuse normal traffic on an
    /// audit-store hiccup. Failures still surface in the audit row
    /// count (it does not increment) and any persistent sink can
    /// report the underlying error through its own diagnostics. The
    /// router accepts the trade-off because the alternative (refuse
    /// to serve when audit is unhealthy) is also a contract violation
    /// for the realtime signal channel.
    fn record(&self, entry: &AuditEntry) {
        let _ = self.audit.emit(entry);
    }

    /// Errors produced by router operations.
    #[allow(clippy::missing_errors_doc)]
    pub fn bucket_create(
        &self,
        bucket_id: BucketId,
        config: BucketConfig,
    ) -> Result<(), terminal_commander_core::BucketError> {
        self.record(
            &AuditEntry::new("bucket_create", bucket_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        self.buckets.create_bucket(bucket_id, config)
    }

    /// Append an event draft to a bucket. The router mints the
    /// SignalEvent and lets the manager assign the seq.
    #[allow(clippy::missing_errors_doc)]
    pub fn bucket_append(
        &self,
        bucket_id: BucketId,
        draft: EventDraft,
    ) -> Result<SignalEvent, terminal_commander_core::BucketError> {
        self.record(
            &AuditEntry::new("bucket_append", bucket_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        let mut ev = draft.into_signal_event(0);
        let seq = self.buckets.append(bucket_id, ev.clone())?;
        ev.seq = seq;
        Ok(ev)
    }

    /// Read events since a cursor.
    #[allow(clippy::missing_errors_doc)]
    pub fn bucket_events_since(
        &self,
        bucket_id: BucketId,
        request: &BucketReadRequest,
    ) -> Result<BucketReadResponse, terminal_commander_core::BucketError> {
        self.record(
            &AuditEntry::new("bucket_events_since", bucket_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        self.buckets.events_since(bucket_id, request)
    }

    /// Wait for matching events (blocking).
    #[allow(clippy::missing_errors_doc)]
    pub async fn bucket_wait(
        &self,
        bucket_id: BucketId,
        request: BucketWaitRequest,
    ) -> Result<BucketWaitResponse, terminal_commander_core::BucketError> {
        self.record(
            &AuditEntry::new("bucket_wait", bucket_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        self.buckets.bucket_wait(bucket_id, request).await
    }

    /// Bucket summary.
    #[allow(clippy::missing_errors_doc)]
    pub fn bucket_summary(
        &self,
        bucket_id: BucketId,
    ) -> Result<BucketSummary, terminal_commander_core::BucketError> {
        self.record(
            &AuditEntry::new("bucket_summary", bucket_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        self.buckets.summary(bucket_id)
    }

    /// Resolve event-context window via the context ring.
    #[allow(clippy::missing_errors_doc)]
    pub fn event_context(
        &self,
        probe_id: ProbeId,
        anchor: terminal_commander_core::FrameId,
        before: u32,
        after: u32,
        max_bytes: Option<usize>,
    ) -> Result<ContextWindowResponse, terminal_commander_core::ContextError> {
        self.record(
            &AuditEntry::new("event_context", probe_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        self.rings.window(&ContextWindowRequest {
            probe_id,
            anchor,
            before,
            after,
            max_bytes,
        })
    }

    /// Start tracking a new job.
    #[must_use]
    pub fn job_start(&self, config: JobConfig) -> JobId {
        self.record(
            &AuditEntry::new("job_start", config.job_id.to_wire_string(), "info")
                .with_actor("router"),
        );
        self.jobs.start(config)
    }

    /// Finalize a job; returns the lifecycle event draft.
    pub fn job_finish(
        &self,
        job_id: JobId,
        exit_code: Option<i32>,
        signal: Option<String>,
    ) -> Option<EventDraft> {
        self.record(
            &AuditEntry::new("job_finish", job_id.to_wire_string(), "info").with_actor("router"),
        );
        self.jobs.finish(job_id, exit_code, signal)
    }

    /// Cancel a job.
    pub fn job_cancel(&self, job_id: JobId) -> Option<EventDraft> {
        self.record(
            &AuditEntry::new("job_cancel", job_id.to_wire_string(), "info").with_actor("router"),
        );
        self.jobs.cancel(job_id)
    }

    /// Snapshot a job record.
    #[must_use]
    pub fn job_get(&self, job_id: JobId) -> Option<JobRecord> {
        self.jobs.get(job_id)
    }

    /// Helper for tests / operators: number of audit rows currently
    /// recorded by the configured sink.
    #[must_use]
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{
        BucketConfig, Captures, EventDraft, EventSource, FrameId, ProbeId, Severity, SourcePointer,
        SourceStream, SourceType,
    };
    use terminal_commander_store::AuditReadRequest;

    fn build_router() -> Router {
        let buckets = Arc::new(BucketManager::new());
        let rings = Arc::new(ContextRingManager::new());
        let jobs = Arc::new(JobManager::new());
        let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
        Router::new(buckets, rings, jobs, sifter)
    }

    fn make_draft(bid: BucketId, pid: ProbeId, sev: Severity, kind: &str) -> EventDraft {
        let mut caps = Captures::new();
        caps.insert("k".to_owned(), "v".to_owned());
        EventDraft {
            bucket_id: bid,
            timestamp: time::OffsetDateTime::now_utc(),
            severity: sev,
            kind: kind.to_owned(),
            summary: "s".to_owned(),
            rule: None,
            source: EventSource {
                probe_id: pid,
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        }
    }

    #[test]
    fn router_creates_bucket_and_appends() {
        let r = build_router();
        let bid = BucketId::new();
        r.bucket_create(bid, BucketConfig::default()).unwrap();
        let ev = r
            .bucket_append(bid, make_draft(bid, ProbeId::new(), Severity::High, "k1"))
            .unwrap();
        assert!(ev.seq >= 1);
        assert!(r.audit_len() >= 2);
    }

    #[test]
    fn router_reads_with_cursor() {
        let r = build_router();
        let bid = BucketId::new();
        r.bucket_create(bid, BucketConfig::default()).unwrap();
        r.bucket_append(bid, make_draft(bid, ProbeId::new(), Severity::Low, "a"))
            .unwrap();
        r.bucket_append(bid, make_draft(bid, ProbeId::new(), Severity::Low, "b"))
            .unwrap();
        let resp = r
            .bucket_events_since(bid, &BucketReadRequest::new(0))
            .unwrap();
        assert_eq!(resp.events.len(), 2);
    }

    #[test]
    fn router_job_lifecycle_emits_drafts() {
        let r = build_router();
        let bid = BucketId::new();
        let pid = ProbeId::new();
        let cfg = JobConfig::new(vec!["echo".to_owned()], bid, pid);
        let id = r.job_start(cfg);
        let draft = r.job_finish(id, Some(0), None).unwrap();
        assert_eq!(draft.kind, "command_exited");
        assert!(r.job_get(id).is_some());
    }

    #[test]
    fn router_audit_records_actions() {
        let r = build_router();
        let bid = BucketId::new();
        r.bucket_create(bid, BucketConfig::default()).unwrap();
        let rows = r.audit.read_since(&AuditReadRequest::new(0)).unwrap();
        assert!(rows.iter().any(|a| a.action == "bucket_create"));
    }

    #[test]
    fn router_event_context_round_trip() {
        let r = build_router();
        let pid = ProbeId::new();
        r.rings.create_ring_default(pid).unwrap();
        let frame =
            terminal_commander_core::SourceFrame::new(pid, SourceStream::Stdout, "line".to_owned())
                .with_line(1);
        let fid = frame.frame_id;
        r.rings.append_frame(pid, frame).unwrap();
        let resp = r.event_context(pid, fid, 0, 0, None).unwrap();
        assert!(!resp.anchor_missing);
        assert_eq!(resp.frames.len(), 1);
    }
}
