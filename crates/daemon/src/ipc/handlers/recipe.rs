// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC handlers for the recipe store. CRUD and scoped activation only.
//! No MCP catalogue and no `recipe_run`.

use std::sync::Arc;

use super::common::validate_scope_against_live_jobs;
use crate::ipc::protocol::{
    IpcError, IpcErrorCode, IpcResponse, MAX_LIST_LIMIT, MAX_RECIPE_SEARCH_LIMIT,
    RecipeActivateParams, RecipeActivateResponse, RecipeActiveEntry, RecipeDeactivateParams,
    RecipeDeactivateResponse, RecipeGetParams, RecipeGetResponse, RecipeListActiveResponse,
    RecipeListVersionsParams, RecipeListVersionsResponse, RecipeSearchHit, RecipeSearchParams,
    RecipeSearchResponse, RecipeTombstoneParams, RecipeTombstoneResponse, RecipeUpsertParams,
    RecipeUpsertResponse, RecipeVersionEntry,
};
use crate::state::DaemonState;
use terminal_commander_core::RecipeDefinition;
use terminal_commander_store::EventStoreError;
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

pub(in crate::ipc::server) fn handle_recipe_activate(
    state: &Arc<DaemonState>,
    params: &RecipeActivateParams,
) -> Result<IpcResponse, IpcError> {
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
