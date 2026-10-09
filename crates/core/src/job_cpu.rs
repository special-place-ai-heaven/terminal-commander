// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Content-free whole-job CPU observations shared by embedded and IPC callers.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Kernel scope from which the sample was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCpuAccounting {
    /// Surviving group members visible in this PID namespace. Positive values
    /// are lower bounds; zero cannot rule out descendants missed between reads.
    /// Descendants that deliberately leave the group are outside this scope.
    LinuxProcessGroup,
    /// The probe governor's owned per-job cgroup, including retired descendants.
    /// The open accounting file pins identity across pathname reuse.
    LinuxCgroup,
    /// The probe's owned Job Object, including retired descendants.
    WindowsJobObject,
    /// Whole-job CPU accounting is not implemented on this platform.
    Unsupported,
}

/// Why a sample cannot establish CPU utilization or inactivity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCpuUnknown {
    FirstSample,
    TooEarly,
    MembershipChanged,
    IdentityChanged,
    InvalidCounters,
    SnapshotAccountingIncomplete,
    JobExited,
}

/// Bounded query failure metadata; never carries command output or paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCpuUnavailable {
    UnsupportedPlatform,
    MissingJobObject,
    IdentityUnavailable,
    QueryFailed,
    ScanLimit,
}

/// CPU percent uses one logical CPU as 100%, so parallel jobs may exceed 100%.
/// Only complete kernel accounting can establish `Known { percent: 0.0 }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCpuState {
    Known { percent: f64 },
    Unknown(JobCpuUnknown),
    Unavailable(JobCpuUnavailable),
}

/// A retained interval belongs to one probe instance, never merely a reused PID.
/// Missing or unknown observations must not authorize an inactivity kill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobCpuSample {
    pub probe_id: crate::ProbeId,
    pub leader_pid: u32,
    /// Linux `/proc` start ticks; absent where the owned kernel object is identity.
    pub leader_start_ticks: Option<u64>,
    pub accounting: JobCpuAccounting,
    /// The actual monotonic interval retained between counter observations.
    pub interval: Option<Duration>,
    pub state: JobCpuState,
}
