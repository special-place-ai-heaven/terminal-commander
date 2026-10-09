// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{BucketId, ContextRingManager, ProbeId};
use terminal_commander_probes::{InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;

fn config() -> ProcessProbeConfig {
    let mut config = ProcessProbeConfig::for_bucket(BucketId::new());
    config.grace = Duration::from_millis(100);
    config
}

fn spawn(argv: &[String]) -> ProcessProbe {
    ProcessProbe::spawn(
        argv,
        &config(),
        Arc::new(ContextRingManager::new()),
        Arc::new(SifterRuntime::build(&[]).unwrap()),
        Arc::new(InMemorySink::new()),
    )
    .unwrap()
}

async fn await_file(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture handshake timed out");
}

#[tokio::test]
async fn raw_activity_is_visible_before_newline_or_exit() {
    for (stdout_bytes, stderr_bytes, fill) in
        [(1, 0, 65), (0, 1, 255), (131_073, 0, 66), (7, 9, 255)]
    {
        let directory = std::env::temp_dir().join(format!("tc-observation-{}", ProbeId::new()));
        std::fs::create_dir(&directory).unwrap();
        let ready = directory.join("ready");
        let release = directory.join("release");
        let script = "import os,sys,time,pathlib\na,b,fill=map(int,sys.argv[1:4])\nfor fd,n in [(1,a),(2,b)]:\n data=bytes([fill])*n\n while data:\n  data=data[os.write(fd,data):]\npathlib.Path(sys.argv[4]).touch()\nwhile not pathlib.Path(sys.argv[5]).exists(): time.sleep(.005)\nos.write(1,b'\\n')\nos.write(2,b'\\n')";
        let started = Instant::now();
        let mut probe = spawn(&[
            "python3".into(),
            "-c".into(),
            script.into(),
            stdout_bytes.to_string(),
            stderr_bytes.to_string(),
            fill.to_string(),
            ready.display().to_string(),
            release.display().to_string(),
        ]);
        await_file(&ready).await;
        let observed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let metrics = probe.metrics();
                if metrics.bytes_total == stdout_bytes + stderr_bytes {
                    return metrics;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        // Release even on a failed assertion so the red test never leaks a fixture.
        std::fs::write(&release, []).unwrap();
        assert!(probe.wait().await.unwrap().success());
        let observed = observed.expect("raw bytes must be published while the pipe remains open");
        assert_eq!(observed.frames_total, 0);
        assert!(observed.last_output_at.unwrap() >= started);
        assert_eq!(observed.observation.stdout.bytes_total, stdout_bytes);
        assert_eq!(observed.observation.stderr.bytes_total, stderr_bytes);
        assert_eq!(probe.metrics().bytes_total, stdout_bytes + stderr_bytes + 2);
        assert!(probe.observation().is_complete());
        assert!(started.elapsed() < Duration::from_secs(10));
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn absent_runtime_is_rejected_without_panicking() {
    let result = std::panic::catch_unwind(|| {
        ProcessProbe::spawn(
            &["python3".into(), "-c".into(), "pass".into()],
            &config(),
            Arc::new(ContextRingManager::new()),
            Arc::new(SifterRuntime::build(&[]).unwrap()),
            Arc::new(InMemorySink::new()),
        )
    });
    assert!(
        result.is_ok(),
        "missing runtime must be a typed error, never a panic"
    );
    assert!(matches!(
        result.unwrap(),
        Err(terminal_commander_probes::ProcessProbeError::MissingRuntime)
    ));
}

#[tokio::test]
async fn utf16_raw_bytes_include_bom_and_newlines_before_transcoding() {
    let mut probe = spawn(&[
        "python3".into(), "-c".into(),
        "import os; os.write(1, b'\\xff\\xfeA\\x00\\n\\x00'); os.write(2, b'\\xfe\\xff\\x00B\\x00\\n')".into(),
    ]);
    let report = probe.wait_report().await.unwrap();
    assert!(report.exit_status.success());
    assert!(report.observation.is_complete());
    assert_eq!(report.observation.stdout.bytes_total, 6);
    assert_eq!(report.observation.stderr.bytes_total, 6);
    assert_eq!(probe.metrics().bytes_total, 12);
    assert_eq!(probe.metrics().frames_total, 2);
}

#[tokio::test]
async fn cleared_environment_is_explicit_and_does_not_inject_host_markers() {
    // Resolve Python before clearing PATH. Values are never printed or compared
    // in diagnostics: the fixture only exits success/failure.
    let python = std::process::Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(python.status.success());
    let executable = String::from_utf8(python.stdout).unwrap().trim().to_owned();
    let mut cfg = config();
    cfg.env
        .push(("TC_EXPLICIT_TEST_MARKER".into(), "present".into()));
    let script = "import os,sys; sys.exit(0 if os.environ.get('TC_EXPLICIT_TEST_MARKER') == 'present' and 'PATH' not in os.environ and 'TC_DAEMON_CHILD' not in os.environ else 42)";
    let mut probe = ProcessProbe::spawn_with_environment(
        &[executable, "-c".into(), script.into()],
        &cfg,
        Arc::new(ContextRingManager::new()),
        Arc::new(SifterRuntime::build(&[]).unwrap()),
        Arc::new(InMemorySink::new()),
        terminal_commander_probes::EnvironmentMode::Clear,
    )
    .unwrap();
    assert!(probe.wait().await.unwrap().success());
}

#[tokio::test]
async fn cancelling_a_wait_future_preserves_wait_ownership() {
    let directory = std::env::temp_dir().join(format!("tc-wait-{}", ProbeId::new()));
    std::fs::create_dir(&directory).unwrap();
    let ready = directory.join("ready");
    let script = "import pathlib,sys,time; pathlib.Path(sys.argv[1]).touch(); time.sleep(60)";
    let mut probe = spawn(&[
        "python3".into(),
        "-c".into(),
        script.into(),
        ready.display().to_string(),
    ]);
    await_file(&ready).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(10), probe.wait())
            .await
            .is_err()
    );
    probe.cancel();
    probe.cancel();
    let report = tokio::time::timeout(Duration::from_secs(5), probe.wait_report())
        .await
        .unwrap()
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(
        report.cleanup,
        terminal_commander_probes::ProcessCleanup::Complete
    );
    assert_eq!(
        probe.wait_report().await.unwrap().exit_status,
        report.exit_status
    );
    std::fs::remove_dir_all(directory).unwrap();
}
