// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Content-free process identity and pipe observation contracts.

use serde::{Deserialize, Serialize};

use crate::ProbeId;

/// Child environment policy. Inheritance remains the default for host commands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentMode {
    #[default]
    Inherit,
    /// Start from an empty environment, then apply the explicit overlay.
    Clear,
}

/// Portable identity. A Windows job is identified by its owning probe, never a raw handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub probe_id: ProbeId,
    pub child_pid: u32,
    pub process_group_id: Option<u32>,
    pub ownership: ProcessOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOwnership {
    UnixProcessGroup,
    WindowsJobObject,
    /// Tree ownership could not be established. Consumers must not assume cleanup.
    LeaderOnly,
}

/// Bounded categories deliberately exclude arbitrary OS error strings and output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipeReadErrorKind {
    Interrupted,
    PermissionDenied,
    BrokenPipe,
    ConnectionReset,
    UnexpectedEof,
    TimedOut,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipeReadFailure {
    pub kind: PipeReadErrorKind,
    pub raw_os_error: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationIncompleteReason {
    Cancelled,
    DrainTimeout,
    RuntimeLost,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum StreamObservationState {
    #[default]
    Reading,
    Complete,
    Failed(PipeReadFailure),
    Incomplete(ObservationIncompleteReason),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamObservation {
    /// Actual transport bytes before transcoding, framing, or truncation, including LF.
    pub bytes_total: u64,
    pub state: StreamObservationState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessObservation {
    pub stdout: StreamObservation,
    pub stderr: StreamObservation,
}

impl ProcessObservation {
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.stdout.state, StreamObservationState::Complete)
            && matches!(self.stderr.state, StreamObservationState::Complete)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum ProcessCleanup {
    #[default]
    Running,
    /// The leader was reaped and no live owned group/job members were observed.
    Complete,
    /// Final termination was delivered; reaping or termination verification is pending.
    Reaping,
    /// Ownership or an OS cleanup operation could not be verified.
    Uncertain { raw_os_error: Option<i32> },
}
