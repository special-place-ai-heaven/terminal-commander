// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Process probe runtime.
//!
//! Unix ownership requires exclusive reaping of TC's children: the host must not
//! use a competing `waitpid(-1)` reaper or change SIGCHLD to automatic reaping.
//! TC retains the unreaped group leader through its last group signal. Processes
//! that leave the owned group remain outside that ownership boundary.
//!
//! Windows: `ProcessProbe::spawn` calls `terminal_commander_core::windows_silent`
//! on the underlying `std::process::Command` so GUI-subsystem daemon children do
//! not allocate a visible console. The JS bridge (`lib/wsl/spawn.js`) intentionally
//! does not use this flag — see `docs/release/windows-wsl-bridge-contract.md` §4.4.

use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use parking_lot::Mutex;
use terminal_commander_core::{
    BucketId, ContextRingManager, EnvironmentMode, EventDraft, ObservationIncompleteReason,
    PipeReadErrorKind, PipeReadFailure, ProbeId, ProcessCleanup, ProcessIdentity,
    ProcessObservation, ProcessOwnership, SourceFrame, SourceStream, StreamObservationState,
};
use terminal_commander_sifters::SifterRuntime;

use crate::governor::{GovernorReport, JobLimits, SharedReport};
use crate::noise_pipeline::{ProbeNoisePipeline, SharedProbeNoisePipeline};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, BufReader, ReadBuf};
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
    /// Environment overlay. `spawn` inherits the parent's environment;
    /// `spawn_with_environment` can explicitly clear it before this overlay.
    pub env: Vec<(OsString, OsString)>,
    /// Grace window between graceful and forced termination.
    /// Unix cancellation sends TERM then escalates to KILL after this window.
    pub grace: Duration,
    /// Strip ANSI/CSI/OSC escape sequences before sifter rule matching
    /// and in emitted summaries (TC-B1, FR-026). The RAW bytes are
    /// always preserved in the frame store regardless of this flag;
    /// stripping affects ONLY the text the sifter sees and echoes.
    /// Defaults to `true` so anchored rules and summaries are not
    /// silently defeated by color codes.
    pub strip_ansi: bool,
    /// Resource governor limits. `JobLimits::default()` = ungoverned, no
    /// extra syscall (today's behaviour).
    pub limits: JobLimits,
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
            limits: JobLimits {
                memory_bytes: None,
                priority: None,
                join_host_ceiling: false,
            },
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
    /// Last successful raw pipe read, before decoding or framing.
    pub last_output_at: Option<std::time::Instant>,
    pub observation: ProcessObservation,
    pub cleanup: ProcessCleanup,
    pub events_emitted: u64,
    pub frames_suppressed: u64,
    pub frames_suppressed_progress: u64,
    pub frames_suppressed_dedupe: u64,
    /// When the most recent frame was captured; `None` until the first one.
    pub last_frame_at: Option<std::time::Instant>,
}

/// Errors from running a process probe.
#[derive(Debug, thiserror::Error)]
pub enum ProcessProbeError {
    #[error("process probes require an active Tokio runtime")]
    MissingRuntime,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("probe was cancelled before the child exited")]
    Cancelled,
}

/// Child exit and output completeness are independent. A successful exit must
/// not be interpreted as a complete observation when a pipe failed.
#[derive(Debug, Clone)]
pub struct ProcessProbeReport {
    pub exit_status: std::process::ExitStatus,
    pub cancelled: bool,
    pub observation: ProcessObservation,
    pub cleanup: ProcessCleanup,
}

/// Sits below decoding and buffering: every successful OS read is observable
/// even while the line framer is waiting for a delimiter.
struct ObservedReader<R> {
    inner: R,
    kind: SourceStream,
    metrics: Arc<Mutex<ProcessProbeMetrics>>,
}

impl<R: AsyncRead + Unpin> AsyncRead for ObservedReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let capacity = buf.remaining();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(ref outcome) = result {
            let mut metrics = this.metrics.lock();
            let bytes = (buf.filled().len() - before) as u64;
            if bytes > 0 {
                metrics.bytes_total = metrics.bytes_total.saturating_add(bytes);
                metrics.last_output_at = Some(std::time::Instant::now());
            }
            let stream = match this.kind {
                SourceStream::Stdout => &mut metrics.observation.stdout,
                SourceStream::Stderr => &mut metrics.observation.stderr,
                _ => return result,
            };
            stream.bytes_total = stream.bytes_total.saturating_add(bytes);
            match outcome {
                Ok(()) if bytes == 0 && capacity > 0 => {
                    stream.state = StreamObservationState::Complete;
                }
                Err(error) => {
                    stream.state = StreamObservationState::Failed(pipe_failure(error));
                }
                Ok(()) => {}
            }
        }
        result
    }
}

fn pipe_failure(error: &std::io::Error) -> PipeReadFailure {
    use std::io::ErrorKind;
    PipeReadFailure {
        kind: match error.kind() {
            ErrorKind::Interrupted => PipeReadErrorKind::Interrupted,
            ErrorKind::PermissionDenied => PipeReadErrorKind::PermissionDenied,
            ErrorKind::BrokenPipe => PipeReadErrorKind::BrokenPipe,
            ErrorKind::ConnectionReset => PipeReadErrorKind::ConnectionReset,
            ErrorKind::UnexpectedEof => PipeReadErrorKind::UnexpectedEof,
            ErrorKind::TimedOut => PipeReadErrorKind::TimedOut,
            _ => PipeReadErrorKind::Other,
        },
        raw_os_error: error.raw_os_error(),
    }
}

fn finish_observation(metrics: &mut ProcessProbeMetrics, reason: ObservationIncompleteReason) {
    for stream in [
        &mut metrics.observation.stdout,
        &mut metrics.observation.stderr,
    ] {
        if stream.state == StreamObservationState::Reading {
            stream.state = StreamObservationState::Incomplete(reason);
        }
    }
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
pub(crate) struct JobHandle(pub(crate) isize);

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
    join: Option<tokio::task::JoinHandle<Result<ProcessProbeReport, ProcessProbeError>>>,
    identity: ProcessIdentity,
    cpu_sampler: Arc<Mutex<crate::job_cpu::JobCpuSampler>>,
    governor: SharedReport,
    report: Option<ProcessProbeReport>,
}

impl Drop for ProcessProbe {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct OwnedProcess {
    child: Option<tokio::process::Child>,
    child_pid: u32,
    metrics: Arc<Mutex<ProcessProbeMetrics>>,
    #[cfg(windows)]
    job: Option<Arc<JobHandle>>,
    armed: bool,
    signal_cleanup: Option<ProcessCleanup>,
}

impl OwnedProcess {
    const fn child_mut(&mut self) -> &mut tokio::process::Child {
        self.child.as_mut().expect("owned child exists until drop")
    }

    /// Deliver the final group/job signal exactly once, before releasing the leader anchor.
    fn kill_tree(&mut self) -> ProcessCleanup {
        if let Some(cleanup) = self.signal_cleanup {
            return cleanup;
        }
        #[cfg(unix)]
        let result = signal_process_group(self.child_pid, libc::SIGKILL);
        #[cfg(windows)]
        let result = self.job.as_deref().map_or_else(
            || Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
            terminate_job,
        );
        #[cfg(not(any(unix, windows)))]
        let result: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
        let cleanup = match result {
            Ok(()) => ProcessCleanup::Reaping,
            Err(error) => ProcessCleanup::Uncertain {
                raw_os_error: error.raw_os_error(),
            },
        };
        // This permanently disarms numeric group signalling, including Drop during a later wait.
        self.signal_cleanup = Some(cleanup);
        let _ = self.child_mut().start_kill();
        cleanup
    }

    async fn observe_exit(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        let result = observe_unreaped_exit(self.child_pid).await;
        #[cfg(not(unix))]
        let result = self.child_mut().wait().await.map(|_| ());
        #[cfg(unix)]
        if let Err(error) = &result
            && error.raw_os_error() == Some(libc::ECHILD)
        {
            // An external reaper or SIGCHLD disposition invalidated our anchor.
            // Never signal an identity whose ownership is no longer provable.
            self.armed = false;
            self.metrics.lock().cleanup = ProcessCleanup::Uncertain {
                raw_os_error: error.raw_os_error(),
            };
        }
        result
    }

    #[cfg(unix)]
    async fn graceful_stop(&self, grace: Duration) {
        let _ = signal_process_group(self.child_pid, libc::SIGTERM);
        wait_group_grace(self.child_pid, grace).await;
    }

    /// All group signals precede leader reaping. Delivery alone never proves completion.
    async fn finish(&mut self) -> std::io::Result<(std::process::ExitStatus, ProcessCleanup)> {
        let signalled = self.kill_tree();
        #[cfg(target_os = "linux")]
        let stopped = wait_stopped(|| group_quiescent(self.child_pid)).await;
        let status = match self.child_mut().wait().await {
            Ok(status) => status,
            Err(error) => {
                self.armed = false;
                self.metrics.lock().cleanup = ProcessCleanup::Uncertain {
                    raw_os_error: error.raw_os_error(),
                };
                return Err(error);
            }
        };
        #[cfg(all(unix, not(target_os = "linux")))]
        let stopped = wait_stopped(|| group_absent(self.child_pid)).await;
        #[cfg(windows)]
        let stopped = wait_stopped(|| job_quiescent(self.job.as_deref())).await;
        #[cfg(not(any(unix, windows)))]
        let stopped = Err(std::io::ErrorKind::Unsupported.into());
        self.armed = false;
        Ok((status, cleanup_from_proof(signalled, stopped)))
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let signalled = self.kill_tree();
        let Some(child) = self.child.take() else {
            return;
        };
        {
            let mut metrics = self.metrics.lock();
            finish_observation(&mut metrics, ObservationIncompleteReason::RuntimeLost);
            metrics.cleanup = signalled;
        }
        let metrics = Arc::clone(&self.metrics);
        let child_pid = self.child_pid;
        #[cfg(windows)]
        let job = self.job.clone();
        // Keep Child owned while native waitpid runs; kill_on_drop is disabled, so
        // dropping the stale Tokio wrapper after native reaping cannot kill a reused PID.
        let reaper = std::thread::Builder::new()
            .name("tc-probe-reaper".into())
            .spawn(move || {
                #[cfg(target_os = "linux")]
                let stopped = wait_stopped_blocking(|| group_quiescent(child_pid));
                #[cfg(unix)]
                let reaped = reap_pid(child_pid);
                #[cfg(all(unix, not(target_os = "linux")))]
                let stopped = wait_stopped_blocking(|| group_absent(child_pid));
                #[cfg(windows)]
                let reaped = {
                    use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
                    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
                    // SAFETY: Child owns this exact process handle throughout the wait.
                    child.raw_handle().map_or(Ok(()), |handle| {
                        if unsafe { WaitForSingleObject(handle as HANDLE, INFINITE) }
                            == WAIT_OBJECT_0
                        {
                            Ok(())
                        } else {
                            Err(std::io::Error::last_os_error())
                        }
                    })
                };
                #[cfg(windows)]
                let stopped = wait_stopped_blocking(|| job_quiescent(job.as_deref()));
                #[cfg(not(any(unix, windows)))]
                let (reaped, stopped): (
                    std::io::Result<()>,
                    std::io::Result<bool>,
                ) = (
                    Err(std::io::ErrorKind::Unsupported.into()),
                    Err(std::io::ErrorKind::Unsupported.into()),
                );
                let _ = child_pid;
                drop(child);
                metrics.lock().cleanup = cleanup_from_proof(signalled, reaped.and(stopped));
            });
        if let Err(error) = reaper {
            self.metrics.lock().cleanup = ProcessCleanup::Uncertain {
                raw_os_error: error.raw_os_error(),
            };
        }
    }
}

#[cfg(unix)]
fn signal_process_group(pgid: u32, signal: i32) -> std::io::Result<()> {
    let pgid = i32::try_from(pgid).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    if pgid <= 0 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    // SAFETY: a checked positive group id is negated for native group signaling.
    if unsafe { libc::kill(-pgid, signal) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(unix)]
fn group_quiescent(pgid: u32) -> std::io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        // The unreaped leader pins PGID throughout this scan. A zombie-only group
        // is stopped; kill(-pgid, 0) cannot distinguish it from runnable members.
        // Any unreadable, malformed, or incomplete scan fails closed.
        let deadline = std::time::Instant::now() + Duration::from_millis(20);
        let mut anchor_seen = false;
        for (index, entry) in std::fs::read_dir("/proc")?.enumerate() {
            if index >= 32_768 || std::time::Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            let entry = entry?;
            let Some(member_pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Some(bytes) =
                checked_stat_read(pgid, member_pid, std::fs::read(entry.path().join("stat")))?
            else {
                continue;
            };
            let end = bytes
                .iter()
                .rposition(|byte| *byte == b')')
                .ok_or(std::io::ErrorKind::InvalidData)?;
            let fields = std::str::from_utf8(&bytes[end + 1..])
                .map_err(|_| std::io::ErrorKind::InvalidData)?;
            let mut fields = fields.split_ascii_whitespace();
            let state = fields.next().ok_or(std::io::ErrorKind::InvalidData)?;
            let group_field = fields.nth(1).ok_or(std::io::ErrorKind::InvalidData)?;
            let in_group = stat_group_matches(group_field, member_pid, pgid)?;
            anchor_seen |= member_pid == pgid;
            if in_group && !matches!(state, "Z" | "X") {
                return Ok(false);
            }
        }
        if !anchor_seen {
            return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
        }
        Ok(true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pgid;
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

#[cfg(target_os = "linux")]
fn checked_stat_read(
    pgid: u32,
    member_pid: u32,
    result: std::io::Result<Vec<u8>>,
) -> std::io::Result<Option<Vec<u8>>> {
    match result {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // procfs may report ESRCH after an enumerated non-anchor PID disappears.
        Err(error) if member_pid != pgid && error.raw_os_error() == Some(libc::ESRCH) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn stat_group_matches(field: &str, member_pid: u32, pgid: u32) -> std::io::Result<bool> {
    // Linux stat field 5 is signed; a departing unrelated process can report -1.
    let group: i32 = field.parse().map_err(|_| std::io::ErrorKind::InvalidData)?;
    let in_group = i64::from(group) == i64::from(pgid);
    if member_pid == pgid && !in_group {
        return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
    }
    Ok(in_group)
}

#[cfg(unix)]
fn reap_pid(pid: u32) -> std::io::Result<()> {
    let pid = i32::try_from(pid).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    loop {
        // SAFETY: wait for exactly our direct child; status is intentionally unused.
        if unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) } >= 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => {}
            _ => return Err(error),
        }
    }
}

#[cfg(unix)]
async fn observe_unreaped_exit(pid: u32) -> std::io::Result<()> {
    let mut changes = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child())?;
    loop {
        // Register before checking, so a concurrent exit cannot lose its notification.
        if exited_without_reaping(pid)? {
            return Ok(());
        }
        changes.recv().await.ok_or(std::io::ErrorKind::BrokenPipe)?;
    }
}

#[cfg(unix)]
fn exited_without_reaping(pid: u32) -> std::io::Result<bool> {
    loop {
        // SAFETY: waitid initializes siginfo; WNOWAIT retains our child as the PGID anchor.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &raw mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(unix)]
async fn wait_group_grace(pgid: u32, grace: Duration) {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        match group_quiescent(pgid) {
            Ok(true) => return,
            Err(_) => {
                tokio::time::sleep_until(deadline).await;
                return;
            }
            Ok(false) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep_until(
            deadline.min(tokio::time::Instant::now() + Duration::from_millis(5)),
        )
        .await;
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn group_absent(pgid: u32) -> std::io::Result<bool> {
    let pgid = i32::try_from(pgid).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    if pgid <= 0 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    // This read-only check occurs after the final signal and reap. A reused PGID
    // can only make the result conservative; it is never signalled again.
    if unsafe { libc::kill(-pgid, 0) } == 0 {
        return Ok(false);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(true)
    } else {
        Err(error)
    }
}

#[cfg(windows)]
fn job_quiescent(job: Option<&JobHandle>) -> std::io::Result<bool> {
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject,
    };
    let job = job.ok_or(std::io::ErrorKind::Unsupported)?;
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the borrowed job handle stays live and the output buffer has the exact API layout.
    if unsafe {
        QueryInformationJobObject(
            job.0 as windows_sys::Win32::Foundation::HANDLE,
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast(),
            u32::try_from(std::mem::size_of_val(&accounting))
                .expect("Win32 accounting size fits u32"),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(accounting.ActiveProcesses == 0)
}

async fn wait_stopped(mut check: impl FnMut() -> std::io::Result<bool>) -> std::io::Result<bool> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
    let mut last_timeout = None;
    loop {
        match check() {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                last_timeout = Some(error);
            }
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            return last_timeout.map_or(Ok(false), Err);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn wait_stopped_blocking(
    mut check: impl FnMut() -> std::io::Result<bool>,
) -> std::io::Result<bool> {
    let deadline = std::time::Instant::now() + Duration::from_millis(100);
    loop {
        if check()? {
            return Ok(true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(windows)]
fn terminate_job(job: &JobHandle) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;
    // SAFETY: caller owns the live job handle for the duration of the call.
    if unsafe { TerminateJobObject(job.0 as HANDLE, 1) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
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
    ///   signals the whole group with native `kill(2)`. An ownership guard
    ///   performs forced cleanup and reaping when the runtime disappears.
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
        Self::spawn_with_environment(argv, config, rings, runtime, sink, EnvironmentMode::Inherit)
    }

    /// Spawn with an explicit environment policy. Clearing does not inject host
    /// environment variables; callers supply any OS essentials in `config.env`.
    #[allow(clippy::too_many_lines)]
    pub fn spawn_with_environment(
        argv: &[String],
        config: &ProcessProbeConfig,
        rings: Arc<ContextRingManager>,
        runtime: Arc<SifterRuntime>,
        sink: Arc<dyn EventSink>,
        environment: EnvironmentMode,
    ) -> Result<Self, ProcessProbeError> {
        // Validate runtime before rings, governor preparation, or process creation.
        let runtime_handle =
            tokio::runtime::Handle::try_current().map_err(|_| ProcessProbeError::MissingRuntime)?;
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
        if environment == EnvironmentMode::Clear {
            cmd.env_clear();
        } else {
            terminal_commander_core::as_daemon_child(cmd.as_std_mut());
        }
        cmd.args(&argv[1..]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::null());
        // OwnedProcess handles every drop path, including native reaping after runtime loss.
        cmd.kill_on_drop(false);
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        #[cfg(unix)]
        cmd.process_group(0);
        #[cfg(windows)]
        terminal_commander_core::windows_silent(cmd.as_std_mut());

        #[cfg(unix)]
        let mut unix_gov = crate::governor::UnixGovernor::prepare(&config.limits, probe_id);
        #[cfg(unix)]
        if let Some(gov) = &unix_gov {
            // SAFETY: only async-signal-safe syscalls on precomputed values.
            unsafe {
                cmd.pre_exec(gov.pre_exec_hook());
            }
        }
        let child = cmd.spawn()?;
        let child_pid = child.id().expect("child has pid immediately after spawn");
        let metrics = Arc::new(Mutex::new(ProcessProbeMetrics::default()));
        let mut owned = OwnedProcess {
            child: Some(child),
            child_pid,
            metrics: Arc::clone(&metrics),
            #[cfg(windows)]
            job: None,
            armed: true,
            signal_cleanup: None,
        };
        #[cfg(windows)]
        let (job, governor, limit_watch) = {
            let (job, report, watch) =
                crate::governor::govern_child(owned.child_mut().raw_handle(), &config.limits);
            let job = job.map(Arc::new);
            owned.job.clone_from(&job);
            (job, report, watch)
        };
        #[cfg(unix)]
        let governor = unix_gov.as_mut().map_or_else(
            || Arc::new(Mutex::new(GovernorReport::default())),
            |gov| {
                gov.attach(Some(child_pid));
                gov.report()
            },
        );
        #[cfg(windows)]
        let governor_for_task = Arc::clone(&governor);
        #[cfg(target_os = "linux")]
        let cpu_sampler = crate::job_cpu::JobCpuSampler::new(
            probe_id,
            child_pid,
            unix_gov
                .as_ref()
                .and_then(crate::governor::UnixGovernor::cgroup_path),
        );
        #[cfg(windows)]
        let cpu_sampler = crate::job_cpu::JobCpuSampler::new(probe_id, child_pid, job.as_ref());
        #[cfg(not(any(target_os = "linux", windows)))]
        let cpu_sampler = crate::job_cpu::JobCpuSampler::new(probe_id, child_pid);
        let cpu_sampler = Arc::new(Mutex::new(cpu_sampler));

        let identity = ProcessIdentity {
            probe_id,
            child_pid,
            process_group_id: if cfg!(unix) { Some(child_pid) } else { None },
            #[cfg(unix)]
            ownership: ProcessOwnership::UnixProcessGroup,
            #[cfg(windows)]
            ownership: if job.is_some() {
                ProcessOwnership::WindowsJobObject
            } else {
                ProcessOwnership::LeaderOnly
            },
            #[cfg(not(any(unix, windows)))]
            ownership: ProcessOwnership::LeaderOnly,
        };
        let stdout = owned.child_mut().stdout.take().expect("piped stdout");
        let stderr = owned.child_mut().stderr.take().expect("piped stderr");
        let metrics_for_task = Arc::clone(&metrics);
        let noise_pipeline: SharedProbeNoisePipeline =
            Arc::new(Mutex::new(ProbeNoisePipeline::with_default_policy()));
        let bucket_id = config.bucket_id;
        let strip_ansi = config.strip_ansi;
        let grace = config.grace;
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        let join = runtime_handle.spawn(async move {
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
                Arc::clone(&metrics_for_task),
                noise_pipeline,
                strip_ansi,
            );
            let drain = async {
                tokio::join!(stdout_task, stderr_task);
            };
            tokio::pin!(drain);
            let mut drained = false;
            let mut cancelled = false;
            // Observe the leader independently of pipe EOF. A descendant retaining
            // stdout cannot hide the leader's exit indefinitely.
            loop {
                tokio::select! {
                    () = &mut drain, if !drained => { drained = true; }
                    status = owned.observe_exit() => {
                        status.map_err(ProcessProbeError::Io)?;
                        break;
                    }
                    _ = &mut cancel_rx => {
                        cancelled = true;
                        #[cfg(unix)]
                        owned.graceful_stop(grace).await;
                        break;
                    }
                }
            }
            if !drained && !cancelled {
                // Keep the Unix leader unreaped while descendants retain the pipes.
                drained = tokio::time::timeout(grace, &mut drain).await.is_ok();
            }
            let (status, cleanup) = owned.finish().await.map_err(ProcessProbeError::Io)?;
            if !drained {
                // Closing a group/job should release pipes immediately; still bound
                // escaped descendants instead of allowing them to hang completion.
                drained = tokio::time::timeout(Duration::from_millis(100), &mut drain)
                    .await
                    .is_ok();
            }
            let mut snapshot = metrics_for_task.lock();
            if !drained {
                finish_observation(
                    &mut snapshot,
                    if cancelled {
                        ObservationIncompleteReason::Cancelled
                    } else {
                        ObservationIncompleteReason::DrainTimeout
                    },
                );
            }
            snapshot.cleanup = cleanup;
            let observation = snapshot.observation;
            drop(snapshot);
            #[cfg(windows)]
            crate::governor::finish_job(
                &governor_for_task,
                owned.job.as_deref(),
                limit_watch,
                cancelled || !status.success(),
            );
            #[cfg(unix)]
            if let Some(gov) = unix_gov {
                gov.finish(cancelled || !status.success());
            }
            Ok(ProcessProbeReport {
                exit_status: status,
                cancelled,
                observation,
                cleanup,
            })
        });
        Ok(Self {
            probe_id,
            metrics,
            cancel_tx: Some(cancel_tx),
            join: Some(join),
            identity,
            cpu_sampler,
            governor,
            report: None,
        })
    }

    /// Resource governor report. `mode == None` for default limits. Valid
    /// after exit; before exit mode and limit are known, peak is `None`.
    #[must_use]
    pub fn governor_report(&self) -> GovernorReport {
        self.governor.lock().clone()
    }

    /// Portable leader PID; use `identity` for the process-group/ownership scope.
    #[must_use]
    pub const fn child_pid(&self) -> u32 {
        self.identity.child_pid
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

    pub async fn wait(&mut self) -> Result<std::process::ExitStatus, ProcessProbeError> {
        let report = self.wait_report().await?;
        if report.cancelled {
            Err(ProcessProbeError::Cancelled)
        } else {
            Ok(report.exit_status)
        }
    }

    /// Cancellation-safe wait retaining both child exit and pipe observation.
    /// Dropping this future does not detach the lifecycle task from its owner.
    pub async fn wait_report(&mut self) -> Result<ProcessProbeReport, ProcessProbeError> {
        if let Some(report) = &self.report {
            return Ok(report.clone());
        }
        let handle = self.join.as_mut().ok_or(ProcessProbeError::Cancelled)?;
        let result = handle.await;
        self.join.take();
        let report =
            result.map_err(|e| ProcessProbeError::Io(std::io::Error::other(e.to_string())))??;
        self.report = Some(report.clone());
        Ok(report)
    }

    #[must_use]
    pub const fn identity(&self) -> ProcessIdentity {
        self.identity
    }

    #[must_use]
    pub fn observation(&self) -> ProcessObservation {
        self.metrics.lock().observation
    }

    #[must_use]
    pub fn cpu_sample(&self) -> crate::job_cpu::JobCpuSample {
        self.cpu_sampler.lock().sample()
    }

    #[must_use]
    pub fn cpu_sampler_handle(&self) -> Arc<Mutex<crate::job_cpu::JobCpuSampler>> {
        Arc::clone(&self.cpu_sampler)
    }
}

#[cfg(unix)]
pub(crate) async fn terminate_process_tree_graceful(
    child: &mut tokio::process::Child,
    grace: Duration,
    pgid: u32,
) {
    // PTY caller also retains the unreaped leader until every group signal is done.
    if child.id() != Some(pgid) || exited_without_reaping(pgid).is_err() {
        return;
    }
    let _ = signal_process_group(pgid, libc::SIGTERM);
    wait_group_grace(pgid, grace).await;
    let _ = signal_process_group(pgid, libc::SIGKILL);
    let _ = child.start_kill();
    let _ = child.wait().await;
}

fn cleanup_from_proof(signalled: ProcessCleanup, stopped: std::io::Result<bool>) -> ProcessCleanup {
    match stopped {
        Ok(true) => ProcessCleanup::Complete,
        Err(error) => ProcessCleanup::Uncertain {
            raw_os_error: error.raw_os_error(),
        },
        Ok(false) => match signalled {
            ProcessCleanup::Uncertain { raw_os_error } => {
                ProcessCleanup::Uncertain { raw_os_error }
            }
            _ => ProcessCleanup::Uncertain { raw_os_error: None },
        },
    }
}

/// Create a Win32 Job Object carrying `limits`, so the OS tears down the whole
/// descendant tree on `TerminateJobObject` / handle close.
///
/// Configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so closing the handle
/// (the `JobHandle` `Drop`) also kills the tree -- defense in depth if the
/// probe is dropped without an explicit cancel. The resource governor's
/// `limits` are set on the same job (nothing extra for default limits).
/// Returns `Err(reason)` (fixed text plus the Win32 code) on a null job handle
/// or a `SetInformationJobObject` failure; the handle is closed on error.
#[cfg(windows)]
pub(crate) fn create_job(limits: &JobLimits) -> Result<JobHandle, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // SAFETY: `CreateJobObjectW` with two null pointers (default security
    // attributes, unnamed job) returns a new Job Object handle or null on
    // failure. We check for null before using it.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        // SAFETY: GetLastError reads thread-local state only.
        let err = unsafe { GetLastError() };
        return Err(format!("CreateJobObjectW failed: {err}"));
    }
    // Owned from here: an early return closes the handle exactly once.
    let job = JobHandle(raw as isize);

    // Configure kill-on-close as defense in depth: closing the handle kills the
    // whole tree even if the explicit `TerminateJobObject` never runs.
    // SAFETY: `info` is a fully owned, zeroed `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
    // (a `#[repr(C)]` POD). `SetInformationJobObject` reads exactly
    // `size_of::<...>()` bytes from `&info` into the kernel for the
    // `JobObjectExtendedLimitInformation` class.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // Resource governor: memory + priority limits on the same job. A default
    // `limits` adds nothing.
    crate::governor::apply_job_limits(&mut info, limits);
    let info_size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .expect("JOBOBJECT_EXTENDED_LIMIT_INFORMATION size fits in u32");
    let set_ok = unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            info_size,
        )
    };
    if set_ok == 0 {
        // SAFETY: GetLastError reads thread-local state only.
        let err = unsafe { GetLastError() };
        return Err(format!("SetInformationJobObject failed: {err}"));
    }
    Ok(job)
}

/// Assign `child` to `job`; from then on every process the child spawns is in
/// the job too. A child already in another job nests that job's hierarchy
/// (Windows 8+). `Err(reason)` carries the Win32 code.
#[cfg(windows)]
pub(crate) fn assign_to_job(
    job: &JobHandle,
    child_handle: std::os::windows::io::RawHandle,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
    // SAFETY: `job.0` is our live Job Object handle and `child_handle` is the
    // child's live OS process handle (owned by the caller's child, borrowed
    // here). The BOOL result is checked.
    let ok = unsafe { AssignProcessToJobObject(job.0 as HANDLE, child_handle as HANDLE) };
    if ok == 0 {
        // SAFETY: GetLastError reads thread-local state only.
        let err = unsafe { GetLastError() };
        return Err(format!("AssignProcessToJobObject failed: {err}"));
    }
    Ok(())
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
    // Transcode a UTF-16 child stream (e.g. `wsl.exe --list --verbose`, which
    // emits UTF-16LE on Windows) to UTF-8 BEFORE line splitting. Without this,
    // the `\n` scan below hits the `0A 00` of a UTF-16 newline and desyncs the
    // stream into NUL-riddled garbage. Non-UTF-16 streams latch pass-through on
    // the first chunk and this adds no cost. Mirrors the strip_ansi seam: a
    // small focused decoder wired at the read boundary, one layer earlier
    // because the encoding decides how bytes group into lines.
    let stream = ObservedReader {
        inner: stream,
        kind: kind.clone(),
        metrics: Arc::clone(&metrics),
    };
    let mut reader = BufReader::new(crate::utf16::Utf16Decoder::new(stream));
    let mut line_no: u64 = 0;
    loop {
        // Invalid UTF-8 does not end capture. Pipe errors and EOF stop framing;
        // ObservedReader records their distinct typed terminal states first.
        let Ok(Some(LineRead {
            bytes: raw,
            dropped,
        })) = read_line_bounded(&mut reader).await
        else {
            break;
        };
        line_no = line_no.saturating_add(1);
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
            m.last_frame_at = Some(std::time::Instant::now());
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
                &terminal_commander_core::SourceType::Process,
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

#[cfg(all(test, unix))]
mod ownership_cleanup_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[cfg(target_os = "linux")]
    #[test]
    fn disappearing_non_anchor_stat_is_skipped_but_anchor_and_permission_errors_fail_closed() {
        let anchor = 100;
        let disappeared = || Err(std::io::Error::from_raw_os_error(libc::ESRCH));
        assert_eq!(
            checked_stat_read(anchor, anchor + 1, disappeared()).unwrap(),
            None
        );
        assert_eq!(
            checked_stat_read(anchor, anchor, disappeared())
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            checked_stat_read(
                anchor,
                anchor + 1,
                Err(std::io::Error::from_raw_os_error(libc::EACCES)),
            )
            .unwrap_err()
            .raw_os_error(),
            Some(libc::EACCES)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn signed_proc_group_field_preserves_membership_and_malformed_errors() {
        assert!(stat_group_matches("100", 100, 100).unwrap());
        assert!(!stat_group_matches("-1", 101, 100).unwrap());
        assert_eq!(
            stat_group_matches("-1", 100, 100)
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(
            stat_group_matches("malformed", 101, 100)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn signal_delivery_does_not_claim_cleanup_before_leader_reap() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "echo READY; exec sleep 60"])
            .process_group(0)
            .stdout(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let child_pid = child.id().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .await
            .unwrap();
        assert_eq!(line.trim(), "READY");
        let mut owned = OwnedProcess {
            child: Some(child),
            child_pid,
            metrics: Arc::new(Mutex::new(ProcessProbeMetrics::default())),
            armed: true,
            signal_cleanup: None,
        };
        let cleanup = owned.kill_tree();
        assert_eq!(
            cleanup,
            ProcessCleanup::Reaping,
            "delivered SIGKILL alone does not prove leader reap or group termination"
        );
        let failed_proof =
            wait_stopped(|| Err(std::io::Error::from_raw_os_error(libc::EACCES))).await;
        assert_eq!(
            cleanup_from_proof(cleanup, failed_proof),
            ProcessCleanup::Uncertain {
                raw_os_error: Some(libc::EACCES),
            }
        );
        owned.child_mut().wait().await.unwrap();
        // An invalid sentinel makes any repeated native group signal fail safely;
        // the cached result proves cleanup never signals again after leader reap.
        owned.child_pid = u32::MAX;
        assert_eq!(owned.kill_tree(), cleanup);
        owned.armed = false;
    }

    #[tokio::test(start_paused = true)]
    async fn wait_stopped_retries_transient_timed_out_proof() {
        let mut checks = 0;
        let stopped = wait_stopped(|| {
            checks += 1;
            if checks == 1 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "injected transient proof timeout",
                ))
            } else {
                Ok(true)
            }
        })
        .await;

        assert!(matches!(stopped, Ok(true)));
        assert_eq!(checks, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_stopped_preserves_timed_out_proof_after_deadline() {
        let mut checks = 0;
        let stopped = wait_stopped(|| {
            checks += 1;
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "injected persistent proof timeout",
            ))
        })
        .await;

        assert_eq!(stopped.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(checks > 1);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_stopped_propagates_non_timeout_proof_error() {
        let mut checks = 0;
        let stopped = wait_stopped(|| {
            checks += 1;
            Err(std::io::Error::from_raw_os_error(libc::EACCES))
        })
        .await;

        assert_eq!(stopped.unwrap_err().raw_os_error(), Some(libc::EACCES));
        assert_eq!(checks, 1);
    }
}

/// Maximum bytes retained for a single logical line before overflow is discarded.
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
mod observation_tests {
    use super::*;

    struct FailAfterBytes {
        sent: bool,
    }

    impl AsyncRead for FailAfterBytes {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.sent {
                Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "untrusted pipe error text",
                )))
            } else {
                self.sent = true;
                buf.put_slice(b"partial");
                Poll::Ready(Ok(()))
            }
        }
    }

    #[tokio::test]
    async fn read_failure_after_partial_bytes_preserves_typed_stream_failure() {
        for kind in [SourceStream::Stdout, SourceStream::Stderr] {
            let metrics = Arc::new(Mutex::new(ProcessProbeMetrics::default()));
            let probe_id = ProbeId::new();
            let rings = Arc::new(ContextRingManager::new());
            rings.create_ring_default(probe_id).unwrap();
            read_stream(
                FailAfterBytes { sent: false },
                probe_id,
                kind.clone(),
                BucketId::new(),
                rings,
                Arc::new(SifterRuntime::build(&[]).unwrap()),
                Arc::new(InMemorySink::new()),
                Arc::clone(&metrics),
                Arc::new(Mutex::new(ProbeNoisePipeline::with_default_policy())),
                true,
            )
            .await;
            let metrics = metrics.lock();
            assert_eq!(metrics.bytes_total, 7);
            assert!(metrics.last_output_at.is_some());
            assert_eq!(metrics.frames_total, 0);
            let stream = if kind == SourceStream::Stdout {
                metrics.observation.stdout
            } else {
                metrics.observation.stderr
            };
            assert_eq!(stream.bytes_total, 7);
            assert_eq!(
                stream.state,
                StreamObservationState::Failed(PipeReadFailure {
                    kind: PipeReadErrorKind::ConnectionReset,
                    raw_os_error: None,
                })
            );
            assert!(!metrics.observation.is_complete());
            assert!(!format!("{:?}", metrics.observation).contains("untrusted"));
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
            // Invalid-UTF-8 lead/continuation bytes as arbitrary noise. The
            // prefix must NOT begin with `FF FE`/`FE FF`: those are the UTF-16
            // BOM, which the read layer now (correctly) latches and transcodes
            // as UTF-16 -- pairing the trailing `\n` into a code unit and
            // yielding one un-split line. This test is about mid-stream invalid
            // UTF-8 surviving capture, not encoding detection, so the BOM bytes
            // sit AFTER a non-BOM lead byte (UTF-16 detection is a separate
            // test: `utf16le_wsl_output_yields_clean_lines`).
            let argv = argv_raw_prefix_then_line(&[0xC0, 0x00, 0xFF, 0xFE], "WARN: real line");
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

    // --- UTF-16LE transcode through the real byte->line pipeline (wsl.exe) ---

    /// An `AsyncRead` that yields its data in fixed-size slices so a UTF-16
    /// code unit can be forced to straddle a `poll_read` boundary -- the same
    /// fragmentation a real pipe produces. `chunk == usize::MAX` means "all at
    /// once" (single-chunk, like a small slice read).
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }
    impl tokio::io::AsyncRead for ChunkedReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let remaining = self.data.len() - self.pos;
            if remaining == 0 {
                return std::task::Poll::Ready(Ok(()));
            }
            let n = remaining.min(self.chunk).min(buf.remaining());
            let start = self.pos;
            let slice = self.data[start..start + n].to_vec();
            buf.put_slice(&slice);
            self.pos += n;
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// Encode `s` as UTF-16LE bytes, optionally prefixed with the LE BOM.
    fn utf16le(s: &str, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend_from_slice(&[0xFF, 0xFE]);
        }
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    /// Drive the EXACT byte->line pipeline `read_stream` uses -- the UTF-16
    /// decoder, the `BufReader`, and `read_line_bounded` -- and collect the
    /// decoded, CR-trimmed lines. `chunk` controls how the raw bytes are
    /// fragmented into `poll_read` calls.
    async fn pipeline_lines(raw: Vec<u8>, chunk: usize) -> Vec<String> {
        let decoder = crate::utf16::Utf16Decoder::new(ChunkedReader {
            data: raw,
            pos: 0,
            chunk,
        });
        let mut reader = BufReader::new(decoder);
        let mut lines = Vec::new();
        while let Ok(Some(LineRead { bytes, .. })) = read_line_bounded(&mut reader).await {
            let text = String::from_utf8_lossy(&bytes);
            lines.push(text.trim_end_matches('\r').to_owned());
        }
        lines
    }

    /// Acceptance (1): a UTF-16LE `wsl.exe --list --verbose`-shaped stream must
    /// yield clean UTF-8 lines through the whole pipeline -- with a BOM, without
    /// a BOM (heuristic), and with a code unit split across a chunk boundary.
    #[test]
    fn utf16le_wsl_output_yields_clean_lines() {
        let runtime = rt();
        runtime.block_on(async {
            // CRLF like real Windows tool output; the pipeline trims the CR.
            let wsl = "  NAME            STATE           VERSION\r\n\
                       * Ubuntu          Running         2\r\n\
                         docker-desktop  Stopped         2\r\n";
            let expected: Vec<String> = wsl
                .split_terminator('\n')
                .map(|l| l.trim_end_matches('\r').to_owned())
                .collect();

            // (a) With BOM, single chunk.
            let with_bom = pipeline_lines(utf16le(wsl, true), usize::MAX).await;
            assert_eq!(with_bom, expected, "BOM'd UTF-16LE must decode cleanly");

            // (b) Without BOM: the heuristic must latch UTF-16LE.
            let no_bom = pipeline_lines(utf16le(wsl, false), usize::MAX).await;
            assert_eq!(no_bom, expected, "BOM-less UTF-16LE must be detected");

            // (c) Odd chunk size splits code units (incl. the `0A 00` newline)
            //     across poll_read boundaries -- the carry must reassemble them.
            let split = pipeline_lines(utf16le(wsl, true), 3).await;
            assert_eq!(split, expected, "split code units must reassemble");

            // No line contains a stray NUL or the desync artifacts of a raw
            // UTF-16 byte scan.
            for line in &with_bom {
                assert!(!line.contains('\u{0}'), "no NUL bytes: {line:?}");
            }
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

    // ---- US3b (T038): grace-ladder cancel (SIGTERM-then-SIGKILL) ----

    /// Spawn `sh -c <script>` as a command probe with the given grace window.
    /// Unix-only: the grace ladder's graceful step is SIGTERM, which is a POSIX
    /// concept. Used by the cancel-ladder tests below.
    #[cfg(unix)]
    fn spawn_sh_with_grace(script: &str, grace: Duration) -> ProcessProbe {
        let rings = Arc::new(ContextRingManager::new());
        let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
        let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
        let mut cfg = ProcessProbeConfig::for_bucket(BucketId::new());
        cfg.grace = grace;
        ProcessProbe::spawn(
            &["sh".to_owned(), "-c".to_owned(), script.to_owned()],
            &cfg,
            rings,
            sifter,
            sink,
        )
        .expect("spawn ok")
    }

    #[cfg(unix)]
    #[test]
    fn cancel_ladder_sigterm_handler_exits_within_grace_no_sigkill() {
        // T038: a child that HANDLES SIGTERM and exits must be reaped during the
        // grace window WITHOUT a SIGKILL. The child traps TERM and exits 0 on it,
        // then sleeps far longer than the grace window. If the graceful SIGTERM
        // works, cancellation completes in well under `grace`; if it did NOT work,
        // the child would survive until SIGKILL at `grace` (here 5s) -- so a fast
        // completion is the proof the SIGTERM path drove the exit.
        let runtime = rt();
        runtime.block_on(async {
            let grace = Duration::from_secs(5);
            // `trap 'exit 0' TERM` + a long sleep; `echo READY` then sleep so the
            // trap is installed before we cancel.
            let mut probe = spawn_sh_with_grace("trap 'exit 0' TERM; echo READY; sleep 30", grace);
            // Give the shell a moment to install the trap.
            tokio::time::sleep(Duration::from_millis(300)).await;

            let start = std::time::Instant::now();
            probe.cancel();
            let outcome = probe.wait().await;
            let elapsed = start.elapsed();

            assert!(
                matches!(outcome, Err(ProcessProbeError::Cancelled)),
                "cancel must report the terminal Cancelled state; got {outcome:?}"
            );
            // The cooperative exit must land well inside the grace window. A
            // generous 3s ceiling (< the 5s grace) keeps the assertion robust on
            // a slow CI runner while still proving SIGKILL-at-grace was NOT what
            // ended it.
            assert!(
                elapsed < Duration::from_secs(3),
                "a SIGTERM-handling child must exit during grace (no wait-for-SIGKILL); \
                 cancel took {elapsed:?}, grace was {grace:?}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn cancel_ladder_sigterm_ignored_escalates_to_sigkill() {
        // T038: a child that IGNORES SIGTERM must be escalated to SIGKILL after
        // the grace window. The child traps (ignores) TERM and sleeps; a SIGKILL
        // is uncatchable, so the child can only die via the escalation. We use a
        // short grace so the test stays well under the nextest 5-min terminate
        // budget, and assert the child outlived the grace (proving the graceful
        // step was attempted and waited out) yet was still reaped (proving the
        // forced escalation fired).
        let runtime = rt();
        runtime.block_on(async {
            let grace = Duration::from_millis(700);
            // `trap '' TERM` makes SIGTERM a no-op; only SIGKILL can end it.
            let mut probe = spawn_sh_with_grace("trap '' TERM; echo READY; sleep 30", grace);
            tokio::time::sleep(Duration::from_millis(300)).await;

            let start = std::time::Instant::now();
            probe.cancel();
            let outcome = probe.wait().await;
            let elapsed = start.elapsed();

            assert!(
                matches!(outcome, Err(ProcessProbeError::Cancelled)),
                "cancel must report the terminal Cancelled state; got {outcome:?}"
            );
            // The SIGTERM was ignored, so the child could only be reaped AFTER the
            // grace window elapsed and SIGKILL was sent. Allow a little scheduling
            // slack below the grace floor.
            assert!(
                elapsed >= Duration::from_millis(500),
                "a SIGTERM-ignoring child must survive until the grace window \
                 elapses and SIGKILL escalates; cancel took {elapsed:?}, grace {grace:?}"
            );
            // And it MUST eventually be reaped (the whole point of escalation):
            // a generous upper bound that a hung kill would blow past, failing
            // fast instead of wedging.
            assert!(
                elapsed < Duration::from_secs(5),
                "SIGKILL escalation must reap the child promptly after grace; \
                 cancel took {elapsed:?}"
            );
        });
    }
}
