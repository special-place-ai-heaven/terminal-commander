//! Verb-dispatched facade input types.
//!
//! Each variant reuses an existing `Mcp*Params` struct so per-action schema
//! validation is preserved; the facade method forwards to the existing
//! `#[tool]` handler unchanged.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::tools::{
    McpBucketEventsSinceParams, McpBucketSummaryParams, McpBucketWaitParams,
    McpCommandOutputTailParams, McpCommandStartParams, McpCommandStatusParams,
    McpCommandStopParams, McpEventContextParams, McpFileReadWindowParams, McpFileSearchParams,
    McpFileWatchStartParams, McpFileWatchStopParams, McpFileWriteParams, McpListLimitParams,
    McpProbeStatusParams, McpPtyCommandStartParams, McpPtyCommandStopParams,
    McpPtyCommandWriteStdinParams, McpRegistryActivateParams, McpRegistryDeactivateParams,
    McpRegistryGetParams, McpRegistryImportPackParams, McpRegistrySearchParams,
    McpRegistrySuggestFromSamplesParams, McpRegistryTestParams, McpRegistryUpsertParams,
    McpRunAndWatchParams, McpShellExecParams, McpShellSessionExecParams, McpShellSessionRefParams,
    McpShellSessionStartParams, McpSubscriptionCloseParams, McpSubscriptionListParams,
    McpSubscriptionOpenParams, McpSubscriptionPullParams, McpSubscriptionSeekParams,
    McpTargetProbeParams, McpWorkspaceSnapshotApplyParams, McpWorkspaceSnapshotCreateParams,
};

/// `command` facade -- run + observe + stream a one-shot command. Internally
/// tagged by `action`; the action's remaining fields are the chosen op's params.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CommandFacadeCall {
    Run(McpCommandStartParams),
    RunAndWatch(McpRunAndWatchParams),
    Exec(McpShellExecParams),
    Status(McpCommandStatusParams),
    OutputTail(McpCommandOutputTailParams),
    Stop(McpCommandStopParams),
    Events(McpBucketEventsSinceParams),
    Wait(McpBucketWaitParams),
    Summary(McpBucketSummaryParams),
    EventContext(McpEventContextParams),
    SubOpen(McpSubscriptionOpenParams),
    SubPull(McpSubscriptionPullParams),
    SubSeek(McpSubscriptionSeekParams),
    SubClose(McpSubscriptionCloseParams),
    SubList(McpSubscriptionListParams),
}

/// `session` facade -- PTY commands + persistent shell sessions.
///
/// Internally tagged by `action`; the action's remaining fields are the chosen
/// op's params. Unit variants (`PtyList`, `ShList`) take no additional fields.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SessionFacadeCall {
    PtyStart(McpPtyCommandStartParams),
    PtyStdin(McpPtyCommandWriteStdinParams),
    PtyStop(McpPtyCommandStopParams),
    /// List all live PTY jobs. No additional fields required.
    PtyList,
    ShStart(McpShellSessionStartParams),
    ShExec(McpShellSessionExecParams),
    ShStatus(McpShellSessionRefParams),
    ShStop(McpShellSessionRefParams),
    /// List all live shell sessions. No additional fields required.
    ShList,
}

/// `files` facade -- file read/search/write + file watches + workspace snapshots.
///
/// Internally tagged by `action`; the action's remaining fields are the chosen
/// op's params. Unit variant (`WatchList`) takes no additional fields.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FilesFacadeCall {
    Read(McpFileReadWindowParams),
    Search(McpFileSearchParams),
    Write(McpFileWriteParams),
    WatchStart(McpFileWatchStartParams),
    WatchStop(McpFileWatchStopParams),
    /// List all live file watches. No additional fields required.
    WatchList,
    SnapshotCreate(McpWorkspaceSnapshotCreateParams),
    SnapshotApply(McpWorkspaceSnapshotApplyParams),
}

/// `registry` facade -- rule registry CRUD + dry-run test + heuristic suggest.
/// Internally tagged by `action`; the action's remaining fields are the chosen
/// op's params.
///
/// Note: `suggest_from_samples` uses the field `sample_lines` (not `samples`)
/// to avoid a shape collision with `test`'s `samples: Vec<{text,stream}>`.
/// The alias `samples` is accepted for backward compatibility.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RegistryFacadeCall {
    Search(McpRegistrySearchParams),
    Get(McpRegistryGetParams),
    Upsert(McpRegistryUpsertParams),
    Test(McpRegistryTestParams),
    Activate(McpRegistryActivateParams),
    Deactivate(McpRegistryDeactivateParams),
    ListActive(McpListLimitParams),
    ImportPack(McpRegistryImportPackParams),
    SuggestFromSamples(McpRegistrySuggestFromSamplesParams),
}

/// `status` facade -- health, policy, runtime state, probes, targets.
/// Internally tagged by `action`; the action's remaining fields are the chosen
/// op's params. Unit variants take no additional fields.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum StatusFacadeCall {
    /// Daemon liveness ping. No additional fields required.
    Health,
    /// Re-run daemon self-check. No additional fields required.
    SelfCheck,
    /// Active policy profile + caps. No additional fields required.
    PolicyStatus,
    RuntimeState(McpListLimitParams),
    ProbeList(McpListLimitParams),
    ProbeStatus(McpProbeStatusParams),
    /// Adapter metadata + tool catalogue. No additional fields required.
    SystemDiscover,
    /// List registered remote targets. No additional fields required.
    TargetList,
    TargetProbe(McpTargetProbeParams),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_call_deserializes_flat_action() {
        // status: flat { action, ...fields }
        let v: CommandFacadeCall =
            serde_json::from_value(serde_json::json!({"action":"status","job_id":"job_abc"}))
                .expect("status must parse");
        assert!(matches!(v, CommandFacadeCall::Status(p) if p.job_id == "job_abc"));

        // run_and_watch: the promoted happy path
        let v: CommandFacadeCall = serde_json::from_value(
            serde_json::json!({"action":"run_and_watch","argv":["echo","hi"]}),
        )
        .expect("run_and_watch must parse");
        assert!(matches!(v, CommandFacadeCall::RunAndWatch(_)));
    }

    #[test]
    fn session_call_deserializes_unit_and_param_actions() {
        // unit variant: pty_list needs no extra fields
        let v: SessionFacadeCall = serde_json::from_value(serde_json::json!({"action":"pty_list"}))
            .expect("pty_list must parse");
        assert!(matches!(v, SessionFacadeCall::PtyList));

        // unit variant: sh_list
        let v: SessionFacadeCall = serde_json::from_value(serde_json::json!({"action":"sh_list"}))
            .expect("sh_list must parse");
        assert!(matches!(v, SessionFacadeCall::ShList));

        // param variant: pty_start
        let v: SessionFacadeCall =
            serde_json::from_value(serde_json::json!({"action":"pty_start","argv":["bash"]}))
                .expect("pty_start must parse");
        assert!(matches!(v, SessionFacadeCall::PtyStart(_)));
    }

    #[test]
    fn files_call_deserializes_unit_and_param_actions() {
        // unit variant: watch_list
        let v: FilesFacadeCall = serde_json::from_value(serde_json::json!({"action":"watch_list"}))
            .expect("watch_list must parse");
        assert!(matches!(v, FilesFacadeCall::WatchList));

        // param variant: read
        let v: FilesFacadeCall =
            serde_json::from_value(serde_json::json!({"action":"read","path":"/tmp/foo.txt"}))
                .expect("read must parse");
        assert!(matches!(v, FilesFacadeCall::Read(_)));
    }

    #[test]
    fn registry_call_deserializes_sample_lines_and_alias() {
        // suggest_from_samples uses sample_lines
        let v: RegistryFacadeCall = serde_json::from_value(serde_json::json!({
            "action": "suggest_from_samples",
            "sample_lines": ["error: something failed"]
        }))
        .expect("suggest_from_samples with sample_lines must parse");
        assert!(matches!(v, RegistryFacadeCall::SuggestFromSamples(_)));

        // backward-compat alias: "samples" still works
        let v: RegistryFacadeCall = serde_json::from_value(serde_json::json!({
            "action": "suggest_from_samples",
            "samples": ["error: something failed"]
        }))
        .expect("suggest_from_samples with alias samples must parse");
        assert!(matches!(v, RegistryFacadeCall::SuggestFromSamples(_)));
    }

    #[test]
    fn status_call_deserializes_unit_and_param_actions() {
        // unit variants
        for action in [
            "health",
            "self_check",
            "policy_status",
            "system_discover",
            "target_list",
        ] {
            let v: StatusFacadeCall = serde_json::from_value(serde_json::json!({"action": action}))
                .unwrap_or_else(|e| panic!("action '{action}' must parse: {e}"));
            let _ = v; // just confirm it parsed
        }

        // param variant: probe_status
        let v: StatusFacadeCall = serde_json::from_value(
            serde_json::json!({"action":"probe_status","probe_id":"prb_abc"}),
        )
        .expect("probe_status must parse");
        assert!(matches!(v, StatusFacadeCall::ProbeStatus(_)));
    }
}
