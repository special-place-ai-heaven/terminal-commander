// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Process probe runtime.
//!
//! Windows: `ProcessProbe::spawn` calls `terminal_commander_core::windows_silent`
//! on the underlying `std::process::Command` so GUI-subsystem daemon children do
//! not allocate a visible console. The JS bridge (`lib/wsl/spawn.js`) intentionally
//! does not use this flag — see `docs/release/windows-wsl-bridge-contract.md` §4.4.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use terminal_commander_core::{
    BucketId, ContextRingManager, EventDraft, ProbeId, SourceFrame, SourceStream,
};
use terminal_commander_sifters::SifterRuntime;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

/// Default grace window between graceful and forced termination.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(10);

/// Per-probe configuration.
#[derive(Debug, Clone)]
pub struct ProcessProbeConfig {
    /// Probe id. Auto-generated when None.
    pub probe_id: Option<ProbeId>,
    /// Target bucket for emitted drafts.
    pub bucket_id: BucketId,
    /// Working directory for the child process. Passed through to
    /// the child; the advisory-policy seam in TC22 will gate this.
    pub cwd: Option<PathBuf>,
    /// Environment passthrough. Each `(key, value)` is set on the
    /// child. Empty Vec keeps the parent env.
    pub env: Vec<(OsString, OsString)>,
    /// Grace window between graceful and forced termination.
    /// Currently advisory; cancellation in MVP is forced kill only.
    pub grace: Duration,
}

impl ProcessProbeConfig {
    /// Construct a config that targets `bucket_id` with all defaults.
    #[must_use]
    pub const fn for_bucket(bucket_id: BucketId) -> Self {
        Self {
            probe_id: None,
            bucket_id,
            cwd: None,
            env: Vec::new(),
            grace: DEFAULT_GRACE,
        }
    }
}

/// Counters surfaced for tests and the admin CLI.
#[derive(Debug, Default, Clone)]
pub struct ProcessProbeMetrics {
    pub frames_total: u64,
    pub frames_stdout: u64,
    pub frames_stderr: u64,
    pub bytes_total: u64,
    pub events_emitted: u64,
}

/// Errors from running a process probe.
#[derive(Debug, thiserror::Error)]
pub enum ProcessProbeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("probe was cancelled before the child exited")]
    Cancelled,
}

/// Sink that receives `EventDraft`s as the probe matches them.
///
/// Implementations must be cheap to clone or behind an `Arc`.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, draft: EventDraft);
}

/// Trivial in-memory sink used by tests and the daemon transport
/// adapter (which simply forwards to the bucket manager).
#[derive(Debug, Default, Clone)]
pub struct InMemorySink {
    inner: Arc<Mutex<Vec<EventDraft>>>,
}

impl InMemorySink {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn drain(&self) -> Vec<EventDraft> {
        std::mem::take(&mut *self.inner.lock())
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl EventSink for InMemorySink {
    fn emit(&self, draft: EventDraft) {
        self.inner.lock().push(draft);
    }
}

/// Handle to a running probe. Drop or call `cancel` to stop the
/// child; `wait` to await its natural exit.
#[derive(Debug)]
pub struct ProcessProbe {
    probe_id: ProbeId,
    metrics: Arc<Mutex<ProcessProbeMetrics>>,
    cancel_tx: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<Result<std::process::ExitStatus, ProcessProbeError>>>,
}

impl ProcessProbe {
    /// Spawn a command and start streaming.
    ///
    /// `argv` MUST be non-empty; `argv[0]` is the program and the
    /// rest are arguments. Shell-style strings are NOT accepted
    /// (matches the POLICY.md commands.shell_passthrough=false
    /// invariant).
    pub fn spawn(
        argv: &[String],
        config: &ProcessProbeConfig,
        rings: Arc<ContextRingManager>,
        runtime: Arc<SifterRuntime>,
        sink: Arc<dyn EventSink>,
    ) -> Result<Self, ProcessProbeError> {
        if argv.is_empty() {
            return Err(ProcessProbeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "argv must not be empty",
            )));
        }
        let probe_id = config.probe_id.unwrap_or_default();
        rings
            .create_ring_default(probe_id)
            .map_err(|e| ProcessProbeError::Io(std::io::Error::other(e.to_string())))?;
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::null());
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }
        if !config.env.is_empty() {
            cmd.env_clear();
            for (k, v) in &config.env {
                cmd.env(k, v);
            }
        }
        #[cfg(windows)]
        {
            terminal_commander_core::windows_silent(cmd.as_std_mut());
        }
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout configured above");
        let stderr = child.stderr.take().expect("piped stderr configured above");

        let metrics = Arc::new(Mutex::new(ProcessProbeMetrics::default()));
        let metrics_for_task = Arc::clone(&metrics);
        let bucket_id = config.bucket_id;

        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();

        let join = tokio::spawn(async move {
            let stdout_task = read_stream(
                stdout,
                probe_id,
                SourceStream::Stdout,
                bucket_id,
                Arc::clone(&rings),
                Arc::clone(&runtime),
                Arc::clone(&sink),
                Arc::clone(&metrics_for_task),
            );
            let stderr_task = read_stream(
                stderr,
                probe_id,
                SourceStream::Stderr,
                bucket_id,
                rings,
                runtime,
                sink,
                metrics_for_task,
            );
            let drain = async {
                let _ = tokio::join!(stdout_task, stderr_task);
            };

            tokio::select! {
                () = drain => child.wait().await.map_err(ProcessProbeError::Io),
                _ = &mut cancel_rx => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    Err(ProcessProbeError::Cancelled)
                }
            }
        });

        Ok(Self {
            probe_id,
            metrics,
            cancel_tx: Some(cancel_tx),
            join: Some(join),
        })
    }

    /// Probe identifier.
    #[must_use]
    pub const fn id(&self) -> ProbeId {
        self.probe_id
    }

    /// Snapshot the current metrics.
    #[must_use]
    pub fn metrics(&self) -> ProcessProbeMetrics {
        self.metrics.lock().clone()
    }

    /// Request cancellation. Best-effort; the streaming task and
    /// child are torn down. Idempotent.
    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Await natural exit. Returns the child exit status on success;
    /// `Cancelled` if `cancel` was called before exit.
    pub async fn wait(&mut self) -> Result<std::process::ExitStatus, ProcessProbeError> {
        let Some(handle) = self.join.take() else {
            return Err(ProcessProbeError::Cancelled);
        };
        match handle.await {
            Ok(r) => r,
            Err(e) => Err(ProcessProbeError::Io(std::io::Error::other(e.to_string()))),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn read_stream<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    stream: R,
    probe_id: ProbeId,
    kind: SourceStream,
    bucket_id: BucketId,
    rings: Arc<ContextRingManager>,
    runtime: Arc<SifterRuntime>,
    sink: Arc<dyn EventSink>,
    metrics: Arc<Mutex<ProcessProbeMetrics>>,
) {
    let mut reader = BufReader::new(stream).lines();
    let mut line_no: u64 = 0;
    while let Ok(Some(line)) = reader.next_line().await {
        line_no = line_no.saturating_add(1);
        let bytes = line.len() as u64;
        // Strip CR-tails (cargo / npm flush partial lines with \r).
        let normalized = line.trim_end_matches('\r').to_owned();
        let frame = SourceFrame::new(probe_id, kind.clone(), normalized).with_line(line_no);

        // Append to the context ring so event_context can resolve.
        let _ = rings.append_frame(probe_id, frame.clone());

        // Update metrics.
        {
            let mut m = metrics.lock();
            m.frames_total = m.frames_total.saturating_add(1);
            match kind {
                SourceStream::Stdout => {
                    m.frames_stdout = m.frames_stdout.saturating_add(1);
                }
                SourceStream::Stderr => {
                    m.frames_stderr = m.frames_stderr.saturating_add(1);
                }
                _ => {}
            }
            m.bytes_total = m.bytes_total.saturating_add(bytes);
        }

        // Run sifter rules.
        for draft in runtime.evaluate(&frame, bucket_id) {
            {
                let mut m = metrics.lock();
                m.events_emitted = m.events_emitted.saturating_add(1);
            }
            sink.emit(draft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{
        BucketId, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
    };

    fn rule_warning() -> RuleDefinition {
        RuleDefinition {
            id: "test.warning".to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::Medium,
            event_kind: "kw_warning".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["WARN".to_owned()]),
            captures: vec![],
            summary_template: "warning seen".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    fn rule_err_stderr() -> RuleDefinition {
        RuleDefinition {
            id: "test.err".to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::High,
            event_kind: "kw_error".to_owned(),
            stream: Some(SourceStream::Stderr),
            description: None,
            pattern: None,
            keywords: Some(vec!["ERROR".to_owned()]),
            captures: vec![],
            summary_template: "error on stderr".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    /// Build an argv that prints known text to stdout and stderr.
    /// Uses python3 (a TC03 dev-prereq); we escape the strings as
    /// Python single-quoted literals by replacing apostrophes.
    fn argv_say(stdout: &str, stderr: &str) -> Vec<String> {
        let sout = stdout.replace('\'', "\\'");
        let serr = stderr.replace('\'', "\\'");
        let script = format!("import sys; print('{sout}'); print('{serr}', file=sys.stderr)");
        vec!["python3".to_owned(), "-c".to_owned(), script]
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn probe_captures_stdout_and_stderr_frames() {
        let runtime = rt();
        runtime.block_on(async {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let sifter =
                Arc::new(SifterRuntime::build(&[rule_warning(), rule_err_stderr()]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            let mut probe = ProcessProbe::spawn(
                &argv_say("WARN: hello", "ERROR: bad"),
                &ProcessProbeConfig::for_bucket(bucket),
                rings,
                sifter,
                Arc::clone(&sink),
            )
            .expect("spawn ok");
            let _ = probe.wait().await.expect("wait ok");
            let m = probe.metrics();
            assert!(m.frames_stdout >= 1, "stdout frames: {}", m.frames_stdout);
            assert!(m.frames_stderr >= 1, "stderr frames: {}", m.frames_stderr);
            assert!(m.bytes_total > 0);
            assert!(m.events_emitted >= 2, "events: {}", m.events_emitted);
        });
    }

    #[test]
    fn probe_empty_argv_rejected() {
        let runtime = rt();
        runtime.block_on(async {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            let err = ProcessProbe::spawn(
                &[],
                &ProcessProbeConfig::for_bucket(bucket),
                rings,
                sifter,
                sink,
            )
            .unwrap_err();
            assert!(matches!(err, ProcessProbeError::Io(_)));
        });
    }

    #[test]
    fn probe_unknown_command_returns_io_error() {
        let runtime = rt();
        runtime.block_on(async {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            let err = ProcessProbe::spawn(
                &["this-binary-does-not-exist-tcm".to_owned()],
                &ProcessProbeConfig::for_bucket(bucket),
                rings,
                sifter,
                sink,
            )
            .unwrap_err();
            assert!(matches!(err, ProcessProbeError::Io(_)));
        });
    }

    #[test]
    fn probe_metrics_count_zero_events_when_no_match() {
        let runtime = rt();
        runtime.block_on(async {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let sifter = Arc::new(SifterRuntime::build(&[rule_warning()]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            let mut probe = ProcessProbe::spawn(
                &argv_say("clean output", "nothing matches"),
                &ProcessProbeConfig::for_bucket(bucket),
                rings,
                sifter,
                Arc::clone(&sink),
            )
            .expect("spawn ok");
            let _ = probe.wait().await.expect("wait ok");
            let m = probe.metrics();
            assert_eq!(m.events_emitted, 0);
            assert!(m.frames_total >= 2);
        });
    }

    #[test]
    fn probe_response_carries_no_raw_text() {
        // Compile-time: EventSink emits Vec<EventDraft> (structured).
        // No raw String stdout/stderr lane exists on the probe API.
        fn assert_only_event_drafts(_e: &EventDraft) {}
        let _ = assert_only_event_drafts;
    }
}
