// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC handlers for the recipe store, dry-run, and argv-lane `recipe_run`.

use std::sync::Arc;

use super::common::validate_scope_against_live_jobs;
use crate::ipc::protocol::{
    CommandStartParams, IpcError, IpcErrorCode, IpcResponse, MAX_LIST_LIMIT,
    MAX_RECIPE_SEARCH_LIMIT, RecipeActivateParams, RecipeActivateResponse, RecipeActiveEntry,
    RecipeDeactivateParams, RecipeDeactivateResponse, RecipeGetParams, RecipeGetResponse,
    RecipeImportFailure, RecipeImportSeedsParams, RecipeImportSeedsResponse,
    RecipeListActiveResponse, RecipeListVersionsParams, RecipeListVersionsResponse,
    RecipeRunParams, RecipeRunResponse, RecipeSearchHit, RecipeSearchParams, RecipeSearchResponse,
    RecipeTestParams, RecipeTestResponse, RecipeTombstoneParams, RecipeTombstoneResponse,
    RecipeUpsertParams, RecipeUpsertResponse, RecipeVersionEntry,
};
use crate::state::DaemonState;
use terminal_commander_core::{ActivationScope, RecipeDefinition, shell_argv_denied};
use terminal_commander_store::EventStoreError;
use terminal_commander_supervisor::identity::PeerIdentity;
use time::format_description::well_known::Rfc3339;

fn map_recipe_store_error(e: EventStoreError) -> IpcError {
    match e {
        EventStoreError::InvalidPayload(msg) => IpcError::new(IpcErrorCode::RecipeInvalid, msg),
        EventStoreError::Unavailable(msg) => IpcError::new(IpcErrorCode::Internal, msg),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

fn require_scope(
    scope: Option<terminal_commander_core::ActivationScope>,
) -> Result<terminal_commander_core::ActivationScope, IpcError> {
    scope.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required; pass {kind:'global'} for explicit global scope",
        )
    })
}

fn lookup_recipe(
    state: &Arc<DaemonState>,
    recipe_id: &str,
    version: Option<u32>,
) -> Result<RecipeDefinition, IpcError> {
    let opt = match version {
        Some(v) => state
            .store
            .get_recipe_version(recipe_id, v)
            .map_err(map_recipe_store_error)?,
        None => state
            .store
            .get_latest_recipe(recipe_id)
            .map_err(map_recipe_store_error)?,
    };
    opt.ok_or_else(|| {
        let message = version.map_or_else(
            || format!("recipe '{recipe_id}' not found"),
            |v| format!("recipe '{recipe_id}' version {v} not found"),
        );
        IpcError::new(IpcErrorCode::RecipeNotFound, message)
    })
}

pub(in crate::ipc::server) fn handle_recipe_search(
    state: &Arc<DaemonState>,
    params: &RecipeSearchParams,
) -> Result<IpcResponse, IpcError> {
    let limit = params.limit.map(|n| n.min(MAX_RECIPE_SEARCH_LIMIT));
    let hits = state
        .store
        .search_recipes(&params.query, limit)
        .map_err(map_recipe_store_error)?;
    let hits = hits
        .into_iter()
        .map(|h| RecipeSearchHit {
            recipe_id: h.recipe_id,
            version: h.version,
            title: h.title,
            summary: h.summary,
            tags: h.tags,
            status: h.status,
            argv0: h.argv0,
        })
        .collect();
    Ok(IpcResponse::RecipeSearch(RecipeSearchResponse { hits }))
}

pub(in crate::ipc::server) fn handle_recipe_get(
    state: &Arc<DaemonState>,
    params: &RecipeGetParams,
) -> Result<IpcResponse, IpcError> {
    let definition = lookup_recipe(state, &params.recipe_id, params.version)?;
    Ok(IpcResponse::RecipeGet(RecipeGetResponse { definition }))
}

pub(in crate::ipc::server) fn handle_recipe_upsert(
    state: &Arc<DaemonState>,
    params: &RecipeUpsertParams,
) -> Result<IpcResponse, IpcError> {
    params
        .definition
        .validate()
        .map_err(|e| IpcError::new(IpcErrorCode::RecipeInvalid, e.to_string()))?;
    let version = state
        .store
        .create_recipe_version(&params.definition)
        .map_err(map_recipe_store_error)?;
    Ok(IpcResponse::RecipeUpsert(RecipeUpsertResponse {
        recipe_id: params.definition.recipe_id.clone(),
        version,
    }))
}

pub(in crate::ipc::server) fn handle_recipe_import_seeds(
    state: &Arc<DaemonState>,
    params: &RecipeImportSeedsParams,
) -> Result<IpcResponse, IpcError> {
    if params.from_mcp && params.activate {
        deny_mcp_recipe_activate(state, true)?;
    }
    let activate_scope = if params.activate {
        Some(params.scope.ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::ScopeInvalid,
                "scope is required when activate=true; pass {kind:'global'} \
                 for explicit global activation",
            )
        })?)
    } else {
        None
    };
    let import = state
        .store
        .import_recipe_seeds(params.activate)
        .map_err(map_recipe_store_error)?;
    let (activated, failed) = if let Some(scope) = activate_scope {
        // Re-activate skipped ids too, so a second operator import --activate
        // still opens rows after the definitions were already stored.
        let mut ids = import.imported.clone();
        ids.extend(import.skipped.iter().cloned());
        activate_imported_recipes(state, &ids, scope, params.from_mcp)
    } else {
        (Vec::new(), Vec::new())
    };
    Ok(IpcResponse::RecipeImportSeeds(RecipeImportSeedsResponse {
        imported: import.imported,
        skipped: import.skipped,
        activated,
        failed,
    }))
}

fn activate_imported_recipes(
    state: &Arc<DaemonState>,
    recipe_ids: &[String],
    scope: terminal_commander_core::ActivationScope,
    from_mcp: bool,
) -> (Vec<String>, Vec<RecipeImportFailure>) {
    let mut activated = Vec::new();
    let mut failed = Vec::new();
    for recipe_id in recipe_ids {
        let params = RecipeActivateParams {
            recipe_id: recipe_id.clone(),
            version: None,
            scope: Some(scope),
            from_mcp,
        };
        match handle_recipe_activate(state, &params) {
            Ok(_) => activated.push(recipe_id.clone()),
            Err(err) => failed.push(RecipeImportFailure {
                recipe_id: recipe_id.clone(),
                reason: err.message,
            }),
        }
    }
    (activated, failed)
}

fn deny_mcp_recipe_activate(state: &DaemonState, from_mcp: bool) -> Result<(), IpcError> {
    if from_mcp && !state.policy.llm_can_activate_recipes() {
        return Err(IpcError::new(
            IpcErrorCode::PolicyDenied,
            "recipe_activate_requires_admin: llm_can_activate_recipes is false, so MCP \
             recipe_activate and recipe_deactivate are denied. An operator activates from \
             the admin CLI. recipe_run is allowed once a recipe is activated.",
        ));
    }
    Ok(())
}

fn map_recipe_argv_error(err: &terminal_commander_core::RecipeError) -> IpcError {
    let message = err.to_string();
    let code = if message.contains("shell interpreter") {
        IpcErrorCode::ShellInterpreterDenied
    } else {
        IpcErrorCode::RecipeInvalid
    };
    IpcError::new(code, message)
}

pub(in crate::ipc::server) fn handle_recipe_activate(
    state: &Arc<DaemonState>,
    params: &RecipeActivateParams,
) -> Result<IpcResponse, IpcError> {
    deny_mcp_recipe_activate(state, params.from_mcp)?;
    let scope = require_scope(params.scope)?;
    let def = lookup_recipe(state, &params.recipe_id, params.version)?;
    let version = def.version;
    if !def.status.is_activatable() {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotActive,
            format!(
                "recipe '{}' v{version} has status {}, which cannot be activated; \
                 re-upsert with \"status\":\"active\"",
                def.recipe_id,
                def.status.as_str()
            ),
        ));
    }
    validate_scope_against_live_jobs(state, scope)?;
    let profile = format!("{:?}", state.policy.profile);
    let inserted = state
        .store
        .record_recipe_activation_scoped(
            &def.recipe_id,
            version,
            scope,
            Some(&profile),
            Some("ipc"),
        )
        .map_err(map_recipe_store_error)?;
    Ok(IpcResponse::RecipeActivate(RecipeActivateResponse {
        recipe_id: def.recipe_id,
        version,
        was_already_active: !inserted,
        scope,
    }))
}

pub(in crate::ipc::server) fn handle_recipe_deactivate(
    state: &Arc<DaemonState>,
    params: &RecipeDeactivateParams,
) -> Result<IpcResponse, IpcError> {
    deny_mcp_recipe_activate(state, params.from_mcp)?;
    let scope = require_scope(params.scope)?;
    validate_scope_against_live_jobs(state, scope)?;
    if state
        .store
        .get_recipe_version(&params.recipe_id, params.version)
        .map_err(map_recipe_store_error)?
        .is_none()
    {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotFound,
            format!(
                "recipe '{}' version {} not found",
                params.recipe_id, params.version
            ),
        ));
    }
    let closed = state
        .store
        .deactivate_recipe_scoped(&params.recipe_id, params.version, scope)
        .map_err(map_recipe_store_error)?;
    if !closed {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotActive,
            format!(
                "no active row for recipe '{}' v{} scope {}",
                params.recipe_id,
                params.version,
                scope.kind_label()
            ),
        ));
    }
    Ok(IpcResponse::RecipeDeactivate(RecipeDeactivateResponse {
        recipe_id: params.recipe_id.clone(),
        version: params.version,
        was_deactivated: true,
        scope,
    }))
}

pub(in crate::ipc::server) fn handle_recipe_list_active(
    state: &Arc<DaemonState>,
    params: &crate::ipc::protocol::ListLimitParams,
) -> Result<IpcResponse, IpcError> {
    let limit = params.limit.unwrap_or(MAX_LIST_LIMIT).min(MAX_LIST_LIMIT);
    let all = state
        .store
        .list_active_recipes()
        .map_err(map_recipe_store_error)?;
    let truncated = all.len() > limit;
    let entries = all
        .into_iter()
        .take(limit)
        .map(|row| RecipeActiveEntry {
            recipe_id: row.definition.recipe_id,
            version: row.definition.version,
            title: row.definition.title,
            scope: row.scope,
        })
        .collect();
    Ok(IpcResponse::RecipeListActive(RecipeListActiveResponse {
        entries,
        truncated,
    }))
}

pub(in crate::ipc::server) fn handle_recipe_list_versions(
    state: &Arc<DaemonState>,
    params: &RecipeListVersionsParams,
) -> Result<IpcResponse, IpcError> {
    if state
        .store
        .get_latest_recipe(&params.recipe_id)
        .map_err(map_recipe_store_error)?
        .is_none()
    {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotFound,
            format!("recipe '{}' not found", params.recipe_id),
        ));
    }
    let versions = state
        .store
        .list_recipe_versions(&params.recipe_id)
        .map_err(map_recipe_store_error)?;
    let mut versions_out = Vec::with_capacity(versions.len());
    for row in versions {
        let created_at = row
            .created_at
            .format(&Rfc3339)
            .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("recipe timestamp: {e}")))?;
        versions_out.push(RecipeVersionEntry {
            version: row.version,
            created_at,
        });
    }
    Ok(IpcResponse::RecipeListVersions(
        RecipeListVersionsResponse {
            recipe_id: params.recipe_id.clone(),
            versions: versions_out,
        },
    ))
}

pub(in crate::ipc::server) fn handle_recipe_tombstone(
    state: &Arc<DaemonState>,
    params: &RecipeTombstoneParams,
) -> Result<IpcResponse, IpcError> {
    let found = state
        .store
        .tombstone_recipe(&params.recipe_id)
        .map_err(map_recipe_store_error)?;
    if !found {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotFound,
            format!("recipe '{}' not found", params.recipe_id),
        ));
    }
    Ok(IpcResponse::RecipeTombstone(RecipeTombstoneResponse {
        recipe_id: params.recipe_id.clone(),
    }))
}

pub(in crate::ipc::server) fn handle_recipe_test(
    state: &Arc<DaemonState>,
    params: &RecipeTestParams,
) -> Result<IpcResponse, IpcError> {
    let definition = match (&params.definition, params.recipe_id.as_deref()) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(IpcError::new(
                IpcErrorCode::RecipeInvalid,
                "recipe_test requires exactly one of definition or recipe_id",
            ));
        }
        (Some(definition), None) => definition.clone(),
        (None, Some(recipe_id)) => lookup_recipe(state, recipe_id, params.version)?,
    };
    definition
        .validate()
        .map_err(|err| map_recipe_argv_error(&err))?;
    let argv = definition
        .resolve_argv(&params.fills)
        .map_err(|err| map_recipe_argv_error(&err))?;
    let (expects_met, notes) = params.expect_argv0.as_deref().map_or_else(
        || (true, Vec::new()),
        |expected| {
            let met = argv.first().map(String::as_str) == Some(expected);
            let notes = if met {
                Vec::new()
            } else {
                vec![format!(
                    "expect_argv0 {expected:?} did not match resolved argv0 {:?}",
                    argv.first()
                )]
            };
            (met, notes)
        },
    );
    Ok(IpcResponse::RecipeTest(RecipeTestResponse {
        ok: expects_met,
        activated: false,
        argv,
        expects_met,
        notes,
    }))
}

fn resolve_activated(
    state: &Arc<DaemonState>,
    recipe_id: &str,
    version: Option<u32>,
    scope: ActivationScope,
) -> Result<RecipeDefinition, IpcError> {
    if let Some(version) = version {
        let _ = lookup_recipe(state, recipe_id, Some(version))?;
    }
    let active = state
        .store
        .list_active_recipes()
        .map_err(map_recipe_store_error)?;
    let mut matches: Vec<RecipeDefinition> = active
        .into_iter()
        .filter(|row| {
            row.definition.recipe_id == recipe_id
                && row.scope == scope
                && version.is_none_or(|v| row.definition.version == v)
        })
        .map(|row| row.definition)
        .collect();
    matches.sort_by_key(|definition| definition.version);
    matches.pop().ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::RecipeNotActive,
            format!(
                "recipe '{recipe_id}' is not activated for scope {}; call recipe_list_active \
                 or activate it from the admin CLI",
                scope.kind_label()
            ),
        )
    })
}

pub(in crate::ipc::server) fn handle_recipe_run(
    state: &Arc<DaemonState>,
    params: &RecipeRunParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    let scope = require_scope(params.scope)?;
    let definition = resolve_activated(state, &params.recipe_id, params.version, scope)?;
    let argv = definition
        .resolve_argv(&params.fills)
        .map_err(|err| map_recipe_argv_error(&err))?;
    // Same predicate as `resolve_argv` (which already re-validates). This is
    // the run re-check: a stored argv cannot reach `command_start` if resolve
    // ever stops applying the deny.
    if let Some(shell) = shell_argv_denied(&argv) {
        return Err(IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!("shell interpreter '{shell}' is denied on recipe_run"),
        ));
    }
    let watched = definition.prefers_watch();
    let wait_ms = definition.watch_budget_ms().unwrap_or(0);
    // ponytail: env_allowlist stores names only. The child inherits the daemon
    // environment; a value-injecting allowlist can replace the empty env later.
    // ponytail: rule_pack_ids select the watch path only. They do not import packs.
    let start = CommandStartParams {
        environment: None,
        argv: argv.clone(),
        cwd: definition.cwd.as_ref().map(std::path::PathBuf::from),
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace_ms: None,
        tag: None,
        dedup_nonce: None,
        strip_ansi: true,
    };
    let started = match super::command::handle_command_start_combed(state, &start, peer)? {
        IpcResponse::CommandStartCombed(body) => body,
        other => {
            return Err(IpcError::new(
                IpcErrorCode::Internal,
                format!("recipe_run expected command_start_combed, got {other:?}"),
            ));
        }
    };
    Ok(IpcResponse::RecipeRun(RecipeRunResponse {
        recipe_id: definition.recipe_id,
        version: definition.version,
        argv,
        lane: "argv".to_owned(),
        watched,
        wait_ms,
        job_id: started.job_id,
        bucket_id: started.bucket_id,
        probe_id: started.probe_id,
        cursor: started.cursor,
    }))
}
