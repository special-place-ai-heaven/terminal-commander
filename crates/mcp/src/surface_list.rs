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

/// Build the single facade `Tool` for a given name + input type, reusing the
/// rmcp `schema_for_type` cache so the schema matches the router's exactly.
fn surface_tool<T>(name: &'static str, description: &'static str) -> Tool
where
    T: schemars::JsonSchema + std::any::Any,
{
    // schemars renders an internally-tagged enum (the facade's discriminated
    // union) as a root `oneOf` with NO top-level `type`. The MCP `tools/list`
    // contract requires every tool's `inputSchema.type == "object"`, and strict
    // clients (e.g. the Claude Code harness) REJECT the entire tools list when
    // it is absent -- which silently zeroes the compact surface. Inject it: the
    // `oneOf` branches are all objects, so `{ "type": "object", "oneOf": [...] }`
    // is sound and satisfies the contract. (Regression: 0.1.55 tools-fetch
    // failed with `inputSchema.type expected "object"`.)
    let mut schema: Map<String, Value> = (*schema_for_type::<T>()).clone();
    schema
        .entry("type".to_string())
        .or_insert_with(|| Value::String("object".to_string()));
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
    fn compact_tools_input_schema_is_object() {
        // Every compact facade tool MUST advertise `inputSchema.type == "object"`
        // or strict MCP clients reject the ENTIRE tools/list. Regression: the
        // discriminated-union schema rendered a root `oneOf` with no `type`,
        // which zeroed the compact surface live in the Claude Code harness
        // (`tools/list` failed: inputSchema.type expected "object").
        for tool in compact_surface_tools() {
            let ty = tool.input_schema.get("type").and_then(|v| v.as_str());
            assert_eq!(
                ty,
                Some("object"),
                "tool '{}' inputSchema.type must be \"object\", got {:?}",
                tool.name,
                tool.input_schema.get("type"),
            );
        }
    }
}
