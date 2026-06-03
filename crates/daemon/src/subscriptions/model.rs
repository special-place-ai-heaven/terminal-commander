// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Subscription predicate grammar + per-open subscription state.
//!
//! Two filter planes (AND semantics; a match = an event whose source bucket is
//! in-scope AND the event satisfies the field filters):
//!
//! - **per-BUCKET routing** ([`SourceSel`]) — resolved against the
//!   [`crate::subscriptions::source::BucketSourceTable`]: `all`, or a fixed set
//!   of jobs / buckets / probes.
//! - **per-EVENT filters** (`severity_min`, `kind`) — applied by
//!   `BucketManager::events_since` via `BucketReadRequest`, NOT re-filtered by
//!   hand here.
//!
//! `sub_id` is OPAQUE per-open (a fresh `Uuid` every `subscription_open`), NOT
//! the predicate hash: two callers with the same predicate get DISTINCT handles
//! and INDEPENDENT offsets (consumer isolation, spec C1/AC8). `predicate_hash`
//! is a content hash of the NORMALIZED predicate, surfaced so an agent can
//! recognize an equivalent predicate and used internally only to share the
//! routing-EVALUATION across subs — never to share offsets.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use terminal_commander_core::{BucketId, JobId, ProbeId, Severity};

use super::source::BucketSource;

/// Per-bucket routing selector. `All` auto-includes future matching buckets
/// (re-evaluated each pull against the side-table); the fixed variants are a
/// closed set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceSel {
    /// Every bucket. Future buckets auto-join on the next routing rebuild.
    All,
    /// A fixed set of owning jobs.
    Jobs(Vec<JobId>),
    /// A fixed set of bucket ids.
    Buckets(Vec<BucketId>),
    /// A fixed set of owning probes.
    Probes(Vec<ProbeId>),
}

impl SourceSel {
    /// Fold the selector into a stable hasher. Vectors are sorted first so a
    /// reordered selector hashes identically.
    fn hash_normalized<H: Hasher>(&self, h: &mut H) {
        match self {
            Self::All => 0u8.hash(h),
            Self::Jobs(v) => {
                1u8.hash(h);
                hash_sorted_ids(v, h);
            }
            Self::Buckets(v) => {
                2u8.hash(h);
                hash_sorted_ids(v, h);
            }
            Self::Probes(v) => {
                3u8.hash(h);
                hash_sorted_ids(v, h);
            }
        }
    }
}

/// Hash a typed-id vector in a stable, order-independent way: stringify,
/// sort, then fold the length + each element into the hasher (an explicit
/// element-by-element read so the normalization is unambiguous).
fn hash_sorted_ids<T: ToString, H: Hasher>(v: &[T], h: &mut H) {
    let mut ids: Vec<String> = v.iter().map(ToString::to_string).collect();
    ids.sort_unstable();
    ids.len().hash(h);
    for id in &ids {
        id.hash(h);
    }
}

/// A subscription predicate. All fields optional; AND semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    /// Minimum severity (per-EVENT; honored by `events_since`).
    pub severity_min: Option<Severity>,
    /// Event-kind allowlist (per-EVENT). `None` = any kind.
    pub kind: Option<Vec<String>>,
    /// Per-BUCKET routing selector.
    pub sources: SourceSel,
}

impl Predicate {
    /// Stable content hash of the NORMALIZED predicate: vectors are sorted
    /// before hashing, so two predicates that differ only in vector ordering
    /// hash equal. Used to share routing EVALUATION across subscriptions
    /// (never to share offsets).
    #[must_use]
    pub fn normalized_hash(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // severity_min
        match self.severity_min {
            Some(s) => {
                1u8.hash(&mut h);
                (s as u8).hash(&mut h);
            }
            None => 0u8.hash(&mut h),
        }
        // kind allowlist (sorted + deduped for a stable normal form)
        match &self.kind {
            Some(kinds) => {
                1u8.hash(&mut h);
                let mut norm = kinds.clone();
                norm.sort_unstable();
                norm.dedup();
                norm.len().hash(&mut h);
                for k in &norm {
                    k.hash(&mut h);
                }
            }
            None => 0u8.hash(&mut h),
        }
        // sources
        self.sources.hash_normalized(&mut h);
        h.finish()
    }

    /// Whether a bucket is in routing scope for this predicate, given its
    /// recorded source identity.
    ///
    /// - `All` -> always true.
    /// - `Buckets` -> the bucket id is in the set.
    /// - `Jobs` -> the bucket's `job_id` is in the set.
    /// - `Probes` -> the bucket's `probe_id` is in the set.
    ///
    /// A fixed selector whose required identity field is absent on the source
    /// (e.g. `Jobs` but the bucket recorded no `job_id`) is OUT of scope.
    #[must_use]
    pub fn bucket_in_scope(&self, id: BucketId, src: &BucketSource) -> bool {
        match &self.sources {
            SourceSel::All => true,
            SourceSel::Buckets(ids) => ids.contains(&id),
            SourceSel::Jobs(jobs) => src.job_id.is_some_and(|j| jobs.contains(&j)),
            SourceSel::Probes(probes) => src.probe_id.is_some_and(|p| probes.contains(&p)),
        }
    }
}

/// One open subscription's server-side state. Keyed in the registry by the
/// opaque `sub_id`.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// OPAQUE per-open handle (fresh `Uuid` each open; NOT the predicate hash).
    pub sub_id: uuid::Uuid,
    /// The routing + filter predicate.
    pub predicate: Predicate,
    /// Normalized predicate hash (routing-evaluation sharing only).
    pub predicate_hash: u64,
    /// Server-advanced per-bucket offsets (this consumer's cursors).
    pub offsets: HashMap<BucketId, u64>,
    /// When this subscription was opened.
    pub created_at: Instant,
    /// When this subscription was last pulled (None until the first pull).
    pub last_pull_at: Option<Instant>,
    /// Round-robin rotation cursor for fair draining across in-scope buckets.
    pub rr_start: usize,
}

impl Subscription {
    /// Construct a fresh subscription with a minted opaque `sub_id` and the
    /// given initial offsets (from-now tails for already-in-scope buckets).
    #[must_use]
    pub fn new(predicate: Predicate, offsets: HashMap<BucketId, u64>) -> Self {
        let predicate_hash = predicate.normalized_hash();
        Self {
            sub_id: uuid::Uuid::new_v4(),
            predicate,
            predicate_hash,
            offsets,
            created_at: Instant::now(),
            last_pull_at: None,
            rr_start: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_ipc::ProbeKind;

    fn src(kind: ProbeKind, job: Option<JobId>, probe: Option<ProbeId>) -> BucketSource {
        BucketSource {
            kind,
            job_id: job,
            probe_id: probe,
            path: None,
        }
    }

    #[test]
    fn reordered_vec_predicates_hash_equal() {
        let j1 = JobId::new();
        let j2 = JobId::new();
        let a = Predicate {
            severity_min: Some(Severity::High),
            kind: Some(vec!["error".to_owned(), "panic".to_owned()]),
            sources: SourceSel::Jobs(vec![j1, j2]),
        };
        let b = Predicate {
            severity_min: Some(Severity::High),
            kind: Some(vec!["panic".to_owned(), "error".to_owned()]),
            sources: SourceSel::Jobs(vec![j2, j1]),
        };
        assert_eq!(
            a.normalized_hash(),
            b.normalized_hash(),
            "reordered vecs normalize to the same hash"
        );
    }

    #[test]
    fn distinct_predicates_hash_differently() {
        let base = Predicate {
            severity_min: Some(Severity::High),
            kind: None,
            sources: SourceSel::All,
        };
        let diff_sev = Predicate {
            severity_min: Some(Severity::Critical),
            kind: None,
            sources: SourceSel::All,
        };
        let diff_kind = Predicate {
            severity_min: Some(Severity::High),
            kind: Some(vec!["error".to_owned()]),
            sources: SourceSel::All,
        };
        let diff_src = Predicate {
            severity_min: Some(Severity::High),
            kind: None,
            sources: SourceSel::Buckets(vec![BucketId::new()]),
        };
        assert_ne!(base.normalized_hash(), diff_sev.normalized_hash());
        assert_ne!(base.normalized_hash(), diff_kind.normalized_hash());
        assert_ne!(base.normalized_hash(), diff_src.normalized_hash());
    }

    #[test]
    fn bucket_in_scope_all_is_always_true() {
        let p = Predicate {
            severity_min: None,
            kind: None,
            sources: SourceSel::All,
        };
        let id = BucketId::new();
        assert!(p.bucket_in_scope(id, &src(ProbeKind::Command, None, None)));
    }

    #[test]
    fn bucket_in_scope_buckets_matches_id_only() {
        let target = BucketId::new();
        let other = BucketId::new();
        let p = Predicate {
            severity_min: None,
            kind: None,
            sources: SourceSel::Buckets(vec![target]),
        };
        let s = src(ProbeKind::Command, Some(JobId::new()), Some(ProbeId::new()));
        assert!(p.bucket_in_scope(target, &s));
        assert!(!p.bucket_in_scope(other, &s));
    }

    #[test]
    fn bucket_in_scope_jobs_matches_source_job() {
        let job = JobId::new();
        let p = Predicate {
            severity_min: None,
            kind: None,
            sources: SourceSel::Jobs(vec![job]),
        };
        let id = BucketId::new();
        assert!(p.bucket_in_scope(id, &src(ProbeKind::Command, Some(job), None)));
        assert!(!p.bucket_in_scope(id, &src(ProbeKind::Command, Some(JobId::new()), None)));
        // Source with no job_id is out of scope for a Jobs selector.
        assert!(!p.bucket_in_scope(id, &src(ProbeKind::Command, None, None)));
    }

    #[test]
    fn bucket_in_scope_probes_matches_source_probe() {
        let probe = ProbeId::new();
        let p = Predicate {
            severity_min: None,
            kind: None,
            sources: SourceSel::Probes(vec![probe]),
        };
        let id = BucketId::new();
        assert!(p.bucket_in_scope(id, &src(ProbeKind::FileWatch, None, Some(probe))));
        assert!(!p.bucket_in_scope(id, &src(ProbeKind::FileWatch, None, Some(ProbeId::new()))));
        assert!(!p.bucket_in_scope(id, &src(ProbeKind::FileWatch, None, None)));
    }

    #[test]
    fn distinct_opens_get_distinct_sub_ids() {
        let p = Predicate {
            severity_min: None,
            kind: None,
            sources: SourceSel::All,
        };
        let a = Subscription::new(p.clone(), HashMap::new());
        let b = Subscription::new(p, HashMap::new());
        assert_ne!(a.sub_id, b.sub_id, "each open mints a fresh opaque sub_id");
        assert_eq!(
            a.predicate_hash, b.predicate_hash,
            "identical predicates share a predicate_hash"
        );
    }
}
