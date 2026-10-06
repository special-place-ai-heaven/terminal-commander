// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! spec 004 US1 / FR-002 / FR-003: a reconstructed outcome must carry the
//! evidence a live observer would have had.
//!
//! The reported incident: a 20-minute suite finished, the daemon was replaced,
//! and the post-restart status reported the true `exited`/`0` with
//! `frames_total: 0`, `bytes_total: 0`, `duration_ms: null`. The agent
//! concluded "a 20-minute suite cannot produce zero bytes, therefore it never
//! ran", discarded a PASSING result, and re-ran it twice -- about 40 minutes.
//!
//! Labelling alone does not fix that: any harness gating expensive work will
//! re-run rather than bank an unverifiable pass. These tests pin that the
//! reconstruction carries real numbers.
//!
//! The reconstruction is compared against the LIVE status of the same job in
//! the same process, then read again by a second boot on the same data dir.
//! That is deterministic -- the receipt is written before either read -- and
//! it pins the exact equality that matters. Process-restart reconstruction is
//! separately covered by `crates/mcp/tests/ledger_compact_wait_restart.rs`.

use std::path::PathBuf;
use std::time::Duration;

use terminal_commander_core::JobState;
use terminal_commanderd::{CommandStartRequest, DaemonConfig, DaemonState, OutcomeTrust};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-recon-ev-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

#[test]
fn reconstructed_status_carries_the_same_evidence_as_the_live_status() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("evidence");
        let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();
        let exe = std::env::current_exe()
            .expect("current test binary path")
            .to_string_lossy()
            .into_owned();

        // `--list` produces real output, so the counters are non-zero and a
        // zeroed reconstruction would be visibly wrong.
        let started = state
            .command
            .start_combed(CommandStartRequest {
                argv: vec![exe, "--list".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace: None,
                tag: None,
                dedup_nonce: None,
                receipt_shape: None,
                strip_ansi: true,
                peer_discriminator: None,
                limits: None,
            })
            .expect("start ok");

        // Wait for the terminal transition AND the receipt write.
        let mut live = None;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let s = state.command.status(started.job_id).expect("status ok");
            if matches!(s.state, JobState::Exited | JobState::Failed) {
                live = Some(s);
                break;
            }
        }
        let live = live.expect("command must reach a terminal state");

        assert!(
            live.bytes_total > 0,
            "precondition: the command must actually produce output, else this \
             test cannot distinguish real evidence from zeroes"
        );

        let recon = state
            .command
            .reconstructed_status(started.job_id)
            .expect("a terminal transition must have persisted a receipt");

        // The payload: evidence, not just a label.
        assert_eq!(
            recon.frames_total, live.frames_total,
            "reconstructed frames must match what was observed live"
        );
        assert_eq!(recon.bytes_total, live.bytes_total);
        assert_eq!(recon.frames_stdout, live.frames_stdout);
        assert_eq!(recon.frames_stderr, live.frames_stderr);
        assert_eq!(recon.duration_ms, live.duration_ms);
        assert_eq!(
            recon.probe_id, live.probe_id,
            "the originating probe id must survive; a fresh placeholder id was \
             part of the original defect"
        );

        // Pins the off-by-one. The persist site moved below the lifecycle
        // append specifically so this equality holds; before that fix the
        // reconstructed count was deterministically one LOWER than live.
        assert_eq!(
            recon.events_emitted, live.events_emitted,
            "reconstructed event count must include the lifecycle event, exactly \
             as the live count does (natural-exit path)"
        );

        // Provenance is explicit and derived. The daemon reading its own
        // receipt saw the job finish, so that read is an observation too.
        assert_eq!(recon.outcome_trust, OutcomeTrust::Observed);
        assert!(!recon.restarted);
        assert_eq!(
            live.outcome_trust,
            OutcomeTrust::Observed,
            "the live read is an observation, not a reconstruction"
        );

        // A truthful exit code is preserved, never forced to null (FR-008).
        assert_eq!(recon.exit_code, live.exit_code);

        // The next boot reads the same receipt as a reconstruction, with the
        // same evidence.
        state.store.shutdown().expect("shutdown store actor");
        drop(state);
        let next_boot = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();
        let after_restart = next_boot
            .command
            .reconstructed_status(started.job_id)
            .expect("the receipt must survive the restart");
        assert_eq!(after_restart.outcome_trust, OutcomeTrust::Reconstructed);
        assert!(
            after_restart.restarted,
            "`restarted` is the compat alias for 'not observed live'"
        );
        assert_eq!(after_restart.frames_total, live.frames_total);
        assert_eq!(after_restart.bytes_total, live.bytes_total);
        assert_eq!(after_restart.events_emitted, live.events_emitted);
        assert_eq!(after_restart.exit_code, live.exit_code);
        next_boot.store.shutdown().expect("shutdown store actor");

        cleanup(&data);
    });
}

#[test]
fn reconstruction_is_reachable_through_the_engine_api() {
    // spec 004 US6 / FR-015: the embedded delivery mode calls the engine
    // directly and must reach reconstruction. While this lived in the IPC
    // handler an embedding host wrote receipt rows it had no supported typed
    // API to read back.
    let data = tmp_data_dir("engine-api");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    // An id the engine has never seen yields None rather than panicking or
    // fabricating an outcome -- the caller then decides unknown vs lost.
    assert!(
        state
            .command
            .reconstructed_status(terminal_commander_core::JobId::new())
            .is_none(),
        "no receipt must mean no reconstruction, never an invented terminal"
    );

    cleanup(&data);
}
