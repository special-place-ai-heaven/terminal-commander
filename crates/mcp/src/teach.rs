// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! A2 policy-denied teach envelope.
//!
//! One serializer owns the MCP field names. rmcp error builders call it;
//! they do not spell the keys themselves. Transport `daemon_unavailable`
//! is a different envelope and never comes through here.

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::{Value, json};
use terminal_commander_ipc::ShellTeach;

pub const KIND: &str = "policy_denied";
pub const RECOVER_HINT: &str = "retry_with_argv";
pub const INTENDED_TOOL: &str = "run_and_watch";

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

/// MCP `-32602` `data` object for a shell-misuse deny.
#[must_use]
pub fn policy_denied_data(teach: &ShellTeach, denied_tool: Option<&str>, ipc_code: &str) -> Value {
    json!({
        "ipc_code": ipc_code,
        "kind": KIND,
        "deny_class": teach.deny_class,
        "profile": teach.profile,
        "denied_capability": teach.denied_capability,
        "denied_tool": denied_tool.unwrap_or(teach.denied_tool.as_str()),
        "reason": teach.reason,
        "intended_tool": INTENDED_TOOL,
        "intended_example": intended_example(),
        "alternatives": alternatives(),
        "recover_hint": RECOVER_HINT,
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
        }
    }

    fn assert_envelope(data: &Value, class: &str) {
        assert_eq!(data["kind"], json!(KIND));
        assert_eq!(data["deny_class"], json!(class));
        assert_eq!(data["intended_tool"], json!(INTENDED_TOOL));
        assert_eq!(data["intended_example"], intended_example());
        assert_eq!(data["recover_hint"], json!(RECOVER_HINT));
        assert_eq!(data["alternatives"], alternatives());
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
        let last = data["alternatives"]
            .as_array()
            .and_then(|a| a.last())
            .expect("alternatives");
        assert_eq!(last["tool"], json!("shell_exec"));
        assert_eq!(last["tag"], json!("operator_opt_in"));
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
        let teach = sample(
            ShellDenyClass::ShellInterpreterDenied,
            "DeveloperLocal",
            None,
            "command_start_combed",
        );
        let data = policy_denied_data(&teach, Some("run_and_watch"), "ShellInterpreterDenied");
        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/a2/shell_interpreter_denied.json"
        ))
        .expect("golden json");
        assert_eq!(data, expected);
        assert_envelope(&data, "shell_interpreter_denied");
        assert!(data["denied_capability"].is_null());
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
    fn allow_shell_default_remains_false() {
        let engine = terminal_commanderd::PolicyEngine::new(
            terminal_commanderd::PolicyProfile::DeveloperLocal,
        );
        assert!(
            !engine.caps_allow_shell(),
            "DeveloperLocal allow_shell default must stay false"
        );
        assert!(!engine.resolved_caps().allow_shell);
    }

    #[test]
    fn recover_hint_constant_is_retry_with_argv() {
        assert_eq!(RECOVER_HINT, "retry_with_argv");
        assert!(!recover_hint_upsells_shell(RECOVER_HINT));
        assert!(!recover_hint_upsells_shell("retry_with_argv"));
        assert!(recover_hint_upsells_shell("set allow_shell true"));
        assert!(recover_hint_upsells_shell("Enable shell and retry"));
    }
}
