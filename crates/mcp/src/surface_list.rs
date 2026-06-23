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

use rmcp::ErrorData as McpError;
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::Tool;

use crate::surface::Surface;

/// Description for the `command` facade. Kept identical to the `#[tool]`
/// attribute on `command_facade` in `tools.rs` so the compact-surface entry
/// matches the router-advertised entry verbatim.
const COMMAND_FACADE_DESCRIPTION: &str = "Run and observe a one-shot command. To run a command and \
get its result in ONE call, use action=\"run_and_watch\" (it does start + bounded \
wait + collect for you). Other actions: run, exec, status, output_tail, stop, \
events, wait, summary, event_context, sub_open, sub_pull, sub_seek, sub_close, \
sub_list. For an interactive shell, see the `session` facade.";

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
    Tool::new(name, description, schema_for_type::<T>())
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
}
