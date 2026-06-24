// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Compact-surface `tools/list` construction + the admission gate.
//!
//! The compact surface advertises the verb-dispatched facade tools instead of
//! the granular legacy tools. Five facades cover the full tool surface:
//! `command`, `session`, `files`, `registry`, `status`.
//!
//! The facade `Tool` here is built from the SAME schema source the rmcp
//! `#[tool]` macro uses (`schema_for_type`), so each entry advertised
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

/// Description for the `session` facade.
const SESSION_FACADE_DESCRIPTION: &str = "PTY commands and persistent shell sessions. To start a PTY command use \
action=\"pty_start\"; write stdin with pty_stdin; stop with pty_stop; list with pty_list. \
For sticky-cwd sessions: sh_start (requires allow_session), sh_exec, sh_status, sh_stop, sh_list.";

/// Description for the `files` facade.
const FILES_FACADE_DESCRIPTION: &str = "File operations: bounded read (action=\"read\"), substring search, \
atomic write, file-watch start/stop/list, and workspace snapshots \
(snapshot_create, snapshot_apply). All paths must be absolute.";

/// Description for the `registry` facade.
const REGISTRY_FACADE_DESCRIPTION: &str = "Rule registry: search, get, upsert, test (dry-run), activate, deactivate, \
list_active, import_pack (25 built-in packs), suggest_from_samples (heuristic DRAFT proposals). \
Rules comb command output into structured signals.";

/// Description for the `status` facade.
const STATUS_FACADE_DESCRIPTION: &str = "Adapter and daemon status: health ping (action=\"health\"), self_check, \
policy_status, runtime_state (aggregate snapshot), probe_list, probe_status, system_discover, \
target_list, target_probe.";

/// The facade tool names advertised + admitted on the compact surface.
/// KEEP IN SYNC with [`compact_surface_tools`].
pub const COMPACT_TOOL_NAMES: &[&str] = &["command", "files", "registry", "session", "status"];

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
///
/// Unit variants (no `$ref` sibling; just the `action` const) are valid: they
/// contribute only the action verb to the `action` enum and no properties.
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
        // Unit variants have no `$ref` sibling -- they contribute only the
        // action verb above and no properties. That is correct and expected.
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
            "description": "The operation to perform; the remaining fields are this action's parameters. See the tool description for the recommended starting action.",
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
/// Returns the five facade `Tool`s: `command`, `session`, `files`, `registry`,
/// `status`. KEEP IN SYNC with [`COMPACT_TOOL_NAMES`].
#[must_use]
pub fn compact_surface_tools() -> Vec<Tool> {
    vec![
        surface_tool::<crate::facades::CommandFacadeCall>("command", COMMAND_FACADE_DESCRIPTION),
        surface_tool::<crate::facades::FilesFacadeCall>("files", FILES_FACADE_DESCRIPTION),
        surface_tool::<crate::facades::RegistryFacadeCall>("registry", REGISTRY_FACADE_DESCRIPTION),
        surface_tool::<crate::facades::SessionFacadeCall>("session", SESSION_FACADE_DESCRIPTION),
        surface_tool::<crate::facades::StatusFacadeCall>("status", STATUS_FACADE_DESCRIPTION),
    ]
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
        for facade in COMPACT_TOOL_NAMES {
            assert!(
                names.contains(&facade.to_string()),
                "compact surface must include '{facade}'"
            );
        }
        // No granular legacy tool leaks into the compact list:
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("command_") || n == "run_and_watch")
        );
        // Exactly 5 tools (one per facade).
        assert_eq!(names.len(), 5, "compact surface must have exactly 5 tools");
    }

    #[test]
    fn gate_blocks_legacy_under_compact_allows_under_full() {
        // Legacy name is rejected on compact, allowed on full.
        assert!(enforce_surface(Surface::Compact, "command_status").is_err());
        assert!(enforce_surface(Surface::Full, "command_status").is_ok());
        // Each facade name is allowed on both surfaces.
        for facade in COMPACT_TOOL_NAMES {
            assert!(
                enforce_surface(Surface::Compact, facade).is_ok(),
                "facade '{facade}' must be admitted on compact"
            );
            assert!(
                enforce_surface(Surface::Full, facade).is_ok(),
                "facade '{facade}' must be admitted on full"
            );
        }
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

    /// A1: per-facade exact action-enum assertion.
    /// Each facade must advertise EXACTLY the named set of verbs.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn all_facade_action_enums_are_exact() {
        // (tool_name, sorted expected verbs)
        let cases: &[(&str, &[&str])] = &[
            (
                "command",
                &[
                    "event_context",
                    "events",
                    "exec",
                    "output_tail",
                    "run",
                    "run_and_watch",
                    "status",
                    "stop",
                    "sub_close",
                    "sub_list",
                    "sub_open",
                    "sub_pull",
                    "sub_seek",
                    "summary",
                    "wait",
                ],
            ),
            (
                "session",
                &[
                    "pty_list",
                    "pty_start",
                    "pty_stdin",
                    "pty_stop",
                    "sh_exec",
                    "sh_list",
                    "sh_start",
                    "sh_status",
                    "sh_stop",
                ],
            ),
            (
                "files",
                &[
                    "read",
                    "search",
                    "snapshot_apply",
                    "snapshot_create",
                    "watch_list",
                    "watch_start",
                    "watch_stop",
                    "write",
                ],
            ),
            (
                "registry",
                &[
                    "activate",
                    "deactivate",
                    "get",
                    "import_pack",
                    "list_active",
                    "search",
                    "suggest_from_samples",
                    "test",
                    "upsert",
                ],
            ),
            (
                "status",
                &[
                    "health",
                    "policy_status",
                    "probe_list",
                    "probe_status",
                    "runtime_state",
                    "self_check",
                    "system_discover",
                    "target_list",
                    "target_probe",
                ],
            ),
        ];

        let tools = compact_surface_tools();

        for (tool_name, expected_verbs) in cases {
            let tool = tools
                .iter()
                .find(|t| t.name.as_ref() == *tool_name)
                .unwrap_or_else(|| panic!("tool '{tool_name}' must be in compact_surface_tools"));

            let got_enum = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.get("action"))
                .and_then(|a| a.get("enum"))
                .and_then(|e| e.as_array())
                .unwrap_or_else(|| panic!("tool '{tool_name}' must have properties.action.enum"));

            let got_set: BTreeSet<&str> = got_enum.iter().filter_map(|v| v.as_str()).collect();
            let expected_set: BTreeSet<&str> = expected_verbs.iter().copied().collect();

            assert_eq!(
                got_set, expected_set,
                "tool '{tool_name}': action.enum set mismatch"
            );
            assert_eq!(
                got_enum.len(),
                expected_verbs.len(),
                "tool '{tool_name}': action.enum has {} entries but expected {} \
                 (possible duplicates)",
                got_enum.len(),
                expected_verbs.len(),
            );
        }
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

    /// A2/A3: flatten is lossless + collision-safe for all 5 facades.
    ///
    /// Re-derives per-action fields from the raw pre-flatten schema and audits:
    /// - Every non-unit action's param fields appear in the flat schema.
    /// - No two actions share a field name with a different structural shape.
    /// - Unit variants (action const present, no $ref) are valid and skipped for
    ///   param checks rather than failing the audit.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn all_facades_flatten_is_lossless_and_collision_safe() {
        use crate::facades::{
            CommandFacadeCall, FilesFacadeCall, RegistryFacadeCall, SessionFacadeCall,
            StatusFacadeCall,
        };

        fn check_facade<T: schemars::JsonSchema>(facade_name: &str) {
            let raw_schema = serde_json::to_value(schemars::schema_for!(T))
                .unwrap_or_else(|e| panic!("{facade_name}: schema serializes: {e}"));

            let raw_defs = raw_schema
                .get("$defs")
                .and_then(|d| d.as_object())
                .cloned()
                .unwrap_or_default();

            let one_of = raw_schema
                .get("oneOf")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| panic!("{facade_name}: raw schema must have a root oneOf"));

            let flat_schema_map = super::flatten_facade_schema(
                raw_schema
                    .as_object()
                    .expect("schema root is object")
                    .clone(),
            );
            let flat_props = flat_schema_map
                .get("properties")
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| panic!("{facade_name}: flat schema must have properties"));

            let mut field_shapes: std::collections::HashMap<String, ((String, String), String)> =
                std::collections::HashMap::new();

            for branch in one_of {
                // Every branch MUST have an action const -- a missing one means
                // the schemars layout no longer matches the assumption. Fail loud.
                let action_name = branch
                    .get("properties")
                    .and_then(|p| p.get("action"))
                    .and_then(|a| a.get("const"))
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| {
                        panic!(
                            "{facade_name}: oneOf branch has no resolvable action.const -- \
                             schemars branch shape may have changed: {branch:?}"
                        )
                    })
                    .to_string();

                // Try to resolve the $ref to the param struct. A branch with NO
                // $ref is a valid unit variant -- skip the param property checks.
                let param_props = branch
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|r| r.rsplit('/').next())
                    .and_then(|name| raw_defs.get(name))
                    .and_then(|d| d.get("properties"))
                    .and_then(Value::as_object);

                // Unit variant: no $ref, no param check. Only confirm the action
                // const resolved (already asserted above). Skip property checks.
                let Some(param_props) = param_props else {
                    // Confirm there is genuinely no $ref (unit variant), not a
                    // broken $ref that failed to resolve.
                    assert!(
                        branch.get("$ref").is_none(),
                        "{facade_name}: action '{action_name}': branch has a $ref that \
                         did not resolve to a properties object -- schemars branch shape \
                         may have changed: {branch:?}",
                    );
                    // Unit variant: valid, nothing to check for param collision.
                    continue;
                };

                for (field_name, field_schema) in param_props {
                    // LOSSLESS: field must appear in the flat schema's properties.
                    assert!(
                        flat_props.contains_key(field_name),
                        "{facade_name}: LOSSLESS FAIL: field '{field_name}' from action \
                         '{action_name}' is missing from flat schema properties",
                    );

                    // COLLISION-SAFE: same field from two actions must have the
                    // same structural shape.
                    let this_shape = shape_key(field_schema);
                    if let Some((prev_shape, prev_action)) = field_shapes.get(field_name) {
                        assert_eq!(
                            &this_shape, prev_shape,
                            "{facade_name}: COLLISION FAIL: field '{field_name}' appears in \
                             actions '{prev_action}' and '{action_name}' with different shapes: \
                             {prev_shape:?} vs {this_shape:?}. The flatten's first-wins will \
                             silently drop the second action's shape.",
                        );
                    } else {
                        field_shapes.insert(field_name.clone(), (this_shape, action_name.clone()));
                    }
                }
            }
        }

        check_facade::<CommandFacadeCall>("command");
        check_facade::<SessionFacadeCall>("session");
        check_facade::<FilesFacadeCall>("files");
        check_facade::<RegistryFacadeCall>("registry");
        check_facade::<StatusFacadeCall>("status");
    }

    /// B1 test: `$defs` in every flat facade schema are lean.
    /// No `Mcp*Params` wrapper structs remain; every retained key is referenced.
    #[test]
    fn all_facades_flat_defs_are_lean() {
        // The Mcp*Params struct names used by CommandFacadeCall (must be pruned).
        const COMMAND_PARAM_STRUCT_NAMES: &[&str] = &[
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

        for tool in compact_surface_tools() {
            let name = &tool.name;
            let s = &tool.input_schema;
            let schema_str =
                serde_json::to_string(s.as_ref()).expect("schema must serialize to string");

            if let Some(defs) = s.get("$defs").and_then(|d| d.as_object()) {
                // command-specific: the Mcp*Params wrappers must be pruned.
                if name.as_ref() == "command" {
                    for param_name in COMMAND_PARAM_STRUCT_NAMES {
                        assert!(
                            !defs.contains_key(*param_name),
                            "tool '{name}': $defs must NOT contain '{param_name}' after pruning",
                        );
                    }
                }

                // For all facades: every retained $defs key must be referenced.
                for key in defs.keys() {
                    let ref_str = format!("#/$defs/{key}");
                    assert!(
                        schema_str.contains(&ref_str),
                        "tool '{name}': $defs key '{key}' is retained but not referenced \
                         anywhere in the schema (ref '{ref_str}' not found)",
                    );
                }
            }
            // If $defs is absent entirely that is also fine.
        }
    }

    /// Backward-compat: the old test names that the test harness expects
    /// (they now delegate to the generalized helpers above).
    #[test]
    fn command_facade_action_enum_is_exactly_15_verbs() {
        // Covered by all_facade_action_enums_are_exact, but kept for CI name
        // stability: the command facade must have exactly 15 verbs.
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
        assert_eq!(
            got_enum.len(),
            15,
            "command facade must have exactly 15 verbs"
        );
    }

    #[test]
    fn command_facade_flatten_is_lossless_and_collision_safe() {
        // Delegate to the generalized all-facades test for the command facade.
        // Kept for CI name stability.
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

        let mut field_shapes: std::collections::HashMap<String, ((String, String), String)> =
            std::collections::HashMap::new();

        for branch in one_of {
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

            let param_props = branch
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.rsplit('/').next())
                .and_then(|name| raw_defs.get(name))
                .and_then(|d| d.get("properties"))
                .and_then(Value::as_object);

            // For CommandFacadeCall all variants have params; unit variants in
            // other facades are handled by all_facades_flatten_is_lossless_and_collision_safe.
            assert!(
                param_props.is_some(),
                "action '{action_name}': $ref'd param struct did not resolve to a \
                 properties object -- schemars branch shape may have changed: {branch:?}",
            );
            let param_props = param_props.expect("asserted Some above");

            for (field_name, field_schema) in param_props {
                assert!(
                    flat_props.contains_key(field_name),
                    "LOSSLESS FAIL: field '{field_name}' from action '{action_name}' \
                     is missing from flat schema properties",
                );

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

        let schema_str =
            serde_json::to_string(s.as_ref()).expect("schema must serialize to string");

        if let Some(defs) = s.get("$defs").and_then(|d| d.as_object()) {
            for name in PARAM_STRUCT_NAMES {
                assert!(
                    !defs.contains_key(*name),
                    "$defs must NOT contain '{name}' after pruning \
                     (it is unreachable after flatten)",
                );
            }

            for key in defs.keys() {
                let ref_str = format!("#/$defs/{key}");
                assert!(
                    schema_str.contains(&ref_str),
                    "$defs key '{key}' is retained but not referenced anywhere \
                     in the schema (ref '{ref_str}' not found)",
                );
            }
        }
    }
}
