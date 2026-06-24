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

use std::collections::BTreeSet;
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

/// Walk `v` recursively and collect every `$defs/<name>` referenced by a
/// `"$ref": "#/$defs/<name>"` string anywhere in the value tree.
fn collect_ref_names(v: &Value, acc: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(r)) = m.get("$ref")
                && let Some(n) = r.strip_prefix("#/$defs/")
            {
                acc.insert(n.to_string());
            }
            for vv in m.values() {
                collect_ref_names(vv, acc);
            }
        }
        Value::Array(a) => {
            for vv in a {
                collect_ref_names(vv, acc);
            }
        }
        _ => {}
    }
}

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
/// when the typed enum deserializes the call). `$defs` is pruned to only the
/// entries actually referenced (transitively) by the flat `properties`, removing
/// the now-unreachable `Mcp*Params` defs that were only used by the removed
/// `oneOf` branches. (Regression: 0.1.55/0.1.56 shipped a root-oneOf schema
/// and zeroed the compact surface live.)
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
    debug_assert!(
        !actions.is_empty(),
        "compact facade flatten produced no actions -- schemars branch shape may have changed"
    );
    props.insert(
        "action".to_string(),
        serde_json::json!({
            "type": "string",
            "enum": actions,
            "description": "The operation. Prefer \"run_and_watch\" to run a command and get its signals + exit in ONE call.",
        }),
    );
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(props.clone()));
    schema.insert(
        "required".to_string(),
        Value::Array(vec![Value::String("action".to_string())]),
    );

    // Prune $defs to only transitively reachable entries from the flat
    // properties. Seed from $refs in the built props, then fixpoint-expand
    // through each retained def's own internal $refs until stable.
    if !defs.is_empty() {
        let mut reachable: BTreeSet<String> = BTreeSet::new();
        collect_ref_names(&Value::Object(props), &mut reachable);
        // Fixpoint: expand through retained defs' own $refs.
        loop {
            let prev_len = reachable.len();
            let snapshot = reachable.clone();
            for name in &snapshot {
                if let Some(def_val) = defs.get(name) {
                    collect_ref_names(def_val, &mut reachable);
                }
            }
            if reachable.len() == prev_len {
                break;
            }
        }
        if reachable.is_empty() {
            schema.remove("$defs");
        } else {
            let pruned: Map<String, Value> = defs
                .into_iter()
                .filter(|(k, _)| reachable.contains(k))
                .collect();
            schema.insert("$defs".to_string(), Value::Object(pruned));
        }
    }

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

    /// A0/A1: strict schema-contract test.
    /// Each compact tool must have `type=="object"`, no root `oneOf`/`anyOf`/`allOf`,
    /// `required==["action"]` with every required name in properties, and
    /// `properties.action.enum` must be a non-empty string array.
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
            let name = &tool.name;

            // type == "object"
            assert_eq!(
                s.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "tool '{name}': inputSchema.type must be \"object\"",
            );

            // No root oneOf / anyOf / allOf
            assert!(
                s.get("oneOf").is_none(),
                "tool '{name}': schema must NOT have a root oneOf (MCP clients drop it)",
            );
            assert!(
                s.get("anyOf").is_none(),
                "tool '{name}': schema must NOT have a root anyOf",
            );
            assert!(
                s.get("allOf").is_none(),
                "tool '{name}': schema must NOT have a root allOf",
            );

            let props = s
                .get("properties")
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| panic!("tool '{name}': missing root properties"));

            // required is exactly ["action"] and action exists in properties
            let required = s
                .get("required")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| panic!("tool '{name}': missing required array"));
            assert_eq!(
                required.len(),
                1,
                "tool '{name}': required must have exactly one entry",
            );
            let req_name = required[0]
                .as_str()
                .unwrap_or_else(|| panic!("tool '{name}': required[0] must be a string"));
            assert_eq!(
                req_name, "action",
                "tool '{name}': required[0] must be \"action\"",
            );
            assert!(
                props.contains_key(req_name),
                "tool '{name}': required name '{req_name}' must exist in properties",
            );

            // properties.action.enum is a non-empty string array
            let action_enum = props
                .get("action")
                .and_then(|a| a.get("enum"))
                .and_then(|e| e.as_array())
                .unwrap_or_else(|| {
                    panic!("tool '{name}': properties.action.enum must be an array")
                });
            assert!(
                !action_enum.is_empty(),
                "tool '{name}': properties.action.enum must be non-empty",
            );
            for (i, v) in action_enum.iter().enumerate() {
                assert!(
                    v.is_string(),
                    "tool '{name}': properties.action.enum[{i}] must be a string, got {v:?}",
                );
            }
        }
    }

    /// A1 command-specific: `properties.action.enum` must be exactly the 15 known verbs.
    #[test]
    fn command_facade_action_enum_is_exactly_15_verbs() {
        const EXPECTED: &[&str] = &[
            "run",
            "run_and_watch",
            "exec",
            "status",
            "output_tail",
            "stop",
            "events",
            "wait",
            "summary",
            "event_context",
            "sub_open",
            "sub_pull",
            "sub_seek",
            "sub_close",
            "sub_list",
        ];

        let tools = compact_surface_tools();
        let command_tool = tools
            .iter()
            .find(|t| t.name.as_ref() == "command")
            .expect("command tool must be in compact_surface_tools");

        let got_enum = command_tool
            .input_schema
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("enum"))
            .and_then(|e| e.as_array())
            .expect("command tool must have properties.action.enum");

        let got_set: BTreeSet<&str> = got_enum.iter().filter_map(|v| v.as_str()).collect();
        let expected_set: BTreeSet<&str> = EXPECTED.iter().copied().collect();

        assert_eq!(got_set, expected_set, "command facade action.enum mismatch");
        let got_len = got_enum.len();
        let exp_len = EXPECTED.len();
        assert_eq!(
            got_len, exp_len,
            "command facade action.enum has {got_len} entries but expected {exp_len} (possible duplicates)",
        );
    }

    /// Compute a shape key for a JSON Schema fragment.
    /// Ignores description, default, format, minimum -- only structural type
    /// and items shape matter for collision detection.
    fn shape_key(v: &Value) -> (String, String) {
        let type_str = v
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let items_shape = v.get("items").map_or_else(String::new, |items| {
            // items $ref takes priority over items.type
            items.get("$ref").and_then(Value::as_str).map_or_else(
                || {
                    items
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                },
                str::to_owned,
            )
        });
        (type_str, items_shape)
    }

    /// A2/A3: flatten is lossless + collision-safe.
    /// Re-derive per-action fields from the raw pre-flatten schema and audit.
    #[test]
    fn command_facade_flatten_is_lossless_and_collision_safe() {
        use crate::facades::CommandFacadeCall;

        let raw_schema = serde_json::to_value(schemars::schema_for!(CommandFacadeCall))
            .expect("CommandFacadeCall schema serializes");

        let raw_defs = raw_schema
            .get("$defs")
            .and_then(|d| d.as_object())
            .expect("raw schema must have $defs");

        let one_of = raw_schema
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("raw CommandFacadeCall schema must have a root oneOf");

        // Build the flat schema via the real function.
        let flat_schema_map = flatten_facade_schema(
            raw_schema
                .as_object()
                .expect("schema root is object")
                .clone(),
        );
        let flat_props = flat_schema_map
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("flat schema must have properties");

        // Track per-field shapes seen so far: field_name -> (shape_key, action_name).
        let mut field_shapes: std::collections::HashMap<String, ((String, String), String)> =
            std::collections::HashMap::new();

        for branch in one_of {
            // Get the action name from the branch's const. A branch whose
            // action const does NOT resolve means the schemars layout no longer
            // matches the sibling-`$ref` + `action.const` assumption this audit
            // (and the production flatten) rely on -- fail LOUD rather than skip
            // it and pass vacuously.
            let action_name = branch
                .get("properties")
                .and_then(|p| p.get("action"))
                .and_then(|a| a.get("const"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "oneOf branch has no resolvable action.const -- schemars branch \
                         shape may have changed: {branch:?}"
                    )
                })
                .to_string();

            // Resolve the $ref to the param struct's properties. Same contract:
            // a branch whose $ref'd param struct has no `properties` must FAIL
            // the audit, not be silently skipped.
            let param_props = branch
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.rsplit('/').next())
                .and_then(|name| raw_defs.get(name))
                .and_then(|d| d.get("properties"))
                .and_then(Value::as_object);
            assert!(
                param_props.is_some(),
                "action '{action_name}': $ref'd param struct did not resolve to a \
                 properties object -- schemars branch shape may have changed: {branch:?}",
            );
            let param_props = param_props.expect("asserted Some above");

            for (field_name, field_schema) in param_props {
                // LOSSLESS: field must appear in the flat schema's properties.
                assert!(
                    flat_props.contains_key(field_name),
                    "LOSSLESS FAIL: field '{field_name}' from action '{action_name}' \
                     is missing from flat schema properties",
                );

                // COLLISION-SAFE: if this field appeared before with a different
                // shape key, fail loudly.
                let this_shape = shape_key(field_schema);
                if let Some((prev_shape, prev_action)) = field_shapes.get(field_name) {
                    assert_eq!(
                        &this_shape, prev_shape,
                        "COLLISION FAIL: field '{field_name}' appears in actions \
                         '{prev_action}' and '{action_name}' with different shapes: \
                         {prev_shape:?} vs {this_shape:?}. The flatten's first-wins \
                         will silently drop the second action's shape.",
                    );
                } else {
                    field_shapes.insert(field_name.clone(), (this_shape, action_name.clone()));
                }
            }
        }
    }

    /// B1 test: `$defs` in the flat command schema are lean (no `Mcp*Params` structs,
    /// every retained key is actually referenced).
    #[test]
    fn command_facade_flat_defs_are_lean() {
        // The 15 Mcp*Params struct names that should be pruned after flattening.
        const PARAM_STRUCT_NAMES: &[&str] = &[
            "McpCommandStartParams",
            "McpRunAndWatchParams",
            "McpShellExecParams",
            "McpCommandStatusParams",
            "McpCommandOutputTailParams",
            "McpCommandStopParams",
            "McpBucketEventsSinceParams",
            "McpBucketWaitParams",
            "McpBucketSummaryParams",
            "McpEventContextParams",
            "McpSubscriptionOpenParams",
            "McpSubscriptionPullParams",
            "McpSubscriptionSeekParams",
            "McpSubscriptionCloseParams",
            "McpSubscriptionListParams",
        ];

        let tools = compact_surface_tools();
        let command_tool = tools
            .iter()
            .find(|t| t.name.as_ref() == "command")
            .expect("command tool must be in compact_surface_tools");

        let s = &command_tool.input_schema;

        // Serialize the whole schema to a string for $ref scanning.
        let schema_str =
            serde_json::to_string(s.as_ref()).expect("schema must serialize to string");

        if let Some(defs) = s.get("$defs").and_then(|d| d.as_object()) {
            // None of the Mcp*Params struct names should be in $defs.
            for name in PARAM_STRUCT_NAMES {
                assert!(
                    !defs.contains_key(*name),
                    "$defs must NOT contain '{name}' after pruning \
                     (it is unreachable after flatten)",
                );
            }

            // Every retained $defs key must be referenced somewhere in the schema.
            for key in defs.keys() {
                let ref_str = format!("#/$defs/{key}");
                assert!(
                    schema_str.contains(&ref_str),
                    "$defs key '{key}' is retained but not referenced anywhere \
                     in the schema (ref '{ref_str}' not found)",
                );
            }
        }
        // If $defs is absent entirely that is also fine (all refs were inlined).
    }
}
