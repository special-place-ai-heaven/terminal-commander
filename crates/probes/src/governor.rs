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
//!   `JOB_OBJECT_LIMIT_PRIORITY_CLASS`). Peak = `PeakJobMemoryUsed`. The host
//!   ceiling is a parent Job Object: a joining child is assigned to it FIRST,
//!   then to its per-job job, which the kernel nests under the host job.
//! * Linux: a SIBLING cgroup v2 `<daemon parent cgroup>/tc-job-<probe_id>`
//!   (or `<parent>/tc-jobs/<probe_id>` when the job joins the host ceiling)
//!   with `memory.max` and `memory.swap.max=0`. The child moves ITSELF into
//!   the cgroup in `pre_exec` (writes `0` to `cgroup.procs`), so nothing it
//!   forks can escape; the parent re-writes the pid after spawn as a checked
//!   fallback. Peak = `memory.peak`, hit =
//!   `memory.events` `oom_kill > 0` with a non-success exit. When the parent
//!   cgroup is not writable (WSL `/non-systemd`, no systemd) the fallback is
//!   `RLIMIT_DATA` set in `pre_exec` (inherited, per-process, not tree-summed;
//!   peak and hit are unknowable). Never `RLIMIT_AS`, never `systemd-run`.
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
    /// Requires a non-success exit everywhere. Windows: peak >= limit;
    /// cgroup: `oom_kill > 0`; rlimit: false (unknowable).
    pub memory_limit_hit: bool,
    /// The child joined the installed host ceiling.
    pub host_ceiling_joined: bool,
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
/// * Linux cgroup mode: `<parent>/tc-jobs` with `memory.max = limit_bytes`,
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
    windows_host::HOST
        .set((job, limit_bytes))
        .map_err(|_| "host ceiling already installed".to_owned())?;
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
    windows_host::HOST.get().map(|(_, limit)| *limit)
}

/// The installed host ceiling in bytes, if any.
#[cfg(target_os = "linux")]
#[must_use]
pub fn host_ceiling() -> Option<u64> {
    cgroup::HOST.get().map(|(_, limit)| *limit)
}

/// The installed host ceiling in bytes, if any.
#[cfg(not(any(windows, target_os = "linux")))]
#[must_use]
pub const fn host_ceiling() -> Option<u64> {
    None
}

/// Boot-time cleanup of stale job cgroups; returns how many were removed.
///
/// Removes empty `tc-job-*` and `tc-jobs/*` dirs a previous daemon left
/// under our parent cgroup (only names this module creates; the kernel
/// refuses to rmdir a populated cgroup).
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
    use std::sync::OnceLock;

    /// The host-ceiling Job Object and its limit. Never dropped: the job (and
    /// its `KILL_ON_JOB_CLOSE`) lives as long as the daemon.
    pub(super) static HOST: OnceLock<(crate::process::JobHandle, u64)> = OnceLock::new();
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
/// Returns the per-job job (for tree-kill and the peak query) and the report,
/// whose mode and `host_ceiling_joined` are final here.
#[cfg(windows)]
pub(crate) fn govern_child(
    child: Option<std::os::windows::io::RawHandle>,
    limits: &JobLimits,
) -> (Option<crate::process::JobHandle>, SharedReport) {
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
        return (None, report);
    };
    let host_joined = limits.join_host_ceiling
        && windows_host::HOST
            .get()
            .is_some_and(|(host, _)| assign_to_job(host, child).is_ok());
    let job_with = |l: &JobLimits| -> Result<JobHandle, String> {
        let job = create_job(l)?;
        assign_to_job(&job, child)?;
        Ok(job)
    };
    let (job, mode) = match job_with(limits) {
        Ok(job) => (Some(job), GovernorMode::JobObject),
        Err(reason) => (
            job_with(&JobLimits::default()).ok(),
            GovernorMode::Unavailable(reason),
        ),
    };
    if governed {
        let mut r = report.lock();
        r.mode = Some(mode);
        r.host_ceiling_joined = host_joined;
    }
    (job, report)
}

/// Fill peak and limit-hit from the job after the child exited.
#[cfg(windows)]
pub(crate) fn finish_job(
    report: &SharedReport,
    job: Option<&crate::process::JobHandle>,
    abnormal_exit: bool,
) {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        QueryInformationJobObject,
    };
    // Ungoverned (default limits) or Unavailable (a kill-only job carries no
    // limit to report against): no query.
    let Some(job) = job.filter(|_| report.lock().mode == Some(GovernorMode::JobObject)) else {
        return;
    };
    // SAFETY: `info` is a zeroed `#[repr(C)]` POD of exactly the size passed;
    // `job.0` is a live Job Object handle owned by `JobHandle` for this borrow.
    // The BOOL result is checked before `info` is read.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let Ok(size) = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
    else {
        return;
    };
    let ok = unsafe {
        QueryInformationJobObject(
            job.0 as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw mut info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return;
    }
    let peak = info.PeakJobMemoryUsed as u64;
    let mut r = report.lock();
    r.peak_memory_bytes = Some(peak);
    r.memory_limit_hit = abnormal_exit && r.memory_limit_bytes.is_some_and(|l| peak >= l);
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
}

#[cfg(unix)]
impl UnixGovernor {
    /// Decide the mode for `limits`. `None` for default limits (no syscall).
    /// In cgroup mode the cgroup dir is created and `memory.max` written here,
    /// before spawn, so a non-writable hierarchy can still fall back to rlimit.
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
        Some(Self {
            report,
            cgroup,
            cgroup_procs,
            rlimit_data,
            nice,
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

    /// After exit: read peak and `oom_kill`, then remove the cgroup. A
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
        let oom_kill = std::fs::read_to_string(dir.join("memory.events"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("oom_kill "))
                    .and_then(|v| v.trim().parse::<u64>().ok())
            })
            .unwrap_or(0);
        #[cfg(target_os = "linux")]
        cgroup::remove(&dir);
        let mut r = self.report.lock();
        r.peak_memory_bytes = peak;
        r.memory_limit_hit = abnormal_exit && oom_kill > 0;
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

    use super::{GovernorMode, JobLimits, NO_AGGREGATE, errno_reason};

    const ROOT: &str = "/sys/fs/cgroup";
    /// Host-ceiling parent dir under the daemon's parent cgroup.
    const HOST_DIR: &str = "tc-jobs";
    /// Sibling per-job dir prefix (no host ceiling).
    const JOB_PREFIX: &str = "tc-job-";
    /// `usable()` probe dir prefix.
    const PROBE_PREFIX: &str = "tc-probe-";

    /// The installed host ceiling: (`tc-jobs` dir, limit).
    pub(super) static HOST: OnceLock<(PathBuf, u64)> = OnceLock::new();

    enum Error {
        /// Hierarchy not usable by us: use the rlimit lane.
        Fallback,
        Unavailable(String),
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
    pub(super) fn decide(limits: &JobLimits, id: &str) -> (GovernorMode, Option<PathBuf>, bool) {
        let host = limits
            .join_host_ceiling
            .then(|| HOST.get().map(|(dir, _)| dir.as_path()))
            .flatten();
        // Priority only, nothing to join: nice in pre_exec, no cgroup dir.
        if limits.memory_bytes.is_none() && host.is_none() {
            return (GovernorMode::Rlimit, None, false);
        }
        let target = host.map_or_else(
            || parent_dir().map(|p| p.join(format!("{JOB_PREFIX}{id}"))),
            |host| Some(host.join(id)),
        );
        let Some(dir) = target else {
            return (GovernorMode::Rlimit, None, false);
        };
        match create(&dir, limits.memory_bytes) {
            Ok(()) => (GovernorMode::Cgroup, Some(dir), host.is_some()),
            Err(Error::Fallback) => (GovernorMode::Rlimit, None, false),
            Err(Error::Unavailable(reason)) => (GovernorMode::Unavailable(reason), None, false),
        }
    }

    /// mkdir `dir`, require `cgroup.procs`, write the memory limits.
    /// Writability is detected by attempting the mkdir itself.
    fn create(dir: &Path, memory: Option<u64>) -> Result<(), Error> {
        if let Err(e) = std::fs::create_dir(dir) {
            return Err(if is_fallback(e.kind()) {
                Error::Fallback
            } else {
                Error::Unavailable(errno_reason("cgroup mkdir", &e))
            });
        }
        if !dir.join("cgroup.procs").exists() {
            let _ = std::fs::remove_dir(dir);
            return Err(Error::Unavailable(
                "cgroup.procs missing after mkdir".to_owned(),
            ));
        }
        if let Some(bytes) = memory {
            if let Err(e) = std::fs::write(dir.join("memory.max"), bytes.to_string()) {
                let _ = std::fs::remove_dir(dir);
                // No delegated memory controller: memory.max is missing.
                return Err(if is_fallback(e.kind()) {
                    Error::Fallback
                } else {
                    Error::Unavailable(errno_reason("cgroup memory.max", &e))
                });
            }
            // Swap accounting may be compiled out (file absent); the
            // memory.max cap above is the enforced limit either way.
            let _ = std::fs::write(dir.join("memory.swap.max"), "0");
        }
        Ok(())
    }

    /// Create (or reuse) `<parent>/tc-jobs`, cap it, enable the memory
    /// controller for its per-job children.
    pub(super) fn install_host(limit: u64) -> Result<(), String> {
        let Some(parent) = parent_dir() else {
            return Err(NO_AGGREGATE.to_owned());
        };
        let dir = parent.join(HOST_DIR);
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
        write("memory.max", &limit.to_string())?;
        write("cgroup.subtree_control", "+memory")?;
        let _ = std::fs::write(dir.join("memory.swap.max"), "0");
        HOST.set((dir, limit))
            .map_err(|_| "host ceiling already installed".to_owned())
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
    /// `cgroup.kill` (kernel >= 5.14) and retry until the kernel lets go.
    // ponytail: blocking 10ms poll for up to 1s on the caller's thread, only
    // on the EBUSY path; move to spawn_blocking if it ever shows up hot.
    pub(super) fn remove(dir: &Path) {
        match std::fs::remove_dir(dir) {
            Err(e) if e.raw_os_error() == Some(libc::EBUSY) => {}
            _ => return,
        }
        let _ = std::fs::write(dir.join("cgroup.kill"), "1");
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            match std::fs::remove_dir(dir) {
                Err(e) if e.raw_os_error() == Some(libc::EBUSY) => {}
                _ => return,
            }
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

    /// Whether `name` is a dir this module creates directly under the parent.
    fn is_ours(name: &str) -> bool {
        name.strip_prefix(JOB_PREFIX).is_some_and(is_probe_id)
            || name
                .strip_prefix(PROBE_PREFIX)
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
    }

    /// rmdir every matching dir; a populated one fails `EBUSY` and stays.
    fn sweep_in(dir: &Path, matches: fn(&str) -> bool) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .filter(|e| e.file_name().to_str().is_some_and(matches))
            .filter(|e| std::fs::remove_dir(e.path()).is_ok())
            .count()
    }

    pub(super) fn sweep() -> usize {
        let Some(parent) = parent_dir() else {
            return 0;
        };
        sweep_in(parent, is_ours) + sweep_in(&parent.join(HOST_DIR), is_probe_id)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn sweep_matches_only_our_names() {
            let id = "prb_0123456789abcdef0123456789abcdef";
            assert!(super::is_ours(&format!("tc-job-{id}")));
            assert!(super::is_ours("tc-probe-4242"));
            assert!(super::is_probe_id(id));
            for other in [
                "tc-job-",
                "tc-job-prb_xyz",
                "tc-jobs",
                "tc-probe-",
                "tc-probe-12a",
                "app.slice",
                "prb_0123456789ABCDEF0123456789abcdef",
            ] {
                assert!(!super::is_ours(other), "{other}");
            }
        }
    }
}
