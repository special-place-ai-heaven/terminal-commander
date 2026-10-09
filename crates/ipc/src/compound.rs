// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Transport-independent command start, bounded observation, and resumable results.
//! The callback selects one engine/transport before entry. This module never retries a start.

use std::future::Future;

use serde::{Deserialize, Serialize};
use terminal_commander_core::SignalEvent;

use crate::{
    CommandStartParams, CommandStartResponse, CommandStatusResponse, IpcError, IpcRequest,
    IpcResponse,
};

pub const RUN_AND_WATCH_DEFAULT_WAIT_MS: u64 = 5_000;
pub const RUN_AND_WATCH_MAX_WAIT_MS: u64 = 60_000;
pub const RUN_AND_WATCH_DEFAULT_MAX_SIGNALS: usize = 50;
pub const RUN_AND_WATCH_MAX_SIGNALS: usize = 500;
pub const MAX_WAIT_SLICE_MS: u64 = 1_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct RunAndWatchOptions {
    /// Observation budget after start, clamped to 60 seconds. Zero still polls once.
    pub wait_ms: u64,
    /// Returned rule matches, clamped to 500. Zero preserves the initial resume cursor.
    pub max_signals: usize,
    /// Keep waiting after reaching the signal cap, up to the same wall-clock deadline.
    pub wait_until_exit: bool,
}

impl Default for RunAndWatchOptions {
    fn default() -> Self {
        Self {
            wait_ms: RUN_AND_WATCH_DEFAULT_WAIT_MS,
            max_signals: RUN_AND_WATCH_DEFAULT_MAX_SIGNALS,
            wait_until_exit: false,
        }
    }
}

/// A known start is always returned, including when later observation fails.
/// `status` is the last actual response, never a fabricated running or success state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct RunAndWatchOutcome {
    pub start: CommandStartResponse,
    pub status: Option<CommandStatusResponse>,
    /// Resume here to recover every rule match omitted because of `max_signals`.
    pub cursor: u64,
    pub signals: Vec<SignalEvent>,
    pub complete: bool,
    pub wait_exhausted: bool,
    pub signals_capped: bool,
    pub degraded: bool,
    /// Includes typed JobLost uncertainty; callers must reconcile, never replay the start.
    pub error: Option<IpcError>,
}

/// Start exactly once, then observe through the supplied engine until completion or a bound.
///
/// Each bucket wait is at most one second and is bounded by the remaining wall-clock budget.
/// Callback round trips may outlast that budget; in-flight operations are never cancelled or replayed.
///
/// # Errors
/// Returns an error only before a start response is known. Later failures are retained in the
/// outcome with the known identities, resume cursor, and last observed status.
pub async fn run_and_watch<F, Fut>(
    start: CommandStartParams,
    options: RunAndWatchOptions,
    mut call: F,
) -> Result<RunAndWatchOutcome, IpcError>
where
    F: FnMut(IpcRequest) -> Fut,
    Fut: Future<Output = Result<IpcResponse, IpcError>>,
{
    let wait_ms = options.wait_ms.min(RUN_AND_WATCH_MAX_WAIT_MS);
    let max_signals = options.max_signals.min(RUN_AND_WATCH_MAX_SIGNALS);
    let IpcResponse::CommandStartCombed(started) =
        call(IpcRequest::CommandStartCombed(start)).await?
    else {
        return Err(unexpected_response());
    };
    let mut cursor = started.cursor;
    let mut outcome = RunAndWatchOutcome {
        start: started,
        status: None,
        cursor,
        signals: Vec::new(),
        complete: false,
        wait_exhausted: true,
        signals_capped: false,
        degraded: false,
        error: None,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
    // Always obtain a real status first, even for a zero wait budget.
    loop {
        let status = match call(IpcRequest::CommandStatus(crate::CommandStatusParams {
            job_id: outcome.start.job_id,
        }))
        .await
        {
            Ok(IpcResponse::CommandStatus(status)) => status,
            Ok(_) => {
                outcome.error = Some(unexpected_response());
                break;
            }
            Err(error) => {
                outcome.error = Some(error);
                break;
            }
        };
        let terminal = terminal(status.state);
        outcome.status = Some(status);
        let remaining = u64::try_from(
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        let slice = if terminal {
            0
        } else {
            remaining.min(MAX_WAIT_SLICE_MS)
        };
        let request = wait_request(&outcome, cursor, max_signals, slice);
        if let Err(error) =
            collect_wait(call(request).await, &mut outcome, &mut cursor, max_signals)
        {
            outcome.error = Some(error);
            break;
        }
        if terminal || (!options.wait_until_exit && outcome.signals.len() >= max_signals) {
            break;
        }
        if std::time::Instant::now() >= deadline {
            // One final nonblocking drain preserves events arriving at the deadline.
            let request = wait_request(&outcome, cursor, max_signals, 0);
            if let Err(error) =
                collect_wait(call(request).await, &mut outcome, &mut cursor, max_signals)
            {
                outcome.error = Some(error);
            }
            break;
        }
    }
    outcome.degraded = outcome.error.is_some();
    outcome.complete = !outcome.degraded
        && outcome
            .status
            .as_ref()
            .is_some_and(|status| terminal(status.state));
    outcome.wait_exhausted = !outcome.complete;
    // Match the existing interrupted-wait presentation: degradation already marks
    // incompleteness; a signal-cap claim is made only on an uninterrupted result.
    outcome.signals_capped = !outcome.degraded && outcome.signals.len() >= max_signals;
    Ok(outcome)
}

const fn terminal(state: terminal_commander_core::JobState) -> bool {
    use terminal_commander_core::JobState;
    matches!(
        state,
        JobState::Exited | JobState::Failed | JobState::Cancelled
    )
}

fn unexpected_response() -> IpcError {
    IpcError::new(
        crate::IpcErrorCode::Internal,
        "daemon returned a response that did not match the request method",
    )
}

fn wait_request(
    outcome: &RunAndWatchOutcome,
    cursor: u64,
    max_signals: usize,
    timeout_ms: u64,
) -> IpcRequest {
    IpcRequest::BucketWait(crate::BucketWaitParams {
        bucket_id: outcome.start.bucket_id,
        cursor,
        severity_min: None,
        kind_filter: None,
        limit: Some(max_signals.saturating_sub(outcome.signals.len()).max(1)),
        timeout_ms: Some(timeout_ms),
    })
}

fn collect_wait(
    response: Result<IpcResponse, IpcError>,
    outcome: &mut RunAndWatchOutcome,
    observation_cursor: &mut u64,
    max_signals: usize,
) -> Result<(), IpcError> {
    let IpcResponse::BucketWait(response) = response? else {
        return Err(unexpected_response());
    };
    let remaining = max_signals.saturating_sub(outcome.signals.len());
    *observation_cursor = response.next_cursor;
    outcome.signals.extend(
        response
            .events
            .into_iter()
            .filter(|event| event.rule.is_some())
            .take(remaining),
    );
    if remaining > 0 {
        outcome.cursor = *observation_cursor;
    }
    Ok(())
}
