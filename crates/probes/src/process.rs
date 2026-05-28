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

use crate::noise_pipeline::{ProbeNoisePipeline, SharedProbeNoisePipeline};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
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
    pub frames_suppressed: u64,
    pub frames_suppressed_progress: u64,
    pub frames_suppressed_dedupe: u64,
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
    /// Append a draft. Returns the assigned bucket seq when known.
    fn emit(&self, draft: EventDraft) -> Option<u64>;

    /// Patch aggregation on an already-appended event (TC11 dedupe).
    fn patch_dedupe_aggregate(
        &self,
        _bucket_id: BucketId,
        _patch: &terminal_commander_sifters::DedupeAggregatePatch,
    ) {
    }
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
    fn emit(&self, draft: EventDraft) -> Option<u64> {
        let mut g = self.inner.lock();
        g.push(draft);
        Some(g.len() as u64)
    }

    fn patch_dedupe_aggregate(
        &self,
        _bucket_id: BucketId,
        patch: &terminal_commander_sifters::DedupeAggregatePatch,
    ) {
        let mut g = self.inner.lock();
        // Test sinks assign seq as 1-based index from emit.
        let Ok(idx) = usize::try_from(patch.seq.saturating_sub(1)) else {
            return;
        };
        if let Some(ev) = g.get_mut(idx) {
            ev.count = patch.count;
            ev.first_seen = Some(patch.first_seen);
            ev.last_seen = Some(patch.last_seen);
        }
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
    /// Child PID captured at spawn (Windows console regression tests).
    #[cfg(windows)]
    child_pid: u32,
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
        #[cfg(windows)]
        let child_pid = child.id();
        let stdout = child.stdout.take().expect("piped stdout configured above");
        let stderr = child.stderr.take().expect("piped stderr configured above");

        let metrics = Arc::new(Mutex::new(ProcessProbeMetrics::default()));
        let metrics_for_task = Arc::clone(&metrics);
        let noise_pipeline: SharedProbeNoisePipeline =
            Arc::new(Mutex::new(ProbeNoisePipeline::with_default_policy()));
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
                Arc::clone(&noise_pipeline),
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
                noise_pipeline,
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
            #[cfg(windows)]
            child_pid,
        })
    }

    /// Windows child PID for AttachConsole regression tests.
    #[cfg(windows)]
    #[must_use]
    pub const fn child_pid(&self) -> u32 {
        self.child_pid
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
    noise_pipeline: SharedProbeNoisePipeline,
) {
    let mut reader = BufReader::new(stream);
    let mut line_no: u64 = 0;
    loop {
        // `read_line_bounded` returns raw bytes and so never errors on
        // non-UTF-8; the only `Err` is a genuine pipe/IO failure (the child
        // closed the stream or died mid-read). Clean EOF (`Ok(None)`) and an
        // IO error both mean nothing remains to capture, so we stop. The key
        // change: invalid UTF-8 is NO LONGER an end-of-capture condition.
        let Ok(Some(LineRead {
            bytes: raw,
            dropped,
        })) = read_line_bounded(&mut reader).await
        else {
            break;
        };
        line_no = line_no.saturating_add(1);
        let bytes = raw.len() as u64;
        // Lossy decode preserves capture on non-UTF-8 streams: invalid byte
        // sequences become U+FFFD replacement chars rather than terminating
        // the loop. The replacement chars ARE the lossy signal; downstream
        // sifters treat them as ordinary text (no dedicated "binary" kind).
        let decoded = String::from_utf8_lossy(&raw);
        // Strip CR-tails (cargo / npm flush partial lines with \r).
        let normalized = decoded.trim_end_matches('\r').to_owned();
        let mut frame = SourceFrame::new(probe_id, kind.clone(), normalized).with_line(line_no);
        // Fold any read-layer overflow (bytes discarded beyond
        // MAX_LINE_BYTES because the line had no newline) into the frame's
        // canonical `truncated_bytes`. `SourceFrame::new` already counts the
        // MAX_FRAME_BYTES trim; this adds the bytes dropped before the text
        // ever reached it, so the total dropped count is honest end-to-end.
        if dropped > 0 {
            let extra = u32::try_from(dropped).unwrap_or(u32::MAX);
            frame.truncated_bytes = frame.truncated_bytes.saturating_add(extra);
        }

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
                std::iter::empty(),
            );
            m.events_emitted = events_emitted;
        }
    }
}

/// Maximum bytes retained for a single logical line before it is force-split.
///
/// A newline-less stream (a stuck progress bar, `cat` of a minified blob, a
/// hung tool emitting megabytes with no `\n`) must never grow the read buffer
/// without bound. At this cap we keep the first `MAX_LINE_BYTES`, count and
/// discard the rest, and resync to the next newline. This is the read-layer
/// memory bound; the per-frame [`MAX_FRAME_BYTES`] cap in `SourceFrame::new`
/// still applies on top of whatever survives here.
///
/// [`MAX_FRAME_BYTES`]: terminal_commander_core::context
const MAX_LINE_BYTES: usize = 64 * 1024;

/// One bounded line read: the retained bytes plus how many were discarded.
struct LineRead {
    /// Raw line bytes with the trailing newline excluded, capped at
    /// [`MAX_LINE_BYTES`]. Decoded lossily by the caller; never required to
    /// be valid UTF-8.
    bytes: Vec<u8>,
    /// Bytes dropped beyond [`MAX_LINE_BYTES`] for this logical line. Zero
    /// when the line fit within the cap.
    dropped: u64,
}

/// Read one newline-terminated line as raw bytes, bounding retained bytes at
/// [`MAX_LINE_BYTES`].
///
/// Unlike `AsyncBufReadExt::read_until` (which buffers a whole line into
/// memory before returning) and `Lines::next_line` (which errors on invalid
/// UTF-8 and so silently ends capture), this scans the reader's buffer for
/// `\n`, keeps at most `MAX_LINE_BYTES`, and consumes-and-counts any overflow
/// so a single pathological line can neither blow the buffer nor desync the
/// stream. Returns `Ok(None)` only at clean EOF with nothing buffered.
async fn read_line_bounded<R>(reader: &mut R) -> std::io::Result<Option<LineRead>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes: Vec<u8> = Vec::new();
    let mut dropped: u64 = 0;
    let mut saw_input = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            // EOF. Emit a trailing newline-less line if we accumulated one.
            if saw_input {
                return Ok(Some(LineRead { bytes, dropped }));
            }
            return Ok(None);
        }
        saw_input = true;
        if let Some(idx) = chunk.iter().position(|&b| b == b'\n') {
            accumulate(&mut bytes, &mut dropped, &chunk[..idx]);
            // Consume through the newline so the next call starts clean.
            reader.consume(idx + 1);
            return Ok(Some(LineRead { bytes, dropped }));
        }
        let take = chunk.len();
        accumulate(&mut bytes, &mut dropped, chunk);
        reader.consume(take);
    }
}

/// Append `more` to `buf`, retaining at most [`MAX_LINE_BYTES`] total and
/// counting any excess into `dropped`.
fn accumulate(buf: &mut Vec<u8>, dropped: &mut u64, more: &[u8]) {
    let room = MAX_LINE_BYTES.saturating_sub(buf.len());
    if more.len() <= room {
        buf.extend_from_slice(more);
    } else {
        buf.extend_from_slice(&more[..room]);
        *dropped = dropped.saturating_add((more.len() - room) as u64);
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

    /// Build an argv that writes raw (possibly non-UTF-8) bytes to stdout,
    /// then a clean UTF-8 line. Mirrors a real tool flushing binary noise or
    /// ANSI control bytes before a human-readable message. The raw prefix is
    /// emitted via `sys.stdout.buffer.write` (bypasses the text layer) so the
    /// bytes hit the pipe verbatim; the trailing line uses `print`.
    fn argv_raw_prefix_then_line(raw: &[u8], line: &str) -> Vec<String> {
        use std::fmt::Write as _;
        let mut esc = String::with_capacity(raw.len() * 4);
        for b in raw {
            let _ = write!(esc, "\\x{b:02x}");
        }
        let safe = line.replace('\'', "\\'");
        let script = format!(
            "import sys; sys.stdout.buffer.write(b'{esc}\\n'); \
             sys.stdout.flush(); print('{safe}')"
        );
        vec!["python3".to_owned(), "-c".to_owned(), script]
    }

    /// Build an argv that writes a single newline-less run of `fill` repeated
    /// to `total` bytes on stdout, then a newline, then a clean UTF-8 line.
    /// Exercises the read-layer line bound and resync.
    fn argv_oversize_then_line(fill: char, total: usize, line: &str) -> Vec<String> {
        let safe = line.replace('\'', "\\'");
        let script = format!(
            "import sys; sys.stdout.write('{fill}' * {total}); \
             sys.stdout.write('\\n'); print('{safe}')"
        );
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

    // --- Regression: non-UTF-8 must not end capture (review finding #1) ---
    //
    // Before the fix, the read loop used `Lines::next_line`, which returns
    // `Err` on the first invalid-UTF-8 byte; the `while let Ok(Some(_))`
    // pattern treated that `Err` as end-of-stream and silently dropped the
    // rest of the command's output. A single stray byte therefore blinded the
    // whole "full signal" promise. This asserts that a raw-byte prefix is
    // captured (as U+FFFD via lossy decode) AND that the real line AFTER it
    // still flows through the noise pipeline and fires its rule.
    #[test]
    fn non_utf8_prefix_does_not_end_capture() {
        let runtime = rt();
        runtime.block_on(async {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let sifter = Arc::new(SifterRuntime::build(&[rule_warning()]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            // 0xFF 0xFE is invalid UTF-8 (lone bytes / BOM-ish noise).
            let argv = argv_raw_prefix_then_line(&[0xFF, 0xFE, 0x00, 0xC0], "WARN: real line");
            let mut probe = ProcessProbe::spawn(
                &argv,
                &ProcessProbeConfig::for_bucket(bucket),
                rings,
                sifter,
                Arc::clone(&sink),
            )
            .expect("spawn ok");
            let _ = probe.wait().await.expect("wait ok");
            let m = probe.metrics();
            // Two physical stdout lines: the raw-byte line + the real line.
            // Pre-fix this was 0 (capture died on the first line's decode).
            assert!(
                m.frames_stdout >= 2,
                "expected >=2 stdout frames (garbage + real), got {}",
                m.frames_stdout
            );
            // The real line, emitted AFTER the non-UTF-8 line, must have
            // reached the sifter -- proving capture did not terminate early.
            assert!(
                m.events_emitted >= 1,
                "real line after non-UTF-8 prefix must fire its rule; events={}",
                m.events_emitted
            );
        });
    }

    // --- Regression: a newline-less line must be bounded + resync (finding #6)
    //
    // A single run of bytes with no `\n` previously buffered unboundedly. The
    // read layer now caps at MAX_LINE_BYTES, counts the overflow, and resyncs
    // to the next newline. This asserts (a) the oversized run is ONE frame
    // (1:1 line->frame preserved, not split or buffered across), and (b) the
    // real line after it still fires -- capture resynced past the giant line.
    #[test]
    fn oversize_line_is_bounded_and_capture_resyncs() {
        let runtime = rt();
        runtime.block_on(async {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let sifter = Arc::new(SifterRuntime::build(&[rule_warning()]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            // 256 KiB of 'x' with no newline -> 4x the 64 KiB cap.
            let argv = argv_oversize_then_line('x', 256 * 1024, "WARN: after the blob");
            let mut probe = ProcessProbe::spawn(
                &argv,
                &ProcessProbeConfig::for_bucket(bucket),
                rings,
                sifter,
                Arc::clone(&sink),
            )
            .expect("spawn ok");
            let _ = probe.wait().await.expect("wait ok");
            let m = probe.metrics();
            // Exactly two stdout lines: the (bounded) blob + the real line.
            // If the blob were split per-chunk this would be much larger; if
            // capture died on overflow the real line would never arrive.
            assert_eq!(
                m.frames_stdout, 2,
                "blob must be one frame and the real line another; got {}",
                m.frames_stdout
            );
            assert!(
                m.events_emitted >= 1,
                "line after the oversized blob must fire its rule; events={}",
                m.events_emitted
            );
        });
    }

    // --- Unit: precise byte accounting for the line bound (finding #6) ---
    //
    // Drives `read_line_bounded` directly over an in-memory reader so the cap
    // and the dropped-byte count are asserted deterministically, without a
    // child process. Also pins the non-UTF-8 byte path at the read layer.
    #[test]
    fn read_line_bounded_caps_and_counts_overflow() {
        let runtime = rt();
        runtime.block_on(async {
            // A line longer than the cap (no newline), then a newline, then a
            // short second line.
            let overflow = 4096usize;
            let mut data = vec![b'a'; MAX_LINE_BYTES + overflow];
            data.push(b'\n');
            data.extend_from_slice(b"second\n");

            let mut reader = BufReader::new(data.as_slice());

            let first = read_line_bounded(&mut reader)
                .await
                .expect("io ok")
                .expect("a line");
            assert_eq!(
                first.bytes.len(),
                MAX_LINE_BYTES,
                "retained bytes must be capped at MAX_LINE_BYTES"
            );
            assert_eq!(
                first.dropped, overflow as u64,
                "dropped count must equal the bytes beyond the cap"
            );

            // Resync: the reader must continue cleanly at the next line.
            let second = read_line_bounded(&mut reader)
                .await
                .expect("io ok")
                .expect("a line");
            assert_eq!(second.bytes, b"second");
            assert_eq!(second.dropped, 0);

            // Clean EOF.
            assert!(
                read_line_bounded(&mut reader)
                    .await
                    .expect("io ok")
                    .is_none()
            );
        });
    }

    // --- Unit: raw non-UTF-8 bytes survive the read layer verbatim ---
    #[test]
    fn read_line_bounded_returns_non_utf8_bytes() {
        let runtime = rt();
        runtime.block_on(async {
            let data: &[u8] = &[0xFF, 0xFE, 0x00, b'\n', b'o', b'k', b'\n'];
            let mut reader = BufReader::new(data);

            let first = read_line_bounded(&mut reader)
                .await
                .expect("io ok")
                .expect("a line");
            assert_eq!(first.bytes, vec![0xFF, 0xFE, 0x00]);
            assert_eq!(first.dropped, 0);
            // Lossy decode is the caller's job; verify it does not panic and
            // yields replacement chars for the invalid bytes.
            let decoded = String::from_utf8_lossy(&first.bytes);
            assert!(decoded.contains('\u{FFFD}'));

            let second = read_line_bounded(&mut reader)
                .await
                .expect("io ok")
                .expect("a line");
            assert_eq!(second.bytes, b"ok");
        });
    }
}
