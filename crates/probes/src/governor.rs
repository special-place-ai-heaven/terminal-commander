// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Resource governor: per-job memory ceiling, CPU priority, and a daemon-wide
//! host ceiling.
//!
//! Enforced by the kernel primitive the host offers. The thing that enforces is the thing
//! that reports: [`GovernorReport`] is filled from the same object (Job Object,
//! cgroup) that applied the limit.
//!
//! * Windows: limits are added to the Job Object the probe already owns
//!   (`JOB_OBJECT_LIMIT_JOB_MEMORY` caps the job's summed commit charge;
//!   `JOB_OBJECT_LIMIT_PRIORITY_CLASS`). Peak = `PeakJobMemoryUsed` (reported
//!   only); hit = the kernel's `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT` messages,
//!   read from one per-daemon I/O completion port every governed job and the
//!   host job are associated with. The host
//!   ceiling is a parent Job Object: a joining child is assigned to it FIRST,
//!   then to its per-job job, which the kernel nests under the host job.
//! * Linux: a SIBLING cgroup v2 `<daemon parent cgroup>/tc-job-<pid>-<probe_id>`
//!   (or `<parent>/tc-jobs-<pid>/<probe_id>` when the job joins the host
//!   ceiling; `<pid>` = the daemon's, so daemons sharing a parent never collide)
//!   with `memory.max` and `memory.swap.max=0`. The child moves ITSELF into
//!   the cgroup in `pre_exec` (writes `0` to `cgroup.procs`), so nothing it
//!   forks can escape; the parent re-writes the pid after spawn as a checked
//!   fallback. Peak = `memory.peak`, hit = `memory.events.local` `oom > 0`
//!   with a non-success exit. When the boot-time probe finds the parent
//!   cgroup not writable (WSL `/non-systemd`, no systemd) the lane is
//!   `RLIMIT_DATA` set in `pre_exec` (inherited, per-process, not tree-summed;
//!   peak and hit are unknowable). That choice is made once: a per-job cgroup
//!   failure after a usable probe is `Unavailable`, never a run-time switch
//!   to rlimit. Never `RLIMIT_AS`, never `systemd-run`.
//! * Other unix: no memory primitive (`RLIMIT_DATA` does not cover
//!   mmap-backed malloc there): memory is reported `Unavailable`.
//! * Priority on unix: `setpriority` (nice 10 BelowNormal, 19 Idle) in
//!   `pre_exec`. [`JobPriority::Normal`] inherits on every platform.
//!
//! Post-spawn assignment window (Windows only): the Job Object assignment
//! happens right AFTER spawn, so a child that forks in its first instructions
//! can place a descendant outside the job. This is the same accepted window as
//! the pre-existing `AssignProcessToJobObject` tree-kill. Linux has no window:
//! the cgroup move, rlimit and nice all run in `pre_exec`, before exec. (A
//! parent-side `cgroup.procs` write alone loses this race to `sh -c`, which
//! forks its command in well under a millisecond; observed live.)
//!
//! [`JobLimits::default()`] adds no syscall and changes nothing: callers skip
//! every governor path when `limits == JobLimits::default()`.
//!
//! Reasons in [`GovernorMode::Unavailable`] never embed filesystem paths: they
//! are fixed strings plus an errno or Win32 code.

use std::sync::Arc;

use parking_lot::Mutex;

/// CPU priority for a governed job. `Normal` = inherit the daemon's priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobPriority {
    Idle,
    BelowNormal,
    #[default]
    Normal,
}

/// Per-job limits. `None` = unconstrained on that axis. Default = no
/// governance at all (today's behaviour).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobLimits {
    pub memory_bytes: Option<u64>,
    pub priority: Option<JobPriority>,
    /// Join the daemon-wide host ceiling ([`install_host_ceiling`]) when one
    /// is installed, even when `memory_bytes` and `priority` are `None`.
    pub join_host_ceiling: bool,
}

/// Which kernel primitive governs the job.
///
/// `Rlimit` also covers a priority-only unix job (nice in `pre_exec`, no
/// memory primitive needed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernorMode {
    JobObject,
    Cgroup,
    Rlimit,
    /// Enforcement could not be applied; the job runs ungoverned. Carries why.
    Unavailable(String),
}

/// What governed a job and what it used.
///
/// `mode`, `memory_limit_bytes` and `host_ceiling_joined` are final right
/// after spawn; `peak_memory_bytes` and `memory_limit_hit` are filled at exit.
/// `mode == None` when `JobLimits::default()` was passed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GovernorReport {
    pub mode: Option<GovernorMode>,
    pub memory_limit_bytes: Option<u64>,
    /// Windows: `PeakJobMemoryUsed`; Linux cgroup: `memory.peak` (None on
    /// kernels < 5.19); rlimit: None.
    pub peak_memory_bytes: Option<u64>,
    /// The kernel refused this job's OWN limit and the job exited
    /// non-success. Windows: the job posted at least one
    /// `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT` (or `..._PROCESS_MEMORY_LIMIT`) to
    /// the daemon's completion port, so a single refused allocation that
    /// never raised the peak still counts. Linux cgroup: the job dir's
    /// `memory.events.local` `oom` count > 0. Rlimit: false (unknowable).
    pub memory_limit_hit: bool,
    /// The child joined the installed host ceiling.
    pub host_ceiling_joined: bool,
    /// The host ceiling was reached while this job ran and the job exited
    /// non-success (filled at exit). An inference, never a guess: unknown is
    /// reported as `false`.
    /// * Windows: the host job's kernel limit-message count on the daemon's
    ///   completion port rose between this job's spawn and exit. A count, not
    ///   a peak, so every later crossing is seen too.
    /// * Linux cgroup: the host dir's `memory.events.local` `oom` count rose
    ///   between spawn and exit.
    /// * Rlimit / no host ceiling joined: `false`.
    ///
    /// Either way a concurrent job reaching the ceiling during this job's
    /// life can flag a job that failed for another reason.
    pub host_ceiling_hit: bool,
}

/// Host memory totals for percent resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostMemory {
    pub total_bytes: u64,
    /// Windows: the commit limit (`ullTotalPageFile`). Linux: None.
    pub commit_limit_bytes: Option<u64>,
}

/// Shared report: written by the probe's lifecycle task at exit, read by
/// `governor_report()`.
pub(crate) type SharedReport = Arc<Mutex<GovernorReport>>;

/// The initial report for `limits`: `mode` filled by the caller once known.
pub(crate) fn new_report(limits: &JobLimits) -> SharedReport {
    Arc::new(Mutex::new(GovernorReport {
        memory_limit_bytes: limits.memory_bytes,
        ..GovernorReport::default()
    }))
}

/// Host memory totals. `None` when the platform query fails or is unsupported.
#[cfg(windows)]
#[must_use]
pub fn host_memory() -> Option<HostMemory> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: `status` is a zeroed `#[repr(C)]` POD with `dwLength` set as the
    // API requires; `GlobalMemoryStatusEx` writes only into it. The BOOL result
    // is checked before any field is read.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?;
    let ok = unsafe { GlobalMemoryStatusEx(&raw mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(HostMemory {
        total_bytes: status.ullTotalPhys,
        commit_limit_bytes: Some(status.ullTotalPageFile),
    })
}

/// Host memory totals. `None` when the platform query fails or is unsupported.
#[cfg(target_os = "linux")]
#[must_use]
pub fn host_memory() -> Option<HostMemory> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kib: u64 = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    (kib > 0).then_some(HostMemory {
        total_bytes: kib.saturating_mul(1024),
        commit_limit_bytes: None,
    })
}

/// Host memory totals. `None` when the platform query fails or is unsupported.
#[cfg(not(any(windows, target_os = "linux")))]
#[must_use]
pub const fn host_memory() -> Option<HostMemory> {
    None
}

/// The mode a governed job would get on this host, for status reporting.
/// Always an explicit call: nothing probes at module init.
///
/// Windows: `JobObject`. Linux: `Cgroup` when a sibling cgroup with a
/// delegated memory controller can actually be created (probed once with a
/// real mkdir + rmdir, then cached), else `Rlimit`. Other unix: memory is
/// `Unavailable`. A per-job attempt can still downgrade; the job's report is
/// authoritative.
#[cfg(windows)]
#[must_use]
pub const fn available_mode() -> GovernorMode {
    GovernorMode::JobObject
}

/// See the Windows variant.
#[cfg(target_os = "linux")]
#[must_use]
pub fn available_mode() -> GovernorMode {
    if cgroup::usable() {
        GovernorMode::Cgroup
    } else {
        GovernorMode::Rlimit
    }
}

/// Why non-Linux unix has no memory primitive.
#[cfg(all(unix, not(target_os = "linux")))]
const NO_RLIMIT_DATA: &str = "RLIMIT_DATA not enforced for mmap-backed malloc";

/// See the Windows variant.
#[cfg(all(unix, not(target_os = "linux")))]
#[must_use]
pub fn available_mode() -> GovernorMode {
    GovernorMode::Unavailable(NO_RLIMIT_DATA.to_owned())
}

/// See the Windows variant.
#[cfg(not(any(unix, windows)))]
#[must_use]
pub fn available_mode() -> GovernorMode {
    GovernorMode::Unavailable("no kernel primitive".to_owned())
}

/// Reason returned by [`install_host_ceiling`] when the host has no
/// primitive that sums memory across jobs.
#[cfg(not(windows))]
const NO_AGGREGATE: &str = "no aggregate primitive";

/// Install the daemon-wide host ceiling every job with
/// [`JobLimits::join_host_ceiling`] joins. Call once at daemon boot; a second
/// call is an error.
///
/// * Windows: a parent Job Object (`KILL_ON_JOB_CLOSE | JOB_MEMORY`), held for
///   the process's life. Per-job jobs nest under it (Windows 8+), one level
///   deeper when the daemon itself already runs inside a job.
/// * Linux cgroup mode: `<parent>/tc-jobs-<pid>` with `memory.max = limit_bytes`,
///   `memory.swap.max = 0` and the memory controller enabled for its children;
///   per-job dirs are created under it, never processes directly in it.
/// * Rlimit / no cgroup / other unix: `Err("no aggregate primitive")`.
///
/// # Errors
/// A reason string (fixed text plus an errno / Win32 code, never a path).
#[cfg(windows)]
pub fn install_host_ceiling(limit_bytes: u64) -> Result<GovernorMode, String> {
    let job = crate::process::create_job(&JobLimits {
        memory_bytes: Some(limit_bytes),
        ..JobLimits::default()
    })?;
    let mut host = windows_host::HOST.lock();
    if host.is_some() {
        return Err("host ceiling already installed".to_owned());
    }
    // Enforcement stands without the port; only `host_ceiling_hit` would be
    // unknowable (reported `false`).
    let watched = limit_messages::associate(&job, limit_messages::HOST_KEY);
    *host = Some((job, limit_bytes, watched));
    Ok(GovernorMode::JobObject)
}

/// See the Windows variant.
///
/// # Errors
/// A reason string (fixed text plus an errno, never a path).
#[cfg(target_os = "linux")]
pub fn install_host_ceiling(limit_bytes: u64) -> Result<GovernorMode, String> {
    cgroup::install_host(limit_bytes).map(|()| GovernorMode::Cgroup)
}

/// See the Windows variant.
///
/// # Errors
/// Always: this platform has no aggregate memory primitive.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn install_host_ceiling(limit_bytes: u64) -> Result<GovernorMode, String> {
    let _ = limit_bytes;
    Err(NO_AGGREGATE.to_owned())
}

/// The installed host ceiling in bytes, if any.
#[cfg(windows)]
#[must_use]
pub fn host_ceiling() -> Option<u64> {
    windows_host::HOST
        .lock()
        .as_ref()
        .map(|(_, limit, _)| *limit)
}

/// The installed host ceiling in bytes, if any.
#[cfg(target_os = "linux")]
#[must_use]
pub fn host_ceiling() -> Option<u64> {
    cgroup::HOST.lock().as_ref().map(|(_, limit)| *limit)
}

/// The installed host ceiling in bytes, if any.
#[cfg(not(any(windows, target_os = "linux")))]
#[must_use]
pub const fn host_ceiling() -> Option<u64> {
    None
}

/// Release the host ceiling at clean daemon shutdown.
///
/// Idempotent; a no-op when none is installed (rlimit, unavailable, other
/// unix). Afterwards [`host_ceiling`] is `None`, a joining job no longer
/// joins, and a new [`install_host_ceiling`] is allowed.
///
/// * Windows: closes the host Job Object handle; its `KILL_ON_JOB_CLOSE`
///   kills any surviving member (what daemon exit already did).
/// * Linux cgroup: writes `1` to `tc-jobs-<pid>/cgroup.kill` (defensive),
///   rmdirs remaining per-job children, then the host dir. An already
///   removed dir is success.
///
/// # Errors
/// Linux: a path-free reason when the host dir could not be removed (e.g.
/// `EBUSY`); the boot sweep of a later daemon removes it once empty.
#[cfg(windows)]
pub fn uninstall_host_ceiling() -> Result<(), String> {
    drop(windows_host::HOST.lock().take());
    Ok(())
}

/// See the Windows variant.
///
/// # Errors
/// A path-free reason when the host dir could not be removed.
#[cfg(target_os = "linux")]
pub fn uninstall_host_ceiling() -> Result<(), String> {
    cgroup::uninstall_host()
}

/// See the Windows variant.
///
/// # Errors
/// Never: no host ceiling exists on this platform.
#[cfg(not(any(windows, target_os = "linux")))]
pub const fn uninstall_host_ceiling() -> Result<(), String> {
    Ok(())
}

/// Boot-time cleanup of stale job cgroups; returns how many were removed.
///
/// Removes the `tc-job-<pid>-*`, `tc-jobs-<pid>` (with its children) and
/// `tc-probe-<pid>` dirs under our parent cgroup whose daemon pid is dead.
/// A live daemon's dirs are never touched, and the kernel refuses to rmdir
/// a populated cgroup, so only empty ones go.
#[cfg(target_os = "linux")]
pub fn sweep_stale_job_dirs() -> usize {
    cgroup::sweep()
}

/// No cgroup dirs off Linux: always 0.
#[cfg(not(target_os = "linux"))]
pub const fn sweep_stale_job_dirs() -> usize {
    0
}

// ---------------------------------------------------------------- Windows --

#[cfg(windows)]
mod windows_host {
    use parking_lot::Mutex;

    /// The host-ceiling Job Object, its limit, and whether it reports to the
    /// completion port. Held until [`super::uninstall_host_ceiling`] or
    /// process exit; dropping it fires its `KILL_ON_JOB_CLOSE`.
    pub(super) static HOST: Mutex<Option<(crate::process::JobHandle, u64, bool)>> =
        parking_lot::const_mutex(None);
}

/// Kernel memory-limit messages, the Windows limit-hit signal.
///
/// One I/O completion port per daemon process (created lazily, never
/// closed). Every governed Job Object, and the host job, is associated with it
/// under its own completion key; the kernel posts `JOB_OBJECT_MSG_*` packets
/// there. Packets are drained non-blocking (timeout 0) into a process-wide
/// per-key count whenever a governed job spawns or finishes, so draining for
/// one job never loses another job's messages.
#[cfg(windows)]
mod limit_messages {
    use std::collections::BTreeMap;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use parking_lot::Mutex;
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::IO::{
        CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
    };
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JobObjectAssociateCompletionPortInformation,
        SetInformationJobObject,
    };

    use crate::process::JobHandle;

    // winnt.h values; windows-sys only exposes them behind the large
    // `Win32_System_SystemServices` feature.
    const JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT: u32 = 9;
    const JOB_OBJECT_MSG_JOB_MEMORY_LIMIT: u32 = 10;

    /// The host-ceiling job's key; per-job keys start at 1.
    pub(super) const HOST_KEY: usize = 0;
    static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
    /// Limit messages seen per key. Holding this lock across the drain means
    /// a reader sees every message the kernel posted before its read.
    static COUNTS: Mutex<BTreeMap<usize, u64>> = parking_lot::const_mutex(BTreeMap::new());

    fn port() -> Option<HANDLE> {
        // Stored as `isize` so the static is `Sync`.
        static PORT: OnceLock<Option<isize>> = OnceLock::new();
        let raw = *PORT.get_or_init(|| {
            // SAFETY: INVALID_HANDLE_VALUE plus a null existing port creates
            // a new port tied to no file; the result is checked for null.
            let h =
                unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
            let port = (!h.is_null()).then_some(h as isize);
            if port.is_some() {
                start_periodic_drain();
            }
            port
        });
        raw.map(|h| h as HANDLE)
    }

    /// A fresh per-job key, never reused within the process.
    pub(super) fn next_key() -> usize {
        NEXT_KEY.fetch_add(1, Ordering::Relaxed)
    }

    /// Route `job`'s notifications to the port under `key`. `false` when the
    /// port or the association is unavailable (hit detection then reports
    /// `false`; the limit itself is unaffected).
    pub(super) fn associate(job: &JobHandle, key: usize) -> bool {
        let Some(port) = port() else {
            return false;
        };
        let info = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: std::ptr::without_provenance_mut(key),
            CompletionPort: port,
        };
        let Ok(size) = u32::try_from(std::mem::size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>())
        else {
            return false;
        };
        // SAFETY: `job.0` is a live Job Object handle owned by `JobHandle`
        // for this borrow; `info` is a `#[repr(C)]` POD of exactly `size`
        // bytes the kernel only reads. The BOOL result is checked.
        let ok = unsafe {
            SetInformationJobObject(
                job.0 as HANDLE,
                JobObjectAssociateCompletionPortInformation,
                std::ptr::from_ref(&info).cast(),
                size,
            )
        };
        ok != 0
    }

    /// Drain the port every 250 ms for the process's life, so a long
    /// fork-heavy job cannot pile NEW/EXIT packets up in nonpaged pool. The
    /// same non-blocking drain under the same lock: a thread blocked in
    /// `GetQueuedCompletionStatus` could hold a dequeued packet outside the
    /// lock and break "a reader sees everything posted before its read".
    fn start_periodic_drain() {
        let spawned = std::thread::Builder::new()
            .name("tc-governor-drain".to_owned())
            .spawn(|| {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    drain(&mut COUNTS.lock());
                }
            });
        // No thread: spawn/finish still drain; only the packet backlog grows.
        drop(spawned);
    }

    /// Pull every queued packet without blocking; count the limit messages.
    fn drain(counts: &mut BTreeMap<usize, u64>) {
        let Some(port) = port() else { return };
        loop {
            let mut msg = 0u32;
            let mut key = 0usize;
            let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
            // SAFETY: every out-pointer is a live local; timeout 0 never
            // blocks. For job notifications `overlapped` carries a process
            // id, not a pointer, and is never dereferenced.
            let ok = unsafe {
                GetQueuedCompletionStatus(port, &raw mut msg, &raw mut key, &raw mut overlapped, 0)
            };
            if ok == 0 {
                return;
            }
            if matches!(
                msg,
                JOB_OBJECT_MSG_JOB_MEMORY_LIMIT | JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT
            ) {
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    /// Limit messages seen for `key` so far.
    pub(super) fn count(key: usize) -> u64 {
        let mut counts = COUNTS.lock();
        drain(&mut counts);
        counts.get(&key).copied().unwrap_or(0)
    }

    /// Limit messages seen for a finished job's `key`; forgets the key.
    pub(super) fn take(key: usize) -> u64 {
        let mut counts = COUNTS.lock();
        drain(&mut counts);
        counts.remove(&key).unwrap_or(0)
    }
}

/// Completion-port state carried from [`govern_child`] to [`finish_job`].
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LimitWatch {
    /// The limited per-job job's completion key, when associated.
    key: Option<usize>,
    /// The host job's limit-message count at spawn (joined and watched).
    host_at_spawn: Option<u64>,
}

/// Add the memory and priority limits to the extended limit info the probe
/// already sets on its Job Object. A default `limits` changes nothing;
/// `Normal` priority inherits (no flag).
#[cfg(windows)]
pub(crate) fn apply_job_limits(
    info: &mut windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    limits: &JobLimits,
) {
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_PRIORITY_CLASS,
    };
    use windows_sys::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, IDLE_PRIORITY_CLASS};
    if let Some(bytes) = limits.memory_bytes {
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
        info.JobMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    let class = match limits.priority {
        Some(JobPriority::Idle) => Some(IDLE_PRIORITY_CLASS),
        Some(JobPriority::BelowNormal) => Some(BELOW_NORMAL_PRIORITY_CLASS),
        Some(JobPriority::Normal) | None => None,
    };
    if let Some(class) = class {
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
        info.BasicLimitInformation.PriorityClass = class;
    }
}

/// Put a freshly spawned child under governance (Windows, both lanes).
///
/// Order: the host-ceiling job first (when joining and installed), then the
/// per-job job, which the kernel nests under it. If the LIMITED per-job job
/// cannot be created or assigned, a kill-only job is retried so cancel keeps
/// its tree-kill, and the limit is reported `Unavailable(reason)`. Default
/// limits: only the pre-existing kill-only job, report mode `None`.
///
/// Returns the per-job job (for tree-kill and the peak query), the report
/// (mode and `host_ceiling_joined` final here), and the completion-port
/// state [`finish_job`] reads the limit messages with.
#[cfg(windows)]
pub(crate) fn govern_child(
    child: Option<std::os::windows::io::RawHandle>,
    limits: &JobLimits,
) -> (Option<crate::process::JobHandle>, SharedReport, LimitWatch) {
    use crate::process::{JobHandle, assign_to_job, create_job};
    let governed = *limits != JobLimits::default();
    let report = if governed {
        new_report(limits)
    } else {
        Arc::new(Mutex::new(GovernorReport::default()))
    };
    let Some(child) = child else {
        if governed {
            report.lock().mode = Some(GovernorMode::Unavailable(
                "child handle unavailable".to_owned(),
            ));
        }
        return (None, report, LimitWatch::default());
    };
    // The host baseline is read BEFORE the assignment: once assigned, the
    // running child's own commits can post host limit messages, which must
    // count as this job's crossing, not as the baseline. A failed host
    // assignment leaves `host_ceiling_joined` false; the per-job job below is
    // still created and its mode reported on its own.
    // Held across the baseline and the assignment so an uninstall cannot
    // close the host job in between.
    let host_guard = windows_host::HOST.lock();
    let host = host_guard.as_ref().filter(|_| limits.join_host_ceiling);
    let baseline = host
        .filter(|(_, _, watched)| *watched)
        .map(|_| limit_messages::count(limit_messages::HOST_KEY));
    let host_joined = host.is_some_and(|(host, _, _)| assign_to_job(host, child).is_ok());
    let host_at_spawn = baseline.filter(|_| host_joined);
    drop(host_guard);
    // Associate before assigning, so no limit message predates the port.
    let job_with = |l: &JobLimits, watch: bool| -> Result<(JobHandle, Option<usize>), String> {
        let job = create_job(l)?;
        let key = watch
            .then(limit_messages::next_key)
            .filter(|key| limit_messages::associate(&job, *key));
        assign_to_job(&job, child)?;
        Ok((job, key))
    };
    let (job, key, mode) = match job_with(limits, governed) {
        Ok((job, key)) => (Some(job), key, GovernorMode::JobObject),
        Err(reason) => (
            job_with(&JobLimits::default(), false)
                .ok()
                .map(|(job, _)| job),
            None,
            GovernorMode::Unavailable(reason),
        ),
    };
    if governed {
        let mut r = report.lock();
        r.mode = Some(mode);
        r.host_ceiling_joined = host_joined;
    }
    (job, report, LimitWatch { key, host_at_spawn })
}

/// Fill peak, limit-hit and host-ceiling-hit after the child exited. The
/// hits come from the kernel's limit messages ([`limit_messages`]); the peak
/// is reported only.
#[cfg(windows)]
pub(crate) fn finish_job(
    report: &SharedReport,
    job: Option<&crate::process::JobHandle>,
    watch: LimitWatch,
    abnormal_exit: bool,
) {
    let (governed, joined) = {
        let r = report.lock();
        (
            r.mode == Some(GovernorMode::JobObject),
            r.host_ceiling_joined,
        )
    };
    // Ungoverned (default limits) or Unavailable (a kill-only job carries no
    // limit to report against): no per-job query.
    let peak = job.filter(|_| governed).and_then(job_peak);
    // Always take the key, so a finished job's count never lingers.
    let own_hits = watch.key.map_or(0, limit_messages::take);
    let host_hit = joined
        && abnormal_exit
        && watch
            .host_at_spawn
            .is_some_and(|before| limit_messages::count(limit_messages::HOST_KEY) > before);
    let mut r = report.lock();
    if peak.is_some() {
        r.peak_memory_bytes = peak;
    }
    r.memory_limit_hit = governed && abnormal_exit && own_hits > 0;
    r.host_ceiling_hit = host_hit;
}

/// `PeakJobMemoryUsed` of `job`, `None` if the query fails.
#[cfg(windows)]
fn job_peak(job: &crate::process::JobHandle) -> Option<u64> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        QueryInformationJobObject,
    };
    let size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).ok()?;
    // SAFETY: `info` is a zeroed `#[repr(C)]` POD of exactly the size passed;
    // `job.0` is a live Job Object handle owned by `JobHandle` for this borrow.
    // The BOOL result is checked before `info` is read.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        QueryInformationJobObject(
            job.0 as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw mut info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(info.PeakJobMemoryUsed as u64)
}

// ------------------------------------------------------------------- unix --

/// Unix governance for one job: decided before spawn, attached after spawn,
/// finished after exit. Dropping it removes (and if needed kills) the cgroup.
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct UnixGovernor {
    report: SharedReport,
    /// Linux per-job cgroup dir, when cgroup mode is active.
    cgroup: Option<std::path::PathBuf>,
    /// `<cgroup>/cgroup.procs` as a C string, built before fork so the
    /// `pre_exec` hook does not allocate.
    cgroup_procs: Option<std::ffi::CString>,
    rlimit_data: Option<u64>,
    nice: Option<i32>,
    /// Host dir's local `oom` count at spawn (host-ceiling jobs only).
    host_oom_at_spawn: Option<u64>,
}

#[cfg(unix)]
impl UnixGovernor {
    /// Decide the mode for `limits`. `None` for default limits (no syscall).
    /// In cgroup mode the cgroup dir is created and `memory.max` written here,
    /// before spawn. The rlimit lane is chosen only from the cached
    /// [`cgroup::usable`] probe; a later per-job failure is `Unavailable`.
    /// Memory `None` without a host ceiling to join = nice only, no cgroup dir.
    pub(crate) fn prepare(limits: &JobLimits, probe_id: impl std::fmt::Display) -> Option<Self> {
        if *limits == JobLimits::default() {
            return None;
        }
        let report = new_report(limits);
        let nice = match limits.priority {
            Some(JobPriority::Idle) => Some(19),
            Some(JobPriority::BelowNormal) => Some(10),
            Some(JobPriority::Normal) | None => None,
        };
        #[cfg(target_os = "linux")]
        let (mode, cgroup, host_joined) = cgroup::decide(limits, &probe_id.to_string());
        #[cfg(not(target_os = "linux"))]
        let (mode, cgroup, host_joined) = {
            let _ = probe_id;
            let mode = if limits.memory_bytes.is_some() {
                GovernorMode::Unavailable(NO_RLIMIT_DATA.to_owned())
            } else {
                GovernorMode::Rlimit
            };
            (mode, None::<std::path::PathBuf>, false)
        };
        let rlimit_data = if mode == GovernorMode::Rlimit {
            limits.memory_bytes
        } else {
            None
        };
        {
            let mut r = report.lock();
            r.mode = Some(mode);
            r.host_ceiling_joined = host_joined;
        }
        let cgroup_procs = cgroup.as_ref().and_then(|dir| {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::CString::new(dir.join("cgroup.procs").as_os_str().as_bytes()).ok()
        });
        #[cfg(target_os = "linux")]
        let host_oom_at_spawn = host_joined.then(cgroup::host_oom_count).flatten();
        #[cfg(not(target_os = "linux"))]
        let host_oom_at_spawn = None;
        Some(Self {
            report,
            cgroup,
            cgroup_procs,
            rlimit_data,
            nice,
            host_oom_at_spawn,
        })
    }

    pub(crate) fn report(&self) -> SharedReport {
        Arc::clone(&self.report)
    }

    /// The `pre_exec` hook: join the cgroup, lower `RLIMIT_DATA`, raise nice.
    /// Errors are
    /// ignored on purpose: a failing `pre_exec` would abort the spawn, and the
    /// job must still run. Lowering both limits to `min(limit, hard)` and
    /// raising nice are always permitted for an unprivileged process, so the
    /// ignored paths are not expected to fail.
    pub(crate) fn pre_exec_hook(
        &self,
    ) -> impl FnMut() -> std::io::Result<()> + Send + Sync + 'static {
        let rlimit_data = self.rlimit_data;
        let nice = self.nice;
        let cgroup_procs = self.cgroup_procs.clone();
        move || {
            if let Some(path) = &cgroup_procs {
                // SAFETY: open/write/close are async-signal-safe syscalls on a
                // NUL-terminated path built before fork. Writing "0" moves the
                // calling process (this child) into the cgroup before exec.
                // A failure here is caught by the parent's `attach`.
                unsafe {
                    let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
                    if fd >= 0 {
                        let _ = libc::write(fd, b"0".as_ptr().cast(), 1);
                        let _ = libc::close(fd);
                    }
                }
            }
            if let Some(bytes) = rlimit_data {
                let want = libc::rlim_t::try_from(bytes).unwrap_or(libc::RLIM_INFINITY);
                let mut cur = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                // SAFETY: getrlimit/setrlimit are async-signal-safe syscalls
                // reading/writing only the local `cur`; safe between fork and
                // exec.
                unsafe {
                    if libc::getrlimit(libc::RLIMIT_DATA, &raw mut cur) == 0 {
                        let v = want.min(cur.rlim_max);
                        cur.rlim_cur = v;
                        cur.rlim_max = v;
                        let _ = libc::setrlimit(libc::RLIMIT_DATA, &raw const cur);
                    }
                }
            }
            if let Some(n) = nice {
                // SAFETY: setpriority is an async-signal-safe syscall on the
                // calling process (who = 0); safe between fork and exec.
                unsafe {
                    let _ = libc::setpriority(libc::PRIO_PROCESS, 0, n);
                }
            }
            Ok(())
        }
    }

    /// Post-spawn: re-write `pid` into the cgroup. The child normally moved
    /// itself in `pre_exec`, so this is the checked fallback. If the write
    /// fails but `/proc/<pid>/cgroup` shows the child already inside, the mode
    /// stays `Cgroup`; otherwise it downgrades to `Unavailable` (never silently
    /// ungoverned) and the dir is removed. `ESRCH` (child already exited) is
    /// not a failure.
    pub(crate) fn attach(&mut self, pid: Option<u32>) {
        let Some(dir) = &self.cgroup else { return };
        let res = pid.map_or_else(
            || Err(std::io::Error::other("child pid unavailable")),
            |pid| std::fs::write(dir.join("cgroup.procs"), pid.to_string()),
        );
        let Err(e) = res else { return };
        if e.raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        #[cfg(target_os = "linux")]
        if pid.is_some_and(|pid| cgroup::contains(dir, pid)) {
            return;
        }
        let _ = std::fs::remove_dir(dir);
        self.cgroup = None;
        let mut r = self.report.lock();
        r.mode = Some(GovernorMode::Unavailable(errno_reason("cgroup attach", &e)));
        r.host_ceiling_joined = false;
    }

    /// After exit: read peak and the limit counters, then remove the cgroup. A
    /// descendant that outlived the child keeps it populated (`EBUSY`): the
    /// whole cgroup is then killed (`cgroup.kill`) and the rmdir retried.
    /// This also covers cancel, which reaches here after the process-group
    /// kill. `abnormal_exit` gates `memory_limit_hit`.
    pub(crate) fn finish(mut self, abnormal_exit: bool) {
        let Some(dir) = self.cgroup.take() else {
            return;
        };
        let peak = std::fs::read_to_string(dir.join("memory.peak"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let counter = |file: &str, key: &str| {
            std::fs::read_to_string(dir.join(file)).ok().and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix(key))
                    .and_then(|v| v.trim().parse::<u64>().ok())
            })
        };
        // This job's OWN limit was reached: the local `oom` count. `oom_kill`
        // also counts kills caused by the host ceiling above it, so it is
        // only the fallback for kernels without `memory.events.local` (< 5.7).
        let own_limit_hits = counter("memory.events.local", "oom ")
            .or_else(|| counter("memory.events", "oom_kill "))
            .unwrap_or(0);
        #[cfg(target_os = "linux")]
        cgroup::remove(&dir);
        #[cfg(target_os = "linux")]
        let host_hit = self
            .host_oom_at_spawn
            .zip(cgroup::host_oom_count())
            .is_some_and(|(before, now)| now > before);
        #[cfg(not(target_os = "linux"))]
        let host_hit = false;
        let mut r = self.report.lock();
        r.peak_memory_bytes = peak;
        r.memory_limit_hit = abnormal_exit && own_limit_hits > 0;
        r.host_ceiling_hit = abnormal_exit && r.host_ceiling_joined && host_hit;
    }
}

#[cfg(unix)]
impl Drop for UnixGovernor {
    /// Spawn failure or an aborted lifecycle task: never leave the dir behind.
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(dir) = self.cgroup.take() {
            cgroup::remove(&dir);
        }
    }
}

/// `"<what> failed: errno N"`: a reason with no path in it.
#[cfg(unix)]
fn errno_reason(what: &str, e: &std::io::Error) -> String {
    e.raw_os_error().map_or_else(
        || format!("{what} failed: {:?}", e.kind()),
        |n| format!("{what} failed: errno {n}"),
    )
}

#[cfg(target_os = "linux")]
mod cgroup {
    use std::io::ErrorKind;
    use std::path::{Component, Path, PathBuf};
    use std::sync::OnceLock;

    use parking_lot::Mutex;

    use super::{GovernorMode, JobLimits, NO_AGGREGATE, errno_reason};

    const ROOT: &str = "/sys/fs/cgroup";
    // Every dir name carries the creating daemon's pid: several daemons
    // (test daemons, a second user session) can share one parent cgroup, and
    // a shared name would let one daemon overwrite another's ceiling or sweep
    // its freshly created job dirs.
    /// Host-ceiling dir: `tc-jobs-<pid>`.
    const HOST_PREFIX: &str = "tc-jobs-";
    /// Sibling per-job dir (no host ceiling): `tc-job-<pid>-<probe_id>`.
    const JOB_PREFIX: &str = "tc-job-";
    /// `usable()` probe dir: `tc-probe-<pid>`.
    const PROBE_PREFIX: &str = "tc-probe-";

    /// The installed host ceiling: (`tc-jobs-<pid>` dir, limit).
    pub(super) static HOST: Mutex<Option<(PathBuf, u64)>> = parking_lot::const_mutex(None);

    /// The installed host dir, if any.
    fn host_dir() -> Option<PathBuf> {
        HOST.lock().as_ref().map(|(dir, _)| dir.clone())
    }

    /// The daemon's PARENT cgroup dir, resolved once from `/proc/self/cgroup`.
    /// `None` when not on cgroup v2, the daemon sits in the root cgroup, or
    /// the path has any non-Normal component (`..`, `.`).
    fn parent_dir() -> Option<&'static Path> {
        static PARENT: OnceLock<Option<PathBuf>> = OnceLock::new();
        PARENT
            .get_or_init(|| {
                let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
                let own = text.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
                let rel = Path::new(own.strip_prefix('/')?).parent()?;
                if rel.as_os_str().is_empty()
                    || !rel.components().all(|c| matches!(c, Component::Normal(_)))
                {
                    return None;
                }
                Some(Path::new(ROOT).join(rel))
            })
            .as_deref()
    }

    const fn is_fallback(kind: ErrorKind) -> bool {
        matches!(
            kind,
            ErrorKind::PermissionDenied | ErrorKind::NotFound | ErrorKind::ReadOnlyFilesystem
        )
    }

    /// Whether a sibling cgroup with a memory controller can be created here.
    /// Probed once (mkdir, check `memory.max`, rmdir) and cached.
    pub(super) fn usable() -> bool {
        static USABLE: OnceLock<bool> = OnceLock::new();
        *USABLE.get_or_init(|| {
            let Some(parent) = parent_dir() else {
                return false;
            };
            let dir = parent.join(format!("{PROBE_PREFIX}{}", std::process::id()));
            if std::fs::create_dir(&dir).is_err() {
                return false;
            }
            let ok = dir.join("memory.max").exists();
            let _ = std::fs::remove_dir(&dir);
            ok
        })
    }

    /// Mode for a job: (mode, per-job dir, joined the host ceiling).
    ///
    /// The rlimit lane comes ONLY from the cached boot-time [`usable`]
    /// probe. Once cgroups were found usable, any per-job failure (a removed
    /// host dir, `EACCES`, `EROFS`, ...) is `Unavailable`: falling back to
    /// `RLIMIT_DATA` at run time would apply a limit the daemon decided not
    /// to apply under rlimit.
    pub(super) fn decide(limits: &JobLimits, id: &str) -> (GovernorMode, Option<PathBuf>, bool) {
        let host = limits.join_host_ceiling.then(host_dir).flatten();
        let host = host.as_deref();
        // Priority only, nothing to join: nice in pre_exec, no cgroup dir.
        if limits.memory_bytes.is_none() && host.is_none() {
            return (GovernorMode::Rlimit, None, false);
        }
        if host.is_none() && !usable() {
            return (GovernorMode::Rlimit, None, false);
        }
        let target = host.map_or_else(
            || parent_dir().map(|p| p.join(format!("{JOB_PREFIX}{}-{id}", std::process::id()))),
            |host| Some(host.join(id)),
        );
        let Some(dir) = target else {
            // `usable()` implies a parent dir; unreachable in practice.
            return (
                GovernorMode::Unavailable("cgroup parent unresolved".to_owned()),
                None,
                false,
            );
        };
        match create(&dir, limits.memory_bytes) {
            Ok(()) => (GovernorMode::Cgroup, Some(dir), host.is_some()),
            Err(reason) => (GovernorMode::Unavailable(reason), None, false),
        }
    }

    /// mkdir `dir`, require `cgroup.procs`, write the memory limits. Every
    /// failure is a path-free reason; the dir is removed on failure.
    fn create(dir: &Path, memory: Option<u64>) -> Result<(), String> {
        if let Err(e) = std::fs::create_dir(dir) {
            return Err(errno_reason("cgroup mkdir", &e));
        }
        if !dir.join("cgroup.procs").exists() {
            let _ = std::fs::remove_dir(dir);
            return Err("cgroup.procs missing after mkdir".to_owned());
        }
        if let Some(bytes) = memory {
            if let Err(e) = std::fs::write(dir.join("memory.max"), bytes.to_string()) {
                let _ = std::fs::remove_dir(dir);
                return Err(errno_reason("cgroup memory.max", &e));
            }
            // Swap accounting may be compiled out (file absent); the
            // memory.max cap above is the enforced limit either way.
            let _ = std::fs::write(dir.join("memory.swap.max"), "0");
        }
        Ok(())
    }

    /// Create (or reuse) `<parent>/tc-jobs-<pid>`, cap it, enable the memory
    /// controller for its per-job children.
    pub(super) fn install_host(limit: u64) -> Result<(), String> {
        let Some(parent) = parent_dir() else {
            return Err(NO_AGGREGATE.to_owned());
        };
        let mut host = HOST.lock();
        if host.is_some() {
            return Err("host ceiling already installed".to_owned());
        }
        let dir = parent.join(format!("{HOST_PREFIX}{}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) if is_fallback(e.kind()) => return Err(NO_AGGREGATE.to_owned()),
            Err(e) => return Err(errno_reason("cgroup mkdir", &e)),
        }
        if !dir.join("cgroup.procs").exists() {
            let _ = std::fs::remove_dir(&dir);
            return Err("cgroup.procs missing after mkdir".to_owned());
        }
        let write = |file: &str, value: &str| {
            std::fs::write(dir.join(file), value).map_err(|e| {
                if is_fallback(e.kind()) {
                    NO_AGGREGATE.to_owned()
                } else {
                    errno_reason(&format!("cgroup {file}"), &e)
                }
            })
        };
        if let Err(reason) = write("memory.max", &limit.to_string())
            .and_then(|()| write("cgroup.subtree_control", "+memory"))
        {
            // Never leave a half-configured host dir behind.
            let _ = std::fs::remove_dir(&dir);
            return Err(reason);
        }
        let _ = std::fs::write(dir.join("memory.swap.max"), "0");
        *host = Some((dir, limit));
        Ok(())
    }

    /// Kill what is left in the host dir, rmdir its per-job children, then
    /// the dir itself. `ENOENT` counts as removed.
    ///
    /// A just-killed member or a per-job dir whose own removal is still
    /// retrying keeps the host dir busy for a moment: on `EBUSY` the whole
    /// removal is retried like [`remove`] (off the async worker, up to 1 s)
    /// and `Ok` is returned; a dir still busy after that is the boot sweep's.
    pub(super) fn uninstall_host() -> Result<(), String> {
        let Some((dir, _)) = HOST.lock().take() else {
            return Ok(());
        };
        let _ = std::fs::write(dir.join("cgroup.kill"), "1");
        let attempt = |dir: &Path| {
            remove_host_dir(dir);
            match std::fs::remove_dir(dir) {
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                other => other,
            }
        };
        match attempt(&dir) {
            Err(e) if is_busy(&e) => {}
            Err(e) => return Err(errno_reason("host cgroup rmdir", &e)),
            Ok(()) => return Ok(()),
        }
        retry_off_worker(move || attempt(&dir));
        Ok(())
    }

    /// The host dir's `memory.events.local` `oom` count: times the host
    /// ceiling itself was reached. The hierarchical `memory.events` would
    /// also count per-job limit hits in its children.
    pub(super) fn host_oom_count() -> Option<u64> {
        let dir = host_dir()?;
        std::fs::read_to_string(dir.join("memory.events.local"))
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("oom "))?
            .trim()
            .parse()
            .ok()
    }

    /// Whether `/proc/<pid>/cgroup` places `pid` in `dir`.
    pub(super) fn contains(dir: &Path, pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|l| l.strip_prefix("0::"))
                    .map(|own| Path::new(ROOT).join(own.trim().trim_start_matches('/')) == dir)
            })
            .unwrap_or(false)
    }

    /// rmdir `dir`; on `EBUSY` (a descendant outlived the job) write
    /// `cgroup.kill` (kernel >= 5.14) and retry for up to 1 s until the
    /// kernel lets go. The retry never sleeps on an async worker: inside a
    /// tokio runtime it runs on the blocking pool; outside one (a plain
    /// thread) it runs inline. Anything left over is the boot sweep's.
    pub(super) fn remove(dir: &Path) {
        match std::fs::remove_dir(dir) {
            Err(e) if is_busy(&e) => {}
            _ => return,
        }
        let _ = std::fs::write(dir.join("cgroup.kill"), "1");
        let dir = dir.to_owned();
        retry_off_worker(move || std::fs::remove_dir(&dir));
    }

    fn is_busy(e: &std::io::Error) -> bool {
        e.raw_os_error() == Some(libc::EBUSY)
    }

    /// Re-run `attempt` every 10 ms for up to 1 s while it fails with
    /// `EBUSY`. Inside a tokio runtime on the blocking pool (never sleeping
    /// on an async worker; runtime shutdown waits for it), else inline.
    fn retry_off_worker(attempt: impl Fn() -> std::io::Result<()> + Send + 'static) {
        let retry = move || {
            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                match attempt() {
                    Err(e) if is_busy(&e) => {}
                    _ => return,
                }
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn_blocking(retry)),
            Err(_) => retry(),
        }
    }

    /// `prb_` + 32 lowercase hex: the `ProbeId` display form.
    fn is_probe_id(name: &str) -> bool {
        name.strip_prefix("prb_").is_some_and(|hex| {
            hex.len() == 32
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }

    /// What a dir directly under the parent is, if this module created it.
    #[derive(Debug, PartialEq, Eq)]
    enum Owned {
        Host(u32),
        Job(u32),
        Probe(u32),
    }

    fn parse_pid(digits: &str) -> Option<u32> {
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| digits.parse().ok())
            .flatten()
    }

    fn classify(name: &str) -> Option<Owned> {
        if let Some(pid) = name.strip_prefix(HOST_PREFIX) {
            return parse_pid(pid).map(Owned::Host);
        }
        if let Some(rest) = name.strip_prefix(JOB_PREFIX) {
            let (pid, id) = rest.split_once('-')?;
            return is_probe_id(id)
                .then(|| parse_pid(pid))
                .flatten()
                .map(Owned::Job);
        }
        parse_pid(name.strip_prefix(PROBE_PREFIX)?).map(Owned::Probe)
    }

    /// rmdir every probe-id child of a host dir, then the dir itself.
    fn remove_host_dir(dir: &Path) -> usize {
        let children = std::fs::read_dir(dir).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_str().is_some_and(is_probe_id))
                .filter(|e| std::fs::remove_dir(e.path()).is_ok())
                .count()
        });
        children + usize::from(std::fs::remove_dir(dir).is_ok())
    }

    /// Remove our dirs whose creating daemon is gone (`/proc/<pid>` absent).
    /// A live daemon's dirs, including our own, are never touched; a
    /// populated cgroup fails rmdir with `EBUSY` and stays.
    pub(super) fn sweep() -> usize {
        let Some(parent) = parent_dir() else {
            return 0;
        };
        let Ok(entries) = std::fs::read_dir(parent) else {
            return 0;
        };
        let dead = |pid: u32| !Path::new(&format!("/proc/{pid}")).exists();
        entries
            .flatten()
            .map(|e| match e.file_name().to_str().and_then(classify) {
                Some(Owned::Host(pid)) if dead(pid) => remove_host_dir(&e.path()),
                Some(Owned::Job(pid) | Owned::Probe(pid)) if dead(pid) => {
                    usize::from(std::fs::remove_dir(e.path()).is_ok())
                }
                _ => 0,
            })
            .sum()
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn sweep_matches_only_our_names() {
            use super::{Owned, classify};
            let id = "prb_0123456789abcdef0123456789abcdef";
            assert_eq!(classify(&format!("tc-job-42-{id}")), Some(Owned::Job(42)));
            assert_eq!(classify("tc-jobs-42"), Some(Owned::Host(42)));
            assert_eq!(classify("tc-probe-4242"), Some(Owned::Probe(4242)));
            for other in [
                "tc-job-",
                &format!("tc-job-{id}"),
                "tc-job-42-prb_xyz",
                "tc-jobs",
                "tc-jobs-",
                "tc-jobs-4a",
                "tc-probe-",
                "tc-probe-12a",
                "app.slice",
                "tc-job-42-prb_0123456789ABCDEF0123456789abcdef",
            ] {
                assert_eq!(classify(other), None, "{other}");
            }
        }
    }
}
