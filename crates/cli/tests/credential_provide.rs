// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! `terminal-commander credential provide <job_id>` end to end: a real
//! daemon, a PTY job blocked on a sudo-style prompt, and the real CLI binary
//! (the admin image the daemon's owner gate accepts) reading the password
//! from piped stdin. The CLI is a sibling of the daemon, not its descendant,
//! exactly like an owner's own terminal.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use terminal_commander_core::JobId;
use terminal_commander_ipc::{
    CommandStatusParams, DaemonClient, IpcRequest, IpcResponse, PtyCommandStartParams,
    PtyCommandStopParams,
};
use terminal_commander_supervisor::pidfile::{pidfile_path, read_pidfile_raw};

#[path = "../../test_support/isolated_env.rs"]
mod isolated_env;

const SECRET: &str = "cli-s3cret-marker-Q9";

/// Reads one line with echo on and exits 0 only on the secret, compared
/// reversed so the secret is never in argv.
const CHILD: &str = "import sys\n\
sys.stdout.write('[sudo] password for dev: ')\n\
sys.stdout.flush()\n\
line = sys.stdin.readline().rstrip('\\r\\n')\n\
sys.exit(0 if line[::-1] == '9Q-rekram-terc3s-ilc' else 3)\n";

fn python() -> Option<&'static str> {
    let candidates: &[&'static str] = if cfg!(windows) {
        &["python", "py"]
    } else {
        &["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"]
    };
    candidates.iter().copied().find(|c| {
        Command::new(c)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let mut bin = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("profile dir")
        .join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

/// A live daemon under an isolated `TC_DATA` + short unique `TC_SESSION`
/// (see `read_subcommands.rs` for why the token is short and per-process).
struct LiveDaemon {
    child: std::process::Child,
    base: PathBuf,
    token: String,
    endpoint: String,
}

impl LiveDaemon {
    fn spawn() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let token = format!("c{:x}{:04x}", std::process::id(), nanos & 0xffff);
        let base = std::env::temp_dir().join(format!("tc-cli-cred-{}-{nanos}", std::process::id()));
        let state_dir = base.join(&token);
        let child =
            isolated_env::isolate(&mut Command::new(target_bin("terminal-commanderd")), &base)
                .args(["start", "--mode", "ipc-server"])
                .env("TC_DATA", &base)
                .env("TC_SESSION", &token)
                .env_remove("TC_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn daemon");
        let mut daemon = Self {
            child,
            base,
            token,
            endpoint: String::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pidfile_path(&state_dir).exists() {
            assert!(Instant::now() < deadline, "daemon never bound");
            std::thread::sleep(Duration::from_millis(50));
        }
        daemon.endpoint = read_pidfile_raw(&state_dir).expect("pidfile").endpoint;
        daemon
    }

    fn cli(&self, args: &[&str], stdin: &str) -> std::process::Output {
        let mut child = isolated_env::isolate(
            &mut Command::new(env!("CARGO_BIN_EXE_terminal-commander")),
            &self.base,
        )
        .args(args)
        .env("TC_DATA", &self.base)
        .env("TC_SESSION", &self.token)
        .env_remove("TC_SOCKET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run cli");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("cli output")
    }
}

impl Drop for LiveDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

async fn call(client: &DaemonClient, req: IpcRequest) -> IpcResponse {
    let mut last = None;
    for id in 1..=100 {
        match client.call(id, req.clone()).await {
            Ok(r) => return r,
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("daemon call failed: {last:?}");
}

async fn wait_awaiting(client: &DaemonClient, job_id: JobId) {
    for _ in 0..600 {
        if let IpcResponse::PtyCommandList(l) = call(client, IpcRequest::PtyCommandList).await
            && l.entries
                .iter()
                .any(|e| e.job_id == job_id && e.awaiting_credential.is_some())
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job never reported awaiting_credential");
}

#[test]
fn credential_provide_reads_stdin_and_the_job_receives_it() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    let daemon = LiveDaemon::spawn();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let client = DaemonClient::new(daemon.endpoint.clone()).with_timeout(Duration::from_secs(5));

    let job_id = match rt.block_on(call(
        &client,
        IpcRequest::PtyCommandStart(PtyCommandStartParams {
            environment: None,
            argv: vec![
                python.to_owned(),
                "-u".to_owned(),
                "-c".to_owned(),
                CHILD.to_owned(),
            ],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            rows: None,
            cols: None,
            tag: None,
        }),
    )) {
        IpcResponse::PtyCommandStart(s) => s.job_id,
        other => panic!("unexpected: {other:?}"),
    };
    let job = job_id.to_wire_string();

    // Not waiting yet is refused before any password is read.
    let early = daemon.cli(&["credential", "provide", "job_not-a-job"], "");
    assert_ne!(early.status.code(), Some(0));

    rt.block_on(wait_awaiting(&client, job_id));
    let out = daemon.cli(&["credential", "provide", &job], &format!("{SECRET}\n"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={stdout} stderr={stderr}"
    );
    assert_eq!(stdout.trim(), "provided");
    assert!(stderr.contains("sudo password"), "{stderr}");
    assert!(!stdout.contains(SECRET) && !stderr.contains(SECRET));

    let deadline = Instant::now() + Duration::from_secs(30);
    let exit_code = loop {
        if let IpcResponse::CommandStatus(s) = rt.block_on(call(
            &client,
            IpcRequest::CommandStatus(CommandStatusParams { job_id }),
        )) && s.state == terminal_commander_core::JobState::Exited
        {
            break s.exit_code;
        }
        assert!(Instant::now() < deadline, "job never exited");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        exit_code,
        Some(0),
        "the PTY child did not receive the secret"
    );

    // A second provide has no prompt to answer.
    let again = daemon.cli(&["credential", "provide", &job], &format!("{SECRET}\n"));
    assert_ne!(again.status.code(), Some(0));
    let _ = rt.block_on(call(
        &client,
        IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id }),
    ));
}
