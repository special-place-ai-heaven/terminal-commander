// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Activation scope DTO (TC42c).
//!
//! [`ActivationScope`] is the wire-stable description of WHICH live
//! stream(s) a registry activation applies to. Every IPC + MCP
//! `registry_activate` / `registry_deactivate` / `registry_list_active`
//! call carries this value (defaulting to [`ActivationScope::Global`]
//! when omitted, preserving TC42/TC42b wire compatibility).
//!
//! Resolution semantics:
//!
//! - [`Global`](ActivationScope::Global) — applies to every job,
//!   matching TC42b behavior.
//! - [`Bucket`](ActivationScope::Bucket) — applies only to jobs whose
//!   `bucket_id` matches.
//! - [`Job`](ActivationScope::Job) — applies only to the job whose
//!   `job_id` matches.
//! - [`Probe`](ActivationScope::Probe) — applies only to the job whose
//!   `probe_id` matches.
//!
//! The daemon resolves the scope to the affected live job(s) at
//! activation time AND at every rebind. A scope referencing a
//! `bucket_id` / `job_id` / `probe_id` that the daemon does not know
//! at the call site is rejected with a typed error; it never
//! silently degrades to [`Global`](ActivationScope::Global).
//!
//! Source-status: live (TC42c) — DTO only, no I/O.

use serde::{Deserialize, Serialize};

use crate::ids::{BucketId, JobId, ProbeId};

/// Closed set of activation targets.
///
/// Serialized as a snake_case externally-tagged enum so the JSON
/// wire form is `{"kind":"global"}` / `{"kind":"bucket","bucket_id":"..."}` etc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivationScope {
    /// Applies to every running and future job.
    #[default]
    Global,
    /// Applies only to jobs whose bucket id matches.
    Bucket {
        #[serde(rename = "bucket_id")]
        bucket_id: BucketId,
    },
    /// Applies only to jobs whose job id matches.
    Job {
        #[serde(rename = "job_id")]
        job_id: JobId,
    },
    /// Applies only to jobs whose probe id matches.
    Probe {
        #[serde(rename = "probe_id")]
        probe_id: ProbeId,
    },
}

impl ActivationScope {
    /// Stable short label used in audit metadata and persistent rows.
    /// Matches the serde tag values.
    #[must_use]
    pub const fn kind_label(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Bucket { .. } => "bucket",
            Self::Job { .. } => "job",
            Self::Probe { .. } => "probe",
        }
    }

    /// Wire-form scope value: the typed id's wire string for non-global
    /// variants, or `None` for [`Global`](Self::Global). Used for the
    /// persistent activation row's `scope_value` column.
    #[must_use]
    pub fn value_wire(&self) -> Option<String> {
        match self {
            Self::Global => None,
            Self::Bucket { bucket_id } => Some(bucket_id.to_wire_string()),
            Self::Job { job_id } => Some(job_id.to_wire_string()),
            Self::Probe { probe_id } => Some(probe_id.to_wire_string()),
        }
    }

    /// Whether this scope matches a live job tagged with the given
    /// `(bucket_id, job_id, probe_id)` triple. `Global` matches every
    /// job.
    #[must_use]
    pub fn matches(&self, bucket_id: BucketId, job_id: JobId, probe_id: ProbeId) -> bool {
        match self {
            Self::Global => true,
            Self::Bucket { bucket_id: want } => *want == bucket_id,
            Self::Job { job_id: want } => *want == job_id,
            Self::Probe { probe_id: want } => *want == probe_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_is_default() {
        assert!(matches!(
            ActivationScope::default(),
            ActivationScope::Global
        ));
    }

    #[test]
    fn kind_label_is_stable() {
        assert_eq!(ActivationScope::Global.kind_label(), "global");
        assert_eq!(
            ActivationScope::Bucket {
                bucket_id: BucketId::new()
            }
            .kind_label(),
            "bucket"
        );
        assert_eq!(
            ActivationScope::Job {
                job_id: JobId::new()
            }
            .kind_label(),
            "job"
        );
        assert_eq!(
            ActivationScope::Probe {
                probe_id: ProbeId::new()
            }
            .kind_label(),
            "probe"
        );
    }

    #[test]
    fn global_matches_any_triple() {
        let s = ActivationScope::Global;
        assert!(s.matches(BucketId::new(), JobId::new(), ProbeId::new()));
    }

    #[test]
    fn bucket_scope_matches_only_same_bucket() {
        let bid = BucketId::new();
        let s = ActivationScope::Bucket { bucket_id: bid };
        assert!(s.matches(bid, JobId::new(), ProbeId::new()));
        assert!(!s.matches(BucketId::new(), JobId::new(), ProbeId::new()));
    }

    #[test]
    fn job_scope_matches_only_same_job() {
        let jid = JobId::new();
        let s = ActivationScope::Job { job_id: jid };
        assert!(s.matches(BucketId::new(), jid, ProbeId::new()));
        assert!(!s.matches(BucketId::new(), JobId::new(), ProbeId::new()));
    }

    #[test]
    fn probe_scope_matches_only_same_probe() {
        let pid = ProbeId::new();
        let s = ActivationScope::Probe { probe_id: pid };
        assert!(s.matches(BucketId::new(), JobId::new(), pid));
        assert!(!s.matches(BucketId::new(), JobId::new(), ProbeId::new()));
    }

    #[test]
    fn serde_round_trip_global() {
        let s = ActivationScope::Global;
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains(r#""kind":"global""#));
        let back: ActivationScope = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn serde_round_trip_bucket() {
        let s = ActivationScope::Bucket {
            bucket_id: BucketId::new(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains(r#""kind":"bucket""#));
        assert!(j.contains(r#""bucket_id""#));
        let back: ActivationScope = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn value_wire_is_some_for_typed_scopes() {
        assert!(ActivationScope::Global.value_wire().is_none());
        let bid = BucketId::new();
        assert_eq!(
            ActivationScope::Bucket { bucket_id: bid }
                .value_wire()
                .unwrap(),
            bid.to_wire_string()
        );
    }
}
