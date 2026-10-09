// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use terminal_commanderd::DaemonConfig;
use terminal_commanderd::embedded::{EmbeddedEngine, IsolatedCommand};
use terminal_commanderd::ipc::protocol::{CommandStartParams, CommandStatusParams, IpcErrorCode};

#[tokio::test]
async fn identity_is_consistent_and_shutdown_closes_admission_without_ipc() {
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let identity = engine.identity();
    assert!(!identity.build.source_fingerprint.is_empty());
    assert_eq!(engine.health().await.unwrap().identity, identity);
    assert_eq!(
        engine.system_discover().await.unwrap().identity,
        Some(identity.clone())
    );
    assert!(!dir.path().join("terminal-commanderd.sock").exists());
    let report = engine.shutdown().await.unwrap();
    assert!(report.store_closed);
    assert_eq!(
        engine.health().await.unwrap_err().code,
        IpcErrorCode::ShuttingDown
    );
    let replacement = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    assert_ne!(replacement.identity().instance_id, identity.instance_id);
    replacement.shutdown().await.unwrap();
}

#[tokio::test]
async fn isolated_command_observes_raw_output_and_recovers_the_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let mut params = CommandStartParams::new(vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".into(),
        "embedded_child".into(),
        "--nocapture".into(),
    ]);
    params.env.push(("TC_EMBED_CHILD".into(), "present".into()));
    let request = IsolatedCommand::new(params, dir.path().to_path_buf());
    let started = engine.command_start_isolated(request).await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        let status = engine
            .command_status(CommandStatusParams {
                job_id: started.job_id,
            })
            .await
            .unwrap();
        if status.exit_code.is_some() {
            break status;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(status.exit_code, Some(0));
    assert!(status.bytes_total > 0);
    assert!(status.process_observation.as_ref().unwrap().is_complete());
    engine.shutdown().await.unwrap();
    let replacement = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let recovered = replacement
        .command_status(CommandStatusParams {
            job_id: started.job_id,
        })
        .await
        .unwrap();
    assert_eq!(recovered.exit_code, Some(0));
    assert!(recovered.restarted);
    assert_eq!(recovered.bytes_total, status.bytes_total);
    replacement.shutdown().await.unwrap();
}

#[test]
fn embedded_child() {
    use std::io::Write;
    if std::env::var_os("TC_EMBED_CHILD").is_none() {
        return;
    }
    assert!(std::env::var_os("PATH").is_none());
    assert!(std::env::var_os("TC_SOCKET").is_none());
    assert!(std::env::var_os("TC_DATA").is_none());
    // The installed login-shell autostart exits early for this reserved marker.
    assert!(matches!(
        std::env::var("TC_DAEMON_CHILD").as_deref(),
        Ok("1")
    ));
    std::io::stdout().write_all(b"partial").unwrap();
    std::io::stdout().flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
}

#[tokio::test]
async fn isolated_command_rejects_conflicting_daemon_child_markers() {
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let mut command = CommandStartParams::new(vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".into(),
        "embedded_child".into(),
    ]);
    let mut keys = vec!["TC_DAEMON_CHILD"];
    if cfg!(windows) {
        keys.push("tc_daemon_child");
    }
    for key in keys {
        for value in ["", "0", "other"] {
            command.env = vec![(key.into(), value.into())];
            let error = engine
                .command_start_isolated(IsolatedCommand::new(
                    command.clone(),
                    dir.path().to_path_buf(),
                ))
                .await
                .unwrap_err();
            assert_eq!(error.code, IpcErrorCode::ArgvInvalid);
        }
        command.env = vec![(key.into(), "1".into())];
        engine
            .command_start_isolated(IsolatedCommand::new(
                command.clone(),
                dir.path().to_path_buf(),
            ))
            .await
            .unwrap();
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn stopped_command_eventually_reports_completed_cleanup() {
    use terminal_commanderd::embedded::core::ProcessCleanup;
    use terminal_commanderd::ipc::protocol::CommandStopParams;
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let mut command = CommandStartParams::new(vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".into(),
        "embedded_long_child".into(),
        "--nocapture".into(),
    ]);
    command
        .env
        .push(("TC_EMBED_CHILD".into(), "present".into()));
    let started = engine
        .command_start_isolated(IsolatedCommand::new(command, dir.path().to_path_buf()))
        .await
        .unwrap();
    engine
        .command_stop(CommandStopParams {
            job_id: started.job_id,
        })
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let status = engine
            .command_status(CommandStatusParams {
                job_id: started.job_id,
            })
            .await
            .unwrap();
        if status.process_cleanup == Some(ProcessCleanup::Complete) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "cancelled status must publish final cleanup evidence; state={:?}, cleanup={:?}, observation={:?}, identity={:?}, exit_code={:?}, restarted={}",
            status.state,
            status.process_cleanup,
            status.process_observation,
            status.process_identity,
            status.exit_code,
            status.restarted,
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(engine.shutdown().await.unwrap().lifecycle_drained);
}

#[test]
fn embedded_long_child() {
    if std::env::var_os("TC_EMBED_CHILD").is_none() {
        return;
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[tokio::test]
async fn isolated_inputs_are_bounded_redacted_and_policy_checked() {
    use terminal_commanderd::PolicyProfile;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut config = DaemonConfig::defaults_in(dir.path().join("data"));
    config.policy.profile = PolicyProfile::RepoOnly;
    config.policy.repo_root = Some(dir.path().to_path_buf());
    let engine = EmbeddedEngine::bootstrap(config).unwrap();
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut command = CommandStartParams::new(vec![executable]);
    command.env = vec![("DISPLAY_VALUE".into(), uuid::Uuid::new_v4().to_string())];
    assert!(!format!("{command:?}").contains(&command.env[0].1));
    let request = IsolatedCommand::new(command.clone(), outside.path().to_path_buf());
    assert!(!format!("{request:?}").contains(&command.env[0].1));
    assert_eq!(
        engine
            .command_start_isolated(request)
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::PolicyDenied
    );
    command.argv[0] = "relative-program".into();
    assert_eq!(
        engine
            .command_start_isolated(IsolatedCommand::new(
                command.clone(),
                dir.path().to_path_buf()
            ))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::ArgvInvalid
    );
    command.argv[0] = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    command.env = vec![("SAME".into(), "one".into()), ("SAME".into(), "two".into())];
    assert_eq!(
        engine
            .command_start_isolated(IsolatedCommand::new(
                command.clone(),
                dir.path().to_path_buf()
            ))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::ArgvInvalid
    );
    command.env = vec![(
        "BOUND".into(),
        "x".repeat(terminal_commanderd::MAX_ARGV_ITEM_BYTES + 1),
    )];
    assert_eq!(
        engine
            .command_start_isolated(IsolatedCommand::new(command, dir.path().to_path_buf()))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::ArgvInvalid
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn owner_authority_is_explicit_and_cannot_be_claimed_in_request_json() {
    use terminal_commanderd::embedded::{EmbeddedAuthority, core::ActivationScope};
    use terminal_commanderd::ipc::protocol::RecipeImportSeedsParams;
    let dir = tempfile::tempdir().unwrap();
    let mut config = DaemonConfig::defaults_in(dir.path());
    config.policy.llm_can_activate_recipes = Some(false);
    let request = RecipeImportSeedsParams {
        activate: true,
        scope: Some(ActivationScope::Global),
        from_mcp: false,
    };
    let engine = EmbeddedEngine::bootstrap(config.clone()).unwrap();
    assert_eq!(
        engine
            .recipe_import_seeds(request.clone())
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::PolicyDenied
    );
    engine.shutdown().await.unwrap();
    let engine =
        EmbeddedEngine::bootstrap_with_authority(config, EmbeddedAuthority::HostAdministrator)
            .unwrap();
    let imported = engine.recipe_import_seeds(request).await.unwrap();
    assert!(!imported.activated.is_empty());
    assert!(imported.failed.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn serialized_service_envelope_preserves_correlation_and_does_not_attach_remotes() {
    use terminal_commanderd::ipc::protocol::{
        IpcRequest, IpcResponse, IpcResult, RequestEnvelope, ResponseEnvelope,
    };
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let envelope = RequestEnvelope {
        correlation_id: 73,
        request: IpcRequest::Health,
    };
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let decoded = serde_json::from_slice(&bytes).unwrap();
    let response = engine.execute_envelope(decoded).await;
    let response: ResponseEnvelope =
        serde_json::from_slice(&serde_json::to_vec(&response).unwrap()).unwrap();
    assert_eq!(response.correlation_id, 73);
    assert!(matches!(
        response.result,
        IpcResult::Ok {
            response: IpcResponse::Health { .. }
        }
    ));
    let mut command = CommandStartParams::new(vec!["unused".into()]);
    command.environment = Some(
        terminal_commanderd::embedded::core::EnvironmentSpec::WslDistro {
            distro: "unavailable-room".into(),
        },
    );
    assert!(engine.command_start_combed(command).await.is_err());
    let request = IpcRequest::CommandStartIsolated(IsolatedCommand::new(
        CommandStartParams::new(vec!["relative-program".into()]),
        dir.path().to_path_buf(),
    ));
    let request = serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
    assert_eq!(
        engine.execute(request).await.unwrap_err().code,
        IpcErrorCode::ArgvInvalid
    );
    engine.shutdown().await.unwrap();
}

#[test]
fn embedded_configuration_rejects_an_unrooted_repo_policy() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = DaemonConfig::defaults_in(dir.path());
    config.policy.profile = terminal_commanderd::PolicyProfile::RepoOnly;
    config.policy.repo_root = None;
    assert!(matches!(
        EmbeddedEngine::bootstrap(config),
        Err(terminal_commanderd::BootstrapError::Config(_))
    ));
}

#[tokio::test]
async fn data_directory_has_one_owner_until_shutdown_releases_it() {
    let dir = tempfile::tempdir().unwrap();
    let config = DaemonConfig::defaults_in(dir.path());
    let engine = EmbeddedEngine::bootstrap(config.clone()).unwrap();
    let duplicate = EmbeddedEngine::bootstrap(config.clone());
    assert!(
        duplicate.is_err(),
        "a live engine must exclusively own its data directory"
    );
    engine.shutdown().await.unwrap();
    let replacement = EmbeddedEngine::bootstrap(config).unwrap();
    replacement.shutdown().await.unwrap();
}

#[tokio::test]
async fn compound_execution_preserves_typed_result_and_identity() {
    use terminal_commanderd::embedded::RunAndWatchOptions;
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path())).unwrap();
    let mut command = CommandStartParams::new(vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".into(),
        "embedded_child".into(),
        "--nocapture".into(),
    ]);
    let env_value = uuid::Uuid::new_v4().to_string();
    command
        .env
        .push(("TC_DIAGNOSTIC_TEST".into(), env_value.clone()));
    let outcome = engine
        .run_and_watch(
            command,
            RunAndWatchOptions {
                wait_ms: 5_000,
                max_signals: 10,
                wait_until_exit: true,
            },
        )
        .await
        .unwrap();
    assert!(!outcome.degraded);
    assert!(outcome.complete);
    let status = outcome.status.unwrap();
    assert_eq!(status.exit_code, Some(0));
    assert!(status.bytes_total > 0);
    let audit = engine
        .audit_since(terminal_commanderd::ipc::protocol::AuditSinceParams {
            cursor: 0,
            action_filter: Some("command_start".into()),
            decision_filter: None,
            limit: Some(10),
        })
        .await
        .unwrap();
    let serialized = serde_json::to_string(&audit).unwrap();
    assert!(!serialized.contains(&env_value));
    assert!(serialized.contains(&engine.identity().instance_id));
    assert!(serialized.contains("engine_build_fingerprint"));
    engine.shutdown().await.unwrap();
}

#[cfg(any(unix, windows))]
#[tokio::test]
#[allow(clippy::too_many_lines)] // One real PTY owner challenge through final delivery.
async fn embedded_owner_challenge_is_headless_serializable_and_prompt_bound() {
    use terminal_commanderd::embedded::{EmbeddedAuthority, EngineFeature, FeatureAvailability};
    use terminal_commanderd::ipc::protocol::{
        CredentialOwnerAction, CredentialProvideChallengeParams, CredentialRequestParams,
        CredentialStatus, CredentialUrlOp, CredentialUrlParams, IpcRequest, IpcResponse,
        OwnerSecret, PtyCommandStartParams,
    };
    let dir = tempfile::tempdir().unwrap();
    let engine = EmbeddedEngine::bootstrap_with_authority(
        DaemonConfig::defaults_in(dir.path().join("owner")),
        EmbeddedAuthority::HostAdministrator,
    )
    .unwrap();
    assert!(
        engine
            .capabilities()
            .iter()
            .any(|cap| cap.feature == EngineFeature::OwnerCredentials
                && cap.availability == FeatureAvailability::Available)
    );
    let python = if cfg!(windows) {
        "python"
    } else {
        "/usr/bin/python3"
    };
    let started = engine.pty_command_start(PtyCommandStartParams {
        environment: None, argv: vec![python.into(), "-u".into(), "-c".into(),
            "import getpass; answer=getpass.getpass('Password: '); assert answer; print('owner input received')".into()],
        cwd: None, env: vec![], bucket_config: None, rules: vec![], rows: None, cols: None,
        tag: None, limits: None,
    }).await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let list = engine.pty_command_list().await.unwrap();
        if list
            .entries
            .iter()
            .any(|entry| entry.job_id == started.job_id && entry.awaiting_credential.is_some())
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "fixture did not request owner input"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    // URL-mode remains callable without daemon IPC; abandoning it returns to
    // the host interaction, not to a native window or an unbound CLI hint.
    engine
        .credential_url(CredentialUrlParams {
            job_id: started.job_id,
            op: CredentialUrlOp::Open,
        })
        .await
        .unwrap();
    engine
        .credential_url(CredentialUrlParams {
            job_id: started.job_id,
            op: CredentialUrlOp::Abandon,
        })
        .await
        .unwrap();
    let requested = engine
        .credential_request(CredentialRequestParams {
            job_id: started.job_id,
            wait_ms: Some(100),
        })
        .await
        .unwrap();
    assert_eq!(requested.status, CredentialStatus::OwnerActionRequired);
    assert!(requested.command.is_none());
    let CredentialOwnerAction::Provide { challenge } = requested.owner_action.unwrap();
    assert_eq!(challenge.instance_id, engine.identity().instance_id);
    assert_eq!(challenge.job_id, started.job_id);
    let value = uuid::Uuid::new_v4().to_string();
    let provide = CredentialProvideChallengeParams {
        challenge: challenge.clone(),
        secret: OwnerSecret::new(value.clone()),
    };
    assert!(!format!("{provide:?}").contains(&value));
    let unprivileged =
        EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(dir.path().join("unprivileged")))
            .unwrap();
    assert_eq!(
        unprivileged
            .credential_provide_challenge(provide.clone())
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::PolicyDenied
    );
    unprivileged.shutdown().await.unwrap();
    let mut stale = provide.clone();
    stale.challenge.prompt_generation = stale.challenge.prompt_generation.wrapping_add(1);
    assert_eq!(
        engine
            .credential_provide_challenge(stale)
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::UnknownJob
    );
    let mut foreign = provide.clone();
    foreign.challenge.instance_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        engine
            .credential_provide_challenge(foreign)
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::UnknownJob
    );
    let wire = serde_json::to_vec(&IpcRequest::CredentialProvideChallenge(provide)).unwrap();
    let request = serde_json::from_slice(&wire).unwrap();
    assert!(matches!(
        engine.execute(request).await.unwrap(),
        IpcResponse::CredentialProvide(_)
    ));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = engine
            .command_status(CommandStatusParams {
                job_id: started.job_id,
            })
            .await
            .unwrap();
        if status.exit_code.is_some() {
            assert_eq!(status.exit_code, Some(0));
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(engine.shutdown().await.unwrap().lifecycle_drained);
}

#[tokio::test]
// Keep the complete cross-lane lifecycle visible in one integration scenario.
#[allow(clippy::too_many_lines)]
async fn files_watches_sifters_buckets_subscriptions_and_registry_share_the_engine() {
    use terminal_commanderd::embedded::core::{
        ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
    };
    use terminal_commanderd::ipc::protocol::*;
    let dir = tempfile::tempdir().unwrap();
    let mut config = DaemonConfig::defaults_in(dir.path().join("data"));
    config.policy.profile = terminal_commanderd::PolicyProfile::FullAccess;
    let engine = EmbeddedEngine::bootstrap(config).unwrap();
    let path = dir.path().join("signal.log");
    engine
        .file_write(FileWriteParams {
            path: path.clone(),
            content: "boot\n".into(),
            create_dirs: false,
            append: false,
        })
        .await
        .unwrap();
    let read = engine
        .file_read_window(FileReadWindowParams {
            path: path.clone(),
            start_line: Some(1),
            max_lines: Some(1),
            max_bytes: Some(256),
        })
        .await
        .unwrap();
    assert_eq!(read.lines[0].text, "boot");
    let rule = RuleDefinition {
        id: "embed.marker".into(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Info,
        event_kind: "embed_marker".into(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec!["NEEDLE".into()]),
        captures: vec![],
        summary_template: "marker observed".into(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    };
    let watch = engine
        .file_watch_start(FileWatchStartParams {
            path: path.clone(),
            bucket_config: None,
            rules: vec![rule],
            follow_from_beginning: Some(true),
            tag: Some("embed".into()),
        })
        .await
        .unwrap();
    let sub = engine
        .subscription_open(SubscriptionOpenParams {
            predicate: SubscriptionPredicate {
                severity_min: None,
                kind: Some(vec!["embed_marker".into()]),
                sources: SubscriptionSourceSel::Buckets {
                    buckets: vec![watch.bucket_id],
                },
                tag: None,
            },
        })
        .await
        .unwrap();
    assert_eq!(sub.boot_id, engine.identity().instance_id);
    engine
        .file_write(FileWriteParams {
            path,
            content: "NEEDLE\n".into(),
            create_dirs: false,
            append: true,
        })
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let pulled = engine
            .subscription_pull(SubscriptionPullParams {
                sub_id: sub.sub_id.clone(),
                max: Some(10),
                timeout_ms: Some(200),
                liveness_delta: false,
            })
            .await
            .unwrap();
        if !pulled.events.is_empty() {
            assert_eq!(pulled.events[0].bucket_id, watch.bucket_id);
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
    }
    assert!(
        engine
            .bucket_summary(BucketSummaryParams {
                bucket_id: watch.bucket_id
            })
            .await
            .is_ok()
    );
    assert!(
        engine
            .registry_import_pack(RegistryImportPackParams {
                pack: "cargo".into(),
                activate: false,
                scope: None
            })
            .await
            .unwrap()
            .failed
            .is_empty()
    );
    assert!(
        engine
            .subscription_close(SubscriptionCloseParams { sub_id: sub.sub_id })
            .await
            .unwrap()
            .closed
    );
    assert_eq!(engine.file_watch_list().await.unwrap().entries.len(), 1);
    assert!(engine.shutdown().await.unwrap().lifecycle_drained);
}
