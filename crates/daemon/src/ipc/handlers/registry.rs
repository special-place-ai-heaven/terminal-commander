// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use super::common::{map_store_error, validate_scope_against_live_jobs};
use crate::ipc::protocol::{
    IpcError, IpcErrorCode, IpcResponse, MAX_REGISTRY_TEST_SAMPLE_BYTES, MAX_REGISTRY_TEST_SAMPLES,
    MAX_SUGGEST_PROPOSED_RULES, MAX_SUGGEST_SAMPLES, RegistryActivateParams,
    RegistryActivateResponse, RegistryActiveEntry, RegistryDeactivateParams,
    RegistryDeactivateResponse, RegistryGetParams, RegistryGetResponse, RegistryImportFailure,
    RegistryImportPackParams, RegistryImportPackResponse, RegistryListActiveResponse,
    RegistrySearchHit, RegistrySearchParams, RegistrySearchResponse,
    RegistrySuggestFromSamplesParams, RegistrySuggestFromSamplesResponse, RegistryTestMatch,
    RegistryTestParams, RegistryTestResponse, RegistryUpsertParams, RegistryUpsertResponse,
};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn handle_registry_search(
    state: &Arc<DaemonState>,
    params: &RegistrySearchParams,
) -> Result<IpcResponse, IpcError> {
    let limit = params
        .limit
        .map(|n| n.min(crate::ipc::protocol::MAX_REGISTRY_SEARCH_LIMIT));
    let hits = state
        .store
        .search_rules(&params.query, limit)
        .map_err(map_store_error)?;
    let projected: Vec<RegistrySearchHit> = hits
        .into_iter()
        .map(|h| RegistrySearchHit {
            rule_id: h.rule_id,
            version: h.version,
            event_kind: h.event_kind,
            summary_template: h.summary_template,
            tags: h.tags,
            severity: h.severity,
            status: h.status,
        })
        .collect();
    Ok(IpcResponse::RegistrySearch(RegistrySearchResponse {
        hits: projected,
    }))
}

pub(in crate::ipc::server) fn lookup_rule_def(
    state: &Arc<DaemonState>,
    rule_id: &str,
    version: Option<u32>,
) -> Result<terminal_commander_core::RuleDefinition, IpcError> {
    let opt = match version {
        Some(v) => state
            .store
            .get_rule_version(rule_id, v)
            .map_err(map_store_error)?,
        None => state
            .store
            .get_latest_rule(rule_id)
            .map_err(map_store_error)?,
    };
    opt.ok_or_else(|| {
        let message = version.map_or_else(
            || format!("rule '{rule_id}' not found"),
            |v| format!("rule '{rule_id}' version {v} not found"),
        );
        IpcError::new(IpcErrorCode::RuleNotFound, message)
    })
}

pub(in crate::ipc::server) fn handle_registry_get(
    state: &Arc<DaemonState>,
    params: &RegistryGetParams,
) -> Result<IpcResponse, IpcError> {
    let def = lookup_rule_def(state, &params.rule_id, params.version)?;
    Ok(IpcResponse::RegistryGet(RegistryGetResponse {
        definition: def,
    }))
}

pub(in crate::ipc::server) fn handle_registry_upsert(
    state: &Arc<DaemonState>,
    params: &RegistryUpsertParams,
) -> Result<IpcResponse, IpcError> {
    // Validate up front so the operator gets a typed RuleInvalid
    // instead of a generic Internal error.
    params
        .definition
        .validate()
        .map_err(|e| IpcError::new(IpcErrorCode::RuleInvalid, e.to_string()))?;
    let version = state
        .store
        .create_rule_version(&params.definition)
        .map_err(map_store_error)?;
    Ok(IpcResponse::RegistryUpsert(RegistryUpsertResponse {
        rule_id: params.definition.id.clone(),
        version,
    }))
}

pub(in crate::ipc::server) fn handle_registry_test(
    state: &Arc<DaemonState>,
    params: &RegistryTestParams,
) -> Result<IpcResponse, IpcError> {
    use terminal_commander_core::{RuleStatus, SourceFrame, SourceStream};
    use terminal_commander_sifters::SifterRuntime;

    if params.samples.len() > MAX_REGISTRY_TEST_SAMPLES {
        return Err(IpcError::new(
            IpcErrorCode::RuleInvalid,
            format!(
                "samples count {} exceeds cap {MAX_REGISTRY_TEST_SAMPLES}",
                params.samples.len()
            ),
        ));
    }

    let mut def = lookup_rule_def(state, &params.rule_id, params.version)?;
    // Force-active so a Draft rule can still be evaluated against
    // samples without persisting an activation. Read-only.
    def.status = RuleStatus::Active;
    let sifter = SifterRuntime::build(std::slice::from_ref(&def))
        .map_err(|e| IpcError::new(IpcErrorCode::RuleInvalid, e.to_string()))?;

    let probe = terminal_commander_core::ProbeId::new();
    let bucket = terminal_commander_core::BucketId::new();
    let mut matches: Vec<RegistryTestMatch> = Vec::new();
    let mut truncated_total: u32 = 0;

    for (i, sample) in params.samples.iter().enumerate() {
        // Per-sample cap; bytes beyond it are dropped before the
        // sifter even sees them.
        let mut text = sample.text.clone();
        if text.len() > MAX_REGISTRY_TEST_SAMPLE_BYTES {
            let mut end = MAX_REGISTRY_TEST_SAMPLE_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let dropped = u32::try_from(text.len() - end).unwrap_or(u32::MAX);
            text.truncate(end);
            truncated_total = truncated_total.saturating_add(dropped);
        }
        let stream = sample.stream.clone().unwrap_or(SourceStream::Stdout);
        let frame = SourceFrame::new(probe, stream, text);
        let drafts = sifter.evaluate(&frame, bucket);
        for draft in drafts {
            let mut captures: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            if let Some(c) = draft.captures.as_ref() {
                for (k, v) in c {
                    captures.insert(k.clone(), v.clone());
                }
            }
            matches.push(RegistryTestMatch {
                sample_index: i,
                severity: draft.severity,
                kind: draft.kind,
                summary: draft.summary,
                captures,
            });
        }
    }

    Ok(IpcResponse::RegistryTest(RegistryTestResponse {
        matches,
        truncated_bytes: truncated_total,
    }))
}

pub(in crate::ipc::server) fn handle_registry_activate(
    state: &Arc<DaemonState>,
    params: &RegistryActivateParams,
) -> Result<IpcResponse, IpcError> {
    // TC42d: scope is REQUIRED. A missing scope is rejected with a
    // typed error rather than silently widened to Global. The
    // dispatcher emits the `ipc_registry_activate` audit row with
    // decision=error so the rejection is durably recorded.
    let scope = params.scope.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required; pass {kind:'global'} for explicit global activation",
        )
    })?;
    let def = lookup_rule_def(state, &params.rule_id, params.version)?;
    let version = def.version;
    // Agent-ergonomics: refuse to bind a rule whose status is not
    // runtime-eligible. Without this gate a Draft rule activates
    // "successfully", then the sifter runtime rejects it at every
    // command_start with SifterError::NotActive, blocking every
    // newly-started command in scope (the draft-poison footgun).
    // Fail up front with the remedy in the message so the LLM can
    // self-correct in one step instead of debugging a global stall.
    if !def.status.is_runtime_eligible() {
        return Err(IpcError::new(
            IpcErrorCode::RuleNotActive,
            format!(
                "rule '{}' v{version} has status {:?}, which is not runtime-eligible; \
                 only Active rules can be activated. Remedy: re-upsert the rule with \
                 \"status\":\"active\" (then activate), or pass it inline via the \
                 command's rules_json with \"status\":\"active\".",
                def.id, def.status
            ),
        ));
    }
    validate_scope_against_live_jobs(state, scope)?;
    // Snapshot prior membership BEFORE any mutation so the response
    // reports whether the rule was already active. Reading memory here
    // is identical whether done before or after the durable write (the
    // write does not touch the in-memory set), so it is safe to capture
    // it up front.
    let was_already_active = state.activation.is_active(&def.id, version, scope);
    // Durability-first ordering (data-integrity invariant): persist the
    // activation row BEFORE mutating the in-memory authority. If the
    // store write fails, `?` returns the error with the in-memory set
    // UNCHANGED, so memory and store never diverge on a failed write.
    // (Previously memory was mutated first; a store failure then left a
    // rule active in memory but absent from the durable store, and a
    // restart would silently drop it -- memory/store divergence.) The
    // store layer is idempotent on the open row (see
    // `record_activation_scoped`), so re-activating an already-open key
    // is a harmless no-op.
    let profile = format!("{:?}", state.policy.profile);
    state
        .store
        .record_activation_scoped(&def.id, version, scope, Some(&profile), Some("ipc"))
        .map_err(map_store_error)?;
    // Store write succeeded: now make the in-memory authority agree so a
    // concurrent command_start picks up the rule. `activate` is an
    // idempotent single-entry upsert keyed on (rule_id, version, scope).
    state.activation.activate(def.clone(), scope);
    // S5: activate-supersedes. Multiple VERSIONS of one rule id active
    // under the SAME scope each fire per matching frame — observed live
    // as duplicate signals (v1 + v2 on one stderr line). The registry's
    // multi-entry key exists for the same (id, version) under DISJOINT
    // scopes, not for version stacking within one scope, so activating
    // vN closes every OTHER version of the same id under this scope.
    // Durability-first like deactivate: close the store row, then
    // memory; on a store failure the error surfaces with both sides
    // still agreeing (the old version simply stays active). The rebinds
    // below run after the supersede so live jobs pick up the final set
    // in one pass.
    let mut superseded_versions: Vec<u32> = active_versions_for_scope(state, &def.id, scope)
        .into_iter()
        .filter(|v| *v != version)
        .collect();
    superseded_versions.sort_unstable();
    for v in &superseded_versions {
        state
            .store
            .deactivate_rule_scoped(&def.id, *v, scope)
            .map_err(map_store_error)?;
        state.activation.deactivate(&def.id, *v, scope);
    }
    // TC42c: push the new rule set into every already-running
    // command's sifter that the scope matches. Global scope rebinds
    // every live job (TC42b behavior preserved). Scoped activations
    // only touch matching jobs. Rebinds run ONLY after memory + store
    // agree, so no job ever rebinds against a half-applied activation.
    let cmd_report = state.command.rebind_jobs_in_scope(Some(scope));
    // TC43: file watches share the activation registry.
    let watch_report = state.watch.rebind_watches_in_scope(Some(scope));
    // TC44: PTY jobs share the activation registry.
    #[cfg(unix)]
    let pty_rebound = state.pty.rebind_jobs_in_scope(Some(scope)).jobs_rebound;
    #[cfg(not(unix))]
    let pty_rebound = 0u32;
    Ok(IpcResponse::RegistryActivate(RegistryActivateResponse {
        rule_id: def.id,
        version,
        was_already_active,
        scope,
        jobs_rebound: cmd_report
            .jobs_rebound
            .saturating_add(watch_report.watches_rebound)
            .saturating_add(pty_rebound),
        superseded_versions,
    }))
}

pub(in crate::ipc::server) fn handle_registry_import_pack(
    state: &Arc<DaemonState>,
    params: &RegistryImportPackParams,
) -> Result<IpcResponse, IpcError> {
    // If activating, scope is required up front (mirror activate so the
    // agent gets a clear remedy rather than a silent global widen).
    if params.activate && params.scope.is_none() {
        return Err(IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required when activate=true; pass {kind:'global'} \
             for explicit global activation",
        ));
    }
    // Import the pack (promote rules to Active iff we will activate
    // them, so the activation eligibility gate below passes honestly).
    let import = state
        .store
        .import_rule_pack_by_name(&params.pack, params.activate)
        .map_err(map_store_error)?;
    // M7 (partial-success): activate each imported rule, collecting
    // per-rule outcomes instead of `?`-bailing on the first failure.
    let (activated, failed) = match params.scope {
        Some(scope) if params.activate => activate_imported_rules(state, &import.imported, scope),
        _ => (Vec::new(), Vec::new()),
    };
    Ok(IpcResponse::RegistryImportPack(
        RegistryImportPackResponse {
            pack: import.pack,
            imported: import.imported,
            skipped: import.skipped,
            activated,
            failed,
        },
    ))
}

/// M7 (partial-success): activate each imported rule independently,
/// returning `(activated, failed)`. A mid-loop failure previously
/// `?`-bailed, leaving rules 0..N-1 active + persisted and the pack
/// imported while the caller received ONLY a bare error -- it could not
/// tell which rules already activated, so it could not safely retry.
///
/// Now every rule's activation is independent: a success lands in
/// `activated`, a failure lands in `failed` with its typed reason, and
/// the loop continues. One rule's failure never rolls back the rules
/// that already activated, and the failed rule's own activate is
/// internally `?`-safe (durability-first ordering in
/// [`handle_registry_activate`] never leaves a half-applied
/// activation). The caller wraps this in a SUCCESSFUL
/// `RegistryImportPack` response: a non-empty `failed` is a partial
/// success, not an IPC error code.
fn activate_imported_rules(
    state: &Arc<DaemonState>,
    rule_ids: &[String],
    scope: terminal_commander_core::ActivationScope,
) -> (Vec<String>, Vec<RegistryImportFailure>) {
    let mut activated = Vec::new();
    let mut failed = Vec::new();
    for rule_id in rule_ids {
        // Reuse the canonical activate path per rule (no fourth copy):
        // it looks up the now-Active stored def, passes the eligibility
        // gate, activates, records, and rebinds live jobs.
        let aparams = RegistryActivateParams {
            rule_id: rule_id.clone(),
            version: None, // latest = the just-imported Active version
            scope: Some(scope),
        };
        match handle_registry_activate(state, &aparams) {
            Ok(_) => activated.push(rule_id.clone()),
            Err(e) => failed.push(RegistryImportFailure {
                rule_id: rule_id.clone(),
                reason: e.message,
            }),
        }
    }
    (activated, failed)
}

/// Versions of `rule_id` currently active under `scope`, sorted
/// ascending. Used by `handle_registry_deactivate` to echo the
/// actually-active version(s) in its teaching error. Reads the
/// in-memory activation authority (the same set `registry_list_active`
/// reports), so the hint matches what the operator can observe.
fn active_versions_for_scope(
    state: &Arc<DaemonState>,
    rule_id: &str,
    scope: terminal_commander_core::ActivationScope,
) -> Vec<u32> {
    let mut versions: Vec<u32> = state
        .activation
        .snapshot_entries()
        .into_iter()
        .filter(|e| e.definition.id == rule_id && e.scope == scope)
        .map(|e| e.definition.version)
        .collect();
    versions.sort_unstable();
    versions.dedup();
    versions
}

pub(in crate::ipc::server) fn handle_registry_deactivate(
    state: &Arc<DaemonState>,
    params: &RegistryDeactivateParams,
) -> Result<IpcResponse, IpcError> {
    // TC42d: scope is REQUIRED. See `handle_registry_activate` for
    // rationale.
    let scope = params.scope.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required; pass {kind:'global'} for explicit global deactivation",
        )
    })?;
    validate_scope_against_live_jobs(state, scope)?;
    // Durability-first ordering (data-integrity invariant): close the
    // persistent row(s) BEFORE removing from the in-memory authority. If
    // the store write fails, `?` returns the error with the in-memory
    // set UNCHANGED, so memory and store never diverge on a failed
    // write. (Previously memory was mutated first; a store failure then
    // left a rule removed from memory but still open in the durable
    // store, and a restart would silently resurrect it -- memory/store
    // divergence.)
    let was_persisted = state
        .store
        .deactivate_rule_scoped(&params.rule_id, params.version, scope)
        .map_err(map_store_error)?;
    // Read the CURRENT in-memory membership (not yet mutated). Combined
    // with `was_persisted`, this drives both the teaching-error branch
    // and the response's `was_deactivated`.
    let was_in_memory = state
        .activation
        .is_active(&params.rule_id, params.version, scope);
    // TB-6: a deactivate that closed NOTHING was previously a silent
    // ok:true / was_deactivated:false no-op. That hides the operator's
    // mistake (wrong version or wrong scope) behind a success envelope.
    // Surface a teaching RuleNotActive instead, echoing the version(s)
    // that ARE active under this scope so the caller can self-correct in
    // one step. (Closed error set: RuleNotActive already exists and fits
    // -- no new code minted.) The store close was a no-op (was_persisted
    // false) and memory is untouched here, so returning early leaves
    // both stores consistent.
    if !was_in_memory && !was_persisted {
        let active_versions = active_versions_for_scope(state, &params.rule_id, scope);
        let active_hint = if active_versions.is_empty() {
            "no active version under this scope (it may be active under a different \
             scope; check registry_list_active)"
                .to_owned()
        } else if active_versions.len() == 1 {
            format!("active version is {}", active_versions[0])
        } else {
            let list = active_versions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("active versions are [{list}]")
        };
        // Render the scope as "<kind>" or "<kind>=<value>" so the bucket /
        // job / probe id is visible when the scope is not Global.
        let scope_desc = scope.value_wire().map_or_else(
            || scope.kind_label().to_owned(),
            |v| format!("{}={v}", scope.kind_label()),
        );
        return Err(IpcError::new(
            IpcErrorCode::RuleNotActive,
            format!(
                "no active row for rule '{}' v{} scope {scope_desc}; {active_hint}",
                params.rule_id, params.version,
            ),
        ));
    }
    // Store row(s) are closed: now make the in-memory authority agree.
    // Runs only after the durable write succeeded, so memory + store are
    // consistent before any rebind observes the change.
    state
        .activation
        .deactivate(&params.rule_id, params.version, scope);
    // TC42c: rebind every running command the scope matches so
    // future frames stop matching against the deactivated rule.
    // In-flight frames finish against the snapshot they captured
    // (no fake historical un-matches). Rebinds run ONLY after memory +
    // store agree.
    let cmd_report = state.command.rebind_jobs_in_scope(Some(scope));
    let watch_report = state.watch.rebind_watches_in_scope(Some(scope));
    #[cfg(unix)]
    let pty_rebound = state.pty.rebind_jobs_in_scope(Some(scope)).jobs_rebound;
    #[cfg(not(unix))]
    let pty_rebound = 0u32;
    Ok(IpcResponse::RegistryDeactivate(
        RegistryDeactivateResponse {
            rule_id: params.rule_id.clone(),
            version: params.version,
            was_deactivated: was_in_memory || was_persisted,
            scope,
            jobs_rebound: cmd_report
                .jobs_rebound
                .saturating_add(watch_report.watches_rebound)
                .saturating_add(pty_rebound),
        },
    ))
}

pub(in crate::ipc::server) fn handle_registry_list_active(
    state: &Arc<DaemonState>,
    params: &crate::ipc::protocol::ListLimitParams,
) -> IpcResponse {
    let limit = params
        .limit
        .unwrap_or(crate::ipc::protocol::MAX_LIST_LIMIT)
        .min(crate::ipc::protocol::MAX_LIST_LIMIT);
    let all: Vec<RegistryActiveEntry> = state
        .activation
        .snapshot_entries()
        .into_iter()
        .map(|e| RegistryActiveEntry {
            rule_id: e.definition.id,
            version: e.definition.version,
            severity: e.definition.severity,
            event_kind: e.definition.event_kind,
            tags: e.definition.tags,
            scope: e.scope,
        })
        .collect();
    let truncated = all.len() > limit;
    let entries: Vec<_> = all.into_iter().take(limit).collect();
    IpcResponse::RegistryListActive(RegistryListActiveResponse { entries, truncated })
}

/// Handle `registry_suggest_from_samples` (US2 / FR-007).
///
/// PURE heuristic suggestion: delegates to the deterministic sifter
/// suggester and wraps the result in the wire response. This handler
/// is READ-ONLY: it touches no store, no activation registry, and no
/// live job. It NEVER activates or persists a rule (FR-008 /
/// constitution VII) -- it cannot, because it is not given the
/// `state` handle the mutating registry handlers receive.
///
/// Sample count is capped at [`MAX_SUGGEST_SAMPLES`] and the proposal
/// count at [`MAX_SUGGEST_PROPOSED_RULES`] so the response stays
/// bounded (constitution III).
pub(in crate::ipc::server) fn handle_registry_suggest_from_samples(
    params: &RegistrySuggestFromSamplesParams,
) -> IpcResponse {
    // Defense-in-depth: cap the sample slice the heuristics scan even
    // though the suggester also caps internally.
    let capped: &[String] = if params.samples.len() > MAX_SUGGEST_SAMPLES {
        &params.samples[..MAX_SUGGEST_SAMPLES]
    } else {
        &params.samples
    };

    let max_rules = params
        .max_rules
        .map(|n| (n as usize).min(MAX_SUGGEST_PROPOSED_RULES))
        .or(Some(MAX_SUGGEST_PROPOSED_RULES));

    let set = terminal_commander_sifters::suggest_rules(capped, max_rules);

    // The next-step loop is constant by design: a suggestion is never
    // live until the operator runs test -> upsert -> activate.
    let next_steps = vec![
        "registry_test".to_owned(),
        "registry_upsert".to_owned(),
        "registry_activate".to_owned(),
    ];

    IpcResponse::RegistrySuggestFromSamples(RegistrySuggestFromSamplesResponse {
        proposed_rules: set.proposed_rules,
        confidence: terminal_commander_sifters::SUGGEST_CONFIDENCE.to_owned(),
        next_steps,
        explanation: set.explanation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use terminal_commander_core::{
        ActivationScope, ContextHint, RuleDefinition, RuleStatus, RuleType, Severity,
    };

    fn unique_data_dir(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        p.push(format!("tc-import-pack-{tag}-{pid}-{nanos}-{n}"));
        p
    }

    fn state_for(data: &std::path::Path) -> Arc<DaemonState> {
        let cfg = crate::config::DaemonConfig::defaults_in(data);
        Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"))
    }

    fn rule(id: &str, status: RuleStatus) -> RuleDefinition {
        RuleDefinition {
            id: id.to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status,
            severity: Severity::Medium,
            event_kind: "test".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["boom".to_owned()]),
            captures: vec![],
            summary_template: "matched boom".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    /// M7: when one imported rule's activation fails mid-loop, the import
    /// must return a PARTIAL-SUCCESS split -- `activated[]` carries the
    /// rules that succeeded and `failed[]` carries the bad one with a
    /// reason -- NOT a bare error that hides the rules that did activate.
    /// The daemon's in-memory activation state must agree with the split:
    /// the good rule is active, the bad rule is not.
    #[test]
    fn one_failed_activation_yields_partial_success_not_bare_error() {
        let data = unique_data_dir("partial");
        let state = state_for(&data);

        // Seed two rules directly in the store, bypassing the pack
        // re-promote so the eligibility gate diverges per rule:
        //   - `good.rule` latest = Active  -> activation eligible
        //   - `bad.rule`  latest = Draft   -> activation rejected
        //     (RuleNotActive) by handle_registry_activate's gate.
        state
            .store
            .create_rule_version(&rule("good.rule", RuleStatus::Active))
            .expect("seed good rule");
        state
            .store
            .create_rule_version(&rule("bad.rule", RuleStatus::Draft))
            .expect("seed bad rule");

        let ids = vec!["good.rule".to_owned(), "bad.rule".to_owned()];
        let (activated, failed) = activate_imported_rules(&state, &ids, ActivationScope::Global);

        // The good rule activated; the bad one is reported as failed --
        // and the call returned BOTH lists, never a bare error.
        assert_eq!(
            activated,
            vec!["good.rule".to_owned()],
            "good rule must activate"
        );
        assert_eq!(failed.len(), 1, "exactly one rule must fail: {failed:?}");
        assert_eq!(failed[0].rule_id, "bad.rule");
        assert!(
            failed[0].reason.contains("not runtime-eligible") || failed[0].reason.contains("Draft"),
            "failure reason should explain the ineligibility, got: {}",
            failed[0].reason
        );

        // Daemon state matches the split: good active, bad NOT active.
        assert!(
            state
                .activation
                .is_active("good.rule", 1, ActivationScope::Global),
            "the activated rule must be live in the in-memory set"
        );
        assert!(
            !state
                .activation
                .is_active("bad.rule", 1, ActivationScope::Global),
            "the failed rule must NOT be live in the in-memory set"
        );

        let _ = std::fs::remove_dir_all(&data);
    }

    /// M7: the all-success path keeps the original shape -- every rule in
    /// `activated`, `failed` empty -- so the additive field never changes
    /// the wire shape when nothing fails.
    #[test]
    fn all_eligible_rules_activate_with_empty_failed() {
        let data = unique_data_dir("allok");
        let state = state_for(&data);
        state
            .store
            .create_rule_version(&rule("a.rule", RuleStatus::Active))
            .expect("seed a");
        state
            .store
            .create_rule_version(&rule("b.rule", RuleStatus::Active))
            .expect("seed b");

        let ids = vec!["a.rule".to_owned(), "b.rule".to_owned()];
        let (activated, failed) = activate_imported_rules(&state, &ids, ActivationScope::Global);

        assert_eq!(activated, ids, "all rules must activate");
        assert!(failed.is_empty(), "no rule should fail: {failed:?}");

        let _ = std::fs::remove_dir_all(&data);
    }

    /// M7 wire-shape: `failed` serializes only when non-empty, so the
    /// all-success response keeps the historical 4-field JSON and older
    /// clients that never knew `failed` still deserialize it.
    #[test]
    fn empty_failed_is_omitted_from_serialized_response() {
        let resp = RegistryImportPackResponse {
            pack: "cargo".to_owned(),
            imported: vec!["cargo.compile-error".to_owned()],
            skipped: vec![],
            activated: vec!["cargo.compile-error".to_owned()],
            failed: vec![],
        };
        let json = serde_json::to_value(&resp).expect("serialize");
        assert!(
            json.get("failed").is_none(),
            "empty `failed` must be omitted to preserve the additive wire shape, got: {json}"
        );

        // A non-empty `failed` serializes as [{rule_id, reason}].
        let resp2 = RegistryImportPackResponse {
            failed: vec![RegistryImportFailure {
                rule_id: "bad.rule".to_owned(),
                reason: "not runtime-eligible".to_owned(),
            }],
            ..resp
        };
        let json2 = serde_json::to_value(&resp2).expect("serialize");
        let failed = json2.get("failed").expect("failed present when non-empty");
        assert_eq!(failed[0]["rule_id"], "bad.rule");
        assert_eq!(failed[0]["reason"], "not runtime-eligible");
    }
}
