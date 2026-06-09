// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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

/// `policy`: active profile, deny counts, the per-call caps, and the resolved
/// capability set (POLICY.md section 4.1).
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
    // Resolved per-call capabilities (the values the engine evaluates against,
    // including any preset ON by `full_access`).
    println!(
        "  caps.allow_shell               : {}",
        resp.caps.allow_shell
    );
    println!(
        "  caps.allow_session             : {}",
        resp.caps.allow_session
    );
    println!(
        "  caps.allow_privileged          : {}",
        resp.caps.allow_privileged
    );
    println!(
        "  caps.allow_remote              : {}",
        resp.caps.allow_remote
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

/// Builds the full probe table (header + rows) as a String. Pure over its
/// input: an empty list yields just the header (honestly-empty). Appends the
/// TC-4 `TAG` and `ARGV_HEAD` columns after the existing columns; `argv_head`
/// is the already-redacted projection joined on spaces and truncated to a
/// readable width. A `None` tag/argv_head degrades to a blank cell.
fn probe_table(probes: &[terminal_commander_ipc::ProbeListEntry]) -> String {
    use std::fmt::Write as _;

    /// Max rendered width of the `ARGV_HEAD` cell (the source value is already
    /// bounded + redacted; this only keeps the operator table readable).
    const ARGV_HEAD_CELL_WIDTH: usize = 48;
    /// Max rendered width of the operator-supplied `TAG` cell. Clamped so a
    /// long tag cannot push the `ARGV_HEAD` column out of alignment.
    const TAG_CELL_WIDTH: usize = 12;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<10} {:<36} {:<36} {:>8} {:>8} {:<12} {:<48}",
        "KIND", "JOB_ID", "BUCKET_ID", "FRAMES", "EVENTS", "TAG", "ARGV_HEAD"
    );
    for p in probes {
        let tag = truncate_chars(p.tag.as_deref().unwrap_or(""), TAG_CELL_WIDTH);
        let argv_head = p
            .argv_head
            .as_deref()
            .map(|h| truncate_chars(&h.join(" "), ARGV_HEAD_CELL_WIDTH))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{:<10} {:<36} {:<36} {:>8} {:>8} {:<12} {:<48}",
            probe_kind_label(p.kind),
            p.job_id,
            p.bucket_id,
            p.frames_total,
            p.events_emitted,
            tag,
            argv_head,
        );
    }
    out
}

/// Truncate a string to at most `max` chars on a char boundary, appending an
/// ellipsis marker when content was dropped. Never splits a multi-byte char.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    // Reserve one char for the ellipsis when truncating.
    let keep = max.saturating_sub(1);
    let mut t: String = s.chars().take(keep).collect();
    t.push('~');
    t
}

/// Shared probe table. Empty list prints just the header (honestly-empty).
fn probe_rows(probes: &[terminal_commander_ipc::ProbeListEntry]) {
    print!("{}", probe_table(probes));
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

    fn sample_probe(
        tag: Option<&str>,
        argv_head: Option<Vec<&str>>,
    ) -> terminal_commander_ipc::ProbeListEntry {
        terminal_commander_ipc::ProbeListEntry {
            kind: ProbeKind::Command,
            job_id: JobId::new(),
            bucket_id: BucketId::new(),
            probe_id: ProbeId::new(),
            frames_total: 0,
            events_emitted: 0,
            frames_suppressed: 0,
            frames_suppressed_progress: 0,
            frames_suppressed_dedupe: 0,
            secret_prompts_total: 0,
            secret_prompt_active: false,
            path: None,
            liveness: terminal_commander_ipc::Liveness::default(),
            tag: tag.map(str::to_string),
            argv_head: argv_head.map(|h| h.into_iter().map(str::to_string).collect()),
        }
    }

    #[test]
    fn probe_table_renders_tag_and_argv_head_columns() {
        let probe = sample_probe(Some("build"), Some(vec!["cargo", "build", "--release"]));
        let table = probe_table(std::slice::from_ref(&probe));
        assert!(table.contains("TAG"), "header missing TAG: {table}");
        assert!(
            table.contains("ARGV_HEAD"),
            "header missing ARGV_HEAD: {table}"
        );
        assert!(table.contains("build"), "row missing tag: {table}");
        assert!(
            table.contains("cargo build --release"),
            "row missing argv_head: {table}"
        );
    }

    #[test]
    fn probe_table_renders_blank_cells_when_tag_and_argv_head_absent() {
        let probe = sample_probe(None, None);
        let table = probe_table(std::slice::from_ref(&probe));
        // Headers always present, even with empty optional cells.
        assert!(table.contains("TAG"), "header missing TAG: {table}");
        assert!(
            table.contains("ARGV_HEAD"),
            "header missing ARGV_HEAD: {table}"
        );
        // The data row renders without panicking and carries the kind label.
        assert!(table.contains("command"), "row missing kind: {table}");
    }

    #[test]
    fn probe_table_empty_list_is_header_only() {
        let table = probe_table(&[]);
        let line_count = table.lines().count();
        assert_eq!(
            line_count, 1,
            "empty list must print just the header: {table}"
        );
        assert!(
            table.contains("KIND") && table.contains("ARGV_HEAD"),
            "{table}"
        );
    }

    #[test]
    fn truncate_chars_marks_truncation_and_preserves_short_input() {
        assert_eq!(truncate_chars("short", 48), "short");
        let long = "x".repeat(60);
        let out = truncate_chars(&long, 48);
        assert_eq!(out.chars().count(), 48);
        assert!(out.ends_with('~'), "{out}");
    }

    #[test]
    fn truncate_chars_never_splits_a_multibyte_char() {
        // A mix of multi-byte chars (each > 1 byte) truncated at a boundary that,
        // under naive byte slicing, would land mid-codepoint. Char-based truncation
        // must keep the result valid UTF-8 and within the char budget.
        let s = "héllo-wörld-🦀-grüße";
        let out = truncate_chars(s, 8);
        assert!(out.chars().count() <= 8, "char budget exceeded: {out:?}");
        assert!(out.ends_with('~'), "truncation marker expected: {out:?}");
        // Round-tripping through str/String proves it is valid UTF-8 (no panic, no
        // replacement char from a split codepoint).
        assert_eq!(out, String::from_utf8(out.clone().into_bytes()).unwrap());
        // A multibyte string already within budget is returned unchanged.
        assert_eq!(truncate_chars("café", 8), "café");
    }
}
