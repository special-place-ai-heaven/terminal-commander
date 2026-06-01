// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Render helpers for the daemon-backed read subcommands (P3).
//!
//! Each function takes a typed `terminal_commander_ipc` response and prints a
//! small operator-facing table or summary to stdout. Rendering is pure over
//! its input: it NEVER reaches the daemon and NEVER fabricates rows. An
//! honestly-empty response (e.g. zero active rules) prints just the header row;
//! the caller still exits 0 because an empty real result is a valid answer.
//!
//! Platform-agnostic: these functions name no transport detail.

// The module is `pub(crate)`; the explicit visibility on its items documents
// the crate-internal contract and matches `ipc.rs` / `update_locks.rs`.
#![allow(clippy::redundant_pub_crate)]

use terminal_commander_ipc::{
    AuditSinceResponse, BucketSummaryResponse, PolicyStatusResponse, ProbeListResponse,
    RegistryGetResponse, RegistryListActiveResponse, RuntimeStateResponse,
};

/// `rules list`: one row per active `(rule_id, version, severity, event_kind,
/// scope)`. Zero entries prints just the header (honestly-empty -> exit 0).
pub(crate) fn rules_list(resp: &RegistryListActiveResponse) {
    println!(
        "{:<28} {:>7} {:<9} {:<22} SCOPE",
        "RULE_ID", "VERSION", "SEVERITY", "EVENT_KIND"
    );
    for e in &resp.entries {
        println!(
            "{:<28} {:>7} {:<9} {:<22} {}",
            e.rule_id,
            e.version,
            e.severity.as_str(),
            e.event_kind,
            scope_label(&e.scope),
        );
    }
}

/// `rules show <id>`: pretty-print one rule definition's salient fields. The
/// daemon already validated the definition; we render its fields verbatim.
pub(crate) fn rule_show(resp: &RegistryGetResponse) {
    let d = &resp.definition;
    println!("rule {}:", d.id);
    println!("  version       : {}", d.version);
    println!("  status        : {:?}", d.status);
    println!("  kind          : {:?}", d.kind);
    println!("  severity      : {}", d.severity.as_str());
    println!("  event_kind    : {}", d.event_kind);
    if let Some(stream) = &d.stream {
        println!("  stream        : {stream:?}");
    }
    if let Some(desc) = &d.description {
        println!("  description   : {desc}");
    }
    if let Some(pattern) = &d.pattern {
        println!("  pattern       : {pattern}");
    }
    if let Some(keywords) = &d.keywords {
        println!("  keywords      : {}", keywords.join(", "));
    }
    if !d.captures.is_empty() {
        println!("  captures      : {}", d.captures.join(", "));
    }
    println!("  summary       : {}", d.summary_template);
    if !d.tags.is_empty() {
        println!("  tags          : {}", d.tags.join(", "));
    }
}

/// `jobs`: aggregate runtime counts plus a probe table and a bucket table.
pub(crate) fn jobs(resp: &RuntimeStateResponse) {
    println!("runtime jobs:");
    println!("  command_jobs       : {}", resp.command_jobs);
    println!("  pty_jobs           : {}", resp.pty_jobs);
    println!("  file_watches       : {}", resp.file_watches);
    println!("  bucket_count       : {}", resp.bucket_count);
    println!("  active_rules_count : {}", resp.active_rules_count);
    println!();
    probe_rows(&resp.probes);
    println!();
    bucket_rows(&resp.buckets);
}

/// `probes`: one row per live probe across every runtime.
pub(crate) fn probes(resp: &ProbeListResponse) {
    probe_rows(&resp.probes);
}

/// `policy`: active profile, deny counts, and the per-call caps.
pub(crate) fn policy(resp: &PolicyStatusResponse) {
    println!("policy status:");
    println!("  profile                        : {}", resp.profile);
    println!(
        "  commands_deny_count            : {}",
        resp.commands_deny_count
    );
    println!(
        "  default_deny_path_suffix_count : {}",
        resp.default_deny_path_suffix_count
    );
    println!(
        "  file_window_bytes              : {}",
        resp.file_window_bytes
    );
    println!(
        "  bucket_read_limit              : {}",
        resp.bucket_read_limit
    );
}

/// `buckets show <id>`: counters plus the per-severity histogram.
pub(crate) fn bucket_summary(resp: &BucketSummaryResponse) {
    println!("bucket {}:", resp.bucket_id);
    println!("  head_seq      : {}", resp.head_seq);
    println!("  tail_seq      : {}", resp.tail_seq);
    println!("  event_count   : {}", resp.event_count);
    println!("  dropped_count : {}", resp.dropped_count);
    let h = &resp.by_severity;
    println!("  by_severity   :");
    println!("    trace    : {}", h.trace);
    println!("    debug    : {}", h.debug);
    println!("    info     : {}", h.info);
    println!("    low      : {}", h.low);
    println!("    medium   : {}", h.medium);
    println!("    high     : {}", h.high);
    println!("    critical : {}", h.critical);
}

/// `buckets list`: render the bucket counters from a `runtime_state` snapshot.
///
/// There is NO daemon `list-buckets` method; `runtime_state` is the only
/// source of the live bucket set, so `buckets list` reuses it and renders the
/// `RuntimeStateResponse.buckets` rows. This intentionally shares the table
/// shape with the `jobs` bucket section.
pub(crate) fn buckets_list(resp: &RuntimeStateResponse) {
    bucket_rows(&resp.buckets);
}

/// `audit`: one row per audit record. Zero rows prints just the header
/// (honestly-empty -> exit 0). Columns: audit_id, timestamp, action,
/// subject, decision, profile. `profile` renders `-` when absent.
pub(crate) fn audit(resp: &AuditSinceResponse) {
    println!(
        "{:>8} {:<25} {:<28} {:<24} {:<10} PROFILE",
        "AUDIT_ID", "TIMESTAMP", "ACTION", "SUBJECT", "DECISION"
    );
    for r in &resp.rows {
        println!(
            "{:>8} {:<25} {:<28} {:<24} {:<10} {}",
            r.audit_id,
            r.timestamp,
            r.action,
            r.subject,
            r.decision,
            r.profile.as_deref().unwrap_or("-"),
        );
    }
}

/// Shared probe table. Empty list prints just the header (honestly-empty).
fn probe_rows(probes: &[terminal_commander_ipc::ProbeListEntry]) {
    println!(
        "{:<10} {:<36} {:<36} {:>8} {:>8}",
        "KIND", "JOB_ID", "BUCKET_ID", "FRAMES", "EVENTS"
    );
    for p in probes {
        println!(
            "{:<10} {:<36} {:<36} {:>8} {:>8}",
            probe_kind_label(p.kind),
            p.job_id,
            p.bucket_id,
            p.frames_total,
            p.events_emitted,
        );
    }
}

/// Shared bucket table. Empty list prints just the header (honestly-empty).
fn bucket_rows(buckets: &[terminal_commander_ipc::RuntimeBucketSummary]) {
    println!(
        "{:<36} {:>8} {:>8} {:>8} {:>8}",
        "BUCKET_ID", "HEAD", "TAIL", "EVENTS", "DROPPED"
    );
    for b in buckets {
        println!(
            "{:<36} {:>8} {:>8} {:>8} {:>8}",
            b.bucket_id, b.head_seq, b.tail_seq, b.event_count, b.dropped_count,
        );
    }
}

/// Stable short label for a probe kind, matching the wire snake_case tags.
const fn probe_kind_label(kind: terminal_commander_ipc::ProbeKind) -> &'static str {
    match kind {
        terminal_commander_ipc::ProbeKind::Command => "command",
        terminal_commander_ipc::ProbeKind::FileWatch => "file_watch",
        terminal_commander_ipc::ProbeKind::Pty => "pty",
    }
}

/// Compact one-token label for an activation scope, with the bound id when the
/// scope is non-global.
fn scope_label(scope: &terminal_commander_core::ActivationScope) -> String {
    match scope {
        terminal_commander_core::ActivationScope::Global => "global".to_string(),
        terminal_commander_core::ActivationScope::Bucket { bucket_id } => {
            format!("bucket:{bucket_id}")
        }
        terminal_commander_core::ActivationScope::Job { job_id } => format!("job:{job_id}"),
        terminal_commander_core::ActivationScope::Probe { probe_id } => format!("probe:{probe_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{ActivationScope, BucketId, JobId, ProbeId};
    use terminal_commander_ipc::ProbeKind;

    #[test]
    fn probe_kind_labels_match_wire_tags() {
        assert_eq!(probe_kind_label(ProbeKind::Command), "command");
        assert_eq!(probe_kind_label(ProbeKind::FileWatch), "file_watch");
        assert_eq!(probe_kind_label(ProbeKind::Pty), "pty");
    }

    #[test]
    fn scope_label_is_global_or_typed() {
        assert_eq!(scope_label(&ActivationScope::Global), "global");
        let bucket_id = BucketId::new();
        assert_eq!(
            scope_label(&ActivationScope::Bucket { bucket_id }),
            format!("bucket:{bucket_id}")
        );
        let job_id = JobId::new();
        assert_eq!(
            scope_label(&ActivationScope::Job { job_id }),
            format!("job:{job_id}")
        );
        let probe_id = ProbeId::new();
        assert_eq!(
            scope_label(&ActivationScope::Probe { probe_id }),
            format!("probe:{probe_id}")
        );
    }
}
