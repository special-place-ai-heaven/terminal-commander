// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Terminal / PTY normalization + prompt detection (TC19).
//!
//! Two portable pieces ship here:
//!
//! 1. [`AnsiNormalizer`]: feeds bytes through a `vte::Parser` and
//!    emits only printable text (ANSI escapes stripped, CR-overwrite
//!    collapsed into a single logical line). Used by the process
//!    probe upstream of the sifter.
//!
//! 2. [`PromptDetector`]: matches a small set of canonical prompts
//!    (sudo password, ssh password, basic shell `$`/`#`) against a
//!    normalized line and returns a `PromptKind`.
//!
//! The actual pty-process spawn path is deferred to a POSIX harness;
//! these portable normalizers are the parts the sifter runtime needs
//! today.
//!
//! Source-status: live (TC19) for normalization + prompt detection.
//! Full PTY spawn deferred (see goal-file decision lock).

use vte::Parser;

/// Reset the buffer when the parser emits a Carriage Return.
const CR_COLLAPSE: bool = true;

/// Stateful ANSI/CR-aware normalizer. Feed bytes via `feed`,
/// pull complete lines via `take_lines`.
#[derive(Default)]
pub struct AnsiNormalizer {
    parser: Parser,
    line: String,
    pending: Vec<String>,
}

#[allow(clippy::missing_fields_in_debug)]
impl core::fmt::Debug for AnsiNormalizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // vte::Parser does not implement Debug; we surface a summary.
        f.debug_struct("AnsiNormalizer")
            .field("line", &self.line)
            .field("pending_count", &self.pending.len())
            .finish()
    }
}

impl AnsiNormalizer {
    /// Construct an empty normalizer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes. Internal state may complete one or more lines.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut sink = Sink {
            line: &mut self.line,
            pending: &mut self.pending,
        };
        self.parser.advance(&mut sink, bytes);
    }

    /// Drain completed lines (without newline terminators).
    pub fn take_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }

    /// Peek the current partial-line buffer without consuming it.
    /// Used by the PTY probe to detect prompts that the child did
    /// NOT terminate with `\n` (typical for `[sudo] password: `
    /// style prompts).
    #[must_use]
    pub fn peek_pending(&self) -> &str {
        &self.line
    }

    /// Flush whatever partial line is pending as a final line.
    pub fn flush(&mut self) -> Option<String> {
        if self.line.is_empty() {
            None
        } else {
            let l = std::mem::take(&mut self.line);
            Some(l)
        }
    }
}

struct Sink<'a> {
    line: &'a mut String,
    pending: &'a mut Vec<String>,
}

impl vte::Perform for Sink<'_> {
    fn print(&mut self, c: char) {
        self.line.push(c);
    }
    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.pending.push(std::mem::take(self.line));
            }
            b'\r' if CR_COLLAPSE => {
                // Carriage return: overwrite from start of line.
                self.line.clear();
            }
            b'\t' => self.line.push('\t'),
            _ => {}
        }
    }
    // ESC sequences, CSI, OSC etc. are intentionally ignored
    // (they're how ANSI color/cursor codes are dispatched).
}

/// Canonical prompt kinds we detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptKind {
    SudoPassword,
    SshPassword,
    GenericPassword,
    Shell,
    YesNo,
    None,
}

/// Detector for canonical prompts.
pub struct PromptDetector;

impl PromptDetector {
    /// Classify a normalized line as a prompt.
    #[must_use]
    pub fn classify(line: &str) -> PromptKind {
        // sudo and ssh password prompts (case insensitive).
        let lower = line.to_lowercase();
        let lower_trim = lower.trim_end();
        if lower_trim.contains("[sudo] password") {
            return PromptKind::SudoPassword;
        }
        if lower_trim.contains("password:") && lower_trim.contains('@') {
            return PromptKind::SshPassword;
        }
        if lower_trim.ends_with("password:") {
            return PromptKind::GenericPassword;
        }
        if lower_trim.ends_with("(y/n)")
            || lower_trim.ends_with("(yes/no)")
            || lower_trim.ends_with("[y/n]")
        {
            return PromptKind::YesNo;
        }
        // Bare shell prompts (heuristic): line ends with `$` or `#`
        // optionally followed by trailing whitespace.
        if lower_trim.ends_with('$') || lower_trim.ends_with('#') {
            return PromptKind::Shell;
        }
        PromptKind::None
    }

    /// Whether the detected prompt is a password / secret prompt.
    #[must_use]
    pub const fn is_secret(kind: PromptKind) -> bool {
        matches!(
            kind,
            PromptKind::SudoPassword | PromptKind::SshPassword | PromptKind::GenericPassword,
        )
    }
}

// =====================================================================
// TC44: PTY probe runtime.
//
// Spawns an argv command attached to a PTY via the `pty-process` crate.
// The PTY's merged output stream is fed through `AnsiNormalizer` to
// strip color/cursor escapes and collapse `\r`-rewritten progress
// lines, then through `PromptDetector` so prompt events surface as
// structured signal. The normalized lines go to the sifter runtime
// exactly like the non-PTY `ProcessProbe`, so the rest of the bucket /
// context / signal pipeline stays unchanged.
//
// SECRET HANDLING (TC44 contract):
// - Secret-bearing prompt detection sets `secret_prompt_active`.
// - `write_stdin` returns `WriteStdinError::SecretInputActive` while
//   that flag is set. The daemon-side `pty_command_write_stdin` IPC
//   handler MUST surface this as `IpcErrorCode::SecretInputDenied`.
// - No secret text is ever copied into events, audit metadata, or
//   logs. The probe itself NEVER receives a secret from the LLM.
//
// Platform: cfg(unix) only. Non-Unix builds compile the rest of this
// file but the PTY runtime is gated; the daemon must surface
// `IpcErrorCode::UnsupportedPlatform` on Windows-native.
// =====================================================================

#[cfg(unix)]
#[allow(
    clippy::needless_pass_by_value, // Arcs are cheap; clarity wins
    clippy::type_complexity,        // mpsc tuple type would obscure intent if aliased here
    clippy::too_many_lines,         // PTY spawn is one tightly-coupled lifecycle
)]
mod runtime {
    use super::{AnsiNormalizer, PromptDetector, PromptKind};

    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;

    use parking_lot::Mutex;
    use terminal_commander_core::{
        BucketId, ContextRingManager, ProbeId, SourceFrame, SourceStream,
    };
    use terminal_commander_sifters::SifterRuntime;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    use crate::noise_pipeline::{
        ProbeNoisePipeline, SharedProbeNoisePipeline, password_prompt_draft,
    };
    use crate::process::EventSink;

    /// Default grace window between graceful and forced terminate.
    pub const DEFAULT_PTY_GRACE: Duration = Duration::from_secs(10);
    /// Hard cap on bytes accepted in one `write_stdin` call. Mirrors
    /// the daemon-side bounded-payload invariant.
    pub const MAX_PTY_STDIN_BYTES: usize = 4096;

    /// Per-PTY-probe configuration.
    #[derive(Debug, Clone)]
    pub struct PtyProbeConfig {
        pub probe_id: Option<ProbeId>,
        pub bucket_id: BucketId,
        pub cwd: Option<PathBuf>,
        /// Environment OVERLAY. The child always inherits the daemon's
        /// parent environment; each `(key, value)` here is ADDED to it
        /// (or overrides an existing entry). Empty Vec = inherit unchanged.
        pub env: Vec<(OsString, OsString)>,
        pub grace: Duration,
        /// PTY rows. Defaults applied when `None`.
        pub rows: Option<u16>,
        /// PTY cols. Defaults applied when `None`.
        pub cols: Option<u16>,
    }

    impl PtyProbeConfig {
        #[must_use]
        pub const fn for_bucket(bucket_id: BucketId) -> Self {
            Self {
                probe_id: None,
                bucket_id,
                cwd: None,
                env: Vec::new(),
                grace: DEFAULT_PTY_GRACE,
                rows: None,
                cols: None,
            }
        }
    }

    /// Counters surfaced to the daemon / admin CLI. Same shape as
    /// `ProcessProbeMetrics` plus PTY-specific counters.
    #[derive(Debug, Default, Clone)]
    pub struct PtyProbeMetrics {
        pub frames_total: u64,
        pub bytes_total: u64,
        pub events_emitted: u64,
        pub prompts_total: u64,
        pub secret_prompts_total: u64,
        pub stdin_bytes_written: u64,
        pub stdin_writes_denied_secret: u64,
        pub frames_suppressed: u64,
        pub frames_suppressed_progress: u64,
        pub frames_suppressed_dedupe: u64,
    }

    /// Errors raised while spawning / driving a PTY probe.
    #[derive(Debug, thiserror::Error)]
    pub enum PtyProbeError {
        #[error("io error: {0}")]
        Io(#[from] std::io::Error),
        #[error("pty-process error: {0}")]
        Pty(String),
        #[error("argv must not be empty")]
        EmptyArgv,
        #[error("probe was cancelled before the child exited")]
        Cancelled,
    }

    impl From<pty_process::Error> for PtyProbeError {
        fn from(value: pty_process::Error) -> Self {
            Self::Pty(value.to_string())
        }
    }

    /// Errors specifically for `write_stdin`. The dedicated enum keeps
    /// the secret-denied case typed so the daemon can map it to
    /// `IpcErrorCode::SecretInputDenied` without sniffing message
    /// strings.
    #[derive(Debug, thiserror::Error)]
    pub enum WriteStdinError {
        #[error("secret prompt active; LLM-supplied input denied")]
        SecretInputActive,
        #[error("input exceeds {MAX_PTY_STDIN_BYTES} byte cap")]
        Oversized,
        #[error("pty handle no longer available")]
        Closed,
        #[error("io error: {0}")]
        Io(#[from] std::io::Error),
    }

    /// Handle to a live PTY probe. Drop or call `cancel` to terminate.
    pub struct PtyProbe {
        probe_id: ProbeId,
        metrics: Arc<Mutex<PtyProbeMetrics>>,
        // Atomic flag set by the prompt detector and read by
        // `write_stdin`. Cleared as soon as the next non-secret frame
        // arrives (so the LLM can respond to non-secret prompts that
        // follow a secret one without operator intervention — the
        // secret line itself is always rejected).
        secret_prompt_active: Arc<AtomicBool>,
        // Atomic generation counter incremented whenever the detector
        // flips the active flag. Exposed for tests + audit metadata
        // only; never carries secret text.
        secret_prompt_gen: Arc<AtomicU64>,
        // Channel sender for stdin writes. The streaming task owns the
        // PTY exclusively (so the borrow-checker is happy with the
        // pty-process `&mut self` read+write API); writes are queued
        // here and applied by that task. The Result channel returns
        // the byte count actually written (or io error).
        stdin_tx: Option<
            tokio::sync::mpsc::Sender<(Vec<u8>, oneshot::Sender<Result<usize, std::io::Error>>)>,
        >,
        cancel_tx: Option<oneshot::Sender<()>>,
        join: Option<tokio::task::JoinHandle<Result<(), PtyProbeError>>>,
    }

    impl std::fmt::Debug for PtyProbe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PtyProbe")
                .field("probe_id", &self.probe_id)
                .field(
                    "secret_prompt_active",
                    &self.secret_prompt_active.load(Ordering::Relaxed),
                )
                .finish_non_exhaustive()
        }
    }

    impl PtyProbe {
        #[must_use]
        pub const fn id(&self) -> ProbeId {
            self.probe_id
        }

        #[must_use]
        pub fn metrics(&self) -> PtyProbeMetrics {
            self.metrics.lock().clone()
        }

        /// Whether a secret prompt is currently active. Tests + audit
        /// metadata only.
        #[must_use]
        pub fn is_secret_prompt_active(&self) -> bool {
            self.secret_prompt_active.load(Ordering::Acquire)
        }

        /// Generation counter. Increments every time the active flag
        /// flips false->true. Bounded counter only; never secret text.
        #[must_use]
        pub fn secret_prompt_generation(&self) -> u64 {
            self.secret_prompt_gen.load(Ordering::Acquire)
        }

        /// Write bytes to the PTY. Returns `SecretInputActive` if the
        /// detector has flagged the current prompt as a secret prompt;
        /// the bytes are NOT written in that case and a metrics counter
        /// is incremented. Bytes above `MAX_PTY_STDIN_BYTES` are
        /// rejected with `Oversized`.
        pub async fn write_stdin(&self, bytes: &[u8]) -> Result<usize, WriteStdinError> {
            if bytes.len() > MAX_PTY_STDIN_BYTES {
                return Err(WriteStdinError::Oversized);
            }
            if self.is_secret_prompt_active() {
                let mut m = self.metrics.lock();
                m.stdin_writes_denied_secret = m.stdin_writes_denied_secret.saturating_add(1);
                return Err(WriteStdinError::SecretInputActive);
            }
            let tx = self.stdin_tx.as_ref().ok_or(WriteStdinError::Closed)?;
            let (reply_tx, reply_rx) = oneshot::channel();
            tx.send((bytes.to_vec(), reply_tx))
                .await
                .map_err(|_| WriteStdinError::Closed)?;
            let result = reply_rx.await.map_err(|_| WriteStdinError::Closed)?;
            let written = result?;
            let mut m = self.metrics.lock();
            m.stdin_bytes_written = m.stdin_bytes_written.saturating_add(written as u64);
            Ok(written)
        }

        /// Cancel the probe. Idempotent.
        pub fn cancel(&mut self) {
            if let Some(tx) = self.cancel_tx.take() {
                let _ = tx.send(());
            }
        }

        /// Wait for the child to exit (or cancellation).
        pub async fn wait(&mut self) -> Result<(), PtyProbeError> {
            let Some(handle) = self.join.take() else {
                return Err(PtyProbeError::Cancelled);
            };
            match handle.await {
                Ok(r) => r,
                Err(e) => Err(PtyProbeError::Io(std::io::Error::other(e.to_string()))),
            }
        }

        /// Spawn an argv command attached to a fresh PTY.
        pub fn spawn(
            argv: &[String],
            config: &PtyProbeConfig,
            rings: Arc<ContextRingManager>,
            runtime: Arc<SifterRuntime>,
            sink: Arc<dyn EventSink>,
        ) -> Result<Self, PtyProbeError> {
            if argv.is_empty() {
                return Err(PtyProbeError::EmptyArgv);
            }
            let probe_id = config.probe_id.unwrap_or_default();
            rings
                .create_ring_default(probe_id)
                .map_err(|e| PtyProbeError::Io(std::io::Error::other(e.to_string())))?;

            let (pty, pts) = pty_process::open()?;
            let rows = config.rows.unwrap_or(24);
            let cols = config.cols.unwrap_or(80);
            pty.resize(pty_process::Size::new(rows, cols))?;

            // pty_process::Command's builder methods consume `self`,
            // so chain them through a single binding.
            let mut cmd = pty_process::Command::new(&argv[0]);
            cmd = cmd.args(&argv[1..]);
            if let Some(cwd) = &config.cwd {
                cmd = cmd.current_dir(cwd);
            }
            // OVERLAY semantics: the child inherits the daemon's full parent
            // environment; each supplied `(key, value)` is ADDED to it (or
            // overrides an existing entry). We deliberately do NOT `env_clear`:
            // clearing it stripped OS-essential vars (e.g. `SystemRoot`, `PATH`
            // on Windows) and crashed Windows children at startup whenever a
            // non-empty env was supplied. An empty `config.env` leaves the
            // loop a no-op, which is exactly "inherit the parent env".
            for (k, v) in &config.env {
                cmd = cmd.env(k, v);
            }
            let mut child = cmd.spawn(pts)?;

            let metrics = Arc::new(Mutex::new(PtyProbeMetrics::default()));
            let secret_prompt_active = Arc::new(AtomicBool::new(false));
            let secret_prompt_gen = Arc::new(AtomicU64::new(0));
            let bucket_id = config.bucket_id;
            let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
            let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<(
                Vec<u8>,
                oneshot::Sender<Result<usize, std::io::Error>>,
            )>(8);

            let metrics_for_task = Arc::clone(&metrics);
            let secret_for_task = Arc::clone(&secret_prompt_active);
            let secret_gen_for_task = Arc::clone(&secret_prompt_gen);
            let rings_for_task = Arc::clone(&rings);
            let noise_pipeline: SharedProbeNoisePipeline =
                Arc::new(Mutex::new(ProbeNoisePipeline::with_default_policy()));

            let join = tokio::spawn(async move {
                // The streaming task owns the pty exclusively. Each
                // loop iteration first drains pending writes via
                // try_recv (non-blocking), then does ONE bounded read
                // with a short timeout. This avoids holding multiple
                // `&mut pty` borrows across a `tokio::select!` which
                // tripped the borrow-checker / triggered an ICE.
                let mut pty = pty;
                let mut normalizer = AnsiNormalizer::new();
                let mut buf = [0u8; 4096];
                let outcome: Result<(), PtyProbeError> = loop {
                    if cancel_rx.try_recv().is_ok() {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                        break Err(PtyProbeError::Cancelled);
                    }
                    // Drain any pending stdin writes.
                    while let Ok((bytes, reply)) = stdin_rx.try_recv() {
                        let result = match pty.write_all(&bytes).await {
                            Ok(()) => pty.flush().await.map(|()| bytes.len()),
                            Err(e) => Err(e),
                        };
                        let _ = reply.send(result);
                    }
                    // One bounded read with a short timeout so the
                    // loop wakes regularly to service cancellation +
                    // queued writes.
                    let read_result = tokio::time::timeout(
                        std::time::Duration::from_millis(50),
                        pty.read(&mut buf),
                    )
                    .await;
                    match read_result {
                        Err(_) => {
                            // Timeout — loop back to service writes /
                            // cancellation.
                        }
                        Ok(Ok(0)) => break Ok(()),
                        Ok(Ok(n)) => {
                            normalizer.feed(&buf[..n]);
                            let lines = normalizer.take_lines();
                            for line in lines {
                                process_line(
                                    &line,
                                    probe_id,
                                    bucket_id,
                                    Arc::clone(&rings_for_task),
                                    Arc::clone(&runtime),
                                    Arc::clone(&sink),
                                    Arc::clone(&metrics_for_task),
                                    Arc::clone(&secret_for_task),
                                    Arc::clone(&secret_gen_for_task),
                                    Arc::clone(&noise_pipeline),
                                );
                            }
                            // TC44: a partial line (no trailing
                            // newline) can still BE a secret prompt
                            // — `[sudo] password for dev: ` is the
                            // canonical example. Run prompt
                            // detection on the pending buffer too
                            // so the secret flag flips before the
                            // LLM gets a chance to write.
                            let pending = normalizer.peek_pending().to_owned();
                            if !pending.is_empty() {
                                let kind = PromptDetector::classify(&pending);
                                if PromptDetector::is_secret(kind)
                                    && !secret_for_task.swap(true, Ordering::AcqRel)
                                {
                                    secret_gen_for_task.fetch_add(1, Ordering::AcqRel);
                                    let mut m = metrics_for_task.lock();
                                    m.prompts_total = m.prompts_total.saturating_add(1);
                                    m.secret_prompts_total =
                                        m.secret_prompts_total.saturating_add(1);
                                }
                            }
                            let mut m = metrics_for_task.lock();
                            m.bytes_total = m.bytes_total.saturating_add(n as u64);
                        }
                        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            break Ok(());
                        }
                        Ok(Err(e)) => {
                            if e.raw_os_error() == Some(libc_eio()) {
                                break Ok(());
                            }
                            break Err(PtyProbeError::Io(e));
                        }
                    }
                };
                if let Some(tail) = normalizer.flush() {
                    process_line(
                        &tail,
                        probe_id,
                        bucket_id,
                        rings_for_task,
                        runtime,
                        sink,
                        Arc::clone(&metrics_for_task),
                        secret_for_task,
                        secret_gen_for_task,
                        noise_pipeline,
                    );
                }
                let _ = child.wait().await;
                outcome
            });

            Ok(Self {
                probe_id,
                metrics,
                secret_prompt_active,
                secret_prompt_gen,
                stdin_tx: Some(stdin_tx),
                cancel_tx: Some(cancel_tx),
                join: Some(join),
            })
        }
    }

    const fn libc_eio() -> i32 {
        5
    }

    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn process_line(
        line: &str,
        probe_id: ProbeId,
        bucket_id: BucketId,
        rings: Arc<ContextRingManager>,
        runtime: Arc<SifterRuntime>,
        sink: Arc<dyn EventSink>,
        metrics: Arc<Mutex<PtyProbeMetrics>>,
        secret_prompt_active: Arc<AtomicBool>,
        secret_prompt_gen: Arc<AtomicU64>,
        noise_pipeline: SharedProbeNoisePipeline,
    ) {
        let kind = PromptDetector::classify(line);
        let is_secret = PromptDetector::is_secret(kind);
        if is_secret {
            // flip false->true and bump generation
            if !secret_prompt_active.swap(true, Ordering::AcqRel) {
                secret_prompt_gen.fetch_add(1, Ordering::AcqRel);
            }
        } else if !matches!(
            kind,
            PromptKind::None | PromptKind::Shell | PromptKind::YesNo
        ) {
            secret_prompt_active.store(false, Ordering::Release);
        } else if matches!(kind, PromptKind::None) {
            // A normal line resolves any pending secret prompt; the
            // child has emitted output past the password line so any
            // typed secret is already consumed.
            secret_prompt_active.store(false, Ordering::Release);
        }

        let frame = SourceFrame::new(probe_id, SourceStream::Stdout, line.to_owned());
        let _ = rings.append_frame(probe_id, frame.clone());
        {
            let mut m = metrics.lock();
            m.frames_total = m.frames_total.saturating_add(1);
            if !matches!(kind, PromptKind::None) {
                m.prompts_total = m.prompts_total.saturating_add(1);
                if is_secret {
                    m.secret_prompts_total = m.secret_prompts_total.saturating_add(1);
                }
            }
        }
        let extra = if is_secret {
            vec![password_prompt_draft(&frame, bucket_id)]
        } else {
            Vec::new()
        };
        let mut events_emitted = metrics.lock().events_emitted;
        {
            let mut pipeline = noise_pipeline.lock();
            let mut m = metrics.lock();
            pipeline.process_frame(
                &frame,
                bucket_id,
                &runtime,
                sink.as_ref(),
                &mut *m,
                &mut events_emitted,
                extra,
            );
            m.events_emitted = events_emitted;
        }
        let _ = secret_prompt_gen;
    }
}

#[cfg(unix)]
pub use runtime::{
    DEFAULT_PTY_GRACE, MAX_PTY_STDIN_BYTES, PtyProbe, PtyProbeConfig, PtyProbeError,
    PtyProbeMetrics, WriteStdinError,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_strips_color_escapes() {
        let mut n = AnsiNormalizer::new();
        n.feed(b"\x1b[31merror:\x1b[0m something broke\n");
        let lines = n.take_lines();
        assert_eq!(lines, vec!["error: something broke".to_owned()]);
    }

    #[test]
    fn ansi_cr_collapses_progress_lines() {
        let mut n = AnsiNormalizer::new();
        // Progress: "10%\r25%\r100%\n" should yield one line "100%".
        n.feed(b"10%\r25%\r100%\n");
        let lines = n.take_lines();
        assert_eq!(lines, vec!["100%".to_owned()]);
    }

    #[test]
    fn ansi_multiline_breakdown() {
        let mut n = AnsiNormalizer::new();
        n.feed(b"first line\nsecond line\nthird");
        let lines = n.take_lines();
        assert_eq!(lines, vec!["first line", "second line"]);
        assert_eq!(n.flush().unwrap(), "third");
    }

    #[test]
    fn prompt_sudo_detected() {
        assert_eq!(
            PromptDetector::classify("[sudo] password for dev: "),
            PromptKind::SudoPassword
        );
        assert!(PromptDetector::is_secret(PromptKind::SudoPassword));
    }

    #[test]
    fn prompt_ssh_password_detected() {
        assert_eq!(
            PromptDetector::classify("dev@host-a's password:"),
            PromptKind::SshPassword
        );
    }

    #[test]
    fn prompt_shell_detected() {
        assert_eq!(PromptDetector::classify("dev@host:~$ "), PromptKind::Shell);
        assert_eq!(PromptDetector::classify("root@host:~# "), PromptKind::Shell);
    }

    #[test]
    fn prompt_yes_no_detected() {
        assert_eq!(
            PromptDetector::classify("Continue? [y/n]"),
            PromptKind::YesNo
        );
        assert_eq!(
            PromptDetector::classify("Are you sure? (yes/no)"),
            PromptKind::YesNo
        );
    }

    #[test]
    fn prompt_non_match_returns_none() {
        assert_eq!(
            PromptDetector::classify("just a regular log line"),
            PromptKind::None
        );
        assert!(!PromptDetector::is_secret(PromptKind::None));
    }
}
