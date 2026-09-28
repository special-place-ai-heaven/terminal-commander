// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! IPC handlers for the recipe store, dry-run, and argv-lane `recipe_run`.

use std::sync::Arc;

use super::common::recipe_scope_runnable;
use crate::ipc::protocol::{
    CommandStartParams, IpcError, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, MAX_LIST_LIMIT,
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
use terminal_commander_store::{EventStoreError, RecipeSeedRow};
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
    let tombstoned = state
        .store
        .is_recipe_tombstoned(&params.recipe_id)
        .map_err(map_recipe_store_error)?;
    Ok(IpcResponse::RecipeGet(RecipeGetResponse {
        definition,
        tombstoned,
    }))
}

pub(in crate::ipc::server) fn handle_recipe_upsert(
    state: &Arc<DaemonState>,
    params: &RecipeUpsertParams,
) -> Result<IpcResponse, IpcError> {
    params
        .definition
        .validate(state.policy.caps_allow_shell())
        .map_err(|err| map_recipe_argv_error(&state.policy, &err))?;
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
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    if params.activate {
        deny_mcp_recipe_activate(state, peer, params.from_mcp)?;
    }
    let activate_scope = if params.activate {
        let scope = params.scope.ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::ScopeInvalid,
                "scope is required when activate=true; pass {kind:'global'} \
                 for explicit global activation",
            )
        })?;
        require_global_recipe_scope(scope)?;
        Some(scope)
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
        // Activate the version import just stored or recognized, not a
        // later "latest" lookup.
        let mut rows = import.imported.clone();
        rows.extend(import.skipped.iter().cloned());
        activate_imported_recipes(state, peer, &rows, scope, params.from_mcp)
    } else {
        (Vec::new(), Vec::new())
    };
    // The store predicts supersession before activating; a failed activation
    // closed nothing, so report only ids whose activation landed.
    let superseded = import
        .superseded
        .into_iter()
        .filter(|row| activated.contains(&row.recipe_id))
        .map(|row| crate::ipc::protocol::RecipeImportSuperseded {
            recipe_id: row.recipe_id,
            closed_version: row.closed_version,
        })
        .collect();
    Ok(IpcResponse::RecipeImportSeeds(RecipeImportSeedsResponse {
        imported: seed_ids(&import.imported),
        skipped: seed_ids(&import.skipped),
        activated,
        tombstoned: import.tombstoned,
        failed,
        superseded,
    }))
}

fn seed_ids(rows: &[RecipeSeedRow]) -> Vec<String> {
    rows.iter().map(|row| row.recipe_id.clone()).collect()
}

fn activate_imported_recipes(
    state: &Arc<DaemonState>,
    peer: &PeerIdentity,
    rows: &[RecipeSeedRow],
    scope: terminal_commander_core::ActivationScope,
    from_mcp: bool,
) -> (Vec<String>, Vec<RecipeImportFailure>) {
    let mut activated = Vec::new();
    let mut failed = Vec::new();
    for row in rows {
        let params = RecipeActivateParams {
            recipe_id: row.recipe_id.clone(),
            version: Some(row.version),
            scope: Some(scope),
            from_mcp,
        };
        // The import handler already ran the admin gate once for this request.
        match activate_recipe(state, &params, peer) {
            Ok(_) => activated.push(row.recipe_id.clone()),
            Err(err) => failed.push(RecipeImportFailure {
                recipe_id: row.recipe_id.clone(),
                reason: err.message,
            }),
        }
    }
    (activated, failed)
}

// ponytail: no job-exit hook. Leftover non-global rows stay in the table
// but list/run/steer ignore a scope that is not live, and deactivate does
// not require the job to still be live. Add a job-exit closer if scoped
// recipes become a real binding.
/// Recipe activations are global. Job, bucket, and probe scopes are
/// refused: rules do not drop those rows when the job exits, and a
/// recipe run is a new command, so a sticky scope would stay runnable
/// with no revoke path.
fn require_global_recipe_scope(scope: ActivationScope) -> Result<(), IpcError> {
    if scope == ActivationScope::Global {
        return Ok(());
    }
    Err(IpcError::new(
        IpcErrorCode::ScopeInvalid,
        format!(
            "recipe activation scope must be global-only (got {}); job, bucket, and probe \
             scopes are refused so an activation cannot stay runnable after that job exits",
            scope.kind_label()
        ),
    ))
}

fn deny_mcp_recipe_activate(
    state: &DaemonState,
    peer: &PeerIdentity,
    from_mcp: bool,
) -> Result<(), IpcError> {
    if state.policy.llm_can_activate_recipes() {
        return Ok(());
    }
    if !caller_may_recipe_admin(state, peer, from_mcp) {
        if peer_image_name(peer).is_none() {
            return Err(IpcError::new(
                IpcErrorCode::PolicyDenied,
                format!(
                    "recipe_activate_requires_admin: llm_can_activate_recipes is false and the \
                 daemon could not resolve the calling program's executable on this \
                 platform, so it cannot recognize the admin CLI (`terminal-commander`) and \
                 denies recipe_activate, recipe_deactivate, and recipe_tombstone. Setting \
                 `[policy] llm_can_activate_recipes = true` in the daemon config opens \
                 recipe admin to the model. recipe_run is allowed once a recipe is \
                 activated. {}",
                    crate::policy::profile_change_hint(state.policy.profile)
                ),
            ));
        }
        return Err(IpcError::new(
            IpcErrorCode::PolicyDenied,
            format!(
                "recipe_activate_requires_admin: llm_can_activate_recipes is false, so only \
                 the admin CLI peer (`terminal-commander`) may recipe_activate, \
                 recipe_deactivate, or recipe_tombstone. Omitting from_mcp does not grant \
                 admin, and the MCP adapter image cannot claim it. The admin CLI runs \
                 `terminal-commander recipes activate`, `terminal-commander recipes \
                 deactivate`, or `terminal-commander recipes tombstone`. recipe_run is \
                 allowed once a recipe is activated. Same-user code can still exec that CLI; \
                 the socket is not a privilege boundary. {}",
                crate::policy::profile_change_hint(state.policy.profile)
            ),
        ));
    }
    if peer_started_by_daemon(peer) {
        return Err(IpcError::new(
            IpcErrorCode::PolicyDenied,
            format!(
                "recipe_activate_requires_admin: recipe admin is refused to processes started \
             by the daemon; run the CLI from your own terminal (`terminal-commander \
             recipes activate|deactivate|tombstone`), or set `[policy] \
             llm_can_activate_recipes = true` in the daemon config to let the model \
             activate recipes. recipe_run is allowed once a recipe is activated. {}",
                crate::policy::profile_change_hint(state.policy.profile)
            ),
        ));
    }
    Ok(())
}

/// Who the peer executable is, for the recipe admin gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgramRole {
    AdminCli,
    McpAdapter,
    Unknown,
}

fn program_role_from_name(name: &str) -> ProgramRole {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let lower = base.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    match stem {
        "terminal-commander-mcp" => ProgramRole::McpAdapter,
        "terminal-commander" => ProgramRole::AdminCli,
        _ => ProgramRole::Unknown,
    }
}

#[cfg(target_os = "linux")]
fn unix_image_name(pid: i32) -> Option<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    path.file_name().map(|s| s.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn unix_image_name(pid: i32) -> Option<String> {
    let path = crate::ipc::peer::image_path(pid)?;
    path.file_name().map(|s| s.to_string_lossy().into_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const fn unix_image_name(_pid: i32) -> Option<String> {
    None
}

fn peer_image_name(peer: &PeerIdentity) -> Option<String> {
    match peer {
        PeerIdentity::Windows { image, .. } => image
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned()),
        PeerIdentity::Unix { pid: Some(pid), .. } => unix_image_name(*pid),
        PeerIdentity::Unix { pid: None, .. } | PeerIdentity::Unknown { .. } => None,
    }
}

fn peer_program_role(peer: &PeerIdentity) -> ProgramRole {
    peer_image_name(peer)
        .as_deref()
        .map_or(ProgramRole::Unknown, program_role_from_name)
}

/// Whether the peer runs under this daemon's process tree (an MCP
/// `command_start` of the admin CLI). Evaluated only for admin verbs.
fn peer_started_by_daemon(peer: &PeerIdentity) -> bool {
    let pid = match peer {
        PeerIdentity::Unix { pid, .. } => pid.and_then(|pid| u32::try_from(pid).ok()),
        PeerIdentity::Windows { pid, .. } => *pid,
        PeerIdentity::Unknown { .. } => None,
    };
    pid.is_some_and(|pid| crate::ipc::peer::descends_from(pid, std::process::id()))
}

/// Release builds grant recipe admin only to the `terminal-commander` image.
/// The MCP image is never admin. An unknown peer cannot grant itself by
/// setting or omitting `from_mcp`.
///
/// ponytail: `DaemonConfig::recipe_admin_test_seam` lets in-process tests
/// pass an explicit `from_mcp: false` from an unknown image. It is skipped
/// by serde, so a config file cannot turn it on. The MCP image stays denied.
const fn recipe_admin_grant(
    role: ProgramRole,
    from_mcp: bool,
    allow_unknown_explicit: bool,
) -> bool {
    match role {
        ProgramRole::AdminCli => true,
        ProgramRole::McpAdapter => false,
        ProgramRole::Unknown => allow_unknown_explicit && !from_mcp,
    }
}

fn caller_may_recipe_admin(state: &DaemonState, peer: &PeerIdentity, from_mcp: bool) -> bool {
    let seam = state.config.recipe_admin_test_seam && !from_mcp;
    recipe_admin_grant(peer_program_role(peer), from_mcp, seam)
}

/// `credential_provide` gate: the admin CLI peer only, from the owner's own
/// terminal. Same image check as recipe admin (the MCP image is never
/// admin), plus the refusal of processes the daemon started. No policy knob
/// opens it to the model.
pub(in crate::ipc::server) fn caller_is_owner_cli(
    state: &DaemonState,
    peer: &PeerIdentity,
    from_mcp: bool,
) -> bool {
    caller_may_recipe_admin(state, peer, from_mcp) && !peer_started_by_daemon(peer)
}

/// `credential_url` gate: the MCP adapter image the harness launched, never
/// one a TC job started (that one could hand the page URL to the model).
///
/// ponytail: in-process tests run under an unknown image; the
/// `credential_prompter_test_seam` (serde-skipped) admits them.
pub(in crate::ipc::server) fn caller_is_harness_adapter(
    state: &DaemonState,
    peer: &PeerIdentity,
) -> bool {
    let role_ok = match peer_program_role(peer) {
        ProgramRole::McpAdapter => true,
        ProgramRole::AdminCli => false,
        ProgramRole::Unknown => state.config.credential_prompter_test_seam.is_some(),
    };
    role_ok && !peer_started_by_daemon(peer)
}

/// Audit actor for recipe admin verbs and `recipe_run`. Known images keep
/// the FCR-014 actor (`admin` / `mcp`). An unknown peer is never `admin`:
/// it is `mcp` only when it claims `from_mcp: true` (an under-claim), else
/// `unknown`. `recipe_run` and tombstone carry no `from_mcp` (`None`).
fn recipe_actor_label(peer: &PeerIdentity, from_mcp: Option<bool>) -> &'static str {
    match peer_program_role(peer) {
        ProgramRole::AdminCli => "admin",
        ProgramRole::McpAdapter => "mcp",
        ProgramRole::Unknown if from_mcp == Some(true) => "mcp",
        ProgramRole::Unknown => "unknown",
    }
}

fn map_recipe_argv_error(
    policy: &crate::policy::PolicyEngine,
    err: &terminal_commander_core::RecipeError,
) -> IpcError {
    let message = err.to_string();
    if message.contains("shell interpreter") {
        let hint = crate::policy::profile_change_hint(policy.profile);
        return IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!("{message} {hint}"),
        );
    }
    IpcError::new(IpcErrorCode::RecipeInvalid, message)
}

pub(in crate::ipc::server) fn handle_recipe_activate(
    state: &Arc<DaemonState>,
    params: &RecipeActivateParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    deny_mcp_recipe_activate(state, peer, params.from_mcp)?;
    activate_recipe(state, params, peer)
}

/// `recipe_activate` after the admin gate. Import calls this per seed so the
/// gate (image lookup + ancestry walk) runs once per request, not per row.
fn activate_recipe(
    state: &Arc<DaemonState>,
    params: &RecipeActivateParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    let scope = require_scope(params.scope)?;
    require_global_recipe_scope(scope)?;
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
    let profile = format!("{:?}", state.policy.profile);
    let inserted = state
        .store
        .record_recipe_activation_scoped(
            &def.recipe_id,
            version,
            scope,
            Some(&profile),
            Some(recipe_actor_label(peer, Some(params.from_mcp))),
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
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    deny_mcp_recipe_activate(state, peer, params.from_mcp)?;
    let scope = require_scope(params.scope)?;
    // A dead job/bucket/probe scope must still be closable. New
    // activations are global-only; this path is the cleanup for leftovers.
    let versions = match params.version {
        Some(version) => {
            if state
                .store
                .get_recipe_version(&params.recipe_id, version)
                .map_err(map_recipe_store_error)?
                .is_none()
            {
                return Err(IpcError::new(
                    IpcErrorCode::RecipeNotFound,
                    format!("recipe '{}' version {version} not found", params.recipe_id),
                ));
            }
            vec![version]
        }
        None => open_versions_for_scope(state, &params.recipe_id, scope)?,
    };
    let mut closed_version = None;
    for version in versions {
        let closed = state
            .store
            .deactivate_recipe_scoped(&params.recipe_id, version, scope)
            .map_err(map_recipe_store_error)?;
        if closed {
            closed_version = Some(version);
        }
    }
    let Some(version) = closed_version else {
        let message = params.version.map_or_else(
            || {
                format!(
                    "no active row for recipe '{}' scope {}; omitted version resolves the \
                     active version, not the latest stored",
                    params.recipe_id,
                    scope.kind_label()
                )
            },
            |version| {
                format!(
                    "no active row for recipe '{}' v{version} scope {}",
                    params.recipe_id,
                    scope.kind_label()
                )
            },
        );
        return Err(IpcError::new(IpcErrorCode::RecipeNotActive, message));
    };
    Ok(IpcResponse::RecipeDeactivate(RecipeDeactivateResponse {
        recipe_id: params.recipe_id.clone(),
        version,
        was_deactivated: true,
        scope,
    }))
}

/// Open versions for `(recipe_id, scope)`, highest last. Missing id is
/// `RecipeNotFound`. No open row is `RecipeNotActive`.
fn open_versions_for_scope(
    state: &DaemonState,
    recipe_id: &str,
    scope: ActivationScope,
) -> Result<Vec<u32>, IpcError> {
    if state
        .store
        .get_latest_recipe(recipe_id)
        .map_err(map_recipe_store_error)?
        .is_none()
    {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotFound,
            format!("recipe '{recipe_id}' not found"),
        ));
    }
    let active = state
        .store
        .list_active_recipes()
        .map_err(map_recipe_store_error)?;
    let mut versions: Vec<u32> = active
        .into_iter()
        .filter(|row| row.definition.recipe_id == recipe_id && row.scope == scope)
        .map(|row| row.definition.version)
        .collect();
    if versions.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::RecipeNotActive,
            format!(
                "no active row for recipe '{recipe_id}' scope {}; omitted version resolves the \
                 active version, not the latest stored",
                scope.kind_label()
            ),
        ));
    }
    versions.sort_unstable();
    versions.dedup();
    Ok(versions)
}

pub(in crate::ipc::server) fn handle_recipe_list_active(
    state: &Arc<DaemonState>,
    params: &crate::ipc::protocol::ListLimitParams,
) -> Result<IpcResponse, IpcError> {
    let limit = params.limit.unwrap_or(MAX_LIST_LIMIT).min(MAX_LIST_LIMIT);
    let all = runnable_active_recipes(state)?;
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
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    // Tombstone has no `from_mcp` field. `false` is not a caller claim: it
    // selects the admin-CLI grant (image basename `terminal-commander`) and
    // the in-process test seam. The MCP image stays denied.
    deny_mcp_recipe_activate(state, peer, false)?;
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
    let allow_shell = state.policy.caps_allow_shell();
    definition
        .validate(allow_shell)
        .map_err(|err| map_recipe_argv_error(&state.policy, &err))?;
    let argv = definition
        .resolve_argv(&params.fills, allow_shell)
        .map_err(|err| map_recipe_argv_error(&state.policy, &err))?;
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
    if !super::common::recipe_scope_runnable(state, scope) {
        return Err(not_activated(recipe_id, scope));
    }
    state
        .store
        .get_active_recipe(recipe_id, version, scope)
        .map_err(map_recipe_store_error)?
        .ok_or_else(|| not_activated(recipe_id, scope))
}

fn not_activated(recipe_id: &str, scope: ActivationScope) -> IpcError {
    IpcError::new(
        IpcErrorCode::RecipeNotActive,
        format!(
            "recipe '{recipe_id}' is not activated for scope {}; call recipe_list_active \
             or run `terminal-commander recipes activate {recipe_id}`",
            scope.kind_label()
        ),
    )
}

fn runnable_active_recipes(
    state: &DaemonState,
) -> Result<Vec<terminal_commander_store::ActiveRecipe>, IpcError> {
    let active = state
        .store
        .list_active_recipes()
        .map_err(map_recipe_store_error)?;
    Ok(active
        .into_iter()
        .filter(|row| recipe_scope_runnable(state, row.scope))
        .collect())
}

pub(in crate::ipc::server) fn handle_recipe_run(
    state: &Arc<DaemonState>,
    params: &RecipeRunParams,
    peer: &PeerIdentity,
) -> Result<IpcResponse, IpcError> {
    let scope = require_scope(params.scope)?;
    let definition = resolve_activated(state, &params.recipe_id, params.version, scope)?;
    // `allow_shell` is the one switch. Off: an interpreter argv is denied
    // here. On: it goes to `command_start_combed`, which evaluates
    // `CommandShellStart` and tags the audit row `nested_shell`.
    let allow_shell = state.policy.caps_allow_shell();
    let argv = definition
        .resolve_argv(&params.fills, allow_shell)
        .map_err(|err| map_recipe_argv_error(&state.policy, &err))?;
    // Same predicate as `resolve_argv` (which already re-validates). This is
    // the run re-check: a stored argv cannot reach `command_start` if resolve
    // ever stops applying the deny.
    if !allow_shell && let Some(shell) = shell_argv_denied(&argv) {
        return Err(IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "shell interpreter '{shell}' is denied on recipe_run: allow_shell is off. \
                 Use a recipe whose argv runs the program directly (e.g. \
                 [\"cargo\",\"build\"]). {}",
                crate::policy::profile_change_hint(state.policy.profile)
            ),
        ));
    }
    if !definition.env_allowlist.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::RecipeInvalid,
            "env_allowlist is not enforced; omit it. recipe_run inherits the daemon \
             environment and does not filter it",
        ));
    }
    let watched = definition.prefers_watch();
    let wait_ms = definition.watch_budget_ms().unwrap_or(0);
    // ponytail: rule_pack_ids select the watched MCP response only. They do
    // not import packs. Active registry rules still comb the job.
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

/// Audit actor plus recipe identity for activate, deactivate, run, and tombstone.
/// Other methods stay on the generic `ipc` row.
pub(in crate::ipc::server) fn recipe_audit_overlay(
    peer: &PeerIdentity,
    req: &IpcRequest,
    result: &IpcResult,
) -> Option<(&'static str, serde_json::Value)> {
    let (recipe_id, version, scope, from_mcp) = match req {
        IpcRequest::RecipeActivate(p) => {
            let (version, scope) = activated_identity(result).unwrap_or((p.version, p.scope));
            (p.recipe_id.clone(), version, scope, Some(p.from_mcp))
        }
        IpcRequest::RecipeDeactivate(p) => {
            let (version, scope) = activated_identity(result).unwrap_or((p.version, p.scope));
            (p.recipe_id.clone(), version, scope, Some(p.from_mcp))
        }
        IpcRequest::RecipeRun(p) => {
            let version = activated_identity(result).map_or(p.version, |(version, _)| version);
            (p.recipe_id.clone(), version, p.scope, None)
        }
        IpcRequest::RecipeTombstone(p) => (p.recipe_id.clone(), None, None, None),
        _ => return None,
    };
    Some((
        recipe_actor_label(peer, from_mcp),
        serde_json::json!({
            "recipe_id": recipe_id,
            "version": version,
            "scope": scope,
            "from_mcp": from_mcp,
        }),
    ))
}

const fn activated_identity(result: &IpcResult) -> Option<(Option<u32>, Option<ActivationScope>)> {
    match result {
        IpcResult::Ok {
            response: IpcResponse::RecipeActivate(body),
        } => Some((Some(body.version), Some(body.scope))),
        IpcResult::Ok {
            response: IpcResponse::RecipeDeactivate(body),
        } => Some((Some(body.version), Some(body.scope))),
        IpcResult::Ok {
            response: IpcResponse::RecipeRun(body),
        } => Some((Some(body.version), None)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_grant_is_the_cli_image_not_the_from_mcp_bit() {
        assert!(recipe_admin_grant(ProgramRole::AdminCli, true, false));
        assert!(recipe_admin_grant(ProgramRole::AdminCli, false, false));
        assert!(!recipe_admin_grant(ProgramRole::McpAdapter, false, false));
        assert!(!recipe_admin_grant(ProgramRole::McpAdapter, true, true));
        assert!(!recipe_admin_grant(ProgramRole::Unknown, false, false));
        assert!(!recipe_admin_grant(ProgramRole::Unknown, true, true));
        assert!(recipe_admin_grant(ProgramRole::Unknown, false, true));
    }

    #[test]
    fn program_role_names() {
        assert_eq!(
            program_role_from_name("/usr/local/bin/terminal-commander"),
            ProgramRole::AdminCli
        );
        assert_eq!(
            program_role_from_name(r"C:\tc\terminal-commander.exe"),
            ProgramRole::AdminCli
        );
        assert_eq!(
            program_role_from_name("terminal-commander-mcp"),
            ProgramRole::McpAdapter
        );
        assert_eq!(
            program_role_from_name("terminal-commander-mcp.exe"),
            ProgramRole::McpAdapter
        );
        assert_eq!(
            program_role_from_name("terminal-commanderd"),
            ProgramRole::Unknown
        );
        assert_eq!(program_role_from_name("python"), ProgramRole::Unknown);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn this_process_is_not_the_admin_cli() {
        let peer = PeerIdentity::Unix {
            uid: 0,
            gid: 0,
            pid: Some(i32::try_from(std::process::id()).unwrap_or(1)),
        };
        assert_eq!(peer_program_role(&peer), ProgramRole::Unknown);
    }

    fn recipe_run_overlay(peer: &PeerIdentity) -> (&'static str, serde_json::Value) {
        let req = IpcRequest::RecipeRun(RecipeRunParams {
            recipe_id: "git.status".to_owned(),
            version: Some(3),
            scope: Some(ActivationScope::Global),
            fills: std::collections::BTreeMap::new(),
        });
        let result = IpcResult::Err {
            error: IpcError::new(IpcErrorCode::RecipeNotActive, "not active"),
        };
        recipe_audit_overlay(peer, &req, &result).expect("recipe_run overlay")
    }

    #[test]
    fn unknown_peer_recipe_run_is_not_labeled_admin() {
        let (actor, meta) = recipe_run_overlay(&PeerIdentity::unknown());
        assert_eq!(actor, "unknown");
        assert_eq!(meta["recipe_id"], "git.status");
        assert_eq!(meta["version"], 3);
        assert_eq!(meta["scope"]["kind"], "global");
        assert!(meta["from_mcp"].is_null());
    }

    #[test]
    fn known_peer_recipe_run_keeps_image_actor() {
        let admin = PeerIdentity::Windows {
            sid: "S-1-5-21".to_owned(),
            pid: None,
            image: Some(std::path::PathBuf::from("terminal-commander.exe")),
        };
        let mcp = PeerIdentity::Windows {
            sid: "S-1-5-21".to_owned(),
            pid: None,
            image: Some(std::path::PathBuf::from("terminal-commander-mcp.exe")),
        };
        let (admin_actor, admin_meta) = recipe_run_overlay(&admin);
        let (mcp_actor, mcp_meta) = recipe_run_overlay(&mcp);
        assert_eq!(admin_actor, "admin");
        assert_eq!(mcp_actor, "mcp");
        assert_eq!(admin_meta["recipe_id"], "git.status");
        assert_eq!(admin_meta["version"], 3);
        assert_eq!(admin_meta["scope"]["kind"], "global");
        assert!(admin_meta["from_mcp"].is_null());
        assert!(mcp_meta["from_mcp"].is_null());
    }

    #[test]
    fn unknown_peer_activate_and_tombstone_are_not_labeled_admin() {
        let peer = PeerIdentity::unknown();
        let result = IpcResult::Err {
            error: IpcError::new(IpcErrorCode::PolicyDenied, "denied"),
        };
        let activate = IpcRequest::RecipeActivate(RecipeActivateParams {
            recipe_id: "git.status".to_owned(),
            version: None,
            scope: Some(ActivationScope::Global),
            from_mcp: false,
        });
        let tombstone = IpcRequest::RecipeTombstone(RecipeTombstoneParams {
            recipe_id: "git.status".to_owned(),
        });
        for req in [activate, tombstone] {
            let (actor, _) = recipe_audit_overlay(&peer, &req, &result).expect("overlay");
            assert_eq!(actor, "unknown", "{req:?}");
        }
    }

    fn test_state(tag: &str, seam: bool) -> Arc<DaemonState> {
        let mut data = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        data.push(format!(
            "tc-recipe-gate-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let mut cfg = crate::config::DaemonConfig::defaults_in(&data);
        // The admin gate is hardening: the default full_access profile
        // leaves recipe admin open to the model.
        cfg.policy.profile = crate::policy::PolicyProfile::DeveloperLocal;
        cfg.recipe_admin_test_seam = seam;
        Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"))
    }

    #[test]
    fn default_profile_lets_the_model_administer_recipes() {
        let data = tempfile::tempdir().expect("temp data dir");
        let cfg = crate::config::DaemonConfig::defaults_in(data.path());
        let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"));
        deny_mcp_recipe_activate(&state, &PeerIdentity::unknown(), true)
            .expect("full_access default allows MCP recipe admin");
    }

    #[test]
    fn unresolved_peer_image_deny_names_the_config_knob() {
        let state = test_state("unresolved", false);
        let err = deny_mcp_recipe_activate(&state, &PeerIdentity::unknown(), false)
            .expect_err("unresolved peer is denied");
        assert_eq!(err.code, IpcErrorCode::PolicyDenied);
        assert!(err.message.contains("recipe_activate_requires_admin"));
        assert!(err.message.contains("could not resolve"), "{}", err.message);
        assert!(
            err.message.contains("llm_can_activate_recipes = true"),
            "{}",
            err.message
        );

        // A resolved non-CLI image keeps pointing at the operator CLI.
        let python = PeerIdentity::Windows {
            sid: "S-1-5-21".to_owned(),
            pid: None,
            image: Some(std::path::PathBuf::from("python.exe")),
        };
        let err = deny_mcp_recipe_activate(&state, &python, false).expect_err("python is denied");
        assert!(
            !err.message.contains("could not resolve"),
            "{}",
            err.message
        );
        assert!(err.message.contains("terminal-commander recipes"));
    }

    /// A long-lived child of this test process. The in-process "daemon" is
    /// this process, so the child stands in for a daemon-spawned job.
    fn spawn_child() -> std::process::Child {
        #[cfg(windows)]
        let mut cmd = {
            let mut cmd = std::process::Command::new("ping");
            cmd.args(["-n", "30", "127.0.0.1"]);
            cmd
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut cmd = std::process::Command::new("sleep");
            cmd.arg("30");
            cmd
        };
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn child")
    }

    #[cfg(windows)]
    fn admin_peer(pid: u32) -> PeerIdentity {
        PeerIdentity::Windows {
            sid: "S-1-5-21".to_owned(),
            pid: Some(pid),
            image: Some(std::path::PathBuf::from("terminal-commander.exe")),
        }
    }

    #[cfg(not(windows))]
    fn admin_peer(pid: u32) -> PeerIdentity {
        PeerIdentity::Unix {
            uid: 0,
            gid: 0,
            pid: Some(i32::try_from(pid).expect("pid fits i32")),
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn daemon_spawned_peer_is_refused_recipe_admin() {
        let state = test_state("spawned", true);
        // Not a descendant: the caller that is this process itself is granted.
        deny_mcp_recipe_activate(&state, &admin_peer(std::process::id()), false)
            .expect("a peer outside the daemon tree keeps admin");

        let mut child = spawn_child();
        let denied = deny_mcp_recipe_activate(&state, &admin_peer(child.id()), false);
        let _ = child.kill();
        let _ = child.wait();
        let err = denied.expect_err("a daemon-spawned peer is refused recipe admin");
        assert_eq!(err.code, IpcErrorCode::PolicyDenied);
        assert!(
            err.message
                .contains("recipe admin is refused to processes started by the daemon"),
            "{}",
            err.message
        );
    }
}
