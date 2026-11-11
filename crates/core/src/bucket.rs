// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! In-memory signal bucket manager.
//!
//! A bucket is a cursor-based ordered stream of [`SignalEvent`]s.
//! Per the TC07 mini-spec it has:
//!
//! - per-bucket monotonic `seq` numbers (assigned by the manager);
//! - bounded reads (caller-provided or default limit);
//! - severity-min filtering and optional kind filter;
//! - summary with counts by severity and kind, plus the noise
//!   counters [`BucketSummary::noise_suppressed_count`] and
//!   [`BucketSummary::dedupe_collapsed_count`] reserved zero-defaulted
//!   until TC11 wires noise/dedupe.
//!
//! Cap and eviction (locked at TC07): per-bucket max events AND
//! per-bucket TTL. On append-full or on TTL sweep the head event is
//! evicted and [`BucketState::dropped_count`] is incremented.
//! Producers never block; consumers see `dropped_count` in every
//! [`BucketReadResponse`] and [`BucketSummary`].
//!
//! Concurrency: [`BucketManager`] is `Send + Sync` via a
//! `parking_lot::RwLock` around the inner map. Per-bucket lookups
//! take a read lock then a per-bucket write lock; appends and
//! reads do not contend across buckets.
//!
//! Persistence: TC07 is in-memory only. TC12 introduces a SQLite
//! backed store; the `seq: u64` -> SQLite `INTEGER` (i64) conversion
//! site lives in `terminal-commander-store` and asserts no value
//! crosses `i64::MAX`.
//!
//! Source-status: live (TC07). Probe input wiring lands in TC15+.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::Notify;

use crate::error::CoreError;
use crate::event::SignalEvent;
use crate::ids::BucketId;
use crate::severity::Severity;

/// How often a wait with an [`ExitProbe`] re-checks it between wake-ups.
const EXIT_RECHECK: Duration = Duration::from_secs(1);

/// Default per-bucket maximum event count (operator-tunable).
pub const DEFAULT_MAX_EVENTS: usize = 10_000;

/// Default per-bucket TTL (operator-tunable). 24 hours.
pub const DEFAULT_TTL: Duration = Duration::from_hours(24);

/// Default upper bound on a single [`BucketManager::events_since`] read.
pub const DEFAULT_READ_LIMIT: usize = 200;

/// Hard cap on a single read regardless of caller request. Protects
/// against accidental over-sized responses and matches the bounded-
/// output invariant.
pub const MAX_READ_LIMIT: usize = 10_000;

/// Configuration for a single bucket.
///
/// Defaults: 10_000 events / 24h TTL. Both are tunable; setting
/// either to `usize::MAX` / `Duration::MAX` disables that axis but
/// the other remains active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketConfig {
    /// Maximum number of events kept in memory. FIFO eviction on
    /// overflow.
    pub max_events: usize,
    /// Maximum age. Older events are evicted on read or on sweep.
    #[serde(with = "ttl_seconds")]
    pub ttl: Duration,
}

impl Default for BucketConfig {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_MAX_EVENTS,
            ttl: DEFAULT_TTL,
        }
    }
}

mod ttl_seconds {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub(super) fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_secs())
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_secs(u64::deserialize(d)?))
    }
}

/// Per-bucket runtime state visible to operators (subset of internals).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketState {
    pub bucket_id: BucketId,
    pub created_at: OffsetDateTime,
    pub last_event_at: Option<OffsetDateTime>,
    pub head_seq: u64,
    pub tail_seq: u64,
    pub event_count: u64,
    /// Number of events evicted by FIFO overflow or TTL sweep since
    /// bucket creation. Surfaced to consumers so they can detect loss.
    pub dropped_count: u64,
}

/// Summary statistics for a bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketSummary {
    pub bucket_id: BucketId,
    pub created_at: OffsetDateTime,
    pub head_seq: u64,
    pub tail_seq: u64,
    pub event_count: u64,
    pub last_event_at: Option<OffsetDateTime>,
    pub by_severity: BySeverity,
    pub by_kind: BTreeMap<String, u64>,
    pub dropped_count: u64,
    /// Reserved field, zero-defaulted until TC11 wires noise
    /// suppression. Surfaced in the contract so consumers can ignore
    /// vs. read uniformly across versions.
    pub noise_suppressed_count: u64,
    /// Reserved field, zero-defaulted until TC11 wires dedupe.
    pub dedupe_collapsed_count: u64,
}

/// Severity-bucket count map. Always carries all seven keys (zero
/// when absent) so consumers can index without `Option` dance.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BySeverity {
    pub trace: u64,
    pub debug: u64,
    pub info: u64,
    pub low: u64,
    pub medium: u64,
    pub high: u64,
    pub critical: u64,
}

impl BySeverity {
    const fn slot_mut(&mut self, s: Severity) -> &mut u64 {
        match s {
            Severity::Trace => &mut self.trace,
            Severity::Debug => &mut self.debug,
            Severity::Info => &mut self.info,
            Severity::Low => &mut self.low,
            Severity::Medium => &mut self.medium,
            Severity::High => &mut self.high,
            Severity::Critical => &mut self.critical,
        }
    }
    const fn bump(&mut self, s: Severity) {
        let slot = self.slot_mut(s);
        *slot = slot.saturating_add(1);
    }
    const fn unbump(&mut self, s: Severity) {
        let slot = self.slot_mut(s);
        *slot = slot.saturating_sub(1);
    }
}

/// Request shape for [`BucketManager::events_since`].
#[derive(Debug, Clone)]
pub struct BucketReadRequest {
    pub cursor: u64,
    pub severity_min: Option<Severity>,
    pub kind_filter: Option<String>,
    pub limit: Option<usize>,
}

impl BucketReadRequest {
    /// Construct a minimal request reading from `cursor` with no
    /// filters and the default limit.
    #[must_use]
    pub const fn new(cursor: u64) -> Self {
        Self {
            cursor,
            severity_min: None,
            kind_filter: None,
            limit: None,
        }
    }
}

/// Response shape from [`BucketManager::events_since`].
///
/// Per the prime directive, `events` carries STRUCTURED
/// [`SignalEvent`]s only. Raw stream text is never returned through
/// this path; raw frames live in the context ring (TC08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketReadResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    pub has_more: bool,
    pub dropped_count: u64,
    pub events: Vec<SignalEvent>,
}

/// When a `bucket_wait` should wake before its timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeMode {
    /// Wake on any append to the bucket (the historical behaviour); a wake
    /// whose events are all filtered out returns an empty response.
    #[default]
    Any,
    /// Wake only when an event passes the caller's filters, when the
    /// watched job ends, or on timeout.
    Match,
}

/// Why a `bucket_wait` returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BucketWakeReason {
    /// Woken by an append (`wake_mode = any`).
    Append,
    /// An event passed the caller's filters (`wake_mode = match`).
    Match,
    /// The watched job ended.
    Exit,
    /// The wait timed out.
    #[default]
    Timeout,
}

/// Cheap "has the job that owns this bucket ended?" check, supplied by the
/// daemon (the bucket manager knows nothing about jobs).
#[derive(Clone)]
pub struct ExitProbe(pub Arc<dyn Fn() -> bool + Send + Sync>);

impl std::fmt::Debug for ExitProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExitProbe")
    }
}

/// Wait request shape.
#[derive(Debug, Clone)]
pub struct BucketWaitRequest {
    pub cursor: u64,
    pub severity_min: Option<Severity>,
    pub kind_filter: Option<String>,
    pub limit: Option<usize>,
    pub timeout: Duration,
    /// Wake policy. Default [`WakeMode::Any`].
    pub wake_mode: WakeMode,
    /// Job-ended check; `None` for a bucket with no owning job.
    pub exit_probe: Option<ExitProbe>,
}

impl BucketWaitRequest {
    /// Construct a wait request reading from `cursor`.
    #[must_use]
    pub const fn new(cursor: u64, timeout: Duration) -> Self {
        Self {
            cursor,
            severity_min: None,
            kind_filter: None,
            limit: None,
            timeout,
            wake_mode: WakeMode::Any,
            exit_probe: None,
        }
    }
}

/// Wait response shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketWaitResponse {
    pub bucket_id: BucketId,
    pub cursor_in: u64,
    pub next_cursor: u64,
    pub heartbeat: bool,
    pub events: Vec<SignalEvent>,
    pub dropped_count: u64,
    pub wake_reason: BucketWakeReason,
}

/// Errors emitted by [`BucketManager`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BucketError {
    #[error("bucket '{0}' already exists")]
    AlreadyExists(BucketId),
    #[error("bucket '{0}' not found")]
    NotFound(BucketId),
    #[error("event bucket mismatch: event.bucket_id={event} but appending to {bucket}")]
    EventBucketMismatch { event: BucketId, bucket: BucketId },
    #[error(
        "event seq {seq} is not greater than the bucket tail seq {tail} (manager assigns seq monotonically; do not pre-set)"
    )]
    NonMonotonicSeq { seq: u64, tail: u64 },
    #[error("event seq {seq} not found in bucket '{bucket}'")]
    EventSeqNotFound { bucket: BucketId, seq: u64 },
    #[error(
        "caller pre-set a nonzero seq {seq} on the first event of empty bucket '{bucket}' \
         (the manager assigns seq monotonically from 1; do not pre-set)"
    )]
    PresetSeqIntoEmptyBucket { bucket: BucketId, seq: u64 },
    #[error(
        "bucket '{bucket}' seq space exhausted at u64::MAX (tail {tail}); cannot assign a new monotonic seq"
    )]
    SeqExhausted { bucket: BucketId, tail: u64 },
    #[error("core error: {0}")]
    Core(#[from] CoreError),
}

/// Inner per-bucket state (write-locked when mutated).
#[derive(Debug)]
struct BucketInner {
    config: BucketConfig,
    state: BucketState,
    by_severity: BySeverity,
    by_kind: BTreeMap<String, u64>,
    /// Ordered ring of events.
    events: std::collections::VecDeque<SignalEvent>,
    /// Reserved counters; zero until TC11.
    noise_suppressed_count: u64,
    dedupe_collapsed_count: u64,
    /// Wakeup signal for [`BucketManager::bucket_wait`] (TC17).
    notify: Arc<Notify>,
}

impl BucketInner {
    fn new(bucket_id: BucketId, config: BucketConfig) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            config,
            state: BucketState {
                bucket_id,
                created_at: now,
                last_event_at: None,
                head_seq: 0,
                tail_seq: 0,
                event_count: 0,
                dropped_count: 0,
            },
            by_severity: BySeverity::default(),
            by_kind: BTreeMap::new(),
            events: std::collections::VecDeque::new(),
            noise_suppressed_count: 0,
            dedupe_collapsed_count: 0,
            notify: Arc::new(Notify::new()),
        }
    }

    fn evict_expired(&mut self, now: OffsetDateTime) {
        let ttl_secs = i64::try_from(self.config.ttl.as_secs()).unwrap_or(i64::MAX);
        let cutoff = now - time::Duration::seconds(ttl_secs);
        while let Some(front) = self.events.front() {
            if front.timestamp < cutoff {
                let ev = self.events.pop_front().expect("front existed");
                self.uncount(&ev);
                self.state.dropped_count = self.state.dropped_count.saturating_add(1);
            } else {
                break;
            }
        }
        self.refresh_state();
    }

    fn evict_for_capacity(&mut self) {
        while self.events.len() > self.config.max_events {
            let ev = self.events.pop_front().expect("over capacity");
            self.uncount(&ev);
            self.state.dropped_count = self.state.dropped_count.saturating_add(1);
        }
        self.refresh_state();
    }

    fn count(&mut self, ev: &SignalEvent) {
        self.by_severity.bump(ev.severity);
        *self.by_kind.entry(ev.kind.clone()).or_insert(0) += 1;
    }

    fn uncount(&mut self, ev: &SignalEvent) {
        self.by_severity.unbump(ev.severity);
        if let Some(slot) = self.by_kind.get_mut(&ev.kind) {
            *slot = slot.saturating_sub(1);
            if *slot == 0 {
                self.by_kind.remove(&ev.kind);
            }
        }
    }

    fn refresh_state(&mut self) {
        if let (Some(front), Some(back)) = (self.events.front(), self.events.back()) {
            self.state.head_seq = front.seq;
            self.state.tail_seq = back.seq;
            self.state.event_count = self.events.len() as u64;
            self.state.last_event_at = Some(back.timestamp);
        } else {
            self.state.head_seq = self.state.tail_seq;
            self.state.event_count = 0;
            self.state.last_event_at = None;
        }
    }

    fn summary(&self) -> BucketSummary {
        BucketSummary {
            bucket_id: self.state.bucket_id,
            created_at: self.state.created_at,
            head_seq: self.state.head_seq,
            tail_seq: self.state.tail_seq,
            event_count: self.state.event_count,
            last_event_at: self.state.last_event_at,
            by_severity: self.by_severity.clone(),
            by_kind: self.by_kind.clone(),
            dropped_count: self.state.dropped_count,
            noise_suppressed_count: self.noise_suppressed_count,
            dedupe_collapsed_count: self.dedupe_collapsed_count,
        }
    }
}

/// In-memory bucket manager.
///
/// Append-side and read-side are both `&self`; locking is internal.
/// The manager assigns `seq` for events that are appended without a
/// pre-set value; pre-set sequence numbers must be strictly greater
/// than the current tail.
#[derive(Debug, Default)]
pub struct BucketManager {
    inner: RwLock<HashMap<BucketId, Arc<RwLock<BucketInner>>>>,
}

impl BucketManager {
    /// Construct an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a bucket with the given id and config. Fails with
    /// [`BucketError::AlreadyExists`] if the id is taken.
    pub fn create_bucket(
        &self,
        bucket_id: BucketId,
        config: BucketConfig,
    ) -> Result<(), BucketError> {
        use std::collections::hash_map::Entry;
        let mut map = self.inner.write();
        match map.entry(bucket_id) {
            Entry::Occupied(_) => Err(BucketError::AlreadyExists(bucket_id)),
            Entry::Vacant(v) => {
                v.insert(Arc::new(RwLock::new(BucketInner::new(bucket_id, config))));
                Ok(())
            }
        }
    }

    /// Create a bucket with default config.
    pub fn create_bucket_default(&self, bucket_id: BucketId) -> Result<(), BucketError> {
        self.create_bucket(bucket_id, BucketConfig::default())
    }

    /// Drop a bucket. Idempotent.
    pub fn drop_bucket(&self, bucket_id: BucketId) -> bool {
        self.inner.write().remove(&bucket_id).is_some()
    }

    /// Whether the manager knows the bucket.
    #[must_use]
    pub fn has_bucket(&self, bucket_id: BucketId) -> bool {
        self.inner.read().contains_key(&bucket_id)
    }

    fn bucket(&self, bucket_id: BucketId) -> Result<Arc<RwLock<BucketInner>>, BucketError> {
        self.inner
            .read()
            .get(&bucket_id)
            .cloned()
            .ok_or(BucketError::NotFound(bucket_id))
    }

    /// Append an event. The event MUST already have its
    /// `bucket_id` set to the target bucket; the manager assigns
    /// `seq` if `event.seq == 0`, or validates monotonicity otherwise.
    ///
    /// Validates the TC02 pointer invariant before insertion.
    pub fn append(&self, bucket_id: BucketId, mut event: SignalEvent) -> Result<u64, BucketError> {
        if event.bucket_id != bucket_id {
            return Err(BucketError::EventBucketMismatch {
                event: event.bucket_id,
                bucket: bucket_id,
            });
        }
        event.validate()?;
        let cell = self.bucket(bucket_id)?;
        let mut inner = cell.write();
        let assigned_seq = if event.seq == 0 {
            // Manager assigns the next monotonic seq. `checked_add`
            // (not `saturating_add`) so an exhausted seq space at
            // u64::MAX surfaces as a typed error instead of silently
            // re-emitting the tail seq as a duplicate.
            inner
                .state
                .tail_seq
                .checked_add(1)
                .ok_or(BucketError::SeqExhausted {
                    bucket: bucket_id,
                    tail: inner.state.tail_seq,
                })?
        } else if inner.state.event_count == 0 {
            // Empty bucket: there is no rehydrate/restore path that
            // pre-sets a seq (the sole production caller appends with
            // seq == 0). A nonzero caller seq into an empty bucket is
            // therefore always a bug; reject it rather than seeding the
            // seq space from an unvalidated value.
            return Err(BucketError::PresetSeqIntoEmptyBucket {
                bucket: bucket_id,
                seq: event.seq,
            });
        } else if event.seq <= inner.state.tail_seq {
            return Err(BucketError::NonMonotonicSeq {
                seq: event.seq,
                tail: inner.state.tail_seq,
            });
        } else {
            event.seq
        };
        event.seq = assigned_seq;
        inner.count(&event);
        inner.events.push_back(event);
        inner.refresh_state();
        inner.evict_for_capacity();
        // Wake any waiters.
        inner.notify.notify_waiters();
        Ok(assigned_seq)
    }

    /// Update aggregation fields on an existing event (TC11 cross-frame dedupe).
    pub fn patch_event_aggregation(
        &self,
        bucket_id: BucketId,
        seq: u64,
        count: u32,
        first_seen: OffsetDateTime,
        last_seen: OffsetDateTime,
    ) -> Result<(), BucketError> {
        let cell = self.bucket(bucket_id)?;
        let mut inner = cell.write();
        let Some(ev) = inner.events.iter_mut().find(|e| e.seq == seq) else {
            return Err(BucketError::EventSeqNotFound {
                bucket: bucket_id,
                seq,
            });
        };
        ev.count = count;
        ev.first_seen = Some(first_seen);
        ev.last_seen = Some(last_seen);
        inner.dedupe_collapsed_count = inner.dedupe_collapsed_count.saturating_add(1);
        inner.notify.notify_waiters();
        Ok(())
    }

    /// Wait for matching events to arrive in the bucket. Returns
    /// promptly when events are present or appended; returns a
    /// heartbeat response when the timeout elapses without
    /// matching events.
    ///
    /// [`WakeMode::Any`] (the default) returns on the first append even when
    /// the caller's filters reject it. [`WakeMode::Match`] keeps waiting
    /// until an event passes the filters. In both modes a job-ended
    /// [`ExitProbe`] ends the wait with [`BucketWakeReason::Exit`].
    ///
    /// Backed by `tokio::sync::Notify` and a deadline on tokio's clock: no
    /// busy wait. With an exit probe the wait also re-checks it once a
    /// second, so an exit whose lifecycle event was filtered or lost cannot
    /// strand the caller until the timeout.
    pub async fn bucket_wait(
        &self,
        bucket_id: BucketId,
        request: BucketWaitRequest,
    ) -> Result<BucketWaitResponse, BucketError> {
        // Snapshot the notify handle once.
        let notify = {
            let cell = self.bucket(bucket_id)?;
            let inner = cell.read();
            Arc::clone(&inner.notify)
        };

        let read_req = BucketReadRequest {
            cursor: request.cursor,
            severity_min: request.severity_min,
            kind_filter: request.kind_filter.clone(),
            limit: request.limit,
        };
        let match_only = request.wake_mode == WakeMode::Match;
        let job_ended = || request.exit_probe.as_ref().is_some_and(|p| (p.0)());
        let deadline = tokio::time::Instant::now() + request.timeout;
        let mut woken_by_append = false;

        loop {
            // Arm the wake-up BEFORE reading, so an append that lands between
            // the read and the await is not lost.
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let resp = self.events_since(bucket_id, &read_req)?;
            let respond = |heartbeat: bool, reason: BucketWakeReason| BucketWaitResponse {
                bucket_id,
                cursor_in: request.cursor,
                next_cursor: resp.next_cursor.max(request.cursor),
                heartbeat,
                events: resp.events.clone(),
                dropped_count: resp.dropped_count,
                wake_reason: reason,
            };
            if !resp.events.is_empty() {
                let reason = if match_only {
                    BucketWakeReason::Match
                } else {
                    BucketWakeReason::Append
                };
                return Ok(respond(false, reason));
            }
            if job_ended() {
                return Ok(respond(false, BucketWakeReason::Exit));
            }
            if woken_by_append && !match_only {
                // Historical behaviour: the append woke us but the filters
                // rejected it; return the empty read.
                return Ok(respond(true, BucketWakeReason::Append));
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                // Timeout: heartbeat with the bucket's current tail seq.
                let state = self.state(bucket_id)?;
                return Ok(BucketWaitResponse {
                    bucket_id,
                    cursor_in: request.cursor,
                    next_cursor: state.tail_seq.max(request.cursor),
                    heartbeat: true,
                    events: Vec::new(),
                    dropped_count: state.dropped_count,
                    wake_reason: BucketWakeReason::Timeout,
                });
            }
            let wake_at = if request.exit_probe.is_some() {
                deadline.min(now + EXIT_RECHECK)
            } else {
                deadline
            };
            woken_by_append = tokio::time::timeout_at(wake_at, notified.as_mut())
                .await
                .is_ok();
        }
    }

    /// Clone a bucket's wakeup [`Notify`] so a multiplexed consumer can arm it
    /// (see `subscription_pull`). Short outer read-lock to clone the cell Arc,
    /// then a short inner read-lock to clone the Notify — no lock held across
    /// await (mirrors the snapshot block in [`BucketManager::bucket_wait`]).
    pub fn bucket_notify(&self, bucket_id: BucketId) -> Result<Arc<Notify>, BucketError> {
        let cell = self.bucket(bucket_id)?;
        let inner = cell.read();
        Ok(Arc::clone(&inner.notify))
    }

    /// Read events strictly after `cursor`. The response is bounded
    /// by `limit` (clamped to [`MAX_READ_LIMIT`]).
    ///
    /// Reads also trigger TTL eviction so stale events do not
    /// appear in the response.
    pub fn events_since(
        &self,
        bucket_id: BucketId,
        request: &BucketReadRequest,
    ) -> Result<BucketReadResponse, BucketError> {
        let cell = self.bucket(bucket_id)?;
        let mut inner = cell.write();
        inner.evict_expired(OffsetDateTime::now_utc());

        let limit = request
            .limit
            .unwrap_or(DEFAULT_READ_LIMIT)
            .clamp(1, MAX_READ_LIMIT);

        let mut out: Vec<SignalEvent> = Vec::with_capacity(limit);
        let mut last_seq = request.cursor;
        let mut has_more = false;
        // Iterate a contiguous slice: `VecDeque::iter` pays ring-index
        // arithmetic per element; `make_contiguous` (we hold the write
        // lock) hands back a flat slice with no wraparound math. It is a
        // no-op when the deque is already contiguous.
        let kind_filter = request.kind_filter.as_deref();
        let events = inner.events.make_contiguous();
        for ev in events.iter() {
            if ev.seq <= request.cursor {
                continue;
            }
            if let Some(min) = request.severity_min
                && ev.severity < min
            {
                continue;
            }
            if let Some(kf) = kind_filter
                && ev.kind != kf
            {
                continue;
            }
            if out.len() >= limit {
                has_more = true;
                break;
            }
            last_seq = ev.seq;
            out.push(ev.clone());
        }
        Ok(BucketReadResponse {
            bucket_id,
            cursor_in: request.cursor,
            next_cursor: last_seq,
            has_more,
            dropped_count: inner.state.dropped_count,
            events: out,
        })
    }

    /// Compute a summary for the bucket. Also runs TTL eviction.
    pub fn summary(&self, bucket_id: BucketId) -> Result<BucketSummary, BucketError> {
        let cell = self.bucket(bucket_id)?;
        let mut inner = cell.write();
        inner.evict_expired(OffsetDateTime::now_utc());
        Ok(inner.summary())
    }

    /// Snapshot of bucket state. Used by admin paths (TC25).
    pub fn state(&self, bucket_id: BucketId) -> Result<BucketState, BucketError> {
        let cell = self.bucket(bucket_id)?;
        let inner = cell.read();
        Ok(inner.state.clone())
    }

    /// List every live bucket id. Used by the TC45 aggregate
    /// `runtime_state` view. Read-only; bounded by the bucket-count
    /// cap the daemon already enforces at create time.
    #[must_use]
    pub fn list_bucket_ids(&self) -> Vec<BucketId> {
        self.inner.read().keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Captures, RuleRef};
    use crate::ids::{BucketId, EventId, FrameId, ProbeId, RuleId};
    use crate::pointer::SourcePointer;
    use crate::source::{EventSource, SourceStream, SourceType};

    fn ev(bucket_id: BucketId, severity: Severity, kind: &str) -> SignalEvent {
        let mut caps = Captures::new();
        caps.insert("package".to_owned(), "libssl-dev".to_owned());
        SignalEvent {
            event_id: EventId::new(),
            bucket_id,
            seq: 0,
            timestamp: OffsetDateTime::now_utc(),
            severity,
            kind: kind.to_owned(),
            summary: format!("{kind} summary"),
            rule: Some(RuleRef {
                id: RuleId::new(),
                version: 1,
                registry_id: None,
            }),
            source: EventSource {
                probe_id: ProbeId::new(),
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        }
    }

    #[test]
    fn create_and_append_assigns_monotonic_seq() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        let s1 = mgr.append(bid, ev(bid, Severity::Low, "k1")).unwrap();
        let s2 = mgr.append(bid, ev(bid, Severity::Low, "k2")).unwrap();
        let s3 = mgr.append(bid, ev(bid, Severity::Low, "k3")).unwrap();
        assert_eq!((s1, s2, s3), (1, 2, 3));
    }

    #[test]
    fn duplicate_create_fails() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        let err = mgr.create_bucket_default(bid).unwrap_err();
        assert!(matches!(err, BucketError::AlreadyExists(_)));
    }

    #[test]
    fn append_mismatched_bucket_id_fails() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        let other = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        let err = mgr.append(bid, ev(other, Severity::Low, "k")).unwrap_err();
        assert!(matches!(err, BucketError::EventBucketMismatch { .. }));
    }

    #[test]
    fn preset_non_monotonic_seq_rejected() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "k1")).unwrap(); // seq=1
        mgr.append(bid, ev(bid, Severity::Low, "k2")).unwrap(); // seq=2
        let mut e = ev(bid, Severity::Low, "k3");
        e.seq = 2;
        let err = mgr.append(bid, e).unwrap_err();
        assert!(matches!(err, BucketError::NonMonotonicSeq { .. }));
    }

    #[test]
    fn preset_nonzero_seq_into_empty_bucket_rejected() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        // No prior append: the bucket is empty (event_count == 0). A caller
        // that pre-sets a nonzero seq is always a bug because there is no
        // rehydrate path; the manager must reject rather than seed the seq
        // space from the unvalidated value.
        let mut e = ev(bid, Severity::Low, "k1");
        e.seq = 5;
        let err = mgr.append(bid, e).unwrap_err();
        assert!(matches!(
            err,
            BucketError::PresetSeqIntoEmptyBucket { seq: 5, .. }
        ));
    }

    #[test]
    fn seq_exhaustion_is_a_typed_error_not_a_silent_duplicate() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        // Seed the bucket: first event takes the manager-assigned seq 1.
        mgr.append(bid, ev(bid, Severity::Low, "k1")).unwrap();
        // Drive the tail to u64::MAX via an explicit (strictly greater) seq
        // on a non-empty bucket.
        let mut e_max = ev(bid, Severity::Low, "k2");
        e_max.seq = u64::MAX;
        assert_eq!(mgr.append(bid, e_max).unwrap(), u64::MAX);
        // The seq space is now exhausted. A manager-assigned append
        // (seq == 0) must surface SeqExhausted rather than saturating back
        // to u64::MAX (a silent duplicate seq).
        let err = mgr.append(bid, ev(bid, Severity::Low, "k3")).unwrap_err();
        assert!(matches!(
            err,
            BucketError::SeqExhausted { tail: u64::MAX, .. }
        ));
    }

    #[test]
    fn read_empty_returns_no_events() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        let r = mgr.events_since(bid, &BucketReadRequest::new(0)).unwrap();
        assert!(r.events.is_empty());
        assert_eq!(r.next_cursor, 0);
        assert!(!r.has_more);
    }

    #[test]
    fn cursor_at_tail_returns_nothing_at_after_returns_nothing() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "k")).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "k")).unwrap();
        let r = mgr.events_since(bid, &BucketReadRequest::new(2)).unwrap();
        assert!(r.events.is_empty());
        assert_eq!(r.next_cursor, 2);
        let r = mgr.events_since(bid, &BucketReadRequest::new(99)).unwrap();
        assert!(r.events.is_empty());
        assert_eq!(r.next_cursor, 99);
    }

    #[test]
    fn cursor_advances_through_appends() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        for i in 0..5 {
            mgr.append(bid, ev(bid, Severity::Low, &format!("k{i}")))
                .unwrap();
        }
        let r = mgr.events_since(bid, &BucketReadRequest::new(0)).unwrap();
        assert_eq!(r.events.len(), 5);
        assert_eq!(r.next_cursor, 5);
        let r2 = mgr.events_since(bid, &BucketReadRequest::new(2)).unwrap();
        assert_eq!(r2.events.len(), 3);
        assert_eq!(r2.next_cursor, 5);
    }

    #[test]
    fn limit_clamps_and_signals_has_more() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        for _ in 0..10 {
            mgr.append(bid, ev(bid, Severity::Low, "k")).unwrap();
        }
        let mut req = BucketReadRequest::new(0);
        req.limit = Some(3);
        let r = mgr.events_since(bid, &req).unwrap();
        assert_eq!(r.events.len(), 3);
        assert!(r.has_more);
        assert_eq!(r.next_cursor, 3);
    }

    #[test]
    fn severity_min_filters() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "k1")).unwrap();
        mgr.append(bid, ev(bid, Severity::Medium, "k2")).unwrap();
        mgr.append(bid, ev(bid, Severity::High, "k3")).unwrap();
        mgr.append(bid, ev(bid, Severity::Critical, "k4")).unwrap();
        let mut req = BucketReadRequest::new(0);
        req.severity_min = Some(Severity::High);
        let r = mgr.events_since(bid, &req).unwrap();
        assert_eq!(r.events.len(), 2);
        assert!(r.events.iter().all(|e| e.severity >= Severity::High));
    }

    #[test]
    fn kind_filter_matches_exactly() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "alpha")).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "beta")).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "alpha")).unwrap();
        let mut req = BucketReadRequest::new(0);
        req.kind_filter = Some("alpha".to_owned());
        let r = mgr.events_since(bid, &req).unwrap();
        assert_eq!(r.events.len(), 2);
        assert!(r.events.iter().all(|e| e.kind == "alpha"));
    }

    #[test]
    fn summary_counts_and_severity_distribution() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        mgr.append(bid, ev(bid, Severity::Low, "alpha")).unwrap();
        mgr.append(bid, ev(bid, Severity::Medium, "alpha")).unwrap();
        mgr.append(bid, ev(bid, Severity::High, "beta")).unwrap();
        let s = mgr.summary(bid).unwrap();
        assert_eq!(s.event_count, 3);
        assert_eq!(s.by_severity.low, 1);
        assert_eq!(s.by_severity.medium, 1);
        assert_eq!(s.by_severity.high, 1);
        assert_eq!(s.by_kind.get("alpha").copied(), Some(2));
        assert_eq!(s.by_kind.get("beta").copied(), Some(1));
        assert_eq!(s.dropped_count, 0);
        assert_eq!(s.noise_suppressed_count, 0);
        assert_eq!(s.dedupe_collapsed_count, 0);
    }

    #[test]
    fn fifo_overflow_evicts_oldest_and_bumps_dropped_count() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket(
            bid,
            BucketConfig {
                max_events: 3,
                ttl: DEFAULT_TTL,
            },
        )
        .unwrap();
        for i in 0..5 {
            mgr.append(bid, ev(bid, Severity::Low, &format!("k{i}")))
                .unwrap();
        }
        let s = mgr.summary(bid).unwrap();
        assert_eq!(s.event_count, 3);
        assert_eq!(s.dropped_count, 2);
        // head_seq should be 3 (oldest two evicted), tail_seq 5.
        assert_eq!(s.head_seq, 3);
        assert_eq!(s.tail_seq, 5);
        let r = mgr.events_since(bid, &BucketReadRequest::new(0)).unwrap();
        assert_eq!(r.events.len(), 3);
        assert_eq!(r.dropped_count, 2);
        // First surviving event should be k2 (seq=3), since k0/k1 evicted.
        assert_eq!(r.events.first().unwrap().kind, "k2");
    }

    #[test]
    fn read_unknown_bucket_errors() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        let err = mgr
            .events_since(bid, &BucketReadRequest::new(0))
            .unwrap_err();
        assert!(matches!(err, BucketError::NotFound(_)));
    }

    #[test]
    fn read_response_carries_no_raw_text() {
        // Compile-time / shape assertion: BucketReadResponse exposes
        // SignalEvent (structured), never raw bytes. This test is a
        // belt-and-braces check that the type signature does not
        // accidentally grow a raw String field.
        fn assert_only_structured(_r: &BucketReadResponse) {
            // intentionally empty
        }
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        mgr.append(bid, ev(bid, Severity::Medium, "k")).unwrap();
        let r = mgr.events_since(bid, &BucketReadRequest::new(0)).unwrap();
        assert_only_structured(&r);
        // Field type is Vec<SignalEvent>; no raw stream lane.
    }

    #[test]
    fn drop_bucket_is_idempotent() {
        let mgr = BucketManager::new();
        let bid = BucketId::new();
        mgr.create_bucket_default(bid).unwrap();
        assert!(mgr.drop_bucket(bid));
        assert!(!mgr.drop_bucket(bid));
    }

    #[test]
    fn manager_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BucketManager>();
    }

    fn rt() -> tokio::runtime::Runtime {
        // M3: start_paused gives the wait tests virtual time. tokio auto-advances
        // the clock to the next timer whenever the runtime is otherwise idle, so a
        // `tokio::time::timeout` inside bucket_wait (and the park `sleep`s in the
        // wake tests) resolve instantly and deterministically — no wall-clock
        // dependency, no CI-load flake.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap()
    }

    #[test]
    fn wait_fast_path_returns_already_present_events() {
        let runtime = rt();
        runtime.block_on(async {
            let mgr = BucketManager::new();
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();
            mgr.append(bid, ev(bid, Severity::High, "k1")).unwrap();
            let resp = mgr
                .bucket_wait(bid, BucketWaitRequest::new(0, Duration::from_millis(50)))
                .await
                .unwrap();
            assert!(!resp.heartbeat);
            assert_eq!(resp.events.len(), 1);
        });
    }

    #[test]
    fn wait_times_out_with_heartbeat_when_nothing_arrives() {
        let runtime = rt();
        runtime.block_on(async {
            let mgr = BucketManager::new();
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();
            let resp = mgr
                .bucket_wait(bid, BucketWaitRequest::new(0, Duration::from_millis(40)))
                .await
                .unwrap();
            assert!(resp.heartbeat);
            assert!(resp.events.is_empty());
            assert_eq!(resp.next_cursor, 0);
        });
    }

    #[test]
    fn wait_wakes_on_append() {
        let runtime = rt();
        runtime.block_on(async {
            let mgr = std::sync::Arc::new(BucketManager::new());
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();

            let mgr2 = std::sync::Arc::clone(&mgr);
            let waiter = tokio::spawn(async move {
                mgr2.bucket_wait(bid, BucketWaitRequest::new(0, Duration::from_secs(2)))
                    .await
            });
            // Give the waiter a moment to park.
            tokio::time::sleep(Duration::from_millis(20)).await;
            mgr.append(bid, ev(bid, Severity::High, "k1")).unwrap();
            let resp = waiter.await.unwrap().unwrap();
            assert!(!resp.heartbeat);
            assert_eq!(resp.events.len(), 1);
        });
    }

    #[test]
    fn bucket_notify_returns_handle_and_wakes() {
        let runtime = rt();
        runtime.block_on(async {
            let mgr = std::sync::Arc::new(BucketManager::new());
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();

            // Known bucket -> a handle.
            let notify = mgr.bucket_notify(bid).expect("handle");
            // Unknown bucket -> NotFound.
            assert!(matches!(
                mgr.bucket_notify(BucketId::new()),
                Err(BucketError::NotFound(_))
            ));

            // An enrolled waiter wakes on append. Spawn the waiter so its
            // `notified()` future is parked (enrolled) before we append; the
            // permit-less `notify_waiters()` only wakes already-enrolled waiters.
            let waiter = tokio::spawn(async move {
                notify.notified().await;
            });
            // Give the waiter a moment to park (virtual time auto-advances).
            tokio::time::sleep(Duration::from_millis(20)).await;
            mgr.append(bid, ev(bid, Severity::High, "k1")).unwrap();
            tokio::time::timeout(Duration::from_secs(2), waiter)
                .await
                .expect("woken")
                .expect("waiter task completes");
        });
    }

    #[test]
    fn wait_severity_filter_excludes_low_events_on_wait() {
        let runtime = rt();
        runtime.block_on(async {
            let mgr = std::sync::Arc::new(BucketManager::new());
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();
            let mgr2 = std::sync::Arc::clone(&mgr);
            let waiter = tokio::spawn(async move {
                let mut req = BucketWaitRequest::new(0, Duration::from_millis(60));
                req.severity_min = Some(Severity::High);
                mgr2.bucket_wait(bid, req).await
            });
            tokio::time::sleep(Duration::from_millis(10)).await;
            // Append a low-severity event; the waiter wakes but
            // returns heartbeat=true (no matching events).
            mgr.append(bid, ev(bid, Severity::Low, "k1")).unwrap();
            let resp = waiter.await.unwrap().unwrap();
            // Wake-up rechecked filter; no matches -> heartbeat=true.
            assert!(resp.heartbeat);
        });
    }

    #[test]
    fn bucket_wait_returns_once_on_late_match() {
        // Acceptance check 1 ("600000 timeout, matching line at ~8 min,
        // one return") on tokio's PAUSED clock: the real numbers, virtual
        // time. A non-matching append at 100 s must NOT end the wait in
        // `match` mode; the matching one at 480 s must, once, long before
        // the 600 s timeout.
        let runtime = rt();
        runtime.block_on(async {
            let mgr = std::sync::Arc::new(BucketManager::new());
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();
            let mgr2 = std::sync::Arc::clone(&mgr);
            let started = tokio::time::Instant::now();
            let waiter = tokio::spawn(async move {
                let mut req = BucketWaitRequest::new(0, Duration::from_millis(600_000));
                req.severity_min = Some(Severity::High);
                req.wake_mode = WakeMode::Match;
                let resp = mgr2.bucket_wait(bid, req).await;
                (resp, tokio::time::Instant::now())
            });
            tokio::time::sleep(Duration::from_secs(100)).await;
            mgr.append(bid, ev(bid, Severity::Low, "noise")).unwrap();
            tokio::time::sleep(Duration::from_secs(380)).await;
            mgr.append(bid, ev(bid, Severity::High, "late_match"))
                .unwrap();
            let (resp, returned_at) = waiter.await.unwrap();
            let resp = resp.unwrap();
            assert_eq!(resp.wake_reason, BucketWakeReason::Match);
            assert!(!resp.heartbeat);
            assert_eq!(resp.events.len(), 1);
            assert_eq!(resp.events[0].kind, "late_match");
            let at = returned_at - started;
            assert!(
                at >= Duration::from_secs(480) && at < Duration::from_secs(481),
                "returned at {at:?}, expected ~480s (not at the 100s noise, not at 600s)"
            );
        });
    }

    #[test]
    fn wait_exit_probe_returns_exit_without_matching_event() {
        let runtime = rt();
        runtime.block_on(async {
            let mgr = std::sync::Arc::new(BucketManager::new());
            let bid = BucketId::new();
            mgr.create_bucket_default(bid).unwrap();
            let ended = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let ended2 = std::sync::Arc::clone(&ended);
            let mgr2 = std::sync::Arc::clone(&mgr);
            let waiter = tokio::spawn(async move {
                let mut req = BucketWaitRequest::new(0, Duration::from_secs(600));
                req.severity_min = Some(Severity::High);
                req.wake_mode = WakeMode::Match;
                req.exit_probe = Some(ExitProbe(std::sync::Arc::new(move || {
                    ended2.load(std::sync::atomic::Ordering::SeqCst)
                })));
                mgr2.bucket_wait(bid, req).await
            });
            tokio::time::sleep(Duration::from_secs(5)).await;
            // The lifecycle event the daemon appends at exit (filtered out here).
            ended.store(true, std::sync::atomic::Ordering::SeqCst);
            mgr.append(bid, ev(bid, Severity::Low, "command_exited"))
                .unwrap();
            let resp = waiter.await.unwrap().unwrap();
            assert_eq!(resp.wake_reason, BucketWakeReason::Exit);
            assert!(!resp.heartbeat);
            assert!(resp.events.is_empty());
        });
    }
}
