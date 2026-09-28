// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Owner credential elicitation over IPC: `credential_request` (model side,
//! status only), `credential_url` (the MCP adapter's loopback page, driven
//! here with a raw HTTP client standing in for the owner's browser) and
//! `credential_provide` (admin CLI side). The native owner prompt is
//! replaced by `DaemonConfig::credential_prompter_test_seam`; no UI runs
//! here. Every test also proves the secret never reaches a response,
//! the audit log, or any file in the daemon's data dir.

#![cfg(any(unix, windows))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::JobId;
use terminal_commander_ipc::{
    AuditSinceParams, CommandOutputTailParams, CommandStatusParams, CredentialKind,
    CredentialProvideParams, CredentialRequestParams, CredentialStatus, CredentialUrlOp,
    CredentialUrlParams, CredentialUrlResponse, OwnerSecret,
};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse,
    PtyCommandStartParams, PtyCommandStopParams, PtyCommandWriteStdinParams,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(unix)]
type ServerHandle = terminal_commanderd::ServerHandle;
#[cfg(windows)]
type ServerHandle = terminal_commanderd::PipeServerHandle;

/// The in-process transport: a unix socket, or a named pipe on Windows.
fn serve(state: &Arc<DaemonState>, tag: &str) -> (PathBuf, ServerHandle) {
    #[cfg(unix)]
    {
        let _ = tag;
        let handle =
            terminal_commanderd::IpcServer::new(Arc::clone(state), state.config.socket_path())
                .spawn()
                .unwrap();
        (handle.socket_path().to_path_buf(), handle)
    }
    #[cfg(windows)]
    {
        let name = format!(
            r"\\.\pipe\tc-test-cred-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let handle = terminal_commanderd::PipeServer::new(Arc::clone(state), name.clone())
            .spawn()
            .unwrap();
        (PathBuf::from(name), handle)
    }
}

const SECRET: &str = "s3cret-marker-K7";

/// Prints a sudo-style prompt, reads one line WITH terminal echo on (the
/// worst case: the tty echoes the answer into the output), exits 0 only if
/// the line is the secret. It compares against the reversed secret so the
/// secret itself is never in argv, which `pty_command_list` returns.
const CHILD: &str = "import sys\n\
sys.stdout.write('[sudo] password for dev: ')\n\
sys.stdout.flush()\n\
line = sys.stdin.readline().rstrip('\\r\\n')\n\
ok = line[::-1] == '7K-rekram-terc3s'\n\
print('MARKER-OK' if ok else 'MARKER-BAD')\n\
sys.exit(0 if ok else 3)\n";

fn python() -> Option<String> {
    let candidates: &[&str] = if cfg!(windows) {
        &["python", "py"]
    } else {
        &["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"]
    };
    candidates
        .iter()
        .find(|c| {
            std::process::Command::new(c)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .map(|c| (*c).to_owned())
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    std::env::temp_dir().join(format!("tc-cred-{tag}-{}-{nanos}-{n}", std::process::id()))
}

struct Harness {
    data: PathBuf,
    _state: Arc<DaemonState>,
    _handle: ServerHandle,
    client: DaemonClient,
    seq: u64,
    /// Every response and error, serialized, for the leak check.
    seen: Vec<String>,
}

impl Harness {
    fn new(tag: &str, prompter: &str) -> Self {
        Self::with(tag, Some(prompter), None)
    }

    /// `prompter: None` leaves the real native prompt in place, so such a
    /// harness must never reach `credential_request`.
    fn with(tag: &str, prompter: Option<&str>, url_ttl: Option<Duration>) -> Self {
        let data = tmp_data_dir(tag);
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.credential_prompter_test_seam = prompter.map(str::to_owned);
        cfg.credential_url_ttl_test_seam = url_ttl;
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let (endpoint, handle) = serve(&state, tag);
        let client = DaemonClient::new(endpoint).with_timeout(Duration::from_secs(90));
        Self {
            data,
            _state: state,
            _handle: handle,
            client,
            seq: 0,
            seen: Vec::new(),
        }
    }

    async fn call(
        &mut self,
        req: IpcRequest,
    ) -> Result<IpcResponse, terminal_commanderd::IpcError> {
        self.seq += 1;
        let r = self.client.call(self.seq, req).await;
        self.seen.push(match &r {
            Ok(resp) => serde_json::to_string(resp).unwrap(),
            Err(e) => format!("{}: {}", serde_json::to_string(&e.code).unwrap(), e.message),
        });
        r
    }

    async fn start_child(&mut self, python: &str) -> JobId {
        self.start_child_with_env(python, vec![]).await
    }

    async fn start_child_with_env(&mut self, python: &str, env: Vec<(String, String)>) -> JobId {
        let r = self
            .call(IpcRequest::PtyCommandStart(PtyCommandStartParams {
                environment: None,
                argv: vec![
                    python.to_owned(),
                    "-u".to_owned(),
                    "-c".to_owned(),
                    CHILD.to_owned(),
                ],
                cwd: None,
                env,
                bucket_config: None,
                rules: vec![],
                rows: None,
                cols: None,
                tag: None,
            }))
            .await
            .expect("pty start");
        match r {
            IpcResponse::PtyCommandStart(s) => s.job_id,
            other => panic!("unexpected: {other:?}"),
        }
    }

    async fn wait_awaiting(&mut self, job_id: JobId) -> CredentialKind {
        for _ in 0..600 {
            if let Ok(IpcResponse::PtyCommandList(l)) = self.call(IpcRequest::PtyCommandList).await
                && let Some(a) = l
                    .entries
                    .iter()
                    .find(|e| e.job_id == job_id)
                    .and_then(|e| e.awaiting_credential)
            {
                return a.kind;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("job never reported awaiting_credential");
    }

    async fn wait_exit_code(&mut self, job_id: JobId) -> Option<i32> {
        for _ in 0..600 {
            if let Ok(IpcResponse::CommandStatus(s)) = self
                .call(IpcRequest::CommandStatus(CommandStatusParams { job_id }))
                .await
                && matches!(
                    s.state,
                    terminal_commander_core::JobState::Exited
                        | terminal_commander_core::JobState::Failed
                )
            {
                return s.exit_code;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("job never exited");
    }

    async fn request(&mut self, job_id: JobId) -> (CredentialStatus, Option<String>) {
        match self
            .call(IpcRequest::CredentialRequest(CredentialRequestParams {
                job_id,
                wait_ms: None,
            }))
            .await
            .expect("credential_request")
        {
            IpcResponse::CredentialRequest(r) => (r.status, r.command),
            other => panic!("unexpected: {other:?}"),
        }
    }

    async fn provide(
        &mut self,
        job_id: JobId,
        from_mcp: bool,
    ) -> Result<IpcResponse, terminal_commanderd::IpcError> {
        self.call(IpcRequest::CredentialProvide(CredentialProvideParams {
            job_id,
            secret: OwnerSecret::new(SECRET.to_owned()),
            from_mcp,
        }))
        .await
    }

    async fn url(&mut self, job_id: JobId, op: CredentialUrlOp) -> CredentialUrlResponse {
        match self
            .call(IpcRequest::CredentialUrl(CredentialUrlParams {
                job_id,
                op,
            }))
            .await
            .expect("credential_url")
        {
            IpcResponse::CredentialUrl(r) => r,
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// One HTTP/1.1 request to the owner page, as a browser sends it.
    /// Returns the status code and body; the body joins the leak check.
    async fn http(
        &mut self,
        url: &str,
        method: &str,
        path: Option<&str>,
        host: Option<&str>,
        body: &str,
    ) -> std::io::Result<(u16, String)> {
        let rest = url.strip_prefix("http://").expect("http url");
        let (addr, own_path) = rest.split_at(rest.find('/').expect("path"));
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        let request = format!(
            "{method} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: \
             application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
            path.unwrap_or(own_path),
            host.unwrap_or(addr),
            body.len()
        );
        stream.write_all(request.as_bytes()).await?;
        let mut raw = String::new();
        stream.read_to_string(&mut raw).await?;
        let status = raw
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b).to_owned();
        self.seen.push(body.clone());
        Ok((status, body))
    }

    async fn credential_audit_rows(&mut self) -> Vec<(String, String)> {
        match self
            .call(IpcRequest::AuditSince(AuditSinceParams {
                cursor: 0,
                action_filter: Some("credential_provided".to_owned()),
                decision_filter: None,
                limit: Some(100),
            }))
            .await
            .expect("audit_since")
        {
            IpcResponse::AuditSince(r) => r
                .rows
                .into_iter()
                .map(|row| (row.decision, row.metadata_json.unwrap_or_default()))
                .collect(),
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Read the job's output back through every model-reachable read path,
    /// then assert the secret is in none of them and in no data-dir file.
    async fn assert_secret_never_surfaced(&mut self, job_id: JobId) {
        let _ = self
            .call(IpcRequest::CommandOutputTail(CommandOutputTailParams {
                job_id,
                max_lines: 200,
                max_bytes: 65_536,
                strip_ansi: false,
            }))
            .await;
        let _ = self
            .call(IpcRequest::AuditSince(AuditSinceParams {
                cursor: 0,
                action_filter: None,
                decision_filter: None,
                limit: Some(500),
            }))
            .await;
        for seen in &self.seen {
            assert!(
                !seen.contains(SECRET),
                "secret leaked into a response: {seen}"
            );
        }
        // Let the store actor flush before scanning its files.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_no_file_contains(&self.data, SECRET.as_bytes());
    }

    async fn stop(&mut self, job_id: JobId) {
        let _ = self
            .call(IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id }))
            .await;
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn assert_no_file_contains(dir: &Path, needle: &[u8]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            assert_no_file_contains(&path, needle);
        } else if let Ok(bytes) = std::fs::read(&path) {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "secret found in {}",
                path.display()
            );
        }
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn credential_request_types_the_owner_answer_and_the_secret_never_surfaces() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::new("native", &format!("test:{SECRET}"));
        let job_id = h.start_child(&python).await;
        assert_eq!(h.wait_awaiting(job_id).await, CredentialKind::Sudo);

        // command_status carries the same field.
        match h
            .call(IpcRequest::CommandStatus(CommandStatusParams { job_id }))
            .await
            .expect("status")
        {
            IpcResponse::CommandStatus(s) => {
                assert_eq!(
                    s.awaiting_credential.map(|a| a.kind),
                    Some(CredentialKind::Sudo)
                );
            }
            other => panic!("unexpected: {other:?}"),
        }

        // TC44 is unchanged, and the deny now teaches the owner path.
        let denied = h
            .call(IpcRequest::PtyCommandWriteStdin(
                PtyCommandWriteStdinParams {
                    job_id,
                    bytes: "model-typed\r".to_owned(),
                    cursor: None,
                    wait_ms: None,
                },
            ))
            .await
            .expect_err("model stdin must stay denied during a password prompt");
        assert_eq!(denied.code, IpcErrorCode::SecretInputDenied);
        assert!(
            denied.message.contains("credential_request"),
            "{}",
            denied.message
        );

        assert_eq!(h.request(job_id).await, (CredentialStatus::Provided, None));
        assert_eq!(
            h.wait_exit_code(job_id).await,
            Some(0),
            "child did not get the secret"
        );
        // Idempotent per prompt: a repeat call replays, never re-asks.
        assert_eq!(h.request(job_id).await.0, CredentialStatus::Provided);

        let rows = h.credential_audit_rows().await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "allow");
        let meta: serde_json::Value = serde_json::from_str(&rows[0].1).unwrap();
        assert_eq!(
            meta,
            serde_json::json!({"kind": "sudo", "source": "native"})
        );

        h.assert_secret_never_surfaced(job_id).await;
    });
}

/// The page closes after its one answer (or expiry): a connect either fails
/// outright or the socket is closed before any HTTP response.
async fn page_is_closed(url: &str) -> bool {
    let addr = url
        .strip_prefix("http://")
        .and_then(|r| r.split('/').next())
        .unwrap()
        .to_owned();
    for _ in 0..40 {
        match tokio::net::TcpStream::connect(&addr).await {
            Err(_) => return true,
            Ok(mut s) => {
                let _ = s.write_all(b"GET / HTTP/1.1\r\n\r\n").await;
                let mut buf = Vec::new();
                let read =
                    tokio::time::timeout(Duration::from_millis(200), s.read_to_end(&mut buf));
                if matches!(read.await, Ok(Ok(0) | Err(_))) {
                    return true;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[test]
fn credential_url_page_takes_exactly_one_post_with_the_right_token() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        // `none`: no native prompt, so only the page can answer.
        let mut h = Harness::new("url", "none");
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;

        let opened = h.url(job_id, CredentialUrlOp::Open).await;
        assert!(opened.fresh);
        assert_eq!(opened.kind, Some(CredentialKind::Sudo));
        let url = opened.url.clone().expect("page url");
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(opened.elicitation_id.is_some());
        // One page per prompt: a repeat open re-finds it.
        let again = h.url(job_id, CredentialUrlOp::Open).await;
        assert!(!again.fresh);
        assert_eq!(again.url.as_deref(), Some(url.as_str()));

        // A short poll while the owner has the page answers `pending`.
        match h
            .call(IpcRequest::CredentialRequest(CredentialRequestParams {
                job_id,
                wait_ms: Some(200),
            }))
            .await
            .expect("credential_request")
        {
            IpcResponse::CredentialRequest(r) => assert_eq!(r.status, CredentialStatus::Pending),
            other => panic!("unexpected: {other:?}"),
        }

        let form = format!("password={SECRET}");
        let wrong_token = format!("/{}", "0".repeat(64));
        assert_eq!(
            h.http(&url, "POST", Some(&wrong_token), None, &form)
                .await
                .unwrap()
                .0,
            404
        );
        assert_eq!(
            h.http(&url, "POST", None, Some("attacker.example:80"), &form)
                .await
                .unwrap()
                .0,
            403,
            "a rebinding Host header must be refused"
        );
        h.wait_awaiting(job_id).await;

        let (status, page) = h.http(&url, "GET", None, None, "").await.unwrap();
        assert_eq!(status, 200);
        assert!(page.contains(&job_id.to_wire_string()), "{page}");
        assert!(page.contains("type=\"password\""), "{page}");

        let (status, done) = h.http(&url, "POST", None, None, &form).await.unwrap();
        assert_eq!(status, 200, "{done}");
        assert!(done.contains("Sent"), "{done}");
        assert!(
            page_is_closed(&url).await,
            "a second POST must find no page"
        );

        // The model-side call reports the page's outcome; nothing re-asks.
        assert_eq!(h.request(job_id).await, (CredentialStatus::Provided, None));
        assert_eq!(
            h.wait_exit_code(job_id).await,
            Some(0),
            "child did not get the secret"
        );

        let rows = h.credential_audit_rows().await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        let meta: serde_json::Value = serde_json::from_str(&rows[0].1).unwrap();
        assert_eq!(meta, serde_json::json!({"kind": "sudo", "source": "url"}));

        h.assert_secret_never_surfaced(job_id).await;
    });
}

/// The owner page for `job_id`'s prompt, plus its `pty_command_list` entry.
async fn owner_view(
    h: &mut Harness,
    job_id: JobId,
) -> (String, terminal_commander_ipc::PtyCommandListEntry) {
    let url = h
        .url(job_id, CredentialUrlOp::Open)
        .await
        .url
        .expect("page url");
    let (status, page) = h.http(&url, "GET", None, None, "").await.unwrap();
    assert_eq!(status, 200, "{page}");
    let Ok(IpcResponse::PtyCommandList(list)) = h.call(IpcRequest::PtyCommandList).await else {
        panic!("pty_command_list failed");
    };
    let entry = list
        .entries
        .into_iter()
        .find(|e| e.job_id == job_id)
        .expect("job listed");
    (page, entry)
}

/// W1: the owner is shown the program the daemon spawned, resolved through
/// the request's PATH, and warned when the request overrode resolution; a
/// familiar `argv[0]` alone never vouches for what receives the password.
#[test]
fn owner_prompt_shows_the_spawned_program_and_warns_on_a_path_override() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    let exe = std::process::Command::new(&python)
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    let exe = PathBuf::from(String::from_utf8(exe.stdout).unwrap().trim());
    let dir = exe.parent().unwrap().to_string_lossy().into_owned();
    let name = exe.file_name().unwrap().to_string_lossy().into_owned();
    let norm = |s: &str| {
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s.to_owned()
        }
    };
    rt().block_on(async {
        let mut h = Harness::new("w1", "none");

        // PATH override: `argv[0]` is a bare name the request's PATH picks.
        let job_id = h
            .start_child_with_env(&name, vec![("PATH".to_owned(), dir.clone())])
            .await;
        h.wait_awaiting(job_id).await;
        let (page, entry) = owner_view(&mut h, job_id).await;
        let program = entry.program.clone().expect("program on an awaiting entry");
        assert!(std::path::Path::new(&program).is_absolute(), "{program}");
        assert!(
            norm(&program).starts_with(&norm(&dir)),
            "{program} not in {dir}"
        );
        assert!(
            norm(&page).contains(&norm(&format!("Program: {program}"))),
            "{page}"
        );
        assert_eq!(entry.program_env, vec!["PATH".to_owned()]);
        assert!(page.contains("\u{26a0} request overrides PATH"), "{page}");
        h.stop(job_id).await;

        // No override: still the absolute program, and no warning.
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;
        let (page, entry) = owner_view(&mut h, job_id).await;
        let program = entry.program.expect("program on an awaiting entry");
        assert!(std::path::Path::new(&program).is_absolute(), "{program}");
        assert!(entry.program_env.is_empty());
        assert!(!page.contains('\u{26a0}'), "{page}");
        h.stop(job_id).await;
    });
}

#[test]
fn credential_url_page_expires_and_a_declined_elicitation_is_final() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::with("url-ttl", Some("none"), Some(Duration::from_millis(800)));
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;

        let first = h.url(job_id, CredentialUrlOp::Open).await.url.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(page_is_closed(&first).await, "the page must expire");
        h.wait_awaiting(job_id).await;

        // An expired page is forgotten: the next open is a fresh one.
        let second = h.url(job_id, CredentialUrlOp::Open).await;
        assert!(second.fresh);
        let second = second.url.unwrap();
        assert_ne!(first, second);

        h.url(job_id, CredentialUrlOp::Declined).await;
        assert!(page_is_closed(&second).await);
        assert_eq!(h.request(job_id).await.0, CredentialStatus::Declined);
        assert!(!h.url(job_id, CredentialUrlOp::Open).await.fresh);
        h.wait_awaiting(job_id).await;
        h.stop(job_id).await;
        h.assert_secret_never_surfaced(job_id).await;
    });
}

#[test]
fn credential_url_abandon_falls_back_to_the_native_prompt() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::new("url-abandon", &format!("test:{SECRET}"));
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;
        let url = h.url(job_id, CredentialUrlOp::Open).await.url.unwrap();
        h.url(job_id, CredentialUrlOp::Abandon).await;
        assert!(page_is_closed(&url).await);
        assert_eq!(h.request(job_id).await.0, CredentialStatus::Provided);
        assert_eq!(h.wait_exit_code(job_id).await, Some(0));
        let rows = h.credential_audit_rows().await;
        let meta: serde_json::Value = serde_json::from_str(&rows[0].1).unwrap();
        assert_eq!(meta["source"], "native");
        h.assert_secret_never_surfaced(job_id).await;
    });
}

#[test]
fn credential_url_is_denied_to_peers_other_than_the_mcp_adapter() {
    rt().block_on(async {
        // No seams: the in-process client is an unknown image, like a
        // script the model wrote to talk to the socket.
        let mut h = Harness::with("url-gate", None, None);
        let denied = h
            .call(IpcRequest::CredentialUrl(CredentialUrlParams {
                job_id: JobId::new(),
                op: CredentialUrlOp::Open,
            }))
            .await
            .expect_err("only the MCP adapter may open the owner page");
        assert_eq!(denied.code, IpcErrorCode::PolicyDenied);
    });
}

#[test]
fn credential_request_without_a_native_prompt_names_the_owner_cli_command() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::new("cli", "none");
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;
        let expected = format!(
            "terminal-commander credential provide {}",
            job_id.to_wire_string()
        );
        assert_eq!(
            h.request(job_id).await,
            (
                CredentialStatus::OwnerActionRequired,
                Some(expected.clone())
            )
        );
        assert_eq!(
            h.request(job_id).await,
            (CredentialStatus::OwnerActionRequired, Some(expected))
        );
        // Still waiting: nothing was typed.
        h.wait_awaiting(job_id).await;
        h.stop(job_id).await;
    });
}

#[test]
fn credential_request_decline_is_final_for_that_prompt() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::new("decline", "test-decline");
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;
        assert_eq!(h.request(job_id).await.0, CredentialStatus::Declined);
        assert_eq!(h.request(job_id).await.0, CredentialStatus::Declined);
        h.wait_awaiting(job_id).await;
        h.stop(job_id).await;
    });
}

#[test]
fn credential_provide_is_denied_to_mcp_labelled_peers_and_answers_for_the_admin_cli() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::new("provide", "none");
        let job_id = h.start_child(&python).await;
        h.wait_awaiting(job_id).await;

        let denied = h
            .provide(job_id, true)
            .await
            .expect_err("an MCP-labelled peer must never type a password");
        assert_eq!(denied.code, IpcErrorCode::PolicyDenied);
        assert!(
            denied.message.contains("credential_request"),
            "{}",
            denied.message
        );
        h.wait_awaiting(job_id).await;

        // The in-process test seam stands in for the admin CLI image here;
        // the real binary is driven in crates/cli/tests/credential_provide.rs.
        match h.provide(job_id, false).await.expect("admin provide") {
            IpcResponse::CredentialProvide(r) => assert_eq!(r.job_id, job_id),
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(h.wait_exit_code(job_id).await, Some(0));
        // A later credential_request for that prompt reports the CLI answer.
        assert_eq!(h.request(job_id).await.0, CredentialStatus::Provided);

        let rows = h.credential_audit_rows().await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        let meta: serde_json::Value = serde_json::from_str(&rows[0].1).unwrap();
        assert_eq!(meta, serde_json::json!({"kind": "sudo", "source": "cli"}));

        h.assert_secret_never_surfaced(job_id).await;
    });
}

#[test]
fn credential_request_for_a_job_without_a_prompt_is_not_awaiting() {
    let Some(python) = python() else {
        eprintln!("skipping: python not on PATH");
        return;
    };
    rt().block_on(async {
        let mut h = Harness::new("idle", &format!("test:{SECRET}"));
        let r = h
            .call(IpcRequest::PtyCommandStart(PtyCommandStartParams {
                environment: None,
                argv: vec![
                    python.clone(),
                    "-c".to_owned(),
                    "import time; time.sleep(30)".to_owned(),
                ],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                rows: None,
                cols: None,
                tag: None,
            }))
            .await
            .expect("start");
        let IpcResponse::PtyCommandStart(s) = r else {
            panic!("unexpected: {r:?}");
        };
        assert_eq!(
            h.request(s.job_id).await,
            (CredentialStatus::NotAwaiting, None)
        );
        let err = h
            .provide(s.job_id, false)
            .await
            .expect_err("nothing to answer");
        assert!(
            err.message.contains("not waiting for a password"),
            "{}",
            err.message
        );
        h.stop(s.job_id).await;
        let unknown = h
            .call(IpcRequest::CredentialRequest(CredentialRequestParams {
                job_id: JobId::new(),
                wait_ms: None,
            }))
            .await
            .expect_err("unknown job");
        assert_eq!(unknown.code, IpcErrorCode::UnknownJob);
    });
}
