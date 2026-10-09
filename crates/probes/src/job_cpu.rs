// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Content-free, retained-interval whole-job CPU accounting.
//!
//! Percentages use one logical CPU as 100%, so parallel jobs may exceed 100%.
//! Windows Job Objects and owned Linux cgroups include retired descendants.
//! Linux process-group snapshots cover surviving members only: positive values
//! are lower bounds, and zero is unknown rather than evidence of job inactivity.
//! Descendants that deliberately leave the process group are outside its scope.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub use terminal_commander_core::job_cpu::{
    JobCpuAccounting, JobCpuSample, JobCpuState, JobCpuUnavailable, JobCpuUnknown,
};

const MIN_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
struct Counters {
    ticks: u64,
    ticks_per_second: u64,
    members: BTreeMap<(u32, u64), u64>,
}

fn measure_interval(
    previous: &Counters,
    current: &Counters,
    interval: Duration,
    accounting: JobCpuAccounting,
) -> JobCpuState {
    if interval < MIN_INTERVAL {
        return JobCpuState::Unknown(JobCpuUnknown::TooEarly);
    }
    if current.ticks_per_second == 0
        || previous.ticks_per_second != current.ticks_per_second
        || current.ticks < previous.ticks
    {
        return JobCpuState::Unknown(JobCpuUnknown::InvalidCounters);
    }
    if accounting == JobCpuAccounting::LinuxProcessGroup {
        if !previous.members.keys().eq(current.members.keys()) {
            return JobCpuState::Unknown(JobCpuUnknown::MembershipChanged);
        }
        if previous
            .members
            .iter()
            .any(|(identity, ticks)| current.members[identity] < *ticks)
        {
            return JobCpuState::Unknown(JobCpuUnknown::InvalidCounters);
        }
    }
    let delta = current.ticks - previous.ticks;
    if delta == 0 && accounting == JobCpuAccounting::LinuxProcessGroup {
        return JobCpuState::Unknown(JobCpuUnknown::SnapshotAccountingIncomplete);
    }
    // Conversion to f64 is deliberately approximate: this is utilization, not
    // an integer counter API. Subtract before conversion to retain small deltas.
    #[allow(clippy::cast_precision_loss)]
    let percent = (delta as f64 / current.ticks_per_second as f64) / interval.as_secs_f64() * 100.0;
    if percent.is_finite() && percent >= 0.0 {
        JobCpuState::Known { percent }
    } else {
        JobCpuState::Unknown(JobCpuUnknown::InvalidCounters)
    }
}

#[derive(Debug, Default)]
struct History {
    previous: Option<(Instant, Counters)>,
}

impl History {
    fn observe(
        &mut self,
        now: Instant,
        counters: Result<Counters, JobCpuState>,
        accounting: JobCpuAccounting,
    ) -> (Option<Duration>, JobCpuState) {
        let counters = match counters {
            Ok(counters) => counters,
            Err(state) => {
                self.previous = None;
                return (None, state);
            }
        };
        let Some((previous_at, previous)) = &self.previous else {
            self.previous = Some((now, counters));
            return (None, JobCpuState::Unknown(JobCpuUnknown::FirstSample));
        };
        let Some(interval) = now.checked_duration_since(*previous_at) else {
            self.previous = None;
            return (None, JobCpuState::Unknown(JobCpuUnknown::InvalidCounters));
        };
        let state = measure_interval(previous, &counters, interval, accounting);
        if state != JobCpuState::Unknown(JobCpuUnknown::TooEarly) {
            self.previous = Some((now, counters));
        }
        (Some(interval), state)
    }
}

/// Stateful CPU sampler for a `ProcessProbe` instance.
///
/// Obtain it from `ProcessProbe::cpu_sampler_handle`; construction stays with
/// the probe so a caller cannot attach to an unrelated process or host cgroup.
/// Queries retain at most 4096 member counters. Sampling never owns cleanup:
/// holding this handle cannot keep a Windows Job Object alive.
#[derive(Debug)]
pub struct JobCpuSampler {
    probe_id: terminal_commander_core::ProbeId,
    leader_pid: u32,
    leader_start_ticks: Option<u64>,
    accounting: JobCpuAccounting,
    source: Source,
    history: History,
}

#[derive(Debug)]
enum Source {
    #[cfg(target_os = "linux")]
    ProcessGroup(GroupSource),
    #[cfg(target_os = "linux")]
    Cgroup(std::fs::File),
    #[cfg(windows)]
    JobObject(std::sync::Weak<crate::process::JobHandle>),
    Unavailable(JobCpuUnavailable),
}

impl JobCpuSampler {
    #[cfg(target_os = "linux")]
    pub(crate) fn new(
        probe_id: terminal_commander_core::ProbeId,
        leader_pid: u32,
        cgroup: Option<std::path::PathBuf>,
    ) -> Self {
        let leader_start_ticks = read_process_stat(leader_pid)
            .ok()
            .filter(|stat| stat.group == leader_pid)
            .map(|stat| stat.start);
        let (accounting, source) = cgroup.map_or_else(
            || {
                let source = leader_start_ticks.map_or(
                    Source::Unavailable(JobCpuUnavailable::IdentityUnavailable),
                    |leader_start| {
                        // SAFETY: sysconf has no pointer arguments and does not mutate Rust state.
                        let ticks_per_second =
                            u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).unwrap_or(0);
                        Source::ProcessGroup(GroupSource {
                            leader_pid,
                            leader_start,
                            ticks_per_second,
                            invalid_identity: false,
                            exited: false,
                            anchors: std::collections::BTreeSet::from([(leader_pid, leader_start)]),
                        })
                    },
                );
                (JobCpuAccounting::LinuxProcessGroup, source)
            },
            |path| {
                let source = std::fs::File::open(path.join("cpu.stat")).map_or(
                    Source::Unavailable(JobCpuUnavailable::QueryFailed),
                    Source::Cgroup,
                );
                (JobCpuAccounting::LinuxCgroup, source)
            },
        );
        Self {
            probe_id,
            leader_pid,
            leader_start_ticks,
            accounting,
            source,
            history: History::default(),
        }
    }

    #[cfg(windows)]
    pub(crate) fn new(
        probe_id: terminal_commander_core::ProbeId,
        leader_pid: u32,
        job: Option<&std::sync::Arc<crate::process::JobHandle>>,
    ) -> Self {
        Self {
            probe_id,
            leader_pid,
            leader_start_ticks: None,
            accounting: JobCpuAccounting::WindowsJobObject,
            source: job.map_or(
                Source::Unavailable(JobCpuUnavailable::MissingJobObject),
                |job| Source::JobObject(std::sync::Arc::downgrade(job)),
            ),
            history: History::default(),
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    pub(crate) fn new(probe_id: terminal_commander_core::ProbeId, leader_pid: u32) -> Self {
        Self {
            probe_id,
            leader_pid,
            leader_start_ticks: None,
            accounting: JobCpuAccounting::Unsupported,
            source: Source::Unavailable(JobCpuUnavailable::UnsupportedPlatform),
            history: History::default(),
        }
    }

    /// Query cumulative counters and compare with the retained baseline.
    /// The first call and calls less than 100 ms apart cannot establish idleness.
    /// Too-early calls preserve the baseline, allowing frequent polling.
    pub fn sample(&mut self) -> JobCpuSample {
        let counters = match &mut self.source {
            #[cfg(target_os = "linux")]
            Source::ProcessGroup(group) => group.read(),
            #[cfg(target_os = "linux")]
            Source::Cgroup(stat) => read_cgroup(stat),
            #[cfg(windows)]
            Source::JobObject(job) => read_job_object(job),
            Source::Unavailable(reason) => Err(JobCpuState::Unavailable(*reason)),
        };
        let (interval, state) = self
            .history
            .observe(Instant::now(), counters, self.accounting);
        JobCpuSample {
            probe_id: self.probe_id,
            leader_pid: self.leader_pid,
            leader_start_ticks: self.leader_start_ticks,
            accounting: self.accounting,
            interval,
            state,
        }
    }
}

#[cfg(windows)]
fn read_job_object(
    job: &std::sync::Weak<crate::process::JobHandle>,
) -> Result<Counters, JobCpuState> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject,
    };
    let job = job.upgrade().ok_or(JobCpuState::Unavailable(
        JobCpuUnavailable::MissingJobObject,
    ))?;
    // SAFETY: the upgraded Arc owns the handle for the duration of the query;
    // the output buffer and size exactly match the requested information class.
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    let result = unsafe {
        QueryInformationJobObject(
            job.0 as HANDLE,
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            u32::try_from(std::mem::size_of_val(&info)).expect("accounting struct fits u32"),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed));
    }
    let ticks = u64::try_from(info.TotalKernelTime)
        .ok()
        .zip(u64::try_from(info.TotalUserTime).ok())
        .and_then(|(kernel, user)| kernel.checked_add(user))
        .ok_or(JobCpuState::Unknown(JobCpuUnknown::InvalidCounters))?;
    Ok(Counters {
        ticks,
        ticks_per_second: 10_000_000,
        members: BTreeMap::new(),
    })
}

#[cfg(target_os = "linux")]
struct ProcessStat {
    group: u32,
    start: u64,
    ticks: u64,
}

#[cfg(target_os = "linux")]
fn parse_process_stat(text: &str) -> Option<ProcessStat> {
    // comm can contain spaces and ')'; the final ')' terminates that field.
    let (_, fields) = text.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    let group = fields.nth(2)?.parse().ok()?;
    let user: u64 = fields.nth(8)?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    let start = fields.nth(6)?.parse().ok()?;
    Some(ProcessStat {
        group,
        start,
        ticks: user.checked_add(system)?,
    })
}

#[cfg(target_os = "linux")]
fn read_process_stat(pid: u32) -> Result<ProcessStat, std::io::Error> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(format!("/proc/{pid}/stat"))?
        .take(4097)
        .read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    parse_process_stat(&text).ok_or_else(|| std::io::ErrorKind::InvalidData.into())
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct GroupSource {
    leader_pid: u32,
    leader_start: u64,
    ticks_per_second: u64,
    invalid_identity: bool,
    exited: bool,
    anchors: std::collections::BTreeSet<(u32, u64)>,
}

#[cfg(target_os = "linux")]
impl GroupSource {
    fn read(&mut self) -> Result<Counters, JobCpuState> {
        if self.invalid_identity {
            return Err(JobCpuState::Unknown(JobCpuUnknown::IdentityChanged));
        }
        if self.exited {
            return Err(JobCpuState::Unknown(JobCpuUnknown::JobExited));
        }
        let mut members = BTreeMap::new();
        let mut ticks = 0_u64;
        let entries = std::fs::read_dir("/proc")
            .map_err(|_| JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed))?;
        for (index, entry) in entries.enumerate() {
            if index >= 65_536 {
                return Err(JobCpuState::Unavailable(JobCpuUnavailable::ScanLimit));
            }
            let entry =
                entry.map_err(|_| JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed))?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let stat = match read_process_stat(pid) {
                Ok(stat) => stat,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed)),
            };
            if pid == self.leader_pid && stat.start != self.leader_start {
                self.invalid_identity = true;
                return Err(JobCpuState::Unknown(JobCpuUnknown::IdentityChanged));
            }
            if stat.group != self.leader_pid {
                continue;
            }
            if members.len() >= 4096 {
                return Err(JobCpuState::Unavailable(JobCpuUnavailable::ScanLimit));
            }
            ticks = ticks
                .checked_add(stat.ticks)
                .ok_or(JobCpuState::Unknown(JobCpuUnknown::InvalidCounters))?;
            members.insert((pid, stat.start), stat.ticks);
        }
        if members.is_empty() {
            self.exited = true;
            return Err(JobCpuState::Unknown(JobCpuUnknown::JobExited));
        }
        self.retain_identity(&members)?;
        Ok(Counters {
            ticks,
            ticks_per_second: self.ticks_per_second,
            members,
        })
    }

    fn retain_identity(&mut self, members: &BTreeMap<(u32, u64), u64>) -> Result<(), JobCpuState> {
        // A group ID alone can be reused after the whole original group exits.
        // Require a surviving PID/start anchor even when the original leader is
        // gone; never adopt a disjoint replacement group under this probe ID.
        if self.invalid_identity
            || !members
                .keys()
                .any(|identity| self.anchors.contains(identity))
        {
            self.invalid_identity = true;
            return Err(JobCpuState::Unknown(JobCpuUnknown::IdentityChanged));
        }
        self.anchors = members.keys().copied().collect();
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn read_cgroup(stat: &mut std::fs::File) -> Result<Counters, JobCpuState> {
    use std::io::{Read, Seek};
    stat.rewind()
        .map_err(|_| JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed))?;
    let mut text = String::new();
    stat.take(4097)
        .read_to_string(&mut text)
        .map_err(|_| JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed))?;
    if text.len() > 4096 {
        return Err(JobCpuState::Unknown(JobCpuUnknown::InvalidCounters));
    }
    let mut values = text
        .lines()
        .filter_map(|line| line.strip_prefix("usage_usec "));
    let ticks = values
        .next()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|_| values.next().is_none())
        .ok_or(JobCpuState::Unknown(JobCpuUnknown::InvalidCounters))?;
    Ok(Counters {
        ticks,
        ticks_per_second: 1_000_000,
        members: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(ticks: u64) -> Counters {
        Counters {
            ticks,
            ticks_per_second: 100,
            members: BTreeMap::new(),
        }
    }

    #[test]
    fn aggregate_normalizes_one_cpu_to_one_hundred_percent() {
        assert_eq!(
            measure_interval(
                &counters(100),
                &counters(300),
                Duration::from_secs(1),
                JobCpuAccounting::WindowsJobObject
            ),
            JobCpuState::Known { percent: 200.0 }
        );
    }

    #[test]
    fn only_lifetime_accounting_can_prove_zero_cpu() {
        for accounting in [
            JobCpuAccounting::LinuxCgroup,
            JobCpuAccounting::WindowsJobObject,
        ] {
            assert_eq!(
                measure_interval(
                    &counters(100),
                    &counters(100),
                    Duration::from_secs(1),
                    accounting
                ),
                JobCpuState::Known { percent: 0.0 }
            );
        }
        assert_eq!(
            measure_interval(
                &counters(100),
                &counters(100),
                Duration::from_secs(1),
                JobCpuAccounting::LinuxProcessGroup
            ),
            JobCpuState::Unknown(JobCpuUnknown::SnapshotAccountingIncomplete)
        );
    }

    #[test]
    fn too_early_samples_do_not_report_inactivity() {
        assert_eq!(
            measure_interval(
                &counters(100),
                &counters(100),
                Duration::from_millis(1),
                JobCpuAccounting::LinuxCgroup
            ),
            JobCpuState::Unknown(JobCpuUnknown::TooEarly)
        );
    }

    #[test]
    fn decreasing_or_invalid_counter_units_are_unknown() {
        let mut invalid = counters(200);
        invalid.ticks_per_second = 0;
        for current in [counters(99), invalid] {
            assert_eq!(
                measure_interval(
                    &counters(100),
                    &current,
                    Duration::from_secs(1),
                    JobCpuAccounting::LinuxCgroup
                ),
                JobCpuState::Unknown(JobCpuUnknown::InvalidCounters)
            );
        }
    }

    #[test]
    fn churn_and_pid_reuse_are_unknown_even_when_total_cpu_increases() {
        let mut before = counters(100);
        before.members.insert((42, 1000), 100);
        let mut after = counters(200);
        after.members.insert((42, 2000), 200);
        assert_eq!(
            measure_interval(
                &before,
                &after,
                Duration::from_secs(1),
                JobCpuAccounting::LinuxProcessGroup
            ),
            JobCpuState::Unknown(JobCpuUnknown::MembershipChanged)
        );
        after.members.insert((43, 3000), 0);
        assert_eq!(
            measure_interval(
                &before,
                &after,
                Duration::from_secs(1),
                JobCpuAccounting::LinuxProcessGroup
            ),
            JobCpuState::Unknown(JobCpuUnknown::MembershipChanged)
        );
    }

    #[test]
    fn member_regression_cannot_hide_behind_another_busy_member() {
        let mut before = counters(100);
        before.members = BTreeMap::from([((42, 1000), 80), ((43, 1001), 20)]);
        let mut after = counters(200);
        after.members = BTreeMap::from([((42, 1000), 60), ((43, 1001), 140)]);
        assert_eq!(
            measure_interval(
                &before,
                &after,
                Duration::from_secs(1),
                JobCpuAccounting::LinuxProcessGroup
            ),
            JobCpuState::Unknown(JobCpuUnknown::InvalidCounters)
        );
    }

    #[test]
    fn first_sample_and_frequent_polling_preserve_the_actual_interval() {
        let start = Instant::now();
        let mut history = History::default();
        assert_eq!(
            history.observe(start, Ok(counters(100)), JobCpuAccounting::LinuxCgroup),
            (None, JobCpuState::Unknown(JobCpuUnknown::FirstSample))
        );
        assert_eq!(
            history
                .observe(
                    start + Duration::from_millis(1),
                    Ok(counters(100)),
                    JobCpuAccounting::LinuxCgroup
                )
                .1,
            JobCpuState::Unknown(JobCpuUnknown::TooEarly)
        );
        assert_eq!(
            history.observe(
                start + Duration::from_secs(1),
                Ok(counters(200)),
                JobCpuAccounting::LinuxCgroup
            ),
            (
                Some(Duration::from_secs(1)),
                JobCpuState::Known { percent: 100.0 }
            )
        );
    }

    #[test]
    fn missing_observation_clears_the_baseline_instead_of_fabricating_zero() {
        let start = Instant::now();
        let mut history = History::default();
        history.observe(start, Ok(counters(100)), JobCpuAccounting::LinuxCgroup);
        let failure = JobCpuState::Unavailable(JobCpuUnavailable::QueryFailed);
        assert_eq!(
            history.observe(
                start + Duration::from_secs(1),
                Err(failure.clone()),
                JobCpuAccounting::LinuxCgroup
            ),
            (None, failure)
        );
        assert_eq!(
            history
                .observe(
                    start + Duration::from_secs(2),
                    Ok(counters(100)),
                    JobCpuAccounting::LinuxCgroup
                )
                .1,
            JobCpuState::Unknown(JobCpuUnknown::FirstSample)
        );
    }

    #[cfg(windows)]
    #[test]
    fn absent_job_object_is_unavailable_not_leader_cpu() {
        let mut sampler = JobCpuSampler::new(
            terminal_commander_core::ProbeId::new(),
            std::process::id(),
            None,
        );
        assert_eq!(
            sampler.sample().state,
            JobCpuState::Unavailable(JobCpuUnavailable::MissingJobObject)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_stat_handles_parentheses_in_command_and_rejects_invalid_counters() {
        let prefix = "42 (odd ) command) S 1 42 0 0 0 0 0 0 0 0";
        let stat = parse_process_stat(&format!("{prefix} 3 4 0 0 0 0 0 0 12345")).unwrap();
        assert_eq!(stat.group, 42);
        assert_eq!(stat.ticks, 7);
        assert_eq!(stat.start, 12345);
        assert!(parse_process_stat(&format!("{prefix} -1 4 0 0 0 0 0 0 12345")).is_none());
        assert!(
            parse_process_stat(&format!("{prefix} {} 4 0 0 0 0 0 0 12345", u64::MAX)).is_none()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reused_leader_identity_is_latched_as_unknown() {
        let pid = std::process::id();
        let stat = read_process_stat(pid).unwrap();
        let mut group = GroupSource {
            leader_pid: pid,
            leader_start: stat.start + 1,
            ticks_per_second: 100,
            invalid_identity: false,
            exited: false,
            anchors: std::collections::BTreeSet::from([(pid, stat.start)]),
        };
        assert_eq!(
            group.read().unwrap_err(),
            JobCpuState::Unknown(JobCpuUnknown::IdentityChanged)
        );
        group.leader_start = stat.start;
        assert_eq!(
            group.read().unwrap_err(),
            JobCpuState::Unknown(JobCpuUnknown::IdentityChanged)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn disjoint_replacement_group_is_never_adopted_after_leader_exit() {
        let mut group = GroupSource {
            leader_pid: 42,
            leader_start: 100,
            ticks_per_second: 100,
            invalid_identity: false,
            exited: false,
            anchors: std::collections::BTreeSet::from([(42, 100), (43, 101)]),
        };
        // The leader may leave when a previously observed descendant survives.
        group
            .retain_identity(&BTreeMap::from([((43, 101), 10), ((44, 102), 5)]))
            .unwrap();
        // All anchors disappear. A different process in the same numeric group
        // is untrusted even if the replacement group's leader has already left.
        assert_eq!(
            group
                .retain_identity(&BTreeMap::from([((99, 900), 20)]))
                .unwrap_err(),
            JobCpuState::Unknown(JobCpuUnknown::IdentityChanged)
        );
        assert_eq!(
            group
                .retain_identity(&BTreeMap::from([((43, 101), 20)]))
                .unwrap_err(),
            JobCpuState::Unknown(JobCpuUnknown::IdentityChanged)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cgroup_accounting_handle_is_pinned_across_path_replacement() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!(
            "tc-cpu-cgroup-{}",
            terminal_commander_core::ProbeId::new()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("cpu.stat");
        std::fs::write(
            &path,
            "usage_usec 1000000\nuser_usec 900000\nsystem_usec 100000\n",
        )
        .unwrap();
        let mut file = std::fs::File::open(&path).unwrap();
        assert_eq!(read_cgroup(&mut file).unwrap().ticks, 1_000_000);
        std::fs::rename(&path, dir.join("retired.stat")).unwrap();
        std::fs::write(&path, "usage_usec 0\n").unwrap();
        assert_eq!(read_cgroup(&mut file).unwrap().ticks, 1_000_000);
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(dir.join("retired.stat"))
            .unwrap();
        writer.write_all(b"usage_usec invalid\n").unwrap();
        assert_eq!(
            read_cgroup(&mut file).unwrap_err(),
            JobCpuState::Unknown(JobCpuUnknown::InvalidCounters)
        );
        drop(file);
        drop(writer);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
