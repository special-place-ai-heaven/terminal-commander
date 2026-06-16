// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
    /// Environment OVERLAY. The child always inherits the daemon's
    /// parent environment; each `(key, value)` here is ADDED to it
    /// (or overrides an existing entry). Empty Vec = inherit unchanged.
    pub env: Vec<(OsString, OsString)>,
    /// Grace window between graceful and forced termination.
    /// Currently advisory; cancellation in MVP is forced kill only.
    pub grace: Duration,
    /// Strip ANSI/CSI/OSC escape sequences before sifter rule matching
    /// and in emitted summaries (TC-B1, FR-026). The RAW bytes are
    /// always preserved in the frame store regardless of this flag;
    /// stripping affects ONLY the text the sifter sees and echoes.
    /// Defaults to `true` so anchored rules and summaries are not
    /// silently defeated by color codes.
    pub strip_ansi: bool,
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
            // TC-B1: strip ANSI by default; raw bytes still land in the
            // frame store. A caller opts out with `strip_ansi = false`.
            strip_ansi: true,
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

/// RAII owner of a Windows Job Object handle.
///
/// The probe assigns its child process to this job at spawn time so the OS can
/// tear down the ENTIRE descendant tree on `TerminateJobObject` (the cancel
/// arm) -- not just the direct child. The handle is also configured with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so `CloseHandle` on the last `Arc`
/// (this `Drop`) likewise kills the tree as defense-in-depth if the probe is
/// dropped without an explicit cancel.
///
/// The raw `HANDLE` (`*mut c_void`) is stored as an `isize` so the wrapper is
/// `Send + Sync` and can be shared (cloned `Arc`) into the spawned lifecycle
/// task while the probe also keeps one for `Drop`. `CloseHandle` runs exactly
/// once, on the last `Arc`.
#[cfg(windows)]
#[derive(Debug)]
struct JobHandle(isize);

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        // SAFETY: `self.0` is a Job Object handle we created with
        // `CreateJobObjectW` (stored as an `isize`) and have not closed yet.
        // Closing it exactly once here is the paired release for that create;
        // a failing `CloseHandle` on a handle we are discarding is ignored.
        unsafe {
            let _ = CloseHandle(self.0 as HANDLE);
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
    /// Job Object owning the child's whole process tree (Windows). Held so the
    /// tree is torn down on `Drop` even without an explicit cancel
    /// (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). Shared (`Arc`) with the
    /// lifecycle task, which terminates the job on cancel. `None` if the job
    /// could not be created (the cancel path then falls back to a
    /// single-process kill).
    #[cfg(windows)]
    _job: Option<Arc<JobHandle>>,
}

impl ProcessProbe {
    /// Spawn a command and start streaming.
    ///
    /// `argv` MUST be non-empty; `argv[0]` is the program and the
    /// rest are arguments. Shell-style strings are NOT accepted
    /// (matches the POLICY.md commands.shell_passthrough=false
    /// invariant).
    ///
    /// Cancellation tears down the WHOLE child process tree, not just the
    /// direct child, so grandchildren cannot orphan:
    ///
    /// * Unix: the child is made its own process-group leader
    ///   (`process_group(0)`, so `pgid == child_pid`) and the cancel arm
    ///   signals the whole group via the `kill(1)` tool with a negative pgid
    ///   (`kill -s KILL -- -<pgid>`), mirroring `supervisor::replace`.
    /// * Windows: the child is assigned to a Job Object at spawn and the
    ///   cancel arm calls `TerminateJobObject`, which kills every process in
    ///   the job (native Win32, no taskkill/powershell).
    #[allow(clippy::too_many_lines)] // spawn is one tightly-coupled lifecycle (mirrors PTY spawn)
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
        // OVERLAY semantics: the child inherits the daemon's full parent
        // environment; each supplied `(key, value)` is ADDED to it (or
        // overrides an existing entry). We deliberately do NOT `env_clear`:
        // clearing it stripped OS-essential vars (e.g. `SystemRoot`, `PATH`
        // on Windows) and crashed Windows children at startup whenever a
        // non-empty env was supplied. An empty `config.env` leaves the
        // loop a no-op, which is exactly "inherit the parent env".
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            // Put the child in its OWN process group so its descendants share
            // the group; the child becomes group leader, so `pgid == child_pid`.
            // The cancel arm then signals the whole group via `kill -<pgid>`,
            // reaping grandchildren that would otherwise orphan.
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            terminal_commander_core::windows_silent(cmd.as_std_mut());
        }
        let mut child = cmd.spawn()?;
        #[cfg(windows)]
        let child_pid = child
            .id()
            .expect("tokio child has pid immediately after spawn");
        // Unix: capture the child pid (== pgid, since it is the group leader)
        // so the cancel arm can kill the whole group without racing the child.
        #[cfg(unix)]
        let child_pid = child
            .id()
            .expect("tokio child has pid immediately after spawn");
        // Windows: assign the child to a fresh Job Object so the OS tears down
        // the whole descendant tree on `TerminateJobObject` (cancel) or on
        // handle close (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, Drop). `None` if
        // the job could not be created/assigned -- the cancel path then falls
        // back to a single-process kill (`start_kill`).
        #[cfg(windows)]
        let job: Option<Arc<JobHandle>> = create_job_for_child(&child).map(Arc::new);

        let stdout = child.stdout.take().expect("piped stdout configured above");
        let stderr = child.stderr.take().expect("piped stderr configured above");

        let metrics = Arc::new(Mutex::new(ProcessProbeMetrics::default()));
        let metrics_for_task = Arc::clone(&metrics);
        let noise_pipeline: SharedProbeNoisePipeline =
            Arc::new(Mutex::new(ProbeNoisePipeline::with_default_policy()));
        let bucket_id = config.bucket_id;
        // TC-B1: snapshot the strip flag before the config borrow ends; the
        // read tasks own it for the life of the probe.
        let strip_ansi = config.strip_ansi;

        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();

        // Move a clone of the job handle (Windows) / the captured pgid (Unix)
        // into the lifecycle task so its cancel arm can tear down the tree.
        #[cfg(windows)]
        let job_for_task = job.clone();

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
                strip_ansi,
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
                strip_ansi,
            );
            let drain = async {
                let _ = tokio::join!(stdout_task, stderr_task);
            };

            tokio::select! {
                () = drain => child.wait().await.map_err(ProcessProbeError::Io),
                _ = &mut cancel_rx => {
                    kill_process_tree(
                        &mut child,
                        #[cfg(unix)] child_pid,
                        #[cfg(windows)] job_for_task.as_deref(),
                    );
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
            #[cfg(windows)]
            _job: job,
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

    /// Clone the shared metrics handle so an owner can snapshot LIVE counters
    /// even after the probe is moved by value into its lifecycle task. The
    /// returned `Arc` points at the same `Mutex` the probe's run loop updates,
    /// so a reader sees the real frame/byte/event counts at read time. Used by
    /// `CommandRuntime::stop` to report the worked workload of a job it kills
    /// (mirrors how the PTY runtime snapshots metrics before cancellation).
    pub fn metrics_handle(&self) -> Arc<Mutex<ProcessProbeMetrics>> {
        Arc::clone(&self.metrics)
    }

    /// Request cancellation. Best-effort; the streaming task and
    /// child are torn down. Idempotent.
    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Take the cancel handle out of the probe so an OWNER (the command runtime's
    /// `stop`) can fire the kill itself. Mirrors `cancel` but hands the sender to
    /// the caller instead of sending. Returns None if already taken/fired.
    pub const fn take_cancel_handle(&mut self) -> Option<tokio::sync::oneshot::Sender<()>> {
        self.cancel_tx.take()
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

/// Tear down the cancelled child's WHOLE process tree (best-effort), then let
/// the caller `wait()` to reap the direct child.
///
/// * Unix: signals the child's process group via the `kill(1)` tool with a
///   negative pgid (`kill -s KILL -- -<pgid>`), reaping grandchildren, then
///   issues `start_kill` on the leader as a belt-and-suspenders fallback (also
///   covers the case where `kill` is absent). Mirrors `supervisor::replace`'s
///   use of the `kill` tool (no `libc`). See the in-body note for why the
///   `-s KILL -- ` form is required over the `-KILL` flag form.
/// * Windows: terminates the Job Object (`TerminateJobObject`), killing every
///   process in the job = the tree, with `start_kill` as a fallback when the
///   job handle is unavailable.
fn kill_process_tree(
    child: &mut tokio::process::Child,
    #[cfg(unix)] pgid: u32,
    #[cfg(windows)] job: Option<&JobHandle>,
) {
    #[cfg(unix)]
    {
        // Signal the whole group: a LEADING MINUS on the target makes `kill`
        // signal the process group (`-<pgid>`), so descendants sharing the
        // group die too. We use the `-s KILL -- -<pgid>` form deliberately:
        //
        //   * `-s KILL` names the signal as a separate argument instead of the
        //     `-KILL` flag form. Empirically, procps-ng `kill` (observed on
        //     WSL2, procps-ng 4.0.4, kernel 6.6.x) MIS-PARSES the combined
        //     `kill -KILL -<pgid>` form and delivers SIGKILL to the CALLER's
        //     process group instead of the target group -- which would kill the
        //     daemon itself. The `-s KILL` + `--` form parses unambiguously and
        //     was verified to (a) reap the target group's grandchildren and
        //     (b) leave the caller alive.
        //   * `--` terminates option parsing so the negative `-<pgid>` target is
        //     never treated as a flag.
        //
        // Best-effort like the supervisor's hard kill: stdio is silenced and
        // the exit status is ignored. If `kill` is somehow absent (Err), the
        // `start_kill` below still reaps the leader.
        let _ = std::process::Command::new("kill")
            .args(["-s", "KILL", "--", &format!("-{pgid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        // Belt-and-suspenders: SIGKILL the leader directly as well.
        let _ = child.start_kill();
    }
    #[cfg(windows)]
    {
        // SAFETY: `handle` is a live Job Object handle created by
        // `CreateJobObjectW` and owned by `JobHandle` for the duration of this
        // borrow. `TerminateJobObject` only reads the handle and posts the exit
        // code to every process in the job; the BOOL result is best-effort and
        // intentionally ignored (the `start_kill` fallback below covers it).
        if let Some(job) = job {
            use windows_sys::Win32::Foundation::HANDLE;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            unsafe {
                let _ = TerminateJobObject(job.0 as HANDLE, 1);
            }
        } else {
            // No job (creation failed at spawn): fall back to killing just the
            // direct child. Grandchildren may orphan in this degraded path.
            let _ = child.start_kill();
        }
    }
    // Platforms with neither cfg: best-effort single-process kill.
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.start_kill();
    }
}

/// Create a Win32 Job Object and assign `child` to it so the OS tears down the
/// whole descendant tree on `TerminateJobObject` / handle close.
///
/// Configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so closing the handle
/// (the `JobHandle` `Drop`) also kills the tree -- defense in depth if the
/// probe is dropped without an explicit cancel. Returns `None` on any failure
/// (null job handle, `SetInformationJobObject` / `AssignProcessToJobObject`
/// failure, or no child handle); the caller then falls back to a
/// single-process kill on cancel.
#[cfg(windows)]
fn create_job_for_child(child: &tokio::process::Child) -> Option<JobHandle> {
    use std::os::windows::io::RawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    // The child's raw OS handle; `None` if it already exited (race-tight: we
    // call this immediately after spawn, before draining).
    let child_handle: RawHandle = child.raw_handle()?;

    // SAFETY: `CreateJobObjectW` with two null pointers (default security
    // attributes, unnamed job) returns a new Job Object handle or null on
    // failure. We check for null before using it.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return None;
    }

    // Configure kill-on-close as defense in depth: closing the handle kills the
    // whole tree even if the explicit `TerminateJobObject` never runs.
    // SAFETY: `info` is a fully owned, zeroed `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
    // (a `#[repr(C)]` POD). `SetInformationJobObject` reads exactly
    // `size_of::<...>()` bytes from `&info` into the kernel for the
    // `JobObjectExtendedLimitInformation` class. We close `job` and bail on
    // failure so a half-configured handle never leaks.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let info_size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .expect("JOBOBJECT_EXTENDED_LIMIT_INFORMATION size fits in u32");
    let set_ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            info_size,
        )
    };
    if set_ok == 0 {
        // SAFETY: `job` is the handle we just created and have not closed yet;
        // close it once before returning so it does not leak.
        unsafe {
            let _ = CloseHandle(job);
        }
        return None;
    }

    // Assign the child to the job; from here on, any process the child spawns
    // is also in the job (jobs are inherited by descendants by default).
    // SAFETY: `job` is our live Job Object handle and `child_handle` is the
    // child's live OS process handle (owned by `child`, borrowed here). The
    // BOOL result is checked; on failure we close `job` and return `None`.
    let assign_ok = unsafe { AssignProcessToJobObject(job, child_handle as HANDLE) };
    if assign_ok == 0 {
        // SAFETY: same as above -- close the unassigned job handle exactly once.
        unsafe {
            let _ = CloseHandle(job);
        }
        return None;
    }

    Some(JobHandle(job as isize))
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
    strip_ansi: bool,
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

        // Append the RAW frame to the context ring so event_context and the
        // output tail can resolve the unmodified bytes (TC-B1: stripping is
        // for matching + summaries only; the frame store keeps raw).
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

        // TC-B1: feed the sifter a STRIPPED view of the frame so anchored
        // rules (`^\[AAP\]`) match colored output and emitted summaries carry
        // no escape bytes. The stripped frame reuses the same `frame_id`, so
        // any emitted event's source pointer still resolves to the RAW frame
        // already stored in the ring above. When `strip_ansi` is off (or the
        // line had no escape byte), the original frame is sifted unchanged.
        let sift_frame = if strip_ansi {
            let stripped = crate::ansi::strip_ansi(&frame.text);
            if stripped == frame.text {
                frame
            } else {
                let mut f = frame;
                f.text = stripped;
                f
            }
        } else {
            frame
        };

        let mut events_emitted = metrics.lock().events_emitted;
        {
            let mut pipeline = noise_pipeline.lock();
            let mut m = metrics.lock();
            pipeline.process_frame(
                &sift_frame,
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

    /// Spawn `/bin/sh -c <script>` with the given env and count how many
    /// events the keyword rule fired. Absolute argv[0] so the program
    /// resolves regardless of PATH; `echo` is a shell builtin so it needs
    /// no PATH either.
    #[cfg(unix)]
    fn run_env_probe(
        env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
        script: &str,
        keyword: &str,
    ) -> u64 {
        let runtime = rt();
        runtime.block_on(async move {
            let rings = Arc::new(ContextRingManager::new());
            let bucket = BucketId::new();
            let mut rule = rule_warning();
            rule.id = "test.envcase".to_owned();
            rule.keywords = Some(vec![keyword.to_owned()]);
            let sifter = Arc::new(SifterRuntime::build(&[rule]).unwrap());
            let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
            let cfg = ProcessProbeConfig {
                env,
                ..ProcessProbeConfig::for_bucket(bucket)
            };
            let mut probe = ProcessProbe::spawn(
                &["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
                &cfg,
                rings,
                sifter,
                Arc::clone(&sink),
            )
            .expect("spawn ok");
            let _ = probe.wait().await.expect("wait ok");
            probe.metrics().events_emitted
        })
    }

    /// (a) Empty env => child INHERITS the parent environment (PATH present).
    #[cfg(unix)]
    #[test]
    fn env_empty_inherits_parent_path() {
        // Probe the child's ACTUAL environment via `printenv PATH`, not the
        // shell `$PATH` variable. A POSIX `sh` repopulates `$PATH` with a
        // compiled-in default when launched without PATH, so `$PATH` cannot
        // distinguish "env supplied PATH" from "shell invented a default" --
        // it is non-empty either way. `printenv PATH` reads the real process
        // environment and exits non-zero when PATH is absent. The form is
        // brace-free so it dodges clippy::literal_string_with_formatting_args
        // (rust 1.95), which mistakes `${...}` for a Rust format placeholder.
        let n = run_env_probe(
            vec![],
            r"if printenv PATH > /dev/null 2>&1; then echo MARK:HASPATH; fi",
            "HASPATH",
        );
        assert!(
            n >= 1,
            "empty env must inherit parent PATH (expected HASPATH match); events={n}"
        );
    }

    /// (b) Non-empty env => the supplied {key,value} REACHES the child
    /// (proves the [{key,value}] -> child round-trip).
    #[cfg(unix)]
    #[test]
    fn env_nonempty_key_reaches_child() {
        let env = vec![(
            std::ffi::OsString::from("TCENV"),
            std::ffi::OsString::from("tcvalue"),
        )];
        let n = run_env_probe(env, "echo MARK:$TCENV", "tcvalue");
        assert!(
            n >= 1,
            "supplied env var must reach the child (expected tcvalue match); events={n}"
        );
    }

    /// (c) Non-empty env OVERLAYS (no env_clear): the supplied vars merge
    /// onto the inherited parent env, so PATH SURVIVES alongside TCENV.
    #[cfg(unix)]
    #[test]
    fn env_nonempty_overlays_and_keeps_path() {
        let env = vec![(
            std::ffi::OsString::from("TCENV"),
            std::ffi::OsString::from("tcvalue"),
        )];
        // `printenv PATH` reads the child's ACTUAL environment. Under overlay
        // semantics the child inherits the parent env and TCENV is layered on
        // top, so the real PATH is present -> `printenv` exits zero -> we see
        // MARK:HASPATH -> events >= 1. We probe `printenv PATH` (not the shell
        // `$PATH` variable): a POSIX `sh` repopulates `$PATH` with a compiled-in
        // default when launched without PATH, so `$PATH` cannot distinguish an
        // inherited PATH from a shell-invented one. The form is brace-free so it
        // dodges clippy::literal_string_with_formatting_args (rust 1.95), which
        // mistakes `${...}` for a Rust format placeholder.
        let n = run_env_probe(
            env,
            r"if printenv PATH > /dev/null 2>&1; then echo MARK:HASPATH; fi",
            "HASPATH",
        );
        assert!(
            n >= 1,
            "non-empty env must OVERLAY (no env_clear) so inherited PATH survives; events={n}"
        );
    }
}
