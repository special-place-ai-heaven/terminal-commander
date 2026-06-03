// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Per-bucket source side-table (subscriptions MUST-ADD #2).
//!
//! Buckets carry NO source identity today (`BucketState` has none;
//! `Router::bucket_create` stores none). This side-table is the routing
//! substrate: it is populated at the 3 existing `bucket_create` call sites
//! (command, file-watch, pty) so a [`crate::subscriptions::model::Predicate`]
//! can resolve `sources: {jobs|buckets|probes}` against per-bucket identity.
//!
//! Buckets are IMMORTAL in Terminal Commander (no production `drop_bucket` call
//! site; `list_bucket_ids()` grows monotonically), so this table has no
//! `remove`: an entry, once written, lives for the daemon session. A
//! `dirty` epoch is bumped on every `record` so the pull engine can skip a
//! routing rebuild when nothing changed (LOAD-BEARING: without it a
//! `sources: all` pull would re-scan every ever-created bucket each call).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use terminal_commander_core::{BucketId, JobId, ProbeId};
use terminal_commander_ipc::ProbeKind;

/// The source identity recorded for a bucket at `bucket_create`.
///
/// `job_id` / `probe_id` enable `sources: {jobs|probes}` routing; `path` is
/// retained for file-watch sources (and future tag/argv routing). Fields are
/// optional because not every probe kind populates every identity (e.g. a
/// file-watch records its `watch_id` as `job_id` and its watched `path`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketSource {
    /// Which kind of probe created the bucket.
    pub kind: ProbeKind,
    /// Owning job (command/pty job id, or a file-watch's `watch_id`).
    pub job_id: Option<JobId>,
    /// Owning probe id.
    pub probe_id: Option<ProbeId>,
    /// Watched path (file-watch only).
    pub path: Option<PathBuf>,
}

/// Immortal bucket -> source map plus a monotonically-increasing dirty epoch.
///
/// Shared (`Arc`) between `DaemonState` and the three runtimes that create
/// buckets, mirroring how `activation: Arc<ActivationRegistry>` is threaded.
#[derive(Debug, Default)]
pub struct BucketSourceTable {
    map: RwLock<HashMap<BucketId, BucketSource>>,
    dirty: AtomicU64,
}

impl BucketSourceTable {
    /// Construct an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the source for a freshly-created bucket and bump the dirty
    /// epoch. Called immediately after `Router::bucket_create` at each of the
    /// three probe-start sites. Idempotent on the value but always bumps the
    /// epoch (a `record` is only issued once per bucket in practice, since
    /// bucket ids are freshly minted).
    pub fn record(&self, id: BucketId, source: BucketSource) {
        self.map.write().insert(id, source);
        self.dirty.fetch_add(1, Ordering::Release);
    }

    /// Look up a bucket's source identity, if recorded.
    #[must_use]
    pub fn get(&self, id: BucketId) -> Option<BucketSource> {
        self.map.read().get(&id).cloned()
    }

    /// Snapshot all `(BucketId, BucketSource)` pairs. Order is unspecified.
    #[must_use]
    pub fn snapshot(&self) -> Vec<(BucketId, BucketSource)> {
        self.map
            .read()
            .iter()
            .map(|(id, src)| (*id, src.clone()))
            .collect()
    }

    /// Current dirty epoch. Bumped on every [`Self::record`]; a pull can
    /// compare it against a cached value to skip a routing rebuild.
    #[must_use]
    pub fn dirty_epoch(&self) -> u64 {
        self.dirty.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_source(job: JobId, probe: ProbeId) -> BucketSource {
        BucketSource {
            kind: ProbeKind::Command,
            job_id: Some(job),
            probe_id: Some(probe),
            path: None,
        }
    }

    #[test]
    fn record_then_get_returns_recorded_source() {
        let table = BucketSourceTable::new();
        let bucket = BucketId::new();
        let job = JobId::new();
        let probe = ProbeId::new();
        let src = cmd_source(job, probe);

        assert_eq!(table.get(bucket), None, "unrecorded bucket has no source");
        table.record(bucket, src.clone());
        assert_eq!(table.get(bucket), Some(src), "recorded source round-trips");
    }

    #[test]
    fn dirty_epoch_increments_on_each_record() {
        let table = BucketSourceTable::new();
        let start = table.dirty_epoch();

        table.record(BucketId::new(), cmd_source(JobId::new(), ProbeId::new()));
        let after_one = table.dirty_epoch();
        assert!(after_one > start, "record bumps the dirty epoch");

        table.record(BucketId::new(), cmd_source(JobId::new(), ProbeId::new()));
        let after_two = table.dirty_epoch();
        assert!(
            after_two > after_one,
            "each record bumps the dirty epoch again"
        );
    }

    #[test]
    fn snapshot_returns_all_recorded_pairs() {
        let table = BucketSourceTable::new();
        let b1 = BucketId::new();
        let b2 = BucketId::new();
        table.record(b1, cmd_source(JobId::new(), ProbeId::new()));
        table.record(b2, cmd_source(JobId::new(), ProbeId::new()));

        let snap = table.snapshot();
        assert_eq!(snap.len(), 2, "snapshot has both recorded buckets");
        let ids: Vec<BucketId> = snap.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&b1) && ids.contains(&b2));
    }
}
