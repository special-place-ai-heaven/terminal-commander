// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::io::Write;
use std::time::Duration;
use terminal_commander_core::context::MAX_FRAME_BYTES;
use terminal_commander_ipc::protocol::{
    CommandOutputTailParams, CommandStartParams, CommandStatusParams, ReceiptShape,
};
use terminal_commanderd::DaemonConfig;
use terminal_commanderd::embedded::{EmbeddedEngine, IsolatedCommand};

#[test]
fn output_tail_child() {
    let Ok(kind) = std::env::var("TC_TAIL_FIXTURE") else {
        return;
    };
    let text = match kind.as_str() {
        "ascii" => "x".repeat(50_000),
        "unicode" => "\u{00e9}\u{1f980}".repeat(20_000),
        // The read cap discards the suffix, then CR normalization leaves
        // only "kept" in the frame, with a positive upstream loss counter.
        "cr-padding" => format!("kept{}lost", "\r".repeat(65_536)),
        _ => panic!("unknown fixture"),
    };
    let mut stdout = std::io::stdout().lock();
    // Separate the payload from the Rust test harness's progress output.
    writeln!(stdout, "\n{text}").unwrap();
    stdout.flush().unwrap();
    // Do not append a harness summary after the single oversized line.
    std::process::exit(0);
}

async fn check_live_tail(kind: &str, unit: &str) {
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let mut params = CommandStartParams::new(vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".into(),
        "output_tail_child".into(),
        "--nocapture".into(),
    ]);
    params.env.push(("TC_TAIL_FIXTURE".into(), kind.into()));
    let started = engine
        .command_start_isolated(IsolatedCommand::new(params, dir.path().into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = engine
                .command_status(CommandStatusParams {
                    job_id: started.job_id,
                })
                .await
                .unwrap();
            if let Some(code) = status.exit_code {
                assert_eq!(code, 0);
                assert!(status.process_observation.unwrap().is_complete());
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("tail fixture must finish and drain");
    let full = engine
        .command_output_tail(CommandOutputTailParams {
            job_id: started.job_id,
            max_lines: 1,
            max_bytes: 65_536,
            strip_ansi: true,
        })
        .await
        .unwrap();
    assert_eq!(full.returned_lines, 1);
    let mut kept = unit.repeat(MAX_FRAME_BYTES / unit.len() + 1);
    let mut end = MAX_FRAME_BYTES;
    while !kept.is_char_boundary(end) {
        end -= 1;
    }
    kept.truncate(end);
    assert_eq!(full.lines[0].len(), kept.len());
    assert!(
        full.lines[0] == kept,
        "retained capture must preserve exact text"
    );
    assert!(
        full.truncated_bytes,
        "capture loss must survive IPC projection"
    );

    for budget in [0, 1, 5, 200] {
        let tail = engine
            .command_output_tail(CommandOutputTailParams {
                job_id: started.job_id,
                max_lines: 1,
                max_bytes: budget,
                strip_ansi: true,
            })
            .await
            .unwrap();
        assert_eq!(tail.returned_lines, 1);
        assert!(tail.lines[0].len() <= budget as usize);
        assert!(kept.ends_with(&tail.lines[0]));
        assert!(tail.truncated_bytes);
    }
    assert!(engine.shutdown().await.unwrap().lifecycle_drained);
}

#[tokio::test]
async fn live_ascii_tail_is_byte_bounded_and_reports_capture_loss() {
    check_live_tail("ascii", "x").await;
}

#[tokio::test]
async fn live_unicode_tail_is_byte_bounded_and_reports_upstream_capture_loss() {
    check_live_tail("unicode", "\u{00e9}\u{1f980}").await;
}

#[tokio::test]
async fn live_head_receipt_reports_capture_loss_after_cr_normalization() {
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let mut params = CommandStartParams::new(vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".into(),
        "output_tail_child".into(),
        "--nocapture".into(),
    ]);
    params
        .env
        .push(("TC_TAIL_FIXTURE".into(), "cr-padding".into()));
    params.receipt_shape = Some(ReceiptShape {
        head_lines: 20,
        tail_lines: 0,
    });
    let started = engine
        .command_start_isolated(IsolatedCommand::new(params, dir.path().into()))
        .await
        .unwrap();
    let receipt = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = engine
                .command_status(CommandStatusParams {
                    job_id: started.job_id,
                })
                .await
                .unwrap();
            if let Some(code) = status.exit_code {
                assert_eq!(code, 0);
                assert!(status.process_observation.unwrap().is_complete());
                break status.receipt.expect("quiet job must return its receipt");
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("head fixture must finish and drain");
    assert!(receipt.head.iter().any(|line| line == "kept"));
    assert!(receipt.head.iter().map(String::len).sum::<usize>() <= 2048);
    assert!(receipt.tail.is_empty());
    assert_eq!(receipt.lines_omitted, 0);
    assert!(
        receipt.tail_incomplete,
        "normalized head must preserve upstream loss"
    );
    assert!(engine.shutdown().await.unwrap().lifecycle_drained);
}
