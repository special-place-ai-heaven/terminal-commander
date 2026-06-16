// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon-owned persistent shell-session runtime (P1 / TC50).
//!
//! A *session* is a long-lived interactive login shell attached to a PTY.
//! It is built ON TOP of the existing [`PtyRuntime`](crate::pty_command)
//! rather than inventing a new process model: the session shell is just a
//! PTY job running `[shell, "-i"]`, so sticky `cwd`/`env` come for free
//! from the persistent shell process (the next `shell_session_exec` line
//! runs in whatever directory the previous `cd` left it in).
//!
//! Pipeline:
//!
//! ```text
//! ShellSessionRuntime::start
//!   -> enforce max_sessions cap (BEFORE spawn)
//!   -> resolve shell (req.shell or default login shell)
//!   -> argv = [shell, "-i"]
//!   -> PtyRuntime::start_session            // SKIP argv shell-deny
//!        -> PolicyAction::SessionStart      // allow_session cap gate
//!        -> shell_session_start audit row (redacted subject)
//!        -> shared PTY spawn core (bucket / probe / waiter)
//!   -> record SessionEntry { session_id <-> job_id/bucket_id }
//! ```
//!
//! Invariants upheld here (constitution I/II/III/V):
//! - The MCP adapter never reaches this code directly; every entry is via
//!   the daemon IPC handler.
//! - `PtyRuntime::start_session` performs the `SessionStart` policy gate
//!   and writes the audit row BEFORE spawn -- this runtime only sizes and
//!   bookkeeps. Default-deny is enforced by the cap; this runtime adds no
//!   second gate.
//! - Session output is combed: `exec` writes the line to the shell and
//!   reads bounded structured signals back from the session bucket via the
//!   router; it NEVER returns a raw stream.
//! - A send to a non-`Live` session fails loudly ([`SessionError::NotLive`])
//!   -- mirroring the PTY waiter guard -- instead of hanging on a dead
//!   shell.
//!
//! Unix-only: the whole module is `#[cfg(unix)]` because the PTY runtime
//! it builds on is unix-only (ConPTY support is a separate P3 slice).

#![cfg(unix)]

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use terminal_commander_core::{BucketConfig, BucketId, JobId, RuleDefinition, SessionId};
use terminal_commander_ipc::{Liveness, MAX_SESSION_ENV_ITEMS, SessionState};

use crate::pty_command::{PtyRuntime, PtyRuntimeError, PtyStartRequest};

/// Default login shell used when a `shell_session_start` request names
/// none. `/bin/bash` on the unix host the session lane targets.
#[must_use]
fn default_session_shell() -> String {
    "/bin/bash".to_owned()
}

/// Typed session-runtime errors. Mapped to `IpcErrorCode` by the handler.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The `max_sessions` cap is already reached; no spawn attempted.
    #[error("session limit reached: {0} live sessions (cap {0})")]
    LimitReached(usize),
    /// Underlying PTY spawn / policy error (includes `PolicyDenied`).
    #[error("session spawn error: {0}")]
    Pty(#[from] PtyRuntimeError),
    /// The session id is unknown to this runtime.
    #[error("unknown session id: {0}")]
    UnknownSession(SessionId),
    /// The session exists but is not `Live`; a send is refused loudly.
    #[error("session {0} is not live (state observed terminal)")]
    NotLive(SessionId),
    /// The session line exceeded the bounded cap.
    #[error("session line exceeds bounded cap")]
    OversizedLine,
    /// A secret prompt is active on the session shell; LLM-supplied input
    /// is denied (reuses the PTY secret-prompt guard).
    #[error("secret prompt active; LLM-supplied input denied")]
    SecretInputDenied,
}

/// A session's restorable workspace: `(cwd, bounded env overlay)`.
/// Returned by [`ShellSessionRuntime::workspace_of`] for snapshot create.
pub type WorkspaceState = (Option<String>, Vec<(String, String)>);

/// Bookkeeping for one live session. The PTY job (owned by
/// [`PtyRuntime`]) is the real shell; this entry only tracks the
/// session-level metadata the `shell_session_*` surface needs.
struct SessionEntry {
    job_id: JobId,
    bucket_id: BucketId,
    /// Best-known current working directory. Seeded from the requested
    /// start cwd, then advanced when an `exec` line is a recognisable
    /// `cd <abs-path>` (best-effort; see [`parse_cd_target`]).
    cwd: Option<String>,
    /// Bounded env overlay captured at start (the `(key, value)` pairs the
    /// caller supplied). Never includes the inherited parent environment,
    /// so no host secrets leak into status/snapshot responses.
    env_snapshot: Vec<(String, String)>,
    /// Last exec/status touch, used by the idle reaper.
    last_active: Instant,
    /// Epoch-seconds copy of `last_active` for the wire response.
    last_active_epoch: u64,
}

/// Daemon-owned persistent shell-session runtime.
///
/// Holds an `Arc<PtyRuntime>` (the SAME instance the daemon wires for the
/// `pty_command_*` surface) plus the session id map, the `max_sessions`
/// cap, and the idle TTL. Cheap to clone via the inner `Arc`s.
pub struct ShellSessionRuntime {
    pty: Arc<PtyRuntime>,
    sessions: Arc<RwLock<HashMap<SessionId, SessionEntry>>>,
    max_sessions: usize,
    idle_ttl: Duration,
}

impl std::fmt::Debug for ShellSessionRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellSessionRuntime")
            .field("max_sessions", &self.max_sessions)
            .field("idle_ttl_secs", &self.idle_ttl.as_secs())
            .finish_non_exhaustive()
    }
}

/// A request to start a persistent session shell (daemon-internal).
#[derive(Debug, Clone)]
pub struct SessionStartRequest {
    /// Interpreter override; `None` -> [`default_session_shell`].
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub rules: Vec<RuleDefinition>,
    pub bucket_config: Option<BucketConfig>,
    pub tag: Option<String>,
}

/// Outcome of a successful [`ShellSessionRuntime::start`].
#[derive(Debug, Clone, Copy)]
pub struct SessionStartOutcome {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub state: SessionState,
}

/// Status snapshot of one session.
#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub state: SessionState,
    pub cwd: Option<String>,
    pub env_snapshot: Vec<(String, String)>,
    pub last_active_at: u64,
}

/// One entry in a session list snapshot.
#[derive(Debug, Clone)]
pub struct SessionListEntry {
    pub session_id: SessionId,
    pub bucket_id: BucketId,
    pub state: SessionState,
    pub cwd: Option<String>,
    pub last_active_at: u64,
}

impl ShellSessionRuntime {
    /// Wrap the shared [`PtyRuntime`] with session bookkeeping + caps.
    #[must_use]
    pub fn new(pty: Arc<PtyRuntime>, max_sessions: usize, idle_ttl_secs: u64) -> Self {
        Self {
            pty,
            sessions: Arc::new(RwLock::new(HashMap::default())),
            max_sessions,
            idle_ttl: Duration::from_secs(idle_ttl_secs),
        }
    }

    fn now_epoch() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// Map the PTY job's [`Liveness`] to a session [`SessionState`].
    /// Thin alias for [`Self::pty_state`] (kept for call-site readability).
    fn state_of(&self, job_id: JobId) -> SessionState {
        self.pty_state(job_id)
    }

    /// Start a persistent session shell.
    ///
    /// Enforces `max_sessions` BEFORE the spawn (constitution II: bounded,
    /// explanatory refusal -- never a silent hang). The policy gate
    /// (`SessionStart`) + audit happen inside
    /// [`PtyRuntime::start_session`]; a denied cap surfaces here as a
    /// [`SessionError::Pty`] carrying `PolicyDenied`.
    ///
    /// # Errors
    /// - [`SessionError::LimitReached`] when the cap is already reached.
    /// - [`SessionError::Pty`] for any PTY spawn / policy error.
    pub fn start(
        &self,
        req: SessionStartRequest,
        peer_subject: &str,
    ) -> Result<SessionStartOutcome, SessionError> {
        // Reap terminal entries first so a dead session does not pin a cap
        // slot, then enforce the cap on the LIVE count.
        self.reap_terminal();
        let live = self.live_count();
        if live >= self.max_sessions {
            return Err(SessionError::LimitReached(self.max_sessions));
        }

        let shell = req.shell.clone().unwrap_or_else(default_session_shell);
        // `-i` keeps the shell interactive so `cd` state persists across
        // sends; the argv is daemon-assembled (never caller-supplied), so
        // the PTY shell-interpreter guard is intentionally skipped by
        // `start_session` and the `SessionStart` cap gates instead.
        let argv = vec![shell, "-i".to_owned()];

        let env_os: Vec<(OsString, OsString)> = req
            .env
            .iter()
            .take(MAX_SESSION_ENV_ITEMS)
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect();

        let pty_req = PtyStartRequest {
            argv,
            cwd: req.cwd.clone(),
            env: env_os,
            bucket_config: req.bucket_config,
            rules: req.rules,
            rows: None,
            cols: None,
            tag: req.tag,
        };

        // Audit subject redaction: `peer_subject` is the redacted identity
        // resolved by the IPC layer; the PTY runtime's `start_session`
        // writes the `shell_session_start` audit row keyed on the job id.
        let _ = peer_subject;
        let started = self.pty.start_session(pty_req)?;

        let session_id = SessionId::new();
        let env_snapshot: Vec<(String, String)> =
            req.env.into_iter().take(MAX_SESSION_ENV_ITEMS).collect();
        let cwd = req.cwd.as_ref().map(|p| p.to_string_lossy().into_owned());
        let now = Self::now_epoch();
        self.sessions.write().insert(
            session_id,
            SessionEntry {
                job_id: started.job_id,
                bucket_id: started.bucket_id,
                cwd,
                env_snapshot,
                last_active: Instant::now(),
                last_active_epoch: now,
            },
        );

        // Prime the interactive shell so its combed output is CLEAN: a
        // bare prompt (`PS1=`) keeps prompts out of the signal stream,
        // `stty -onlcr -echo` stops the TTY from mapping the program's
        // `\n` to `\r\n` and from echoing typed input, and disabling
        // bracketed paste removes the `\x1b[?2004h/l` noise. Each piece is
        // guarded with `2>/dev/null` so a shell lacking it is harmless.
        //
        // KEPT (TC-B1): the `-onlcr` piece historically existed because the
        // probe's `AnsiNormalizer` CR-collapse dropped the output line
        // preceding a `\r\n` (it landed in the overwrite buffer, not the
        // combed stream). TC-B1 made the normalizer CRLF-aware, so a
        // single-feed `pwd` -> `/tmp\r\n` is now preserved WITHOUT `-onlcr`.
        // The priming is retained intact as belt-and-suspenders: it still
        // suppresses prompts/echo/bracketed-paste, covers the rarer
        // CR/LF-split-across-PTY-reads case the single-feed fix does not, and
        // ripping it out is not re-proven by an O-02 session test exercising
        // that split path -- so the conservative choice is to leave it.
        // Fire-and-forget: the priming bytes are written to the PTY; the
        // bucket combs the (silent, prompt-free) result. A failure here is
        // non-fatal — the session is still usable, just noisier.
        let pty = Arc::clone(&self.pty);
        let job_id = started.job_id;
        tokio::spawn(async move {
            let prime = b"PS1=; stty -onlcr -echo 2>/dev/null; \
                bind 'set enable-bracketed-paste off' 2>/dev/null\n";
            let _ = pty.write_stdin(job_id, prime).await;
        });

        Ok(SessionStartOutcome {
            session_id,
            bucket_id: started.bucket_id,
            state: self.state_of(started.job_id),
        })
    }

    /// Resolve a session's `(job_id, bucket_id)` and assert it is `Live`.
    ///
    /// The terminal-state guard: a send to a non-live session returns
    /// [`SessionError::NotLive`] (mirrors the PTY waiter guard) rather than
    /// writing into a dead shell and blocking.
    fn resolve_live(&self, session_id: SessionId) -> Result<(JobId, BucketId), SessionError> {
        let (job_id, bucket_id) = {
            let g = self.sessions.read();
            let e = g
                .get(&session_id)
                .ok_or(SessionError::UnknownSession(session_id))?;
            (e.job_id, e.bucket_id)
        };
        match self.state_of(job_id) {
            SessionState::Live | SessionState::Starting => Ok((job_id, bucket_id)),
            SessionState::Exited | SessionState::Failed => Err(SessionError::NotLive(session_id)),
        }
    }

    /// Write ONE line to a live session shell.
    ///
    /// The caller reads the combed signals back from the session bucket
    /// (the handler waits on the bucket via the router); this method only
    /// validates, enforces the terminal-state guard, writes the line +
    /// newline through the PTY (which applies the secret-prompt guard),
    /// and advances cwd tracking on a recognisable `cd`.
    ///
    /// # Errors
    /// - [`SessionError::UnknownSession`] / [`SessionError::NotLive`] for
    ///   the terminal-state guard.
    /// - [`SessionError::OversizedLine`] when the line exceeds the cap.
    /// - [`SessionError::SecretInputDenied`] while a secret prompt is up.
    pub async fn exec(
        &self,
        session_id: SessionId,
        line: &str,
    ) -> Result<(BucketId, u64), SessionError> {
        if line.len() > terminal_commander_ipc::MAX_SESSION_LINE_BYTES {
            return Err(SessionError::OversizedLine);
        }
        let (job_id, bucket_id) = self.resolve_live(session_id)?;

        // Append the newline so the shell executes the line. The PTY probe
        // enforces the secret-prompt guard + the stdin byte cap.
        let mut payload = line.as_bytes().to_vec();
        payload.push(b'\n');
        match self.pty.write_stdin(job_id, &payload).await {
            Ok(_) => {}
            Err(PtyRuntimeError::SecretInputDenied) => {
                return Err(SessionError::SecretInputDenied);
            }
            Err(PtyRuntimeError::UnknownJob(_)) => {
                return Err(SessionError::NotLive(session_id));
            }
            Err(other) => return Err(SessionError::Pty(other)),
        }

        // Best-effort cwd tracking + activity touch.
        let now = Self::now_epoch();
        if let Some(entry) = self.sessions.write().get_mut(&session_id) {
            entry.last_active = Instant::now();
            entry.last_active_epoch = now;
            if let Some(target) = parse_cd_target(line) {
                entry.cwd = Some(target);
            }
        }

        Ok((bucket_id, now))
    }

    /// Status snapshot for one session.
    ///
    /// # Errors
    /// [`SessionError::UnknownSession`] if the id is not tracked.
    pub fn status(&self, session_id: SessionId) -> Result<SessionStatus, SessionError> {
        let now = Self::now_epoch();
        let mut g = self.sessions.write();
        let e = g
            .get_mut(&session_id)
            .ok_or(SessionError::UnknownSession(session_id))?;
        // A status read counts as activity so a polled-but-idle session is
        // not reaped out from under an attentive caller.
        e.last_active = Instant::now();
        e.last_active_epoch = now;
        Ok(SessionStatus {
            session_id,
            bucket_id: e.bucket_id,
            state: self.state_of(e.job_id),
            cwd: e.cwd.clone(),
            env_snapshot: e.env_snapshot.clone(),
            last_active_at: e.last_active_epoch,
        })
    }

    /// Read a session's current cwd + bounded env (for snapshot create).
    /// Does NOT touch activity. Returns `None` if the session is unknown.
    #[must_use]
    pub fn workspace_of(&self, session_id: SessionId) -> Option<WorkspaceState> {
        let g = self.sessions.read();
        g.get(&session_id)
            .map(|e| (e.cwd.clone(), e.env_snapshot.clone()))
    }

    /// Restore a snapshot's `cwd`/`env` into a live session.
    ///
    /// `cwd` restoration is by `cd <cwd>`; each env entry by
    /// `export K=V`. Lines run through [`Self::exec`] so the
    /// terminal-state + secret guards apply. Updates the tracked cwd/env.
    ///
    /// # Errors
    /// Propagates [`Self::exec`] errors (terminal-state guard, oversized,
    /// secret prompt).
    pub async fn apply_workspace(
        &self,
        session_id: SessionId,
        cwd: Option<String>,
        env: Vec<(String, String)>,
    ) -> Result<Option<String>, SessionError> {
        // env first, then cd, so a later cd is the final tracked state.
        for (k, v) in env.iter().take(MAX_SESSION_ENV_ITEMS) {
            if is_safe_env_key(k) {
                let line = format!("export {k}={}", shell_single_quote(v));
                self.exec(session_id, &line).await?;
            }
        }
        if let Some(dir) = cwd.as_ref() {
            let line = format!("cd {}", shell_single_quote(dir));
            self.exec(session_id, &line).await?;
        }
        // Record the restored env overlay on the entry too (so a later
        // status / snapshot reflects the applied workspace).
        if let Some(entry) = self.sessions.write().get_mut(&session_id) {
            if !env.is_empty() {
                entry.env_snapshot = env.into_iter().take(MAX_SESSION_ENV_ITEMS).collect();
            }
            if let Some(dir) = cwd.clone() {
                entry.cwd = Some(dir);
            }
        }
        Ok(cwd)
    }

    /// Stop a session: terminate the shell (graceful-then-forced via the
    /// PTY runtime's `stop`) and drop the entry. Idempotent: stopping an
    /// already-terminal/unknown session returns the terminal state with a
    /// bounded reason, never an error.
    pub fn stop(&self, session_id: SessionId) -> (SessionState, String) {
        let entry = self.sessions.write().remove(&session_id);
        let Some(e) = entry else {
            return (SessionState::Exited, "unknown or already reaped".to_owned());
        };
        match self.pty.stop(e.job_id) {
            Ok(_) => (SessionState::Exited, "stopped".to_owned()),
            Err(_) => (SessionState::Exited, "already terminal".to_owned()),
        }
    }

    /// Bounded snapshot of every tracked session (live + lingering).
    #[must_use]
    pub fn list(&self) -> Vec<SessionListEntry> {
        let g = self.sessions.read();
        g.iter()
            .map(|(sid, e)| SessionListEntry {
                session_id: *sid,
                bucket_id: e.bucket_id,
                state: self.state_of(e.job_id),
                cwd: e.cwd.clone(),
                last_active_at: e.last_active_epoch,
            })
            .collect()
    }

    /// Count sessions whose PTY job is still live (Starting / Live).
    fn live_count(&self) -> usize {
        let g = self.sessions.read();
        g.values()
            .filter(|e| {
                matches!(
                    self.state_of(e.job_id),
                    SessionState::Starting | SessionState::Live
                )
            })
            .count()
    }

    /// Drop entries whose PTY job has reached a terminal state. Keeps the
    /// session map from leaking dead bookkeeping and frees cap slots.
    fn reap_terminal(&self) {
        let mut g = self.sessions.write();
        g.retain(|_, e| {
            !matches!(
                self.pty_state(e.job_id),
                SessionState::Exited | SessionState::Failed
            )
        });
    }

    /// Liveness lookup that does not borrow `self.sessions` (so it can be
    /// used inside `retain`).
    fn pty_state(&self, job_id: JobId) -> SessionState {
        match self.pty.liveness(job_id) {
            Liveness::Starting => SessionState::Starting,
            Liveness::Running | Liveness::Dropped { .. } => SessionState::Live,
            Liveness::Exited { .. } | Liveness::Cancelled | Liveness::Stopped => {
                SessionState::Exited
            }
            Liveness::Failed { .. } => SessionState::Failed,
        }
    }

    /// Idle-TTL reaper pass: stop + drop sessions idle past the TTL, plus
    /// any already-terminal entry. Returns the number reaped. Intended to
    /// be driven by a periodic task (see `runtime.rs`). A zero TTL disables
    /// the idle path (terminal entries are still reaped).
    pub fn reap_idle(&self) -> usize {
        let now = Instant::now();
        let idle = self.idle_ttl;
        // Collect victims under a read lock, then stop + remove. Stopping
        // calls into the PTY runtime (which takes its own locks), so we do
        // NOT hold the session write lock across the stop.
        let victims: Vec<SessionId> = {
            let g = self.sessions.read();
            g.iter()
                .filter(|(_, e)| {
                    let terminal = matches!(
                        self.pty_state(e.job_id),
                        SessionState::Exited | SessionState::Failed
                    );
                    let idled = !idle.is_zero() && now.duration_since(e.last_active) >= idle;
                    terminal || idled
                })
                .map(|(sid, _)| *sid)
                .collect()
        };
        for sid in &victims {
            let _ = self.stop(*sid);
        }
        victims.len()
    }
}

/// Best-effort parse of a `cd <target>` session line into an absolute-ish
/// target string for cwd tracking. Returns `None` for anything that is not
/// a plain `cd <single-arg>` (compound lines, `cd` with no arg, `cd -`,
/// etc.) -- cwd tracking is advisory and the persistent shell remains the
/// real source of truth; `shell_session_exec` of `pwd` returns the real
/// directory as a combed signal regardless.
fn parse_cd_target(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("cd ")?;
    let arg = rest.trim();
    // Reject compounds / pipelines / multiple args; keep it conservative.
    if arg.is_empty()
        || arg == "-"
        || arg.contains([';', '|', '&', '\n'])
        || arg.split_whitespace().count() != 1
    {
        return None;
    }
    Some(unquote(arg))
}

/// Strip a single layer of matching single/double quotes.
fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        return s[1..s.len() - 1].to_owned();
    }
    s.to_owned()
}

/// Single-quote a value for safe interpolation into a shell line. Any
/// embedded single quote is escaped as the canonical `'\''` sequence.
fn shell_single_quote(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Whether an env key is a safe shell identifier (letters, digits,
/// underscore, not leading-digit). Snapshot-applied env keys are
/// validated so a malformed key can never assemble an injection line.
fn is_safe_env_key(k: &str) -> bool {
    !k.is_empty()
        && !k.as_bytes()[0].is_ascii_digit()
        && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cd_target_plain() {
        assert_eq!(parse_cd_target("cd /tmp"), Some("/tmp".to_owned()));
        assert_eq!(
            parse_cd_target("  cd /var/log  "),
            Some("/var/log".to_owned())
        );
    }

    #[test]
    fn parse_cd_target_quoted_no_space() {
        // A single-token quoted path unquotes one layer.
        assert_eq!(
            parse_cd_target("cd '/tmp/sub'"),
            Some("/tmp/sub".to_owned())
        );
        assert_eq!(parse_cd_target("cd \"/var\""), Some("/var".to_owned()));
    }

    #[test]
    fn parse_cd_target_rejects_quoted_with_space() {
        // Conservative: a quoted path containing a space looks like two
        // tokens to the whitespace splitter and is rejected (cwd tracking is
        // advisory; `pwd` returns the authoritative directory as a signal).
        assert_eq!(parse_cd_target("cd '/tmp/a b'"), None);
    }

    #[test]
    fn parse_cd_target_rejects_compound_and_dash() {
        assert_eq!(parse_cd_target("cd -"), None);
        assert_eq!(parse_cd_target("cd /tmp && ls"), None);
        assert_eq!(parse_cd_target("cd a b"), None);
        assert_eq!(parse_cd_target("pwd"), None);
        assert_eq!(parse_cd_target("cd"), None);
    }

    #[test]
    fn shell_single_quote_escapes_inner_quote() {
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        assert_eq!(shell_single_quote("/tmp"), "'/tmp'");
    }

    #[test]
    fn env_key_safety() {
        assert!(is_safe_env_key("FOO_BAR"));
        assert!(!is_safe_env_key("1FOO"));
        assert!(!is_safe_env_key("FOO;rm"));
        assert!(!is_safe_env_key(""));
    }
}
