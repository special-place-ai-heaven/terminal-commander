// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! `credential_request` through MCP with URL-mode elicitation. The test
//! client plays the MCP client AND the owner's browser: it receives the
//! `elicitation/create` (URL mode), accepts, then GETs and POSTs the
//! one-shot loopback page. A client without the capability gets the
//! daemon's own chain instead (here the admin-CLI fallback).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, CustomNotification,
    ElicitRequestParams, ElicitResult, ElicitationAction, Implementation,
};
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::{ClientHandler, ClientServiceExt, ErrorData as McpError, RoleClient, ServiceExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;
use terminal_commanderd::{DaemonConfig, DaemonState, IpcServer, ServerHandle};

const SECRET: &str = "s3cret-marker-K7";

/// Prints a sudo-style prompt and exits 0 only if the next line is the
/// secret (compared reversed, so the secret is never in argv).
const CHILD: &str = "import sys\n\
sys.stdout.write('[sudo] password for dev: ')\n\
sys.stdout.flush()\n\
line = sys.stdin.readline().rstrip('\\r\\n')\n\
ok = line[::-1] == '7K-rekram-terc3s'\n\
print('MARKER-OK' if ok else 'MARKER-BAD')\n\
sys.exit(0 if ok else 3)\n";

/// How the client answers `elicitation/create`.
#[derive(Clone, Copy, Default)]
enum Reply {
    /// Accept, then the owner opens the page and later POSTs the answer.
    #[default]
    Accept,
    /// Accept, but nobody ever opens the link: the client showed nothing.
    AcceptNeverOpen,
    /// Never answer: the client showed the owner nothing.
    Silent,
    Decline,
    Error,
}

#[derive(Clone, Default)]
struct OwnerClient {
    url_mode: bool,
    reply: Reply,
    /// Every page URL the client received, to probe it after a fallback.
    urls: Arc<std::sync::Mutex<Vec<String>>>,
    elicitations: Arc<AtomicUsize>,
    /// Every elicitation message, for the assertion on the owner text.
    messages: Arc<std::sync::Mutex<Vec<String>>>,
    completed: Arc<AtomicBool>,
    /// How long the owner takes after accepting (at least 200 ms).
    answer_after: Duration,
}

impl ClientHandler for OwnerClient {
    fn get_info(&self) -> ClientConfig {
        let caps: ClientCapabilities = if self.url_mode {
            serde_json::from_value(serde_json::json!({"elicitation": {"form": {}, "url": {}}}))
                .unwrap()
        } else {
            ClientCapabilities::default()
        };
        ClientConfig::new(caps, Implementation::new("tc-owner-test", "0.0.0"))
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, McpError> {
        self.elicitations.fetch_add(1, Ordering::SeqCst);
        let ElicitRequestParams::UrlElicitationParams { message, url, .. } = request else {
            return Ok(ElicitResult::new(ElicitationAction::Decline));
        };
        self.messages.lock().unwrap().push(message);
        self.urls.lock().unwrap().push(url.clone());
        match self.reply {
            Reply::Accept => {}
            Reply::AcceptNeverOpen => return Ok(ElicitResult::new(ElicitationAction::Accept)),
            Reply::Silent => std::future::pending::<()>().await,
            Reply::Decline => return Ok(ElicitResult::new(ElicitationAction::Decline)),
            Reply::Error => return Err(McpError::internal_error("no dialog", None)),
        }
        // The browser opens the link at once; the owner answers later.
        let answer_after = self.answer_after.max(Duration::from_millis(200));
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let (status, page) = http(&url, "GET", "").await;
            assert_eq!(status, 200, "{page}");
            tokio::time::sleep(answer_after).await;
            let (status, done) = http(&url, "POST", &format!("password={SECRET}")).await;
            assert_eq!(status, 200, "{done}");
        });
        Ok(ElicitResult::new(ElicitationAction::Accept))
    }

    async fn on_custom_notification(
        &self,
        notification: CustomNotification,
        _context: NotificationContext<RoleClient>,
    ) {
        if notification.method == "notifications/elicitation/complete" {
            self.completed.store(true, Ordering::SeqCst);
        }
    }
}

async fn http(url: &str, method: &str, body: &str) -> (u16, String) {
    let rest = url.strip_prefix("http://").expect("http url");
    let (addr, path) = rest.split_at(rest.find('/').expect("path"));
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: \
         application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.expect("read");
    let status = raw
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b).to_owned();
    (status, body)
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("tc-mcp-cred-{tag}-{}-{nanos}", std::process::id()))
}

fn python3() -> Option<&'static str> {
    ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

/// `none`: the daemon has no native prompt, so only the page (or the CLI
/// fallback) can answer. The seam also admits this in-process adapter to
/// `credential_url` (its image is the test binary, not the MCP adapter).
fn spawn_daemon(data: &std::path::Path) -> ServerHandle {
    spawn_daemon_with(data, "none", None)
}

/// `prompter` stands in for the native prompt; `open_within` shortens how
/// long an accepted page may go unopened.
fn spawn_daemon_with(
    data: &std::path::Path,
    prompter: &str,
    open_within: Option<Duration>,
) -> ServerHandle {
    let mut cfg = DaemonConfig::defaults_in(data);
    cfg.credential_prompter_test_seam = Some(prompter.to_owned());
    cfg.credential_page_open_test_seam = open_within;
    let state = Arc::new(DaemonState::bootstrap(cfg).expect("daemon bootstrap"));
    let socket = state.config.socket_path();
    IpcServer::new(state, socket)
        .spawn()
        .expect("ipc server spawn")
}

async fn connect(
    handle: &ServerHandle,
    client: OwnerClient,
) -> (
    rmcp::service::RunningService<rmcp::RoleServer, TerminalCommanderMcpServer>,
    rmcp::service::RunningService<RoleClient, OwnerClient>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let daemon = McpDaemonClient::new(handle.socket_path().to_path_buf())
        .with_timeout(Duration::from_secs(5));
    let server = TerminalCommanderMcpServer::new(daemon);
    let server_task =
        tokio::spawn(async move { server.serve(server_transport).await.expect("server serve") });
    let client = client
        .serve_with_lifecycle(
            client_transport,
            rmcp::ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("client serve");
    (server_task.await.expect("server join"), client)
}

async fn call(
    client: &rmcp::service::RunningService<RoleClient, OwnerClient>,
    name: &'static str,
    args: serde_json::Value,
) -> String {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = args {
        params.arguments = Some(map);
    }
    let result = client
        .call_tool(params)
        .await
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .expect("text content")
}

/// Start the prompting child and wait until the daemon reports it.
async fn start_prompting_child(
    client: &rmcp::service::RunningService<RoleClient, OwnerClient>,
    python: &str,
) -> String {
    let start: serde_json::Value = serde_json::from_str(
        &call(
            client,
            "pty_command_start",
            serde_json::json!({"argv": [python, "-u", "-c", CHILD]}),
        )
        .await,
    )
    .unwrap();
    let job_id = start["job_id"].as_str().unwrap().to_owned();
    for _ in 0..200 {
        let list: serde_json::Value =
            serde_json::from_str(&call(client, "pty_command_list", serde_json::json!({})).await)
                .unwrap();
        let awaiting = list["entries"]
            .as_array()
            .and_then(|e| e.iter().find(|e| e["job_id"] == job_id.as_str()))
            .is_some_and(|e| e["awaiting_credential"]["kind"] == "sudo");
        if awaiting {
            return job_id;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job never reported awaiting_credential");
}

async fn exit_code(
    client: &rmcp::service::RunningService<RoleClient, OwnerClient>,
    job_id: &str,
) -> Option<i64> {
    for _ in 0..200 {
        let s: serde_json::Value = serde_json::from_str(
            &call(
                client,
                "command_status",
                serde_json::json!({"job_id": job_id}),
            )
            .await,
        )
        .unwrap();
        if s["state"] == "exited" || s["state"] == "failed" {
            return s["exit_code"].as_i64();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn url_elicitation_client_gets_the_owner_page_and_the_model_only_a_status() {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let data = tmp_data_dir("url");
    let handle = spawn_daemon(&data);
    {
        let owner = OwnerClient {
            url_mode: true,
            ..OwnerClient::default()
        };
        let (_server, client) = connect(&handle, owner.clone()).await;
        let job_id = start_prompting_child(&client, python).await;

        let result = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["status"], "provided", "{result}");
        // The model sees a status only: no secret, no page URL or token.
        assert!(!result.contains(SECRET), "{result}");
        assert!(!result.contains("http://"), "{result}");
        assert_eq!(owner.elicitations.load(Ordering::SeqCst), 1);
        assert_eq!(
            owner.messages.lock().unwrap().as_slice(),
            [format!(
                "TC needs the owner's password for sudo in job {job_id}"
            )]
        );
        assert_eq!(exit_code(&client, &job_id).await, Some(0));
        for _ in 0..40 {
            if owner.completed.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            owner.completed.load(Ordering::SeqCst),
            "notifications/elicitation/complete must close the client's waiting state"
        );

        // A repeat call replays the outcome, never re-elicits.
        let again = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        assert!(
            again.contains("provided") || again.contains("not_awaiting"),
            "{again}"
        );
        assert_eq!(owner.elicitations.load(Ordering::SeqCst), 1);

        let tail = call(
            &client,
            "command_output_tail",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        assert!(!tail.contains(SECRET), "{tail}");
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

/// S1: an owner who has not answered yet does not hold the tool call for
/// minutes. The call answers `pending` after ~10 s, the dialog and page stay
/// up, and a later poll reports `provided` without a second elicitation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_owner_gets_pending_then_provided_on_a_later_poll() {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let data = tmp_data_dir("pending");
    let handle = spawn_daemon(&data);
    {
        let owner = OwnerClient {
            url_mode: true,
            answer_after: Duration::from_secs(13),
            ..OwnerClient::default()
        };
        let (_server, client) = connect(&handle, owner.clone()).await;
        let job_id = start_prompting_child(&client, python).await;

        let started = std::time::Instant::now();
        let first = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let waited = started.elapsed();
        let v: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(v["status"], "pending", "{first}");
        assert!(waited < Duration::from_secs(13), "one call took {waited:?}");
        assert!(!first.contains("http://"), "{first}");

        let mut status = String::new();
        for _ in 0..5 {
            let again = call(
                &client,
                "credential_request",
                serde_json::json!({"job_id": job_id}),
            )
            .await;
            let v: serde_json::Value = serde_json::from_str(&again).unwrap();
            status = v["status"].as_str().unwrap_or_default().to_owned();
            if status != "pending" {
                break;
            }
        }
        assert_eq!(status, "provided");
        assert_eq!(
            owner.elicitations.load(Ordering::SeqCst),
            1,
            "polls never re-elicit"
        );
        assert_eq!(exit_code(&client, &job_id).await, Some(0));
        for _ in 0..40 {
            if owner.completed.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(owner.completed.load(Ordering::SeqCst));
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_without_url_elicitation_falls_back_to_the_daemon_chain() {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let data = tmp_data_dir("no-url");
    let handle = spawn_daemon(&data);
    {
        let owner = OwnerClient::default();
        let (_server, client) = connect(&handle, owner.clone()).await;
        let job_id = start_prompting_child(&client, python).await;
        let result = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["status"], "owner_action_required", "{result}");
        // The owner's terminal has no TC_SESSION: the command names this
        // daemon's endpoint.
        assert_eq!(
            v["command"],
            format!(
                "terminal-commander credential provide {job_id} --socket {}",
                handle.socket_path().display()
            )
        );
        assert_eq!(owner.elicitations.load(Ordering::SeqCst), 0);
        let _ = call(
            &client,
            "pty_command_stop",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

/// True once nothing serves the page any more: refused, or not a 200.
async fn page_is_closed(url: &str) -> bool {
    let rest = url.strip_prefix("http://").expect("http url");
    let addr = &rest[..rest.find('/').expect("path")];
    if tokio::net::TcpStream::connect(addr).await.is_err() {
        return true;
    }
    http(url, "GET", "").await.0 != 200
}

/// A client that declared URL elicitation but does not take it (never
/// answers, declines, or errors) must not leave the owner with nothing to
/// answer while the model polls `pending`: the same call falls back to the
/// daemon's chain (here no native prompt, so the CLI command), the
/// abandoned page is closed, and later polls never re-elicit.
async fn untaken_elicitation_falls_back(tag: &str, reply: Reply) {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let data = tmp_data_dir(tag);
    let handle = spawn_daemon(&data);
    {
        let owner = OwnerClient {
            url_mode: true,
            reply,
            ..OwnerClient::default()
        };
        let (_server, client) = connect(&handle, owner.clone()).await;
        let job_id = start_prompting_child(&client, python).await;

        let first = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(v["status"], "owner_action_required", "{first}");
        assert_eq!(
            v["command"],
            format!(
                "terminal-commander credential provide {job_id} --socket {}",
                handle.socket_path().display()
            )
        );
        assert_eq!(owner.elicitations.load(Ordering::SeqCst), 1);
        let url = owner.urls.lock().unwrap()[0].clone();
        assert!(page_is_closed(&url).await, "the abandoned page must close");

        let again = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        assert!(again.contains("owner_action_required"), "{again}");
        assert_eq!(
            owner.elicitations.load(Ordering::SeqCst),
            1,
            "polls never re-elicit"
        );
        // The model never sees the page: no URL, token, or elicitation id.
        let token = url.rsplit('/').next().unwrap();
        for result in [&first, &again] {
            assert!(!result.contains("http://"), "{result}");
            assert!(!result.contains(token), "{result}");
            assert!(!result.contains(SECRET), "{result}");
        }
        let _ = call(
            &client,
            "pty_command_stop",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_never_answers_the_elicitation_falls_back_in_the_same_call() {
    untaken_elicitation_falls_back("silent", Reply::Silent).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declined_elicitation_falls_back_in_the_same_call() {
    untaken_elicitation_falls_back("decline", Reply::Decline).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_error_on_the_elicitation_falls_back_in_the_same_call() {
    untaken_elicitation_falls_back("error", Reply::Error).await;
}

/// The residual gap: a client that ACCEPTS the elicitation but never shows
/// the link. Nobody opens the page, so after the open deadline the daemon
/// closes it and asks through the native prompt (here answering), without
/// the model polling `pending` for five minutes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_accepted_link_nobody_opens_falls_back_to_the_native_prompt() {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let data = tmp_data_dir("never-open");
    let handle = spawn_daemon_with(
        &data,
        &format!("test:{SECRET}"),
        Some(Duration::from_millis(500)),
    );
    {
        let owner = OwnerClient {
            url_mode: true,
            reply: Reply::AcceptNeverOpen,
            ..OwnerClient::default()
        };
        let (_server, client) = connect(&handle, owner.clone()).await;
        let job_id = start_prompting_child(&client, python).await;

        let mut results = Vec::new();
        let mut status = String::new();
        for _ in 0..5 {
            let r = call(
                &client,
                "credential_request",
                serde_json::json!({"job_id": job_id}),
            )
            .await;
            let v: serde_json::Value = serde_json::from_str(&r).unwrap();
            status = v["status"].as_str().unwrap_or_default().to_owned();
            results.push(r);
            if status != "pending" {
                break;
            }
        }
        // The native seam typed the answer: the child exits 0.
        assert_eq!(status, "provided", "{results:?}");
        assert_eq!(exit_code(&client, &job_id).await, Some(0));
        assert_eq!(owner.elicitations.load(Ordering::SeqCst), 1);
        // The abandoned link is dead: the late URL cannot be used.
        let url = owner.urls.lock().unwrap()[0].clone();
        assert!(page_is_closed(&url).await, "the unopened page must close");
        for _ in 0..40 {
            if owner.completed.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            owner.completed.load(Ordering::SeqCst),
            "the client's waiting state must be closed"
        );
        let token = url.rsplit('/').next().unwrap();
        for r in &results {
            assert!(!r.contains("http://") && !r.contains(token), "{r}");
            assert!(!r.contains(SECRET), "{r}");
        }
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}

/// An opened page keeps its normal life: the owner is looking at it. The
/// native prompt (here answering WRONG, so using it would fail the child)
/// is never used, even well past the open deadline.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_opened_page_stays_pending_and_never_reaches_the_native_prompt() {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not found");
        return;
    };
    let data = tmp_data_dir("opened");
    let handle = spawn_daemon_with(&data, "test:wrong-answer", Some(Duration::from_millis(500)));
    {
        let owner = OwnerClient {
            url_mode: true,
            // Past the first call's 10 s page wait, so it must say `pending`.
            answer_after: Duration::from_secs(12),
            ..OwnerClient::default()
        };
        let (_server, client) = connect(&handle, owner.clone()).await;
        let job_id = start_prompting_child(&client, python).await;
        let first = call(
            &client,
            "credential_request",
            serde_json::json!({"job_id": job_id}),
        )
        .await;
        assert!(first.contains("\"pending\""), "{first}");
        let mut status = String::new();
        for _ in 0..5 {
            let r = call(
                &client,
                "credential_request",
                serde_json::json!({"job_id": job_id}),
            )
            .await;
            let v: serde_json::Value = serde_json::from_str(&r).unwrap();
            status = v["status"].as_str().unwrap_or_default().to_owned();
            if status != "pending" {
                break;
            }
        }
        assert_eq!(status, "provided");
        assert_eq!(
            exit_code(&client, &job_id).await,
            Some(0),
            "the page answered, not the native seam"
        );
        let _ = client.cancel().await;
    }
    handle.shutdown().await;
    let _ = std::fs::remove_dir_all(&data);
}
