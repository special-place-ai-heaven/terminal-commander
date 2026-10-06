// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Resource governor: per-job memory ceiling and CPU priority.
//!
//! Enforced by the kernel primitive the host offers. The thing that enforces is the thing
//! that reports: [`GovernorReport`] is filled from the same object (Job Object,
//! cgroup) that applied the limit.
//!
//! * Windows: limits are added to the Job Object the probe already owns
//!   (`JOB_OBJECT_LIMIT_JOB_MEMORY` caps the job's summed commit charge;
//!   `JOB_OBJECT_LIMIT_PRIORITY_CLASS`). Peak = `PeakJobMemoryUsed`.
//! * Linux: a SIBLING cgroup v2 `<daemon parent cgroup>/tc-job-<probe_id>`
//!   with `memory.max` and `memory.swap.max=0`. The child moves ITSELF into
//!   the cgroup in `pre_exec` (writes `0` to `cgroup.procs`), so nothing it
//!   forks can escape; the parent re-writes the pid after spawn as a checked
//!   fallback. Peak = `memory.peak`, hit =
//!   `memory.events` `oom_kill > 0`. When the parent cgroup is not writable
//!   (WSL `/non-systemd`, no systemd) the fallback is `RLIMIT_DATA` set in
//!   `pre_exec` (inherited, per-process, not tree-summed; peak and hit are
//!   unknowable). Never `RLIMIT_AS`, never `systemd-run`.
//! * Other unix: `RLIMIT_DATA` in `pre_exec`.
//! * Priority on unix: `setpriority` (nice 10 BelowNormal, 19 Idle) in
//!   `pre_exec`.
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

use std::sync::Arc;

use parking_lot::Mutex;

/// CPU priority for a governed job.
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
}

/// Which kernel primitive governs the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernorMode {
    JobObject,
    Cgroup,
    Rlimit,
    /// Enforcement could not be applied; the job runs ungoverned. Carries why.
    Unavailable(String),
}

/// Filled when the job has exited. `mode == None` when `JobLimits::default()`
/// was passed. Before exit: mode and limit are known, peak is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GovernorReport {
    pub mode: Option<GovernorMode>,
    pub memory_limit_bytes: Option<u64>,
    /// Windows: `PeakJobMemoryUsed`; Linux cgroup: `memory.peak` (None on
    /// kernels < 5.19); rlimit: None.
    pub peak_memory_bytes: Option<u64>,
    /// Windows: peak >= limit and abnormal exit; cgroup: `oom_kill > 0`;
    /// rlimit: false (unknowable).
    pub memory_limit_hit: bool,
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
///
/// Windows: `JobObject`. Linux: `Cgroup` when a sibling cgroup with a
/// delegated memory controller can actually be created (probed once with a
/// real mkdir + rmdir, then cached), else `Rlimit`. Other unix: `Rlimit`.
/// A per-job attempt can still downgrade; the job's report is authoritative.
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

/// See the Windows variant.
#[cfg(all(unix, not(target_os = "linux")))]
#[must_use]
pub const fn available_mode() -> GovernorMode {
    GovernorMode::Rlimit
}

/// See the Windows variant.
#[cfg(not(any(unix, windows)))]
#[must_use]
pub fn available_mode() -> GovernorMode {
    GovernorMode::Unavailable("no kernel primitive".to_owned())
}

// ---------------------------------------------------------------- Windows --

/// Add the memory and priority limits to the extended limit info the probe
/// already sets on its Job Object. A default `limits` changes nothing.
#[cfg(windows)]
pub(crate) fn apply_job_limits(
    info: &mut windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    limits: &JobLimits,
) {
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_PRIORITY_CLASS,
    };
    use windows_sys::Win32::System::Threading::{
        BELOW_NORMAL_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    };
    if let Some(bytes) = limits.memory_bytes {
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
        info.JobMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    if let Some(priority) = limits.priority {
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
        info.BasicLimitInformation.PriorityClass = match priority {
            JobPriority::Idle => IDLE_PRIORITY_CLASS,
            JobPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            JobPriority::Normal => NORMAL_PRIORITY_CLASS,
        };
    }
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
    // Ungoverned (default limits): the job exists only for tree-kill; no query.
    let Some(job) = job.filter(|_| report.lock().mode.is_some()) else {
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
/// finished after exit.
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct UnixGovernor {
    report: SharedReport,
    /// Linux sibling cgroup dir, when cgroup mode is active.
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
        let (mode, cgroup) = match cgroup::create(limits, &probe_id.to_string()) {
            Ok(dir) => (GovernorMode::Cgroup, Some(dir)),
            Err(cgroup::Error::Fallback) => (GovernorMode::Rlimit, None),
            Err(cgroup::Error::Unavailable(reason)) => (GovernorMode::Unavailable(reason), None),
        };
        #[cfg(not(target_os = "linux"))]
        let (mode, cgroup) = {
            let _ = probe_id;
            (GovernorMode::Rlimit, None)
        };
        let rlimit_data = if mode == GovernorMode::Rlimit {
            limits.memory_bytes
        } else {
            None
        };
        report.lock().mode = Some(mode);
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
    /// itself in `pre_exec`, so this is the checked fallback: a failure
    /// downgrades the mode to `Unavailable` (never silently ungoverned) and
    /// removes the dir. `ESRCH` (child already exited) is not a failure.
    pub(crate) fn attach(&mut self, pid: Option<u32>) {
        let Some(dir) = &self.cgroup else { return };
        let res = pid.map_or_else(
            || Err(std::io::Error::other("child pid unavailable")),
            |pid| std::fs::write(dir.join("cgroup.procs"), pid.to_string()),
        );
        if let Err(e) = res.or_else(|e| {
            if e.raw_os_error() == Some(libc::ESRCH) {
                Ok(())
            } else {
                Err(e)
            }
        }) {
            let _ = std::fs::remove_dir(dir);
            self.cgroup = None;
            self.report.lock().mode = Some(GovernorMode::Unavailable(format!(
                "cgroup attach failed: {e}"
            )));
        }
    }

    /// After exit: read peak and oom_kill, then remove the cgroup. The rmdir
    /// is best effort: a descendant that outlived the child keeps the cgroup
    /// populated (EBUSY) and the dir stays until that process exits.
    // ponytail: leftover populated cgroup is left in place; add cgroup.kill
    // or a reaper if orphaned tc-job-* dirs ever show up in practice.
    pub(crate) fn finish(self) {
        let Some(dir) = self.cgroup else { return };
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
        let _ = std::fs::remove_dir(&dir);
        let mut r = self.report.lock();
        r.peak_memory_bytes = peak;
        r.memory_limit_hit = oom_kill > 0;
    }
}

#[cfg(target_os = "linux")]
mod cgroup {
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use super::JobLimits;

    pub(super) enum Error {
        /// Hierarchy not usable by us: use the rlimit lane.
        Fallback,
        Unavailable(String),
    }

    /// The daemon's PARENT cgroup dir, resolved once from `/proc/self/cgroup`.
    /// `None` when not on cgroup v2 or the daemon sits in the root cgroup.
    fn parent_dir() -> Option<&'static Path> {
        static PARENT: OnceLock<Option<PathBuf>> = OnceLock::new();
        PARENT
            .get_or_init(|| {
                let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
                let own = text.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
                let parent = Path::new(own).parent()?;
                let rel = parent.strip_prefix("/").unwrap_or(parent);
                Some(Path::new("/sys/fs/cgroup").join(rel))
            })
            .as_deref()
    }

    const fn is_fallback(kind: ErrorKind) -> bool {
        matches!(
            kind,
            ErrorKind::PermissionDenied | ErrorKind::NotFound | ErrorKind::ReadOnlyFilesystem
        )
    }

    /// Create `<parent>/tc-job-<id>` and write the memory limits. Writability
    /// is detected by attempting the mkdir itself.
    /// Whether a sibling cgroup with a memory controller can be created here.
    /// Probed once (mkdir, check `memory.max`, rmdir) and cached.
    pub(super) fn usable() -> bool {
        static USABLE: OnceLock<bool> = OnceLock::new();
        *USABLE.get_or_init(|| {
            let Some(parent) = parent_dir() else {
                return false;
            };
            let dir = parent.join(format!("tc-probe-{}", std::process::id()));
            if std::fs::create_dir(&dir).is_err() {
                return false;
            }
            let ok = dir.join("memory.max").exists();
            let _ = std::fs::remove_dir(&dir);
            ok
        })
    }

    pub(super) fn create(limits: &JobLimits, id: &str) -> Result<PathBuf, Error> {
        let Some(parent) = parent_dir() else {
            return Err(Error::Fallback);
        };
        let dir = parent.join(format!("tc-job-{id}"));
        if let Err(e) = std::fs::create_dir(&dir) {
            return Err(if is_fallback(e.kind()) {
                Error::Fallback
            } else {
                Error::Unavailable(format!("cgroup mkdir {}: {e}", dir.display()))
            });
        }
        if let Some(bytes) = limits.memory_bytes {
            if let Err(e) = std::fs::write(dir.join("memory.max"), bytes.to_string()) {
                let _ = std::fs::remove_dir(&dir);
                // No delegated memory controller: memory.max is missing.
                return Err(if is_fallback(e.kind()) {
                    Error::Fallback
                } else {
                    Error::Unavailable(format!("cgroup memory.max: {e}"))
                });
            }
            // Swap accounting may be compiled out (file absent); the
            // memory.max cap above is the enforced limit either way.
            let _ = std::fs::write(dir.join("memory.swap.max"), "0");
        }
        Ok(dir)
    }
}
