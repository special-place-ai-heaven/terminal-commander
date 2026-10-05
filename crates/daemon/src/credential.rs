// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Owner credential elicitation for PTY password prompts.
//!
//! TC44 is unchanged: `pty_command_write_stdin` never types into a secret
//! prompt. `credential_request` asks the OWNER instead, through a channel
//! the model cannot read, in this order: a one-shot loopback page the MCP
//! client opens through URL-mode elicitation (when the client supports
//! it); a prompt the daemon opens itself (Windows CredUI; `$SSH_ASKPASS`,
//! `ssh-askpass`, `zenity`, `kdialog`, or `pinentry` on a unix desktop);
//! else the admin CLI (`terminal-commander credential provide <job_id>`).
//! The daemon types the answer into the PTY; the model only ever sees a
//! [`CredentialStatus`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::JobId;
use terminal_commander_ipc::protocol::{
    CredentialKind, CredentialRequestResponse, CredentialStatus, CredentialUrlOp,
    CredentialUrlResponse, owner_prompt_text,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::pty_command::{CredentialPrompt, PtyRuntime, PtyRuntimeError};

/// How long one `credential_request` waits for the owner before answering
/// `timeout`. The owner prompt stays open; a repeat call waits on it again.
pub const CREDENTIAL_WAIT: Duration =
    Duration::from_millis(terminal_commander_ipc::protocol::CREDENTIAL_REQUEST_WAIT_MS);

/// The command the owner runs when no native prompt is available.
#[must_use]
pub fn provide_command(job_id: JobId) -> String {
    format!(
        "terminal-commander credential provide {}",
        job_id.to_wire_string()
    )
}

/// How long the owner's loopback page stays open.
pub const CREDENTIAL_URL_TTL: Duration =
    Duration::from_millis(terminal_commander_ipc::protocol::CREDENTIAL_URL_TTL_MS);

type Outcome = Arc<watch::Sender<Option<CredentialStatus>>>;

/// The generation the owner was asked about, its outcome (`None` while the
/// owner is still being asked), and the loopback page when that is the
/// channel.
struct Ask {
    generation: u64,
    outcome: Outcome,
    page: Option<Page>,
}

#[derive(Clone)]
struct Page {
    url: String,
    elicitation_id: String,
}

/// One owner prompt per PTY prompt generation. A repeat request replays or
/// re-attaches, never re-asks.
pub struct CredentialBroker {
    /// `DaemonConfig::credential_prompter_test_seam`.
    prompter: Option<String>,
    /// Owner page lifetime ([`CREDENTIAL_URL_TTL`] outside tests).
    url_ttl: Duration,
    asked: parking_lot::Mutex<HashMap<JobId, Ask>>,
}

impl std::fmt::Debug for CredentialBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialBroker").finish_non_exhaustive()
    }
}

impl CredentialBroker {
    #[must_use]
    pub fn new(prompter: Option<String>, url_ttl: Duration) -> Self {
        Self {
            prompter,
            url_ttl,
            asked: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Ask the owner for the password `job_id` is waiting on and wait up to
    /// [`CREDENTIAL_WAIT`] for the outcome.
    pub async fn request(
        &self,
        pty: &Arc<PtyRuntime>,
        job_id: JobId,
        wait: Duration,
    ) -> Result<CredentialRequestResponse, PtyRuntimeError> {
        let prompt = pty.credential_prompt(job_id).await?;
        let mut on_page = false;
        let mut rx = {
            let mut asked = self.asked.lock();
            prune(&mut asked, pty);
            match asked.get(&job_id) {
                Some(ask) if ask.generation == prompt.generation => {
                    on_page = ask.page.is_some();
                    ask.outcome.subscribe()
                }
                _ => {
                    let Some(awaiting) = prompt.awaiting else {
                        return Ok(response(job_id, CredentialStatus::NotAwaiting));
                    };
                    let outcome: Outcome = Arc::new(watch::channel(None).0);
                    asked.insert(
                        job_id,
                        Ask {
                            generation: prompt.generation,
                            outcome: Arc::clone(&outcome),
                            page: None,
                        },
                    );
                    let rx = outcome.subscribe();
                    let text = PromptText::new(job_id, awaiting.kind, &prompt);
                    spawn_owner_prompt(
                        Arc::clone(pty),
                        self.prompter.clone(),
                        job_id,
                        prompt.generation,
                        text,
                        outcome,
                    );
                    rx
                }
            }
        };
        let unanswered = if on_page {
            CredentialStatus::Pending
        } else {
            CredentialStatus::Timeout
        };
        let status =
            match tokio::time::timeout(wait.min(CREDENTIAL_WAIT), rx.wait_for(Option::is_some))
                .await
            {
                Ok(Ok(done)) => done.unwrap_or(unanswered),
                _ => unanswered,
            };
        Ok(response(job_id, status))
    }

    /// Record an answer that arrived through the admin CLI, so a pending or
    /// repeat `credential_request` for that prompt reports `provided`.
    pub fn record_provided(&self, job_id: JobId, generation: u64) {
        let mut asked = self.asked.lock();
        match asked.get(&job_id) {
            Some(ask) if ask.generation == generation => {
                ask.outcome.send_replace(Some(CredentialStatus::Provided));
            }
            _ => {
                let outcome = Arc::new(watch::channel(Some(CredentialStatus::Provided)).0);
                asked.insert(
                    job_id,
                    Ask {
                        generation,
                        outcome,
                        page: None,
                    },
                );
            }
        }
    }

    /// URL-mode elicitation, adapter side. `Open` starts the loopback page
    /// for the job's current prompt (or re-finds it); `Declined` makes the
    /// owner's refusal final for that prompt; `Abandon` closes the page so
    /// `request` falls back to the native prompt. A later `request` for
    /// the same prompt waits on the page's outcome.
    pub async fn url(
        self: &Arc<Self>,
        pty: &Arc<PtyRuntime>,
        job_id: JobId,
        op: CredentialUrlOp,
    ) -> Result<CredentialUrlResponse, PtyRuntimeError> {
        let prompt = pty.credential_prompt(job_id).await?;
        let mut resp = CredentialUrlResponse {
            job_id,
            url: None,
            elicitation_id: None,
            kind: prompt.awaiting.map(|a| a.kind),
            fresh: false,
        };
        let mut asked = self.asked.lock();
        prune(&mut asked, pty);
        let current = asked
            .get(&job_id)
            .filter(|ask| ask.generation == prompt.generation);
        match op {
            CredentialUrlOp::Open => {
                if let Some(ask) = current {
                    if let Some(page) = &ask.page {
                        resp.url = Some(page.url.clone());
                        resp.elicitation_id = Some(page.elicitation_id.clone());
                    }
                    return Ok(resp);
                }
                let Some(awaiting) = prompt.awaiting else {
                    return Ok(resp);
                };
                // No listener: `request` still has the native prompt and CLI.
                let Ok(listener) = bind_loopback() else {
                    return Ok(resp);
                };
                let Ok(addr) = listener.local_addr() else {
                    return Ok(resp);
                };
                // Two v4 UUIDs: 244 random bits from the OS CSPRNG.
                let token = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                let page = Page {
                    url: format!("http://{addr}/{token}"),
                    elicitation_id: uuid::Uuid::new_v4().simple().to_string(),
                };
                let outcome: Outcome = Arc::new(watch::channel(None).0);
                asked.insert(
                    job_id,
                    Ask {
                        generation: prompt.generation,
                        outcome: Arc::clone(&outcome),
                        page: Some(page.clone()),
                    },
                );
                drop(asked);
                tokio::spawn(serve_page(
                    Arc::clone(self),
                    Arc::clone(pty),
                    PageJob {
                        job_id,
                        generation: prompt.generation,
                        host: addr.to_string(),
                        token,
                        text: PromptText::new(job_id, awaiting.kind, &prompt),
                        ttl: self.url_ttl,
                    },
                    listener,
                    outcome,
                ));
                resp.url = Some(page.url);
                resp.elicitation_id = Some(page.elicitation_id);
                resp.fresh = true;
            }
            CredentialUrlOp::Declined => {
                if let Some(ask) = current.filter(|ask| ask.page.is_some()) {
                    settle(&ask.outcome, CredentialStatus::Declined);
                }
            }
            CredentialUrlOp::Abandon => {
                if current.is_some_and(|ask| ask.page.is_some() && ask.outcome.borrow().is_none())
                    && let Some(ask) = asked.remove(&job_id)
                {
                    // Stops the listener; the entry is gone, so the next
                    // `request` opens the native prompt.
                    settle(&ask.outcome, CredentialStatus::NotAwaiting);
                }
            }
        }
        Ok(resp)
    }

    /// An expired page: forget it so the next `Open` starts a fresh one.
    fn forget_page(&self, job_id: JobId, outcome: &Outcome) {
        let mut asked = self.asked.lock();
        if asked
            .get(&job_id)
            .is_some_and(|ask| Arc::ptr_eq(&ask.outcome, outcome))
        {
            asked.remove(&job_id);
        }
    }
}

// ponytail: prune on request, linear in live jobs; a job-exit hook if PTY job
// counts grow.
fn prune(asked: &mut HashMap<JobId, Ask>, pty: &PtyRuntime) {
    let live = pty.live_jobs();
    asked.retain(|id, _| live.iter().any(|l| l.job_id == *id));
}

/// First outcome wins.
fn settle(outcome: &Outcome, status: CredentialStatus) {
    outcome.send_if_modified(|current| {
        let unset = current.is_none();
        if unset {
            *current = Some(status);
        }
        unset
    });
}

fn response(job_id: JobId, status: CredentialStatus) -> CredentialRequestResponse {
    CredentialRequestResponse {
        job_id,
        status,
        command: (status == CredentialStatus::OwnerActionRequired).then(|| provide_command(job_id)),
    }
}

fn spawn_owner_prompt(
    pty: Arc<PtyRuntime>,
    prompter: Option<String>,
    job_id: JobId,
    generation: u64,
    text: PromptText,
    outcome: Outcome,
) {
    tokio::spawn(async move {
        // A detached thread, not `spawn_blocking`: an owner prompt can stay
        // open indefinitely, and the runtime waits for blocking-pool tasks on
        // shutdown, so an unanswered dialog must not hold the daemon up.
        let (tx, rx) = tokio::sync::oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("tc-owner-prompt".to_owned())
            .spawn(move || {
                let _ = tx.send(ask_owner(prompter.as_deref(), &text));
            });
        let asked = match spawned {
            Ok(_) => rx.await.unwrap_or(Asked::Unavailable),
            Err(_) => Asked::Unavailable,
        };
        let status = match asked {
            Asked::Secret(secret) => {
                let delivered = pty
                    .deliver_credential(job_id, &secret.0, Some(generation), "native")
                    .await;
                // A late answer to a prompt that is gone is dropped, not
                // typed into whatever the job shows now.
                if delivered.is_ok() {
                    CredentialStatus::Provided
                } else {
                    CredentialStatus::NotAwaiting
                }
            }
            Asked::Declined => CredentialStatus::Declined,
            Asked::Unavailable => CredentialStatus::OwnerActionRequired,
        };
        // A CLI answer that landed first wins.
        settle(&outcome, status);
    });
}

// ---------------------------------------------------------------------
// The owner's loopback page (URL-mode elicitation).
//
// Plain HTTP on 127.0.0.1 only, on a random port, while one prompt is
// pending: the path carries a single-use random token, the Host header must
// name that exact address (DNS rebinding), and the page closes after one
// answer or `CREDENTIAL_URL_TTL`. Loopback traffic never leaves the
// machine, so there is no TLS.

/// Request head plus the form body; the buffer never grows past this, so no
/// copy of the password is left behind by a reallocation.
const MAX_HTTP_REQUEST: usize = 16 * 1024;
/// ponytail: one connection at a time, each bounded by this; a per-connection
/// task if owners ever share the page.
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn bind_loopback() -> std::io::Result<TcpListener> {
    let std_listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    std_listener.set_nonblocking(true)?;
    TcpListener::from_std(std_listener)
}

struct PageJob {
    job_id: JobId,
    generation: u64,
    /// `127.0.0.1:<port>`, the only accepted Host header.
    host: String,
    token: String,
    text: PromptText,
    ttl: Duration,
}

async fn serve_page(
    broker: Arc<CredentialBroker>,
    pty: Arc<PtyRuntime>,
    job: PageJob,
    listener: TcpListener,
    outcome: Outcome,
) {
    let mut settled = outcome.subscribe();
    let expiry = tokio::time::sleep(job.ttl);
    tokio::pin!(expiry);
    loop {
        let mut stream = tokio::select! {
            () = &mut expiry => {
                settle(&outcome, CredentialStatus::Timeout);
                broker.forget_page(job.job_id, &outcome);
                return;
            }
            // Declined, abandoned, or answered through another channel.
            _ = settled.wait_for(Option::is_some) => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        let Ok(Some(secret)) =
            tokio::time::timeout(HTTP_REQUEST_TIMEOUT, read_answer(&mut stream, &job)).await
        else {
            continue;
        };
        let delivered = pty
            .deliver_credential(job.job_id, &secret.0, Some(job.generation), "url")
            .await;
        drop(secret);
        let id = job.job_id.to_wire_string();
        if delivered.is_ok() {
            settle(&outcome, CredentialStatus::Provided);
            let _ = reply(
                &mut stream,
                "200 OK",
                &format!("Sent to job {id}. You can close this tab."),
            )
            .await;
        } else {
            settle(&outcome, CredentialStatus::NotAwaiting);
            let _ = reply(
                &mut stream,
                "409 Conflict",
                &format!("Job {id} is no longer waiting for this password; nothing was typed."),
            )
            .await;
        }
        // Single use: the listener closes when this task returns.
        return;
    }
}

/// Serve one request. Returns the typed password for a valid POST; answers
/// everything else itself.
async fn read_answer(stream: &mut TcpStream, job: &PageJob) -> Option<SecretBuf> {
    let mut buf = SecretBuf(Vec::with_capacity(MAX_HTTP_REQUEST));
    let mut chunk = SecretBuf(vec![0; 2048]);
    let head_len = loop {
        if let Some(i) = buf.0.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if !read_more(stream, &mut buf, &mut chunk).await? {
            let _ = reply(stream, "431 Request Header Fields Too Large", "Too large.").await;
            return None;
        }
    };
    let head = std::str::from_utf8(&buf.0[..head_len]).ok()?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let (method, target) = (request_line.next()?, request_line.next()?);
    let mut host = None;
    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("host") {
                host = Some(value.trim());
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().ok()?;
            }
        }
    }
    if host != Some(job.host.as_str()) {
        let _ = reply(stream, "403 Forbidden", "Wrong host.").await;
        return None;
    }
    let path = target.split('?').next().unwrap_or_default();
    if !same_token(path.as_bytes(), format!("/{}", job.token).as_bytes()) {
        let _ = reply(stream, "404 Not Found", "Not found.").await;
        return None;
    }
    match method {
        "GET" => {
            let _ = reply_page(stream, &job.text, job.ttl).await;
            None
        }
        "POST" => {
            let total = head_len.checked_add(content_length)?;
            while buf.0.len() < total {
                if !read_more(stream, &mut buf, &mut chunk).await? {
                    let _ = reply(stream, "413 Content Too Large", "Too large.").await;
                    return None;
                }
            }
            form_value(buf.0.get(head_len..total)?, b"password")
        }
        _ => {
            let _ = reply(stream, "405 Method Not Allowed", "Not allowed.").await;
            None
        }
    }
}

/// Append one read to `buf`. `Some(false)`: the request outgrew the cap;
/// `None`: the peer closed or errored.
async fn read_more(
    stream: &mut TcpStream,
    buf: &mut SecretBuf,
    chunk: &mut SecretBuf,
) -> Option<bool> {
    let room = MAX_HTTP_REQUEST - buf.0.len();
    if room == 0 {
        return Some(false);
    }
    let want = room.min(chunk.0.len());
    let n = stream.read(&mut chunk.0[..want]).await.ok()?;
    if n == 0 {
        return None;
    }
    buf.0.extend_from_slice(&chunk.0[..n]);
    Some(true)
}

/// Constant-time token comparison.
fn same_token(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Decode one `application/x-www-form-urlencoded` field.
fn form_value(body: &[u8], key: &[u8]) -> Option<SecretBuf> {
    let raw = body.split(|b| *b == b'&').find_map(|pair| {
        let eq = pair.iter().position(|b| *b == b'=')?;
        (&pair[..eq] == key).then(|| &pair[eq + 1..])
    })?;
    let mut out = SecretBuf(Vec::with_capacity(raw.len()));
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'+' => out.0.push(b' '),
            b'%' => {
                let hex = std::str::from_utf8(raw.get(i + 1..i + 3)?).ok()?;
                out.0.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            b => out.0.push(b),
        }
        i += 1;
    }
    Some(out)
}

async fn reply_page(
    stream: &mut TcpStream,
    text: &PromptText,
    ttl: Duration,
) -> std::io::Result<()> {
    // `PromptText` is printable ASCII with `<`, `>` and `&` replaced, so it
    // is safe as HTML text.
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font:16px system-ui,sans-serif;margin:3em auto;max-width:36em}}\
         p{{white-space:pre-line}}</style><h1>{title}</h1><p>{message}</p>\
         <form method=\"post\"><input type=\"password\" name=\"password\" autofocus \
         autocomplete=\"off\" aria-label=\"Password\"> <button>Send</button></form>\
         <p>This page works once and closes after {secs} seconds.</p>",
        title = text.title,
        message = text.message,
        secs = ttl.as_secs(),
    );
    write_response(stream, "200 OK", &body).await
}

async fn reply(stream: &mut TcpStream, status: &str, message: &str) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Terminal Commander</title><p>{message}</p>"
    );
    write_response(stream, status, &body).await
}

async fn write_response(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; \
         style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await?;
    // Lingering close: wait for the client's FIN so closing with unread
    // request bytes does not reset the connection before it reads this.
    let mut sink = [0_u8; 512];
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {}
    })
    .await;
    Ok(())
}

pub(crate) fn wipe(buf: &mut [u8]) {
    buf.fill(0);
    std::hint::black_box(buf);
}

/// Owner-typed bytes, wiped on every drop path.
struct SecretBuf(Vec<u8>);

impl Drop for SecretBuf {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// What the owner prompt returned. The secret never leaves the daemon.
enum Asked {
    Secret(SecretBuf),
    Declined,
    /// No prompt could be shown; fall back to the admin CLI.
    Unavailable,
}

/// Owner-facing prompt text: [`owner_prompt_text`] (job id, prompt kind, the
/// program the daemon actually spawned, sanitized argv, and a warning when
/// the request overrode program resolution), so the owner can tell which
/// job is asking and what will receive the answer.
#[derive(Debug, Clone)]
struct PromptText {
    title: String,
    message: String,
}

impl PromptText {
    fn new(job_id: JobId, kind: CredentialKind, prompt: &CredentialPrompt) -> Self {
        Self {
            title: format!("Terminal Commander: {} password", kind.as_str()),
            message: owner_prompt_text(
                job_id,
                kind,
                &prompt.program,
                &prompt.argv,
                &prompt.program_env,
            ),
        }
    }
}

/// `prompter` is the test seam: `test:<secret>` answers, `test-decline`
/// cancels, `none` forces the CLI fallback. Unset: the native prompt.
fn ask_owner(prompter: Option<&str>, text: &PromptText) -> Asked {
    match prompter {
        None => native::ask(text),
        Some("test-decline") => Asked::Declined,
        Some(seam) => seam.strip_prefix("test:").map_or(Asked::Unavailable, |v| {
            Asked::Secret(SecretBuf(v.as_bytes().to_vec()))
        }),
    }
}

#[cfg(windows)]
mod native {
    #![allow(unsafe_code)]

    use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, NO_ERROR};
    use windows::Win32::Graphics::Gdi::HBITMAP;
    use windows::Win32::Security::Credentials::{
        CREDUI_FLAGS, CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST,
        CREDUI_FLAGS_GENERIC_CREDENTIALS, CREDUI_FLAGS_KEEP_USERNAME, CREDUI_INFOW,
        CREDUI_MAX_CAPTION_LENGTH, CREDUI_MAX_MESSAGE_LENGTH, CREDUI_MAX_USERNAME_LENGTH,
        CredUIPromptForCredentialsW,
    };
    use windows::core::PCWSTR;

    use super::{Asked, PromptText, SecretBuf};

    /// `CREDUI_MAX_PASSWORD_LENGTH` from wincred.h (not exported by the crate).
    const MAX_PASSWORD_LENGTH: usize = 256;

    /// Never saved to the credential store; always shown; the user-name
    /// field is fixed (it only labels the prompt).
    pub(super) const FLAGS: CREDUI_FLAGS = CREDUI_FLAGS(
        CREDUI_FLAGS_GENERIC_CREDENTIALS.0
            | CREDUI_FLAGS_DO_NOT_PERSIST.0
            | CREDUI_FLAGS_ALWAYS_SHOW_UI.0
            | CREDUI_FLAGS_KEEP_USERNAME.0,
    );

    /// NUL-terminated UTF-16, truncated to `max` units including the NUL.
    pub(super) fn wide(s: &str, max: u32) -> Vec<u16> {
        let limit = usize::try_from(max).unwrap_or(usize::MAX).saturating_sub(1);
        let mut w: Vec<u16> = s.encode_utf16().take(limit).collect();
        w.push(0);
        w
    }

    pub(super) fn ask(text: &PromptText) -> Asked {
        let caption = wide(&text.title, CREDUI_MAX_CAPTION_LENGTH);
        let message = wide(&text.message, CREDUI_MAX_MESSAGE_LENGTH);
        let target = wide("terminal-commander", CREDUI_MAX_CAPTION_LENGTH);
        let info = CREDUI_INFOW {
            cbSize: u32::try_from(std::mem::size_of::<CREDUI_INFOW>()).unwrap_or(0),
            hwndParent: HWND::default(),
            pszMessageText: PCWSTR(message.as_ptr()),
            pszCaptionText: PCWSTR(caption.as_ptr()),
            hbmBanner: HBITMAP::default(),
        };
        let mut user = wide("terminal-commander", CREDUI_MAX_USERNAME_LENGTH);
        user.resize(CREDUI_MAX_USERNAME_LENGTH as usize + 1, 0);
        let mut pass = vec![0u16; MAX_PASSWORD_LENGTH + 1];
        // SAFETY: `info` and every string point at live NUL-terminated
        // buffers owned by this frame; the user/password slices carry their
        // own lengths. The call is modal and returns before they drop.
        let rc = unsafe {
            CredUIPromptForCredentialsW(
                Some(&raw const info),
                PCWSTR(target.as_ptr()),
                None,
                0,
                &mut user,
                &mut pass,
                None,
                FLAGS,
            )
        };
        let asked = if rc == NO_ERROR {
            let n = pass.iter().position(|&c| c == 0).unwrap_or(pass.len());
            Asked::Secret(SecretBuf(String::from_utf16_lossy(&pass[..n]).into_bytes()))
        } else if rc == ERROR_CANCELLED {
            Asked::Declined
        } else {
            Asked::Unavailable
        };
        pass.fill(0);
        std::hint::black_box(&pass);
        asked
    }
}

#[cfg(unix)]
mod native {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use terminal_commander_core::as_daemon_child;

    use super::{Asked, PromptText, SecretBuf, wipe};

    /// First available of `$SSH_ASKPASS`, then (on a desktop) `ssh-askpass`,
    /// `zenity`, `kdialog`, `pinentry`. Each is spawned by the daemon with
    /// the prompt text only and prints the secret on stdout.
    pub(super) fn ask(text: &PromptText) -> Asked {
        let label = format!("{}\n{}", text.title, text.message);
        let desktop = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
            || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let mut helpers = Vec::new();
        if let Some(askpass) = std::env::var_os("SSH_ASKPASS").filter(|v| !v.is_empty()) {
            let mut c = Command::new(askpass);
            as_daemon_child(&mut c).arg(&label);
            helpers.push(c);
        }
        if desktop {
            let mut c = Command::new("ssh-askpass");
            as_daemon_child(&mut c).arg(&label);
            helpers.push(c);
            let mut c = Command::new("zenity");
            as_daemon_child(&mut c).args(["--password", "--title", &text.title]);
            helpers.push(c);
            let mut c = Command::new("kdialog");
            as_daemon_child(&mut c).args(["--title", &text.title, "--password", &text.message]);
            helpers.push(c);
        }
        for helper in helpers {
            if let Some(asked) = run_helper(helper) {
                return asked;
            }
        }
        if desktop && let Some(asked) = pinentry(text) {
            return asked;
        }
        Asked::Unavailable
    }

    /// `None` when the helper is missing or could not show a prompt (it
    /// wrote an error), so the next one is tried.
    ///
    /// ponytail: "nonzero exit + stderr" is read as "could not prompt";
    /// a helper that prints on cancel reads as unavailable, not declined.
    fn run_helper(mut helper: Command) -> Option<Asked> {
        let out = helper.stdin(Stdio::null()).output().ok()?;
        if out.status.success() {
            let mut secret = SecretBuf(out.stdout);
            while secret.0.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
                secret.0.pop();
            }
            return Some(Asked::Secret(secret));
        }
        let mut stdout = out.stdout;
        wipe(&mut stdout);
        out.stderr.is_empty().then_some(Asked::Declined)
    }

    /// Minimal Assuan exchange: `D <pin>` answers, a cancel `ERR` declines.
    fn pinentry(text: &PromptText) -> Option<Asked> {
        let mut child = as_daemon_child(&mut Command::new("pinentry"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let script = format!(
            "SETTITLE {}\nSETDESC {}\nSETPROMPT Password:\nGETPIN\nBYE\n",
            escape(&text.title),
            escape(&text.message)
        );
        let wrote = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(script.as_bytes()).is_ok());
        let out = child.wait_with_output().ok()?;
        let mut stdout = out.stdout;
        let asked = wrote
            .then(|| {
                stdout.split(|&b| b == b'\n').find_map(|line| {
                    line.strip_prefix(b"D ").map_or_else(
                        || {
                            let cancelled = line.starts_with(b"ERR")
                                && line.to_ascii_lowercase().windows(6).any(|w| w == b"cancel");
                            cancelled.then_some(Asked::Declined)
                        },
                        |pin| Some(Asked::Secret(SecretBuf(unescape(pin)))),
                    )
                })
            })
            .flatten();
        wipe(&mut stdout);
        asked
    }

    fn escape(s: &str) -> String {
        s.replace('%', "%25")
            .replace('\n', "%0A")
            .replace('\r', "%0D")
    }

    fn unescape(raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            let hex = raw
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            match (raw[i], hex) {
                (b'%', Some(byte)) => {
                    out.push(byte);
                    i += 3;
                }
                (b, _) => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn assuan_escaping_round_trips() {
            assert_eq!(super::escape("50%\nok"), "50%25%0Aok");
            assert_eq!(super::unescape(b"p%25w%0Ad"), b"p%w\nd".to_vec());
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod native {
    use super::{Asked, PromptText};

    pub(super) const fn ask(_text: &PromptText) -> Asked {
        Asked::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(argv: &[&str]) -> CredentialPrompt {
        CredentialPrompt {
            generation: 1,
            awaiting: None,
            argv: argv.iter().map(|a| (*a).to_owned()).collect(),
            program: "/usr/bin/sudo".to_owned(),
            program_env: Vec::new(),
        }
    }

    #[test]
    fn prompt_text_names_job_and_strips_markup() {
        let job = JobId::new();
        let text = PromptText::new(
            job,
            CredentialKind::Sudo,
            &prompt(&["sudo", "<b>x</b>\u{1b}[31m"]),
        );
        assert_eq!(text.title, "Terminal Commander: sudo password");
        assert!(text.message.contains(&job.to_wire_string()));
        assert!(text.message.contains("Program: /usr/bin/sudo"));
        assert!(text.message.contains("Command: sudo ?b?x?/b??[31m"));
    }

    #[test]
    fn test_seam_never_reaches_a_native_prompt() {
        let text = PromptText::new(JobId::new(), CredentialKind::Password, &prompt(&[]));
        assert!(matches!(ask_owner(Some("test:pw"), &text), Asked::Secret(s) if s.0 == b"pw"));
        assert!(matches!(
            ask_owner(Some("test-decline"), &text),
            Asked::Declined
        ));
        assert!(matches!(ask_owner(Some("none"), &text), Asked::Unavailable));
    }

    #[test]
    fn only_owner_action_required_carries_the_cli_command() {
        let job = JobId::new();
        let r = response(job, CredentialStatus::OwnerActionRequired);
        assert_eq!(r.command.as_deref(), Some(provide_command(job).as_str()));
        assert!(response(job, CredentialStatus::Declined).command.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn credui_arguments_are_bounded_and_never_persist() {
        use windows::Win32::Security::Credentials::{
            CREDUI_FLAGS_DO_NOT_PERSIST, CREDUI_FLAGS_GENERIC_CREDENTIALS,
            CREDUI_MAX_CAPTION_LENGTH,
        };
        let long = "x".repeat(500);
        let w = native::wide(&long, CREDUI_MAX_CAPTION_LENGTH);
        assert_eq!(w.len(), CREDUI_MAX_CAPTION_LENGTH as usize);
        assert_eq!(w.last(), Some(&0));
        assert!(native::FLAGS.contains(CREDUI_FLAGS_DO_NOT_PERSIST));
        assert!(native::FLAGS.contains(CREDUI_FLAGS_GENERIC_CREDENTIALS));
    }
}
