// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Owner credential elicitation for PTY password prompts.
//!
//! TC44 is unchanged: `pty_command_write_stdin` never types into a secret
//! prompt. `credential_request` asks the OWNER instead, through a channel
//! the model cannot read: a prompt the daemon opens itself (Windows CredUI;
//! `$SSH_ASKPASS`, `ssh-askpass`, `zenity`, `kdialog`, or `pinentry` on a
//! unix desktop), else the admin CLI (`terminal-commander credential
//! provide <job_id>`). The daemon types the answer into the PTY; the model
//! only ever sees a [`CredentialStatus`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::JobId;
use terminal_commander_ipc::protocol::{
    CredentialKind, CredentialRequestResponse, CredentialStatus,
};
use tokio::sync::watch;

use crate::pty_command::{PtyRuntime, PtyRuntimeError};

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

type Outcome = Arc<watch::Sender<Option<CredentialStatus>>>;

/// One owner prompt per PTY prompt generation.
///
/// Per job: the generation the owner was asked about and its outcome (`None`
/// while the owner prompt is open). A repeat request replays or re-attaches,
/// never re-asks.
pub struct CredentialBroker {
    /// `DaemonConfig::credential_prompter_test_seam`.
    prompter: Option<String>,
    asked: parking_lot::Mutex<HashMap<JobId, (u64, Outcome)>>,
}

impl std::fmt::Debug for CredentialBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialBroker").finish_non_exhaustive()
    }
}

impl CredentialBroker {
    #[must_use]
    pub fn new(prompter: Option<String>) -> Self {
        Self {
            prompter,
            asked: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Ask the owner for the password `job_id` is waiting on and wait up to
    /// [`CREDENTIAL_WAIT`] for the outcome.
    pub async fn request(
        &self,
        pty: &Arc<PtyRuntime>,
        job_id: JobId,
    ) -> Result<CredentialRequestResponse, PtyRuntimeError> {
        let prompt = pty.credential_prompt(job_id).await?;
        let mut rx = {
            let mut asked = self.asked.lock();
            // ponytail: prune on request, linear in live jobs; a job-exit hook
            // if PTY job counts grow.
            let live = pty.live_jobs();
            asked.retain(|id, _| live.iter().any(|l| l.job_id == *id));
            match asked.get(&job_id) {
                Some((generation, outcome)) if *generation == prompt.generation => {
                    outcome.subscribe()
                }
                _ => {
                    let Some(awaiting) = prompt.awaiting else {
                        return Ok(response(job_id, CredentialStatus::NotAwaiting));
                    };
                    let outcome: Outcome = Arc::new(watch::channel(None).0);
                    asked.insert(job_id, (prompt.generation, Arc::clone(&outcome)));
                    let rx = outcome.subscribe();
                    let text = PromptText::new(job_id, awaiting.kind, &prompt.argv);
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
        let status = match tokio::time::timeout(CREDENTIAL_WAIT, rx.wait_for(Option::is_some)).await
        {
            Ok(Ok(done)) => done.unwrap_or(CredentialStatus::Timeout),
            _ => CredentialStatus::Timeout,
        };
        Ok(response(job_id, status))
    }

    /// Record an answer that arrived through the admin CLI, so a pending or
    /// repeat `credential_request` for that prompt reports `provided`.
    pub fn record_provided(&self, job_id: JobId, generation: u64) {
        let mut asked = self.asked.lock();
        match asked.get(&job_id) {
            Some((g, outcome)) if *g == generation => {
                outcome.send_replace(Some(CredentialStatus::Provided));
            }
            _ => {
                let outcome = Arc::new(watch::channel(Some(CredentialStatus::Provided)).0);
                asked.insert(job_id, (generation, outcome));
            }
        }
    }
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
        let asked = tokio::task::spawn_blocking(move || ask_owner(prompter.as_deref(), &text))
            .await
            .unwrap_or(Asked::Unavailable);
        let status = match asked {
            Asked::Secret(mut secret) => {
                let delivered = pty
                    .deliver_credential(job_id, &secret, Some(generation), "native")
                    .await;
                wipe(&mut secret);
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
        outcome.send_if_modified(|current| {
            let unset = current.is_none();
            if unset {
                *current = Some(status);
            }
            unset
        });
    });
}

pub(crate) fn wipe(buf: &mut [u8]) {
    buf.fill(0);
    std::hint::black_box(buf);
}

/// What the owner prompt returned. The secret never leaves the daemon.
enum Asked {
    Secret(Vec<u8>),
    Declined,
    /// No prompt could be shown; fall back to the admin CLI.
    Unavailable,
}

/// Owner-facing prompt text. Built from the job id, prompt kind, and a
/// sanitized argv so the owner can tell which job is asking (a job the
/// model started can print a fake prompt; the owner decides).
#[derive(Debug, Clone)]
struct PromptText {
    title: String,
    message: String,
}

impl PromptText {
    fn new(job_id: JobId, kind: CredentialKind, argv: &[String]) -> Self {
        let kind = match kind {
            CredentialKind::Sudo => "sudo",
            CredentialKind::Ssh => "ssh",
            CredentialKind::Password => "password",
        };
        // Plain printable ASCII only: helpers such as zenity and kdialog
        // render markup, and argv is model-chosen.
        let mut command: String = argv
            .join(" ")
            .chars()
            .map(|c| {
                if (c.is_ascii_graphic() || c == ' ') && !matches!(c, '<' | '>' | '&') {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        if command.len() > 200 {
            command.truncate(197);
            command.push_str("...");
        }
        Self {
            title: format!("Terminal Commander: {kind} password"),
            message: format!(
                "Job {} is waiting for a {kind} password.\nCommand: {command}\n\
                 Terminal Commander types your answer into that job only; the AI model never \
                 sees it. Cancel if you did not expect this.",
                job_id.to_wire_string()
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
        Some(seam) => seam
            .strip_prefix("test:")
            .map_or(Asked::Unavailable, |v| Asked::Secret(v.as_bytes().to_vec())),
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

    use super::{Asked, PromptText};

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
            Asked::Secret(String::from_utf16_lossy(&pass[..n]).into_bytes())
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

    use super::{Asked, PromptText, wipe};

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
            c.arg(&label);
            helpers.push(c);
        }
        if desktop {
            let mut c = Command::new("ssh-askpass");
            c.arg(&label);
            helpers.push(c);
            let mut c = Command::new("zenity");
            c.args(["--password", "--title", &text.title]);
            helpers.push(c);
            let mut c = Command::new("kdialog");
            c.args(["--title", &text.title, "--password", &text.message]);
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
            let mut secret = out.stdout;
            while secret.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
                secret.pop();
            }
            return Some(Asked::Secret(secret));
        }
        let mut stdout = out.stdout;
        wipe(&mut stdout);
        out.stderr.is_empty().then_some(Asked::Declined)
    }

    /// Minimal Assuan exchange: `D <pin>` answers, a cancel `ERR` declines.
    fn pinentry(text: &PromptText) -> Option<Asked> {
        let mut child = Command::new("pinentry")
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
                    if let Some(pin) = line.strip_prefix(b"D ") {
                        Some(Asked::Secret(unescape(pin)))
                    } else if line.starts_with(b"ERR")
                        && line.to_ascii_lowercase().windows(6).any(|w| w == b"cancel")
                    {
                        Some(Asked::Declined)
                    } else {
                        None
                    }
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

    #[test]
    fn prompt_text_names_job_and_strips_markup() {
        let job = JobId::new();
        let text = PromptText::new(
            job,
            CredentialKind::Sudo,
            &["sudo".to_owned(), "<b>x</b>\u{1b}[31m".to_owned()],
        );
        assert_eq!(text.title, "Terminal Commander: sudo password");
        assert!(text.message.contains(&job.to_wire_string()));
        assert!(text.message.contains("Command: sudo ?b?x??b??[31m"));
    }

    #[test]
    fn test_seam_never_reaches_a_native_prompt() {
        let text = PromptText::new(JobId::new(), CredentialKind::Password, &[]);
        assert!(matches!(ask_owner(Some("test:pw"), &text), Asked::Secret(s) if s == b"pw"));
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
