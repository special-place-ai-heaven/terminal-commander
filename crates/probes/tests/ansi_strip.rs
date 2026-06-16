// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC-B1 (omni spec 001 FR-026): ANSI/CSI/OSC stripping on the non-PTY
//! process path.
//!
//! Field-ledger TC-B1 repro: a launcher prints color-coded lines like
//! `\x1b[0;32m[AAP]\x1b[0m MySQL is ready`. An agent's `^\[AAP\]`-anchored
//! rule silently never matched (the line started with the SGR escape), and
//! summaries carried raw escape bytes into the LLM context.
//!
//! These tests prove the fix end-to-end against the live process probe:
//!
//! 1. A `^`-anchored rule MATCHES colored output once the escapes are
//!    stripped before the sifter sees the line.
//! 2. The emitted summary contains NO escape bytes (`0x1b`).
//! 3. The RAW bytes (escapes intact) are still retrievable from the frame
//!    store (`ContextRingManager`) -- stripping is for matching + summary
//!    only.
//! 4. UTF-8 multibyte text survives stripping uncorrupted.
//! 5. `strip_ansi: false` opts out: the raw escapes reach the sifter, so an
//!    anchored rule does NOT match the colored line.
//!
//! Source-status: live (TC-B1). Runs the real `ProcessProbe` over `python3`
//! (a dev prereq used elsewhere in this crate's tests).

use std::sync::Arc;

use terminal_commander_core::{
    BucketId, ContextHint, ContextRingManager, ProbeId, RuleDefinition, RuleStatus, RuleType,
    Severity,
};
use terminal_commander_probes::{
    EventSink, InMemorySink, ProcessProbe, ProcessProbeConfig, strip_ansi,
};
use terminal_commander_sifters::SifterRuntime;

/// A regex rule whose pattern is `^`-anchored to the bracketed `[AAP]` tag.
/// This is the field-ledger failure mode: an anchored rule that color codes
/// silently defeat. The summary echoes the whole matched line via `${match}`
/// (the canonical full-match key, TC-E4).
fn anchored_aap_rule() -> RuleDefinition {
    RuleDefinition {
        id: "test.aap.anchored".to_owned(),
        version: 1,
        kind: RuleType::Regex,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "aap_line".to_owned(),
        stream: None,
        description: None,
        pattern: Some(r"^\[AAP\] (?P<msg>.+)$".to_owned()),
        keywords: None,
        captures: vec!["msg".to_owned()],
        summary_template: "${match}".to_owned(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// Build an argv that emits a single SGR-colored line on stdout via Python.
/// `\x1b[0;32m[AAP]\x1b[0m <msg>` -- green `[AAP]` tag, reset, then the msg.
fn argv_colored_aap_line(msg: &str) -> Vec<String> {
    let safe = msg.replace('\'', "\\'");
    // \x1b[0;32m  green   [AAP]   \x1b[0m reset   ' ' + msg
    let script = format!("print('\\x1b[0;32m[AAP]\\x1b[0m {safe}')");
    vec!["python3".to_owned(), "-c".to_owned(), script]
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Run a probe over `argv` with `rule` active and `strip_ansi`, returning the
/// drained drafts plus the raw frame texts from the ring.
fn run_probe(
    argv: &[String],
    rule: RuleDefinition,
    strip_ansi_flag: bool,
) -> (Vec<terminal_commander_core::EventDraft>, Vec<String>) {
    let rt = rt();
    rt.block_on(async {
        let rings = Arc::new(ContextRingManager::new());
        let bucket = BucketId::new();
        let probe_id = ProbeId::new();
        let sifter = Arc::new(SifterRuntime::build(&[rule]).unwrap());
        let sink = Arc::new(InMemorySink::new());
        let sink_dyn: Arc<dyn EventSink> = sink.clone();
        let cfg = ProcessProbeConfig {
            probe_id: Some(probe_id),
            strip_ansi: strip_ansi_flag,
            ..ProcessProbeConfig::for_bucket(bucket)
        };
        let mut probe = ProcessProbe::spawn(argv, &cfg, Arc::clone(&rings), sifter, sink_dyn)
            .expect("spawn ok");
        let _ = probe.wait().await.expect("wait ok");
        let drafts = sink.drain();
        // Pull the raw frame texts back from the frame store. The ring keeps
        // RAW bytes regardless of strip_ansi.
        let tail = rings
            .tail_frames(probe_id, 50, 64 * 1024)
            .expect("ring present");
        (drafts, tail.lines)
    })
}

#[test]
fn anchored_rule_matches_colored_output_after_strip() {
    // strip_ansi default-on: the `^\[AAP\]` rule must match the colored line.
    let (drafts, _raw) = run_probe(
        &argv_colored_aap_line("MySQL is ready"),
        anchored_aap_rule(),
        true,
    );
    assert_eq!(
        drafts.len(),
        1,
        "anchored rule must fire on colored output after strip; drafts={drafts:?}"
    );
}

#[test]
fn emitted_summary_has_no_escape_bytes() {
    let (drafts, _raw) = run_probe(
        &argv_colored_aap_line("MySQL is ready"),
        anchored_aap_rule(),
        true,
    );
    assert_eq!(drafts.len(), 1);
    let summary = &drafts[0].summary;
    assert!(
        !summary.as_bytes().contains(&0x1b),
        "summary must carry no ESC byte: {summary:?}"
    );
    assert!(
        summary.contains("[AAP] MySQL is ready"),
        "summary must echo the stripped matched line: {summary:?}"
    );
}

#[test]
fn raw_bytes_remain_retrievable_from_frame_store() {
    let (_drafts, raw) = run_probe(
        &argv_colored_aap_line("MySQL is ready"),
        anchored_aap_rule(),
        true,
    );
    // The ring keeps the RAW frame: at least one line still carries the ESC
    // byte even though the sifter saw a stripped copy.
    assert!(
        raw.iter().any(|l| l.as_bytes().contains(&0x1b)),
        "frame store must retain raw escape bytes; raw={raw:?}"
    );
    assert!(
        raw.iter().any(|l| l.contains("[AAP]")),
        "raw frame must contain the tag text: {raw:?}"
    );
}

#[test]
fn utf8_multibyte_not_corrupted_by_strip() {
    // A colored line carrying CJK + emoji. After strip the matched summary
    // must be byte-for-byte valid UTF-8 with the multibyte chars intact.
    let (drafts, _raw) = run_probe(
        &argv_colored_aap_line("\u{65e5}\u{672c}\u{8a9e} \u{1f680} up"),
        anchored_aap_rule(),
        true,
    );
    assert_eq!(drafts.len(), 1);
    let summary = &drafts[0].summary;
    assert!(summary.contains('\u{65e5}'), "CJK lost: {summary:?}");
    assert!(summary.contains('\u{1f680}'), "emoji lost: {summary:?}");
    assert!(
        !summary.as_bytes().contains(&0x1b),
        "ESC leaked: {summary:?}"
    );
}

#[test]
fn strip_ansi_false_lets_escapes_defeat_anchored_rule() {
    // Opt-out: with strip_ansi=false the raw escapes reach the sifter, so the
    // `^\[AAP\]` anchor no longer matches the colored line (the documented
    // pre-fix behavior the flag preserves for callers who want raw matching).
    let (drafts, raw) = run_probe(
        &argv_colored_aap_line("MySQL is ready"),
        anchored_aap_rule(),
        false,
    );
    assert!(
        drafts.is_empty(),
        "with strip_ansi=false an anchored rule must NOT match colored output; drafts={drafts:?}"
    );
    // The raw line is still captured in the store.
    assert!(
        raw.iter().any(|l| l.as_bytes().contains(&0x1b)),
        "raw frame still present with escapes; raw={raw:?}"
    );
}

#[test]
fn strip_ansi_helper_is_idempotent_on_plain_text() {
    // Direct unit check on the exported helper: plain text is unchanged and
    // a colored string loses its escapes.
    assert_eq!(strip_ansi("plain line"), "plain line");
    assert_eq!(
        strip_ansi("\u{1b}[0;32m[AAP]\u{1b}[0m ready"),
        "[AAP] ready"
    );
}
