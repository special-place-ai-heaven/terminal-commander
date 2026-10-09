// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use terminal_commander_core::{BucketId, EventId, JobId, JobState, ProbeId, RuleId, SignalEvent};
use terminal_commander_ipc::compound::{RunAndWatchOptions, run_and_watch};
use terminal_commander_ipc::{
    BucketWaitResponse, CommandStartParams, CommandStartResponse, CommandStatusResponse, IpcError,
    IpcErrorCode, IpcRequest, IpcResponse,
};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn params() -> CommandStartParams {
    serde_json::from_value(serde_json::json!({"argv":["fixture"]})).unwrap()
}

fn started() -> CommandStartResponse {
    serde_json::from_value(serde_json::json!({
        "job_id": JobId::new(), "bucket_id":BucketId::new(), "probe_id":ProbeId::new(), "cursor":4
    }))
    .unwrap()
}

fn status(start: &CommandStartResponse, state: JobState) -> IpcResponse {
    IpcResponse::CommandStatus(
        serde_json::from_value::<CommandStatusResponse>(serde_json::json!({
            "job_id":start.job_id,"bucket_id":start.bucket_id,"probe_id":start.probe_id,
            "state":state,"frames_total":0,"frames_stdout":0,"frames_stderr":0,"bytes_total":0,
            "events_emitted":0,"exit_code":if state == JobState::Exited {Some(0)} else {None},
            "signal":null,"duration_ms":null,"receipt":null
        }))
        .unwrap(),
    )
}

fn signal(start: &CommandStartResponse, seq: u64, matched: bool) -> SignalEvent {
    serde_json::from_value(serde_json::json!({
        "event_id":EventId::new(),"bucket_id":start.bucket_id,"seq":seq,
        "timestamp":"2026-10-09T00:00:00Z","severity":"info","kind":"fixture","summary":"fixture",
        "source":{"probe_id":start.probe_id,"source_type":"process","stream":"stdout"},
        "rule":matched.then(||serde_json::json!({"id":RuleId::new(),"version":1}))
    }))
    .unwrap()
}

const fn bucket(
    start: &CommandStartResponse,
    from: u64,
    to: u64,
    events: Vec<SignalEvent>,
) -> IpcResponse {
    IpcResponse::BucketWait(BucketWaitResponse {
        bucket_id: start.bucket_id,
        cursor_in: from,
        next_cursor: to,
        heartbeat: events.is_empty(),
        dropped_count: 0,
        events,
    })
}

#[test]
fn zero_budget_starts_once_polls_status_then_drains_and_keeps_identity() {
    runtime().block_on(async {
        let start = started();
        let mut replies = VecDeque::from([
            Ok(IpcResponse::CommandStartCombed(start.clone())),
            Ok(status(&start, JobState::Running)),
            Ok(bucket(&start, 4, 5, vec![signal(&start, 5, false)])),
            Ok(bucket(&start, 5, 6, vec![signal(&start, 6, true)])),
        ]);
        let mut calls = Vec::new();
        let outcome = run_and_watch(
            params(),
            RunAndWatchOptions {
                wait_ms: 0,
                ..Default::default()
            },
            |request| {
                calls.push(request);
                std::future::ready(replies.pop_front().expect("no extra calls"))
            },
        )
        .await
        .unwrap();
        assert!(matches!(&calls[0], IpcRequest::CommandStartCombed(_)));
        assert!(matches!(&calls[1], IpcRequest::CommandStatus(_)));
        assert!(
            calls[2..]
                .iter()
                .all(|r| matches!(r,IpcRequest::BucketWait(p) if p.timeout_ms==Some(0)))
        );
        assert_eq!(outcome.start.job_id, start.job_id);
        assert_eq!(outcome.start.bucket_id, start.bucket_id);
        assert_eq!(outcome.cursor, 6);
        assert_eq!(outcome.signals.len(), 1);
        assert_eq!(outcome.signals[0].seq, 6);
        assert!(!outcome.complete);
        assert!(outcome.wait_exhausted);
        assert!(!outcome.degraded);
        assert!(replies.is_empty());
    });
}

#[test]
fn signal_cap_keeps_observation_cursor_separate_from_resume_cursor() {
    runtime().block_on(async {
        let start = started();
        let mut replies = VecDeque::from([
            Ok(IpcResponse::CommandStartCombed(start.clone())),
            Ok(status(&start, JobState::Running)),
            Ok(bucket(&start, 4, 5, vec![signal(&start, 5, true)])),
            Ok(status(&start, JobState::Exited)),
            Ok(bucket(&start, 5, 6, vec![signal(&start, 6, true)])),
        ]);
        let mut cursors = Vec::new();
        let outcome = run_and_watch(
            params(),
            RunAndWatchOptions {
                wait_ms: 1000,
                max_signals: 1,
                wait_until_exit: true,
            },
            |r| {
                if let IpcRequest::BucketWait(p) = r {
                    cursors.push(p.cursor);
                    assert_eq!(p.limit, Some(1));
                }
                std::future::ready(replies.pop_front().expect("no extra calls"))
            },
        )
        .await
        .unwrap();
        assert_eq!(cursors, [4, 5]);
        assert_eq!(outcome.cursor, 5);
        assert_eq!(outcome.signals.len(), 1);
        assert!(outcome.complete);
        assert!(outcome.signals_capped);
        assert!(!outcome.wait_exhausted);
    });
}

#[test]
fn post_start_job_lost_preserves_ids_and_actual_unknown_state_without_replaying() {
    runtime().block_on(async {
        let start = started();
        let mut replies = VecDeque::from([
            Ok(IpcResponse::CommandStartCombed(start.clone())),
            Err(IpcError::new(
                IpcErrorCode::JobLost,
                "fixture receipt missing",
            )),
        ]);
        let mut starts = 0;
        let outcome = run_and_watch(params(), RunAndWatchOptions::default(), |r| {
            starts += usize::from(matches!(r, IpcRequest::CommandStartCombed(_)));
            std::future::ready(replies.pop_front().expect("never retry"))
        })
        .await
        .unwrap();
        assert_eq!(starts, 1);
        assert_eq!(outcome.start.job_id, start.job_id);
        assert_eq!(outcome.cursor, start.cursor);
        assert!(outcome.status.is_none());
        assert_eq!(outcome.error.unwrap().code, IpcErrorCode::JobLost);
        assert!(outcome.degraded);
        assert!(!outcome.complete);
    });
}

#[test]
fn wall_clock_deadline_bounds_wait_slices_and_performs_final_nonblocking_drain() {
    runtime().block_on(async {
        let start = started();
        let mut waits = Vec::new();
        let beginning = Instant::now();
        let outcome = run_and_watch(
            params(),
            RunAndWatchOptions {
                wait_ms: 25,
                ..Default::default()
            },
            |r| {
                let response = match r {
                    IpcRequest::CommandStartCombed(_) => {
                        IpcResponse::CommandStartCombed(start.clone())
                    }
                    IpcRequest::CommandStatus(_) => status(&start, JobState::Running),
                    IpcRequest::BucketWait(p) => {
                        let ms = p.timeout_ms.unwrap();
                        assert!(ms <= 25);
                        waits.push(ms);
                        bucket(&start, p.cursor, p.cursor, vec![])
                    }
                    _ => panic!("unexpected request"),
                };
                let delay = waits.last().copied().unwrap_or(0);
                async move {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    Ok(response)
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(waits.last(), Some(&0));
        assert!(beginning.elapsed() < Duration::from_secs(1));
        assert!(outcome.wait_exhausted);
    });
}

#[test]
fn start_failure_is_returned_once_without_observation_or_retry() {
    runtime().block_on(async {
        let mut calls = 0;
        let result = run_and_watch(params(), RunAndWatchOptions::default(), |_| {
            calls += 1;
            std::future::ready(Err(IpcError::new(
                IpcErrorCode::ArgvInvalid,
                "fixture denied",
            )))
        })
        .await;
        assert_eq!(calls, 1);
        assert_eq!(result.unwrap_err().code, IpcErrorCode::ArgvInvalid);
    });
}

#[test]
fn bucket_failure_retains_last_observed_status_but_never_claims_completion() {
    runtime().block_on(async {
        let start = started();
        let mut replies = VecDeque::from([
            Ok(IpcResponse::CommandStartCombed(start.clone())),
            Ok(status(&start, JobState::Exited)),
            Err(IpcError::new(
                IpcErrorCode::Internal,
                "fixture bucket failure",
            )),
        ]);
        let outcome = run_and_watch(params(), RunAndWatchOptions::default(), |_| {
            std::future::ready(replies.pop_front().unwrap())
        })
        .await
        .unwrap();
        assert_eq!(outcome.start.job_id, start.job_id);
        assert_eq!(outcome.status.unwrap().state, JobState::Exited);
        assert!(!outcome.complete);
        assert!(outcome.degraded);
        assert!(outcome.wait_exhausted);
        assert!(!outcome.signals_capped);
    });
}

#[test]
fn deadline_drain_failure_is_explicit_and_keeps_the_recovery_cursor() {
    runtime().block_on(async {
        let start = started();
        let mut replies = VecDeque::from([
            Ok(IpcResponse::CommandStartCombed(start.clone())),
            Ok(status(&start, JobState::Running)),
            Ok(bucket(&start, 4, 5, vec![signal(&start, 5, true)])),
            Err(IpcError::new(
                IpcErrorCode::JobLost,
                "fixture final drain lost",
            )),
        ]);
        let outcome = run_and_watch(
            params(),
            RunAndWatchOptions {
                wait_ms: 0,
                ..Default::default()
            },
            |_| std::future::ready(replies.pop_front().unwrap()),
        )
        .await
        .unwrap();
        assert!(outcome.degraded);
        assert_eq!(outcome.cursor, 5);
        assert_eq!(outcome.signals.len(), 1);
        assert_eq!(outcome.error.unwrap().code, IpcErrorCode::JobLost);
    });
}

#[test]
fn wrong_response_after_start_is_degraded_and_contains_no_response_payload() {
    runtime().block_on(async {
        let start = started();
        let mut unrelated = started();
        unrelated
            .wslenv_dropped
            .push("untrusted-payload-marker".into());
        let mut replies = VecDeque::from([
            Ok(IpcResponse::CommandStartCombed(start.clone())),
            Ok(IpcResponse::CommandStartCombed(unrelated)),
        ]);
        let outcome = run_and_watch(params(), RunAndWatchOptions::default(), |_| {
            std::future::ready(replies.pop_front().unwrap())
        })
        .await
        .unwrap();
        assert!(outcome.degraded);
        assert_eq!(outcome.start.job_id, start.job_id);
        assert!(outcome.status.is_none());
        assert!(
            !outcome
                .error
                .unwrap()
                .message
                .contains("untrusted-payload-marker")
        );
    });
}

#[test]
fn default_signal_cap_ends_wait_and_hard_caps_are_enforced() {
    runtime().block_on(async {
        for max in [0, 1, usize::MAX] {
            let start = started();
            let effective = max.min(500);
            let mut calls = 0;
            let outcome = run_and_watch(
                params(),
                RunAndWatchOptions {
                    wait_ms: u64::MAX,
                    max_signals: max,
                    wait_until_exit: false,
                },
                |request| {
                    calls += 1;
                    let response = match request {
                        IpcRequest::CommandStartCombed(_) => {
                            IpcResponse::CommandStartCombed(start.clone())
                        }
                        IpcRequest::CommandStatus(_) => status(&start, JobState::Running),
                        IpcRequest::BucketWait(p) => {
                            assert_eq!(p.limit, Some(effective.max(1)));
                            assert!(p.timeout_ms.unwrap() <= 1000);
                            bucket(
                                &start,
                                4,
                                504,
                                (0..effective.max(1))
                                    .map(|i| signal(&start, i as u64 + 5, true))
                                    .collect(),
                            )
                        }
                        _ => panic!("unexpected request"),
                    };
                    std::future::ready(Ok(response))
                },
            )
            .await
            .unwrap();
            assert_eq!(calls, 3);
            assert_eq!(outcome.signals.len(), effective);
            assert!(outcome.signals_capped);
            assert!(!outcome.complete);
            if effective == 0 {
                assert_eq!(outcome.cursor, 4);
            }
        }
    });
}
