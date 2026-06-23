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
    McpCommandStopParams, McpEventContextParams, McpRunAndWatchParams, McpShellExecParams,
    McpSubscriptionCloseParams, McpSubscriptionListParams, McpSubscriptionOpenParams,
    McpSubscriptionPullParams, McpSubscriptionSeekParams,
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
}
