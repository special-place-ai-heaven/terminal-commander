// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Compact-surface `tools/list` construction + the admission gate.
//!
//! The compact surface advertises the verb-dispatched facade tools instead of
//! the granular legacy tools. This milestone exposes a single facade,
//! `command`; later plans extend [`COMPACT_TOOL_NAMES`] and
//! [`compact_surface_tools`] in lockstep.
//!
//! The facade `Tool` here is built from the SAME schema source the rmcp
//! `#[tool]` macro uses (`schema_for_type`), so the `command` entry advertised
//! on the compact surface is byte-identical to what the router would advertise
//! for the same input type -- no hand-maintained schema drift.

use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::Tool;
use serde_json::{Map, Value};

use crate::surface::Surface;

/// Description for the `command` facade. Kept identical to the `#[tool]`
/// attribute on `command_facade` in `tools.rs` so the compact-surface entry
/// matches the router-advertised entry verbatim.
const COMMAND_FACADE_DESCRIPTION: &str = "Run and observe a one-shot command. To run a command and \
get its result in ONE call, use action=\"run_and_watch\" (it does start + bounded \
wait + collect for you). Other actions: run, exec, status, output_tail, stop, \
events, wait, summary, event_context, sub_open, sub_pull, sub_seek, sub_close, \
sub_list.";

/// The facade tool names advertised + admitted on the compact surface.
///
/// This milestone: `command` only (session/files/registry/status land in
/// follow-on plans). KEEP IN SYNC with [`compact_surface_tools`].
pub const COMPACT_TOOL_NAMES: &[&str] = &["command"];

/// Flatten an internally-tagged-enum schema (root `oneOf`) into the flat
/// `{ "type":"object", "properties": {...}, "required": ["action"] }` shape MCP
/// clients require.
///
/// The facade enum is `#[serde(tag = "action")]`, so its WIRE format is already
/// flat (`{"action": "...", ...fields}` deserializes directly) -- only the
/// ADVERTISED schema is a root `oneOf`, which strict MCP clients (the Claude
/// Code harness, the MCP TS SDK) SILENTLY DROP ("connected, no tools"), because a
/// tool inputSchema must be a flat object with `properties` (proven against
/// symforge's working flat `symforge` tool). Dispatch is untouched; we only
/// reshape the schema: collect every variant's `action` const into an enum, and
/// union every referenced param struct's properties into a flat `properties`
/// (all optional at the root -- per-action required fields are still enforced
/// when the typed enum deserializes the call). `$defs` is kept as-is so the
/// inlined props' `$ref`s (EnvEntry / RuleInput / McpSubscriptionSourceSel) still
/// resolve; now-unused `Mcp*Params` defs are harmless. (Regression: 0.1.55/0.1.56
/// shipped a root-oneOf schema and zeroed the compact surface live.)
fn flatten_facade_schema(mut schema: Map<String, Value>) -> Map<String, Value> {
    let Some(one_of) = schema.remove("oneOf").and_then(|v| match v {
        Value::Array(a) => Some(a),
        _ => None,
    }) else {
        // Already a flat object schema (e.g. a struct facade): just ensure type.
        schema
            .entry("type".to_string())
            .or_insert_with(|| Value::String("object".to_string()));
        return schema;
    };
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut actions: Vec<Value> = Vec::new();
    let mut props = Map::new();
    for branch in &one_of {
        let Some(b) = branch.as_object() else {
            continue;
        };
        if let Some(a) = b
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("const"))
            .and_then(Value::as_str)
        {
            actions.push(Value::String(a.to_string()));
        }
        if let Some(p) = b
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.rsplit('/').next())
            .and_then(|name| defs.get(name))
            .and_then(|d| d.get("properties"))
            .and_then(Value::as_object)
        {
            for (k, v) in p {
                props.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
    }
    props.insert(
        "action".to_string(),
        serde_json::json!({
            "type": "string",
            "enum": actions,
            "description": "The operation. Prefer \"run_and_watch\" to run a command and get its signals + exit in ONE call.",
        }),
    );
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(props));
    schema.insert(
        "required".to_string(),
        Value::Array(vec![Value::String("action".to_string())]),
    );
    schema
}

/// Build the single facade `Tool` for a given name + input type. The schema is
/// flattened to the MCP-required flat object shape (see [`flatten_facade_schema`]).
fn surface_tool<T>(name: &'static str, description: &'static str) -> Tool
where
    T: schemars::JsonSchema + std::any::Any,
{
    let schema = flatten_facade_schema((*schema_for_type::<T>()).clone());
    Tool::new(name, description, Arc::new(schema))
}

/// `tools/list` payload for `TC_SURFACE=compact`.
///
/// This milestone returns the single `command` facade `Tool`.
#[must_use]
pub fn compact_surface_tools() -> Vec<Tool> {
    vec![surface_tool::<crate::facades::CommandFacadeCall>(
        "command",
        COMMAND_FACADE_DESCRIPTION,
    )]
}

/// Admission gate: under [`Surface::Compact`], reject any tool name not in the
/// facade set. Under [`Surface::Full`] every name is admitted (the router does
/// its own per-name routing).
///
/// # Errors
///
/// Returns an `invalid_request` error when `surface` is `Compact` and
/// `tool_name` is not one of [`COMPACT_TOOL_NAMES`]. The message tells the
/// caller to set `TC_SURFACE=full` to reach the granular tool.
pub fn enforce_surface(surface: Surface, tool_name: &str) -> Result<(), McpError> {
    if surface == Surface::Compact && !COMPACT_TOOL_NAMES.contains(&tool_name) {
        return Err(McpError::invalid_request(
            format!("tool '{tool_name}' not on compact surface; set TC_SURFACE=full"),
            None,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::Surface;

    #[test]
    fn compact_list_is_only_facades() {
        let names: Vec<_> = compact_surface_tools()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert!(names.contains(&"command".to_string()));
        // No granular legacy tool leaks into the compact list this milestone:
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("command_") || n == "run_and_watch")
        );
    }

    #[test]
    fn gate_blocks_legacy_under_compact_allows_under_full() {
        // Legacy name is rejected on compact, allowed on full.
        assert!(enforce_surface(Surface::Compact, "command_status").is_err());
        assert!(enforce_surface(Surface::Full, "command_status").is_ok());
        // Facade name is allowed on both.
        assert!(enforce_surface(Surface::Compact, "command").is_ok());
        assert!(enforce_surface(Surface::Full, "command").is_ok());
    }

    #[test]
    fn compact_tools_schema_is_flat_object() {
        // MCP clients (the Claude Code harness, the MCP TS SDK) accept a flat
        // `{type:"object", properties:{...}}` tool inputSchema and SILENTLY DROP
        // a root-`oneOf` tool ("connected, no tools"). Assert the facade schema
        // is flat: type==object, has `properties` with an `action` enum, NO root
        // `oneOf`, and that the flatten is lossless enough to surface params from
        // different actions. (Regression: 0.1.55/0.1.56 shipped a root-oneOf
        // schema and zeroed the compact surface live.)
        for tool in compact_surface_tools() {
            let s = &tool.input_schema;
            assert_eq!(
                s.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "tool '{}': inputSchema.type must be \"object\"",
                tool.name,
            );
            assert!(
                s.get("oneOf").is_none(),
                "tool '{}': schema must NOT have a root oneOf (MCP clients drop it)",
                tool.name,
            );
            let props = s
                .get("properties")
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| panic!("tool '{}': missing root properties", tool.name));
            assert!(
                props.contains_key("action"),
                "tool '{}': properties must include the action enum",
                tool.name,
            );
            assert!(
                props.contains_key("argv") && props.contains_key("job_id"),
                "tool '{}': flatten must surface params from multiple actions (argv, job_id)",
                tool.name,
            );
        }
    }
}
