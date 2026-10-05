// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! The `environment` field of the start requests.
//!
//! Environment runners were never built: a non-local value used to reach a
//! half-built forward that failed as an `Internal` error. It is now refused up
//! front with an actionable `SchemaMismatch`, on both start paths. The field
//! stays on the wire, so every shape an older client sends still decodes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use terminal_commander_core::EnvironmentSpec;
use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::{
    CommandStartParams, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse,
    IpcResult, PtyCommandStartParams, RequestEnvelope,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!(
        "tc-environment-field-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ))
}

async fn dispatch(state: &Arc<DaemonState>, request: IpcRequest) -> IpcResult {
    let envelope = RequestEnvelope {
        correlation_id: 1,
        request,
    };
    terminal_commanderd::ipc::dispatch_envelope(
        state,
        Instant::now(),
        &envelope,
        &PeerIdentity::unknown(),
    )
    .await
    .result
}

const fn combed(environment: Option<EnvironmentSpec>, argv: Vec<String>) -> IpcRequest {
    IpcRequest::CommandStartCombed(CommandStartParams {
        environment,
        argv,
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace_ms: None,
        tag: None,
        strip_ansi: true,
        dedup_nonce: None,
        receipt_shape: None,
    })
}

const fn pty(environment: Option<EnvironmentSpec>, argv: Vec<String>) -> IpcRequest {
    IpcRequest::PtyCommandStart(PtyCommandStartParams {
        environment,
        argv,
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        rows: None,
        cols: None,
        tag: None,
    })
}

fn non_local() -> [EnvironmentSpec; 2] {
    [
        EnvironmentSpec::WslDistro {
            distro: "Ubuntu-24.04".to_owned(),
        },
        EnvironmentSpec::SshHost {
            host: "build-box".to_owned(),
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_local_environment_is_refused_with_what_to_use_instead() {
    let data = tmp_data_dir("refused");
    let state = Arc::new(DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap());
    let argv = vec!["anything".to_owned()];

    for environment in non_local() {
        for request in [
            combed(Some(environment.clone()), argv.clone()),
            pty(Some(environment.clone()), argv.clone()),
        ] {
            match dispatch(&state, request).await {
                IpcResult::Err { error } => {
                    assert_eq!(error.code, IpcErrorCode::SchemaMismatch, "{error:?}");
                    for hint in [
                        "environment runners are not supported",
                        "wsl_argv",
                        "target_id",
                    ] {
                        assert!(error.message.contains(hint), "{hint}: {error:?}");
                    }
                }
                IpcResult::Ok { response } => {
                    panic!("{environment:?} must be refused, got {response:?}")
                }
            }
        }
    }

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_local_environment_starts_as_before() {
    let data = tmp_data_dir("local");
    let state = Arc::new(DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap());
    let exe = std::env::current_exe()
        .expect("current test binary path")
        .to_string_lossy()
        .into_owned();

    match dispatch(
        &state,
        combed(Some(EnvironmentSpec::Local), vec![exe, "--list".to_owned()]),
    )
    .await
    {
        IpcResult::Ok {
            response: IpcResponse::CommandStartCombed(_),
        } => {}
        other => panic!("a local environment must start the command, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn every_environment_shape_an_older_client_sends_still_decodes() {
    let shapes = [
        ("", None),
        (r#""environment":null,"#, None),
        (
            r#""environment":{"kind":"local"},"#,
            Some(EnvironmentSpec::Local),
        ),
        (
            r#""environment":{"kind":"wsl_distro","distro":"Ubuntu-24.04"},"#,
            Some(non_local()[0].clone()),
        ),
        (
            r#""environment":{"kind":"ssh_host","host":"build-box"},"#,
            Some(non_local()[1].clone()),
        ),
    ];
    for (field, expected) in shapes {
        let json = format!(r#"{{{field}"argv":["true"]}}"#);
        let combed: CommandStartParams = serde_json::from_str(&json).expect(&json);
        assert_eq!(combed.environment, expected, "{json}");
        let pty: PtyCommandStartParams = serde_json::from_str(&json).expect(&json);
        assert_eq!(pty.environment, expected, "{json}");
    }
}
