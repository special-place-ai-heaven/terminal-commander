// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! A2 policy-denied teach envelope.
//!
//! One serializer owns the MCP field names. rmcp error builders call it;
//! they do not spell the keys themselves. Transport `daemon_unavailable`
//! is a different envelope and never comes through here.
//!
//! A deny whose [`ShellTeach::recipe_id`] is set steers to `recipe_run`
//! (`retry_with_recipe`). No match keeps `run_and_watch` / `retry_with_argv`.
//! Alternatives stay the argv list either way. This does not flip `allow_shell`.

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::{Value, json};
use terminal_commander_ipc::{ShellDenyClass, ShellTeach};

pub const KIND: &str = "policy_denied";
pub const RECOVER_HINT: &str = "retry_with_argv";
pub const INTENDED_TOOL: &str = "run_and_watch";
pub const RECIPE_RECOVER_HINT: &str = "retry_with_recipe";
pub const RECIPE_INTENDED_TOOL: &str = "recipe_run";

/// Discover steer and the error envelope share this object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgvSteer {
    pub intended_tool: &'static str,
    pub intended_example: Value,
    pub recover_hint: &'static str,
}

#[must_use]
pub fn intended_example() -> Value {
    json!({"argv": ["git", "status"]})
}

#[must_use]
pub fn alternatives() -> Value {
    json!([
        {"tool": "run_and_watch"},
        {"tool": "command_start_combed"},
        {"tool": "file_read_window"},
        {"tool": "file_search"},
        {"tool": "file_write"},
        {"tool": "pty_command_start"},
        {"tool": "shell_exec", "tag": "operator_opt_in"}
    ])
}

#[must_use]
pub fn argv_steer() -> ArgvSteer {
    ArgvSteer {
        intended_tool: INTENDED_TOOL,
        intended_example: intended_example(),
        recover_hint: RECOVER_HINT,
    }
}

/// `recipe_run` when the daemon named one activated recipe and its scope.
/// A recipe id without `scope` is not callable (`recipe_run` requires it),
/// so that case stays on argv teach.
fn steer_for(teach: &ShellTeach) -> (&'static str, Value, &'static str) {
    match (teach.recipe_id.as_deref(), teach.recipe_scope) {
        (Some(id), Some(scope)) if !id.is_empty() => (
            RECIPE_INTENDED_TOOL,
            json!({"recipe_id": id, "scope": scope}),
            RECIPE_RECOVER_HINT,
        ),
        _ => (INTENDED_TOOL, intended_example(), RECOVER_HINT),
    }
}

/// MCP `-32602` `data` object for a shell-misuse deny.
#[must_use]
pub fn policy_denied_data(teach: &ShellTeach, denied_tool: Option<&str>, ipc_code: &str) -> Value {
    let (intended_tool, intended_example, recover_hint) = steer_for(teach);
    let mut alternatives = alternatives();
    // No operator knob enables shell on a profile that forbids it.
    if teach.deny_class == ShellDenyClass::ProfileForbidsShell
        && let Some(list) = alternatives.as_array_mut()
    {
        list.retain(|alt| alt["tool"] != "shell_exec");
    }
    json!({
        "ipc_code": ipc_code,
        "kind": KIND,
        "deny_class": teach.deny_class,
        "profile": teach.profile,
        "denied_capability": teach.denied_capability,
        "denied_tool": denied_tool.unwrap_or(teach.denied_tool.as_str()),
        "reason": teach.reason,
        "intended_tool": intended_tool,
        "intended_example": intended_example,
        "alternatives": alternatives,
        "recover_hint": recover_hint,
    })
}

/// Facade calls share the full-surface handlers. Rewrite `denied_tool` to the
/// tool the harness actually invoked when the payload is a teach envelope.
pub fn retarget_denied_tool(
    result: Result<CallToolResult, McpError>,
    tool: &str,
) -> Result<CallToolResult, McpError> {
    match result {
        Ok(value) => Ok(value),
        Err(mut err) => {
            if let Some(obj) = err.data.as_mut().and_then(Value::as_object_mut)
                && obj.get("kind").and_then(Value::as_str) == Some(KIND)
            {
                obj.insert("denied_tool".to_owned(), Value::String(tool.to_owned()));
            }
            Err(err)
        }
    }
}

#[must_use]
pub fn recover_hint_upsells_shell(hint: &str) -> bool {
    let lower = hint.to_ascii_lowercase();
    lower.contains("enable shell")
        || lower.contains("set allow_shell")
        || lower.contains("allow_shell true")
        || lower.contains("allow_shell = true")
        || lower.contains("turn shell on")
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_ipc::ShellDenyClass;

    fn sample(
        class: ShellDenyClass,
        profile: &str,
        capability: Option<&str>,
        tool: &str,
    ) -> ShellTeach {
        ShellTeach {
            deny_class: class,
            profile: profile.to_owned(),
            denied_capability: capability.map(str::to_owned),
            denied_tool: tool.to_owned(),
            reason: class.reason().to_owned(),
            recipe_id: None,
            recipe_scope: None,
        }
    }

    fn assert_envelope(data: &Value, class: &str) {
        assert_eq!(data["kind"], json!(KIND));
        assert_eq!(data["deny_class"], json!(class));
        assert_eq!(data["intended_tool"], json!(INTENDED_TOOL));
        assert_eq!(data["intended_example"], intended_example());
        assert_eq!(data["recover_hint"], json!(RECOVER_HINT));
        let hint = data["recover_hint"].as_str().expect("recover_hint");
        assert!(
            !recover_hint_upsells_shell(hint),
            "recover_hint must not upsell enabling shell: {hint}"
        );
        assert!(
            !data["reason"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("enable shell")
        );
        assert!(
            !data["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("set allow_shell")
        );
        if class == "profile_forbids_shell" {
            assert!(
                !data["alternatives"].to_string().contains("shell_exec"),
                "a profile that forbids shell offers no shell_exec opt-in"
            );
        } else {
            assert_eq!(data["alternatives"], alternatives());
            let last = data["alternatives"]
                .as_array()
                .and_then(|a| a.last())
                .expect("alternatives");
            assert_eq!(last["tool"], json!("shell_exec"));
            assert_eq!(last["tag"], json!("operator_opt_in"));
        }
        assert!(
            !data["intended_example"].to_string().contains("shell_line"),
            "intended example must be argv, never shell_line"
        );
    }

    #[test]
    fn golden_shell_capability_off() {
        let teach = sample(
            ShellDenyClass::ShellCapabilityOff,
            "DeveloperLocal",
            Some("allow_shell"),
            "shell_exec",
        );
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/a2/shell_capability_off.json"
        ))
        .expect("golden json");
        assert_eq!(data, expected);
        assert_envelope(&data, "shell_capability_off");
        assert_eq!(data["denied_capability"], json!("allow_shell"));
    }

    #[test]
    fn golden_shell_interpreter_denied() {
        let mut teach = sample(
            ShellDenyClass::ShellInterpreterDenied,
            "DeveloperLocal",
            Some("allow_shell"),
            "command_start_combed",
        );
        // An interpreter deny carries the argv lane's own text (pinned in the
        // daemon's `shell_teach_keeps_lane_text_and_names_profile_forbid`).
        teach.reason = "shell interpreter 'bash' denied: allow_shell is off. Run the program directly as argv (e.g. [\"cargo\",\"build\"] instead of [\"bash\",\"-c\",\"cargo build\"]), or have the operator set [policy.caps] allow_shell = true, which allows this argv and enables shell_exec (command with action=\"exec\" on the compact MCP surface).".to_owned();
        let data = policy_denied_data(&teach, Some("run_and_watch"), "ShellInterpreterDenied");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/a2/shell_interpreter_denied.json"
        ))
        .expect("golden json");
        assert_eq!(data, expected);
        assert_envelope(&data, "shell_interpreter_denied");
        assert_eq!(data["denied_capability"], json!("allow_shell"));
        assert_eq!(data["denied_tool"], json!("run_and_watch"));
    }

    #[test]
    fn golden_profile_forbids_shell() {
        let teach = sample(
            ShellDenyClass::ProfileForbidsShell,
            "RepoOnly",
            None,
            "shell_exec",
        );
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/a2/profile_forbids_shell.json"
        ))
        .expect("golden json");
        assert_eq!(data, expected);
        assert_envelope(&data, "profile_forbids_shell");
        assert!(data["denied_capability"].is_null());
    }

    #[test]
    fn allow_shell_default_is_on_for_developer_local() {
        let engine = terminal_commanderd::PolicyEngine::new(
            terminal_commanderd::PolicyProfile::DeveloperLocal,
        );
        assert!(
            engine.caps_allow_shell(),
            "DeveloperLocal allow_shell default is on (owner decision D0)"
        );
        assert!(engine.resolved_caps().allow_shell);
    }

    #[test]
    fn recover_hint_constant_is_retry_with_argv() {
        assert_eq!(RECOVER_HINT, "retry_with_argv");
        assert_eq!(RECIPE_RECOVER_HINT, "retry_with_recipe");
        assert!(!recover_hint_upsells_shell(RECOVER_HINT));
        assert!(!recover_hint_upsells_shell(RECIPE_RECOVER_HINT));
        assert!(!recover_hint_upsells_shell("retry_with_argv"));
        assert!(recover_hint_upsells_shell("set allow_shell true"));
        assert!(recover_hint_upsells_shell("Enable shell and retry"));
    }

    fn assert_recipe_envelope(data: &Value, recipe_id: &str) {
        assert_eq!(data["intended_tool"], json!(RECIPE_INTENDED_TOOL));
        assert_eq!(
            data["intended_example"],
            json!({"recipe_id": recipe_id, "scope": {"kind": "global"}})
        );
        assert_eq!(data["recover_hint"], json!(RECIPE_RECOVER_HINT));
        assert_eq!(data["alternatives"], alternatives());
        let tools: Vec<&str> = data["alternatives"]
            .as_array()
            .expect("alternatives")
            .iter()
            .filter_map(|entry| entry["tool"].as_str())
            .collect();
        assert!(tools.contains(&"run_and_watch"));
        assert!(tools.contains(&"command_start_combed"));
        let hint = data["recover_hint"].as_str().expect("recover_hint");
        assert!(!recover_hint_upsells_shell(hint));
        assert!(
            !data["reason"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("enable shell")
        );
        assert!(
            !data["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("set allow_shell")
        );
        assert!(
            !data["intended_example"].to_string().contains("shell_line"),
            "recipe example is recipe_id only"
        );
        assert!(data["intended_example"].get("argv").is_none());
    }

    #[test]
    fn golden_retry_with_recipe_when_one_recipe_matches() {
        let mut teach = sample(
            ShellDenyClass::ShellCapabilityOff,
            "DeveloperLocal",
            Some("allow_shell"),
            "shell_exec",
        );
        teach.recipe_id = Some("git.status".to_owned());
        teach.recipe_scope = Some(terminal_commander_core::ActivationScope::Global);
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        let expected: Value =
            serde_json::from_str(include_str!("../tests/fixtures/a2/retry_with_recipe.json"))
                .expect("golden json");
        assert_eq!(data, expected);
        assert_recipe_envelope(&data, "git.status");
        assert_eq!(data["denied_capability"], json!("allow_shell"));
        assert_eq!(data["kind"], json!(KIND));
    }

    #[test]
    fn empty_recipe_id_keeps_argv_golden() {
        let mut teach = sample(
            ShellDenyClass::ShellCapabilityOff,
            "DeveloperLocal",
            Some("allow_shell"),
            "shell_exec",
        );
        teach.recipe_id = Some(String::new());
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/a2/shell_capability_off.json"
        ))
        .expect("golden json");
        assert_eq!(data, expected);
    }

    #[test]
    fn omitted_recipe_id_deserializes_as_argv_teach() {
        let teach: ShellTeach = serde_json::from_str(
            r#"{"deny_class":"shell_capability_off","profile":"DeveloperLocal","denied_capability":"allow_shell","denied_tool":"shell_exec","reason":"Shell execution denied: allow_shell is off. Retry with an argv array, or ask the operator to set [policy.caps] allow_shell = true."}"#,
        )
        .expect("old teach payload");
        assert!(teach.recipe_id.is_none());
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/a2/shell_capability_off.json"
        ))
        .expect("golden json");
        assert_eq!(data, expected);
    }

    #[test]
    fn retarget_denied_tool_keeps_recipe_steer() {
        let mut teach = sample(
            ShellDenyClass::ShellInterpreterDenied,
            "DeveloperLocal",
            None,
            "command_start_combed",
        );
        teach.recipe_id = Some("git.status".to_owned());
        teach.recipe_scope = Some(terminal_commander_core::ActivationScope::Global);
        let data = policy_denied_data(&teach, None, "ShellInterpreterDenied");
        let err = McpError::invalid_params("policy_denied", Some(data));
        let err = retarget_denied_tool(Err(err), "run_and_watch").expect_err("deny");
        let data = err.data.expect("envelope");
        assert_eq!(data["denied_tool"], json!("run_and_watch"));
        assert_recipe_envelope(&data, "git.status");
        assert_eq!(data["deny_class"], json!("shell_interpreter_denied"));
    }

    #[test]
    fn recipe_id_without_scope_stays_on_argv_teach() {
        let mut teach = sample(
            ShellDenyClass::ShellCapabilityOff,
            "DeveloperLocal",
            Some("allow_shell"),
            "shell_exec",
        );
        teach.recipe_id = Some("git.status".to_owned());
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        assert_eq!(data["recover_hint"], json!(RECOVER_HINT));
        assert_eq!(data["intended_tool"], json!(INTENDED_TOOL));
    }

    #[test]
    fn intended_example_is_callable_as_recipe_run() {
        let mut teach = sample(
            ShellDenyClass::ShellCapabilityOff,
            "DeveloperLocal",
            Some("allow_shell"),
            "shell_exec",
        );
        teach.recipe_id = Some("git.status".to_owned());
        teach.recipe_scope = Some(terminal_commander_core::ActivationScope::Global);
        let data = policy_denied_data(&teach, None, "PolicyDenied");
        let example = data["intended_example"].clone();
        let params: crate::tools::McpRecipeRunParams =
            serde_json::from_value(example.clone()).expect("recipe_run params");
        assert_eq!(params.recipe_id, "git.status");
        let scope = params.scope.into_ipc_scope().expect("scope");
        assert_eq!(scope, terminal_commander_core::ActivationScope::Global);
        let mut facade = example;
        facade["action"] = json!("run");
        crate::facade_strict::validate_facade_call("recipe", &facade).expect("facade");
        let parsed: crate::facades::RecipeFacadeCall =
            serde_json::from_value(facade).expect("facade run");
        assert!(matches!(parsed, crate::facades::RecipeFacadeCall::Run(_)));
    }
}
