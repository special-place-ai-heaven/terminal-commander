// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC smoke for the recipe store: upsert, get, search, activate,
//! list_active, deactivate, list_versions, tombstone. No MCP tools.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ActivationScope, JobId, RecipeDefinition, RecipeStatus, shell_interpreter_denied,
};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    ListLimitParams, RecipeActivateParams, RecipeDeactivateParams, RecipeGetParams,
    RecipeImportSeedsParams, RecipeListVersionsParams, RecipeRunParams, RecipeSearchParams,
    RecipeTombstoneParams, RecipeUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-ipc-recipe-{tag}-{}-{n}", std::process::id()));
    p
}

fn recipe(status: RecipeStatus) -> RecipeDefinition {
    RecipeDefinition {
        recipe_id: "git.status".to_owned(),
        version: 1,
        title: "Git status".to_owned(),
        summary: "Short working tree status".to_owned(),
        argv: vec!["git".to_owned(), "status".to_owned(), "--short".to_owned()],
        status,
        tags: vec!["git".to_owned(), "vcs".to_owned()],
        cwd: None,
        env_allowlist: vec![],
        timeout_ms: None,
        rule_pack_ids: vec![],
        placeholders: vec![],
    }
}

fn shell_argv() -> RecipeDefinition {
    let mut def = recipe(RecipeStatus::Draft);
    def.argv = vec!["bash".to_owned(), "-c".to_owned(), "echo hi".to_owned()];
    def
}

#[test]
#[allow(clippy::too_many_lines)] // one IPC lifecycle, same shape as registry_ipc
fn recipe_ipc_lifecycle_and_interpreter_deny() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("life");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.recipe_admin_test_seam = true;
        // The interpreter deny follows allow_shell, which developer_local grants.
        cfg.policy.caps = Some(terminal_commanderd::PolicyCapsSection {
            allow_shell: Some(false),
            ..Default::default()
        });
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let denied = client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: shell_argv(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(denied.code, IpcErrorCode::RecipeInvalid);
        assert!(denied.message.contains("shell interpreter"));
        assert!(denied.message.contains("[policy.caps] allow_shell = true"));

        let upsert = client
            .call(
                2,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe(RecipeStatus::Active),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeUpsert(created) = upsert else {
            panic!("upsert: {upsert:?}");
        };
        assert_eq!(created.version, 1);

        let got = client
            .call(
                3,
                IpcRequest::RecipeGet(RecipeGetParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeGet(body) = got else {
            panic!("get: {got:?}");
        };
        assert_eq!(body.definition.argv[0], "git");
        assert_eq!(serde_json::to_value(&body).unwrap()["tombstoned"], false);

        let found = client
            .call(
                4,
                IpcRequest::RecipeSearch(RecipeSearchParams {
                    query: "status".to_owned(),
                    limit: Some(10),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeSearch(hits) = found else {
            panic!("search: {found:?}");
        };
        assert_eq!(hits.hits.len(), 1);

        let scope = ActivationScope::Global;
        let activated = client
            .call(
                5,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeActivate(act) = activated else {
            panic!("activate: {activated:?}");
        };
        assert!(!act.was_already_active);

        let listed = client
            .call(
                6,
                IpcRequest::RecipeListActive(terminal_commanderd::ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert_eq!(active.entries.len(), 1);
        assert_eq!(active.entries[0].recipe_id, "git.status");

        let closed = client
            .call(
                7,
                IpcRequest::RecipeDeactivate(RecipeDeactivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: Some(1),
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeDeactivate(de) = closed else {
            panic!("deactivate: {closed:?}");
        };
        assert!(de.was_deactivated);

        let versions = client
            .call(
                8,
                IpcRequest::RecipeListVersions(RecipeListVersionsParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListVersions(listed_versions) = versions else {
            panic!("versions: {versions:?}");
        };
        assert_eq!(listed_versions.versions.len(), 1);

        let tombstoned = client
            .call(
                9,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap();
        assert!(matches!(tombstoned, IpcResponse::RecipeTombstone(_)));

        // FCR2-012: recipe_get marks a retired id instead of looking active.
        let got = client
            .call(
                10,
                IpcRequest::RecipeGet(RecipeGetParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeGet(body) = got else {
            panic!("get after tombstone: {got:?}");
        };
        let wire = serde_json::to_value(&body).unwrap();
        assert_eq!(wire["tombstoned"], true, "{wire}");

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // one import/activate lifecycle, same shape as the test above
fn recipe_seed_import_stays_tested_until_operator_activates() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("seeds");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.recipe_admin_test_seam = true;
        cfg.policy.caps = Some(terminal_commanderd::PolicyCapsSection {
            allow_shell: Some(false),
            ..Default::default()
        });
        assert!(!cfg.policy.llm_can_activate_recipes);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        assert!(!state.policy.llm_can_activate_recipes());
        assert!(!state.policy.caps_allow_shell());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let denied = client
            .call(
                1,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: true,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(denied.code, IpcErrorCode::PolicyDenied);
        assert!(denied.message.contains("recipe_activate_requires_admin"));
        assert!(
            denied.message.contains("recipes activate"),
            "deny text must name the operator verb: {}",
            denied.message
        );

        let imported = client
            .call(
                2,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: false,
                    scope: None,
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(report) = imported else {
            panic!("import: {imported:?}");
        };
        assert!(report.imported.len() >= 8, "{:?}", report.imported);
        assert_eq!(report.imported.len(), 8);
        assert!(report.activated.is_empty());
        assert!(report.failed.is_empty());
        assert!(!report.imported.iter().any(|id| id == "rg.files"));

        let missing_scope = client
            .call(
                3,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: None,
                    from_mcp: false,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(missing_scope.code, IpcErrorCode::ScopeInvalid);

        for id in &report.imported {
            let got = client
                .call(
                    4,
                    IpcRequest::RecipeGet(RecipeGetParams {
                        recipe_id: id.clone(),
                        version: None,
                    }),
                )
                .await
                .unwrap();
            let IpcResponse::RecipeGet(body) = got else {
                panic!("get {id}: {got:?}");
            };
            assert_eq!(body.definition.status, RecipeStatus::Tested);
            assert!(!body.definition.argv.is_empty());
            assert!(
                shell_interpreter_denied(&body.definition.argv[0]).is_none(),
                "{id}"
            );
        }

        let activated = client
            .call(
                5,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(live) = activated else {
            panic!("activate import: {activated:?}");
        };
        assert_eq!(live.imported.len(), 8);
        assert_eq!(live.activated.len(), 8);
        assert!(live.failed.is_empty());

        let listed = client
            .call(
                6,
                IpcRequest::RecipeListActive(ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert_eq!(active.entries.len(), 8);

        let again = client
            .call(
                7,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(second) = again else {
            panic!("reimport: {again:?}");
        };
        assert!(second.imported.is_empty());
        assert_eq!(second.skipped.len(), 8);
        assert_eq!(second.activated.len(), 8);

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // one daemon: tombstone, omitted version, dead scope
fn recipe_tombstone_deactivate_and_dead_scope_are_not_runnable() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("life-close");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        let scope = ActivationScope::Global;

        client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe(RecipeStatus::Active),
                }),
            )
            .await
            .unwrap();
        client
            .call(
                2,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: Some(1),
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let mut v2 = recipe(RecipeStatus::Active);
        v2.summary = "Newer stored body".to_owned();
        client
            .call(
                3,
                IpcRequest::RecipeUpsert(RecipeUpsertParams { definition: v2 }),
            )
            .await
            .unwrap();

        let closed = client
            .call(
                4,
                IpcRequest::RecipeDeactivate(RecipeDeactivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeDeactivate(de) = closed else {
            panic!("deactivate: {closed:?}");
        };
        assert_eq!(de.version, 1, "omitted version must close the active row");

        client
            .call(
                5,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: Some(1),
                    scope: Some(scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        client
            .call(
                6,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap();
        let listed = client
            .call(
                7,
                IpcRequest::RecipeListActive(ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert!(
            active
                .entries
                .iter()
                .all(|entry| entry.recipe_id != "git.status"),
            "tombstoned recipe must not stay listed: {active:?}"
        );
        let found = client
            .call(
                8,
                IpcRequest::RecipeSearch(RecipeSearchParams {
                    query: "status".to_owned(),
                    limit: Some(10),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeSearch(hits) = found else {
            panic!("search: {found:?}");
        };
        assert!(hits.hits.is_empty());
        let ran = client
            .call(
                9,
                IpcRequest::RecipeRun(RecipeRunParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                    scope: Some(scope),
                    fills: std::collections::BTreeMap::new(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(ran.code, IpcErrorCode::RecipeNotActive);

        let mut echo = recipe(RecipeStatus::Active);
        echo.recipe_id = "echo.true".to_owned();
        echo.argv = vec!["true".to_owned()];
        echo.tags.clear();
        client
            .call(
                10,
                IpcRequest::RecipeUpsert(RecipeUpsertParams { definition: echo }),
            )
            .await
            .unwrap();
        let job_scope = ActivationScope::Job {
            job_id: JobId::new(),
        };
        let refused = client
            .call(
                11,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "echo.true".to_owned(),
                    version: Some(1),
                    scope: Some(job_scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(refused.code, IpcErrorCode::ScopeInvalid);
        assert!(
            refused.message.contains("global-only"),
            "{}",
            refused.message
        );

        assert!(
            state
                .store
                .record_recipe_activation_scoped("echo.true", 1, job_scope, None, Some("test"))
                .unwrap()
        );
        let hidden = client
            .call(
                12,
                IpcRequest::RecipeListActive(ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(hidden_active) = hidden else {
            panic!("list: {hidden:?}");
        };
        assert!(
            hidden_active
                .entries
                .iter()
                .all(|entry| entry.recipe_id != "echo.true")
        );
        let dead_run = client
            .call(
                13,
                IpcRequest::RecipeRun(RecipeRunParams {
                    recipe_id: "echo.true".to_owned(),
                    version: None,
                    scope: Some(job_scope),
                    fills: std::collections::BTreeMap::new(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(dead_run.code, IpcErrorCode::RecipeNotActive);
        let cleaned = client
            .call(
                14,
                IpcRequest::RecipeDeactivate(RecipeDeactivateParams {
                    recipe_id: "echo.true".to_owned(),
                    version: Some(1),
                    scope: Some(job_scope),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        assert!(matches!(cleaned, IpcResponse::RecipeDeactivate(_)));
        assert!(
            state
                .store
                .list_active_recipes()
                .unwrap()
                .iter()
                .all(|row| row.definition.recipe_id != "echo.true")
        );

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // seed tombstone skip plus single-version reimport
fn recipe_seed_import_skips_tombstone_and_activates_imported_version() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("seed-life");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        client
            .call(
                1,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: false,
                    scope: None,
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        client
            .call(
                2,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.log".to_owned(),
                }),
            )
            .await
            .unwrap();
        let imported = client
            .call(
                3,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(report) = imported else {
            panic!("import: {imported:?}");
        };
        assert_eq!(report.tombstoned, vec!["git.log".to_owned()]);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(!report.activated.iter().any(|id| id == "git.log"));
        assert_eq!(report.activated.len(), 7);
        assert!(!report.imported.iter().any(|id| id == "git.log"));

        let retry = client
            .call(
                4,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(again) = retry else {
            panic!("retry: {retry:?}");
        };
        assert!(again.imported.is_empty());
        assert_eq!(again.tombstoned, vec!["git.log".to_owned()]);
        assert!(again.failed.is_empty(), "{:?}", again.failed);
        assert!(!again.activated.iter().any(|id| id == "git.log"));

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });

    runtime.block_on(async {
        let data = tmp_data_dir("seed-version");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        let mut custom = recipe(RecipeStatus::Active);
        custom.argv = vec![
            "git".to_owned(),
            "status".to_owned(),
            "--porcelain=v2".to_owned(),
        ];
        client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams { definition: custom }),
            )
            .await
            .unwrap();
        client
            .call(
                2,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: Some(1),
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let imported = client
            .call(
                3,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        // FCR2-007: the customized v1 the activation closed is reported.
        let IpcResponse::RecipeImportSeeds(report) = imported else {
            panic!("import: {imported:?}");
        };
        assert_eq!(
            report.superseded,
            vec![terminal_commander_ipc::protocol::RecipeImportSuperseded {
                recipe_id: "git.status".to_owned(),
                closed_version: 1,
            }]
        );
        let listed = client
            .call(
                4,
                IpcRequest::RecipeListActive(ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        let status: Vec<_> = active
            .entries
            .iter()
            .filter(|entry| entry.recipe_id == "git.status")
            .collect();
        assert_eq!(status.len(), 1, "one open version per scope: {active:?}");
        assert_eq!(status[0].version, 2);
        let got = client
            .call(
                5,
                IpcRequest::RecipeGet(RecipeGetParams {
                    recipe_id: "git.status".to_owned(),
                    version: Some(2),
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeGet(body) = got else {
            panic!("get: {got:?}");
        };
        assert_eq!(
            body.definition.argv,
            ["git".to_owned(), "status".to_owned(), "--short".to_owned()]
        );

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // one daemon denied, one daemon allowed via the test seam
fn recipe_tombstone_requires_admin_peer() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let denied_dir = tmp_data_dir("tomb-deny");
        let denied_cfg = DaemonConfig::defaults_in(&denied_dir);
        assert!(!denied_cfg.recipe_admin_test_seam);
        assert!(!denied_cfg.policy.llm_can_activate_recipes);
        let denied_state = Arc::new(DaemonState::bootstrap(denied_cfg).unwrap());
        let denied_handle =
            IpcServer::new(Arc::clone(&denied_state), denied_state.config.socket_path())
                .spawn()
                .unwrap();
        let denied_client = DaemonClient::new(denied_handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        denied_client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe(RecipeStatus::Active),
                }),
            )
            .await
            .unwrap();
        let denied = denied_client
            .call(
                2,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(denied.code, IpcErrorCode::PolicyDenied);
        assert!(
            denied.message.contains("recipe_activate_requires_admin"),
            "{}",
            denied.message
        );
        let still_there = denied_client
            .call(
                3,
                IpcRequest::RecipeGet(RecipeGetParams {
                    recipe_id: "git.status".to_owned(),
                    version: None,
                }),
            )
            .await
            .unwrap();
        assert!(matches!(still_there, IpcResponse::RecipeGet(_)));
        denied_handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&denied_dir);

        let allowed_dir = tmp_data_dir("tomb-allow");
        let mut allowed_cfg = DaemonConfig::defaults_in(&allowed_dir);
        allowed_cfg.recipe_admin_test_seam = true;
        let allowed_state = Arc::new(DaemonState::bootstrap(allowed_cfg).unwrap());
        let allowed_handle = IpcServer::new(
            Arc::clone(&allowed_state),
            allowed_state.config.socket_path(),
        )
        .spawn()
        .unwrap();
        let allowed_client = DaemonClient::new(allowed_handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        allowed_client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe(RecipeStatus::Active),
                }),
            )
            .await
            .unwrap();
        let tombstoned = allowed_client
            .call(
                2,
                IpcRequest::RecipeTombstone(RecipeTombstoneParams {
                    recipe_id: "git.status".to_owned(),
                }),
            )
            .await
            .unwrap();
        assert!(matches!(tombstoned, IpcResponse::RecipeTombstone(_)));
        let blocked = allowed_client
            .call(
                3,
                IpcRequest::RecipeUpsert(RecipeUpsertParams {
                    definition: recipe(RecipeStatus::Active),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(blocked.code, IpcErrorCode::RecipeInvalid);
        assert!(
            blocked.message.contains("tombstoned"),
            "{}",
            blocked.message
        );
        allowed_handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&allowed_dir);
    });
}

/// FCR2-007: the store predicts supersession before activating. When the
/// activation of that seed then fails, its open customized version was not
/// closed, so `superseded` must not report it.
#[test]
fn import_superseded_omits_a_seed_whose_activation_failed() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("superseded-failed");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.recipe_admin_test_seam = true;
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
            .spawn()
            .unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        let mut custom = recipe(RecipeStatus::Active);
        custom.argv = vec![
            "git".to_owned(),
            "status".to_owned(),
            "--porcelain=v2".to_owned(),
        ];
        client
            .call(
                1,
                IpcRequest::RecipeUpsert(RecipeUpsertParams { definition: custom }),
            )
            .await
            .unwrap();
        client
            .call(
                2,
                IpcRequest::RecipeActivate(RecipeActivateParams {
                    recipe_id: "git.status".to_owned(),
                    version: Some(1),
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        // Fault injection: refuse every new git.status activation row.
        rusqlite::Connection::open(state.config.db_path())
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER test_refuse_git_status BEFORE INSERT ON recipe_activations \
                 WHEN NEW.recipe_id = 'git.status' \
                 BEGIN SELECT RAISE(ABORT, 'test: activation refused'); END;",
            )
            .unwrap();

        let imported = client
            .call(
                3,
                IpcRequest::RecipeImportSeeds(RecipeImportSeedsParams {
                    activate: true,
                    scope: Some(ActivationScope::Global),
                    from_mcp: false,
                }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeImportSeeds(report) = imported else {
            panic!("import: {imported:?}");
        };
        assert!(
            report.failed.iter().any(|f| f.recipe_id == "git.status"),
            "{:?}",
            report.failed
        );
        assert!(!report.activated.iter().any(|id| id == "git.status"));
        assert!(
            !report
                .superseded
                .iter()
                .any(|row| row.recipe_id == "git.status"),
            "a failed activation closed nothing: {:?}",
            report.superseded
        );
        let listed = client
            .call(
                4,
                IpcRequest::RecipeListActive(ListLimitParams { limit: None }),
            )
            .await
            .unwrap();
        let IpcResponse::RecipeListActive(active) = listed else {
            panic!("list: {listed:?}");
        };
        assert!(
            active
                .entries
                .iter()
                .any(|entry| entry.recipe_id == "git.status" && entry.version == 1),
            "the customized v1 stays open: {active:?}"
        );

        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}
