// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shared `JobState -> Liveness` projection (subscriptions spec MUST-ADD #3).
//!
//! Both the runtime probe-list handler (`ipc::handlers::runtime`) and the
//! subscription pull engine (`subscriptions::pull`) project a command job's
//! authoritative `JobState` (+ exit metadata) onto the wire [`Liveness`]. They
//! historically carried byte-identical copies of this match; a single source
//! prevents a future `JobState` variant from silently diverging the two
//! consumers.
//!
//! The load-bearing invariant: a cancelled job reports [`Liveness::Cancelled`],
//! NOT [`Liveness::Failed`], even though `cancel()` stamps a `"CANCELLED"`
//! signal on the exit info. Reading the ledger `JobState` (not live-map
//! presence) is the caller's responsibility — bindings linger after exit.

use terminal_commander_ipc::Liveness;

/// Map a command job's ledger `JobState` (+ exit metadata) to wire [`Liveness`].
///
/// `finish` only assigns [`terminal_commander_core::JobState::Exited`] for a
/// clean (code 0, no signal) exit, so a missing exit code on `Exited` defaults
/// to 0. Cancellation MUST NOT fold into `Failed`.
pub(crate) fn command_liveness(
    state: terminal_commander_core::JobState,
    exit_code: Option<i32>,
    signal: Option<String>,
) -> Liveness {
    use terminal_commander_core::JobState;
    match state {
        JobState::Starting => Liveness::Starting,
        JobState::Running => Liveness::Running,
        JobState::Exited => Liveness::Exited {
            code: exit_code.unwrap_or(0),
        },
        JobState::Failed => Liveness::Failed {
            code: exit_code,
            signal,
        },
        JobState::Cancelled => Liveness::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::JobState;

    // The load-bearing invariant (spec MUST-ADD #3): a cancelled job
    // reports `Cancelled`, NOT `Failed`, even though `cancel()` stamps a
    // `"CANCELLED"` signal on the exit info. An exited job reports
    // `Exited{code}`.
    #[test]
    fn command_liveness_maps_jobstate_without_folding_cancel_into_failed() {
        assert_eq!(
            command_liveness(JobState::Starting, None, None),
            Liveness::Starting
        );
        assert_eq!(
            command_liveness(JobState::Running, None, None),
            Liveness::Running
        );
        // Clean exit -> Exited{code}. `finish` only sets Exited for code 0.
        assert_eq!(
            command_liveness(JobState::Exited, Some(0), None),
            Liveness::Exited { code: 0 }
        );
        // Missing exit code on an Exited state defaults to 0 (clean).
        assert_eq!(
            command_liveness(JobState::Exited, None, None),
            Liveness::Exited { code: 0 }
        );
        // Non-zero / signalled exit -> Failed{code,signal}.
        assert_eq!(
            command_liveness(JobState::Failed, Some(2), None),
            Liveness::Failed {
                code: Some(2),
                signal: None
            }
        );
        assert_eq!(
            command_liveness(JobState::Failed, None, Some("SIGTERM".to_owned())),
            Liveness::Failed {
                code: None,
                signal: Some("SIGTERM".to_owned())
            }
        );
        // Cancellation -> Cancelled, NOT Failed (even with a CANCELLED
        // signal present on the exit info).
        assert_eq!(
            command_liveness(JobState::Cancelled, None, Some("CANCELLED".to_owned())),
            Liveness::Cancelled
        );
    }
}
