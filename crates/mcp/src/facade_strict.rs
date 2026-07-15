// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! US1 facade strictness validation.
//!
//! The five compact-surface facade tools (`command`, `session`, `files`,
//! `registry`, `status`) accept a flat `{"action": "...", ...fields}` object.
//! Serde's derived deserializer reports only the FIRST missing required field
//! and SILENTLY DROPS unknown fields (internally-tagged enums cannot use
//! `deny_unknown_fields`). This module adds a validation pass run BEFORE the
//! router deserializes the action enum, so a malformed call gets ONE teaching
//! error naming the action, EVERY missing required field, and EVERY unknown
//! field -- with a same-purpose counterpart named as the remedy where one
//! exists.
//!
//! Source of truth: the SAME schemars-generated facade schema `tools/list`
//! advertises. Per-action `required`/`properties` are harvested from the raw
//! internally-tagged-enum schema (`oneOf` branches -> `$defs` param structs),
//! so the validator is self-updating for every field/action added elsewhere and
//! can never drift from the advertised contract (guarded by
//! `facade_validator_derives_from_advertised_schema`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use rmcp::ErrorData as McpError;
use schemars::JsonSchema;
use serde_json::Value;

/// Per-action allowed/required field sets, derived from the advertised schema.
struct ActionSchema {
    /// Field names the action's param struct marks `required[]`.
    required: BTreeSet<String>,
    /// Every field name the action's param struct advertises in `properties`.
    properties: BTreeSet<String>,
}

/// One facade's action map, keyed by the `action` verb (sorted).
struct FacadeSchema {
    actions: BTreeMap<String, ActionSchema>,
}

/// Runtime-only aliases that are deliberately absent from the advertised schema
/// but must still be accepted (FR-003). Keyed by `(facade, action)`; each entry
/// is `(alias, canonical_field)`. The alias both passes the unknown-field check
/// AND satisfies its canonical field's required-ness.
///
/// Exactly one pair exists today: `samples` -> `sample_lines` on registry
/// `suggest_from_samples` (schema advertises only `sample_lines`). `rules_json`
/// is an advertised schema property and needs NO special-casing.
fn action_aliases(facade: &str, action: &str) -> &'static [(&'static str, &'static str)] {
    match (facade, action) {
        ("registry", "suggest_from_samples") => &[("samples", "sample_lines")],
        _ => &[],
    }
}

/// Static counterpart table: an unknown field on an action whose same-purpose
/// twin is the field the caller almost certainly meant. Returns the counterpart
/// NAME; it is only surfaced when that name is a live property of the chosen
/// action (checked by the caller), so entries for fields a later story adds
/// activate only once those fields ship.
fn counterpart(action: &str, field: &str) -> Option<&'static str> {
    match (action, field) {
        ("sub_pull" | "wait", "wait_ms") => Some("timeout_ms"),
        ("run_and_watch" | "sh_exec" | "pty_stdin", "timeout_ms") => Some("wait_ms"),
        _ => None,
    }
}

/// Build a facade's per-action field map from its raw schemars schema. The enum
/// is `#[serde(tag = "action")]`, so the raw schema is a root `oneOf`; each
/// branch carries `properties.action.const` and a `$ref` to the param struct in
/// `$defs`, where the real `properties`/`required` live.
fn build_facade_schema<T: JsonSchema>() -> FacadeSchema {
    let schema =
        serde_json::to_value(schemars::schema_for!(T)).expect("facade schema serializes to JSON");
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let one_of = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut actions = BTreeMap::new();
    for branch in &one_of {
        let Some(action) = branch
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("const"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        // Unit variants have no `$ref` -> empty properties/required, which is
        // correct: they take only the `action` tag.
        let (properties, required) = branch
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.rsplit('/').next())
            .and_then(|name| defs.get(name))
            .and_then(Value::as_object)
            .map(|def| {
                let properties = def
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|p| p.keys().cloned().collect::<BTreeSet<String>>())
                    .unwrap_or_default();
                let required = def
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|r| {
                        r.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<BTreeSet<String>>()
                    })
                    .unwrap_or_default();
                (properties, required)
            })
            .unwrap_or_default();
        actions.insert(
            action.to_owned(),
            ActionSchema {
                required,
                properties,
            },
        );
    }
    FacadeSchema { actions }
}

/// Cached per-facade schema. `None` for any name that is not one of the five
/// facades (the caller only passes facade names; this is defensive).
fn facade_schema(facade: &str) -> Option<&'static FacadeSchema> {
    static COMMAND: OnceLock<FacadeSchema> = OnceLock::new();
    static SESSION: OnceLock<FacadeSchema> = OnceLock::new();
    static FILES: OnceLock<FacadeSchema> = OnceLock::new();
    static REGISTRY: OnceLock<FacadeSchema> = OnceLock::new();
    static STATUS: OnceLock<FacadeSchema> = OnceLock::new();
    match facade {
        "command" => {
            Some(COMMAND.get_or_init(build_facade_schema::<crate::facades::CommandFacadeCall>))
        }
        "session" => {
            Some(SESSION.get_or_init(build_facade_schema::<crate::facades::SessionFacadeCall>))
        }
        "files" => Some(FILES.get_or_init(build_facade_schema::<crate::facades::FilesFacadeCall>)),
        "registry" => {
            Some(REGISTRY.get_or_init(build_facade_schema::<crate::facades::RegistryFacadeCall>))
        }
        "status" => {
            Some(STATUS.get_or_init(build_facade_schema::<crate::facades::StatusFacadeCall>))
        }
        _ => None,
    }
}

/// The valid actions of a facade, comma-joined and sorted (BTreeMap order).
fn action_list(schema: &FacadeSchema) -> String {
    schema
        .actions
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The first sibling action (sorted) that DOES accept `field`, if any.
fn sibling_with_field<'a>(schema: &'a FacadeSchema, action: &str, field: &str) -> Option<&'a str> {
    schema
        .actions
        .iter()
        .find(|(name, a)| name.as_str() != action && a.properties.contains(field))
        .map(|(name, _)| name.as_str())
}

fn invalid(message: String) -> McpError {
    McpError::invalid_params(message, None)
}

/// Compose the single aggregate error naming the action, every missing required
/// field, and every unknown field with its remedy.
fn aggregate_error(
    action: &str,
    action_schema: &ActionSchema,
    schema: &FacadeSchema,
    missing: &[&str],
    unknown: &[&str],
) -> McpError {
    let mut clauses: Vec<String> = Vec::new();
    if !missing.is_empty() {
        clauses.push(format!(
            "is missing required field(s): {}",
            missing.join(", ")
        ));
    }
    if !unknown.is_empty() {
        let mut clause = format!("does not accept field(s): {}", unknown.join(", "));
        let mut remedies: Vec<String> = Vec::new();
        for &f in unknown {
            if let Some(cp) = counterpart(action, f)
                && action_schema.properties.contains(cp)
            {
                remedies.push(format!(
                    "Did you mean {cp} (the bounded-wait field for {action})?"
                ));
                continue;
            }
            if let Some(sib) = sibling_with_field(schema, action, f) {
                remedies.push(format!("Field '{f}' is accepted by action '{sib}'."));
            }
        }
        if !remedies.is_empty() {
            clause.push(' ');
            clause.push_str(&remedies.join(" "));
        }
        clauses.push(clause);
    }
    invalid(format!("action '{action}' {}", clauses.join("; ")))
}

/// Validate a raw facade call object against the advertised action schema.
///
/// Returns `Ok(())` when the call is well-formed for its action -- the router
/// then deserializes and dispatches EXACTLY as before (byte-identical). On a
/// violation it returns a single `invalid_params` error aggregating ALL missing
/// required fields and ALL unknown-for-action fields, naming the action.
///
/// # Errors
///
/// Returns `invalid_params` when: the call is not a JSON object; `action` is
/// absent or not a known verb (the error lists the facade's valid actions); a
/// required field is missing; or a field is not consumed by the chosen action.
pub fn validate_facade_call(facade: &str, call: &Value) -> Result<(), McpError> {
    let Some(schema) = facade_schema(facade) else {
        // Not a strict facade -> nothing to validate.
        return Ok(());
    };

    let Some(obj) = call.as_object() else {
        return Err(invalid(format!(
            "the '{facade}' facade requires a JSON object with an 'action' field; valid actions: {}",
            action_list(schema)
        )));
    };

    let action = match obj.get("action") {
        None => {
            return Err(invalid(format!(
                "the '{facade}' facade requires an 'action' field; valid actions: {}",
                action_list(schema)
            )));
        }
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(invalid(format!(
                "unknown action {other} for the '{facade}' facade; valid actions: {}",
                action_list(schema)
            )));
        }
    };

    let Some(action_schema) = schema.actions.get(action) else {
        return Err(invalid(format!(
            "unknown action '{action}' for the '{facade}' facade; valid actions: {}",
            action_list(schema)
        )));
    };

    let aliases = action_aliases(facade, action);

    // Missing required fields (an alias satisfies its canonical field).
    let mut missing: Vec<&str> = Vec::new();
    for req in &action_schema.required {
        let satisfied = obj.contains_key(req)
            || aliases
                .iter()
                .any(|&(alias, canon)| canon == req.as_str() && obj.contains_key(alias));
        if !satisfied {
            missing.push(req.as_str());
        }
    }

    // Unknown fields (not a property, not the `action` tag, not an alias).
    let mut unknown: Vec<&str> = Vec::new();
    for key in obj.keys() {
        if key == "action" || action_schema.properties.contains(key) {
            continue;
        }
        if aliases.iter().any(|&(alias, _)| alias == key.as_str()) {
            continue;
        }
        unknown.push(key.as_str());
    }

    if missing.is_empty() && unknown.is_empty() {
        return Ok(());
    }
    Err(aggregate_error(
        action,
        action_schema,
        schema,
        &missing,
        &unknown,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn facade_missing_fields_are_reported_all_at_once_with_action() {
        // command `wait` requires BOTH bucket_id and cursor. Omit both -> one
        // error naming the action AND every missing field (not just the first).
        let err = validate_facade_call("command", &json!({ "action": "wait" }))
            .expect_err("missing required fields must error");
        let msg = err.message.to_string();
        assert!(msg.contains("wait"), "must name the action; got: {msg}");
        assert!(msg.contains("bucket_id"), "must name bucket_id; got: {msg}");
        assert!(msg.contains("cursor"), "must name cursor; got: {msg}");
        assert!(msg.contains("missing"), "must say missing; got: {msg}");
    }

    #[test]
    fn facade_unknown_field_is_rejected_with_counterpart_remedy() {
        let err = validate_facade_call(
            "command",
            &json!({ "action": "sub_pull", "sub_id": "sub_x", "wait_ms": 30000 }),
        )
        .expect_err("unknown-for-action field must error");
        let msg = err.message.to_string();
        assert!(msg.contains("sub_pull"), "must name the action; got: {msg}");
        assert!(msg.contains("wait_ms"), "must name the field; got: {msg}");
        assert!(
            msg.contains("does not accept"),
            "must reject the field; got: {msg}"
        );
        assert!(
            msg.contains("timeout_ms"),
            "must name the counterpart remedy; got: {msg}"
        );
    }

    #[test]
    fn facade_sub_pull_wait_ms_names_timeout_ms_as_remedy() {
        let err = validate_facade_call(
            "command",
            &json!({ "action": "sub_pull", "sub_id": "sub_x", "wait_ms": 1 }),
        )
        .expect_err("wait_ms is not a sub_pull field");
        let msg = err.message.to_string();
        assert!(
            msg.contains("wait_ms"),
            "names the unknown field; got: {msg}"
        );
        assert!(
            msg.contains("timeout_ms"),
            "names timeout_ms as the remedy; got: {msg}"
        );
    }

    #[test]
    fn facade_unknown_action_lists_valid_actions() {
        let err = validate_facade_call("registry", &json!({ "action": "frobnicate" }))
            .expect_err("unknown action must error");
        let msg = err.message.to_string();
        assert!(
            msg.contains("frobnicate"),
            "echoes the bad value; got: {msg}"
        );
        assert!(msg.contains("registry"), "names the facade; got: {msg}");
        // A representative sample of the valid actions must be listed.
        for a in [
            "activate",
            "deactivate",
            "import_pack",
            "suggest_from_samples",
        ] {
            assert!(msg.contains(a), "must list valid action '{a}'; got: {msg}");
        }
    }

    #[test]
    fn facade_valid_calls_are_byte_identical_after_validation() {
        use crate::facades::{
            CommandFacadeCall, RegistryFacadeCall, SessionFacadeCall, StatusFacadeCall,
        };

        // Each valid call: passes validation AND deserializes into the same
        // variant it would have without the validation gate.
        let run = json!({ "action": "run_and_watch", "argv": ["echo", "hi"] });
        validate_facade_call("command", &run).expect("valid run_and_watch");
        assert!(matches!(
            serde_json::from_value::<CommandFacadeCall>(run).unwrap(),
            CommandFacadeCall::RunAndWatch(_)
        ));

        let wait = json!({
            "action": "wait",
            "bucket_id": "bkt_x",
            "cursor": 0,
            "max_signals": 30
        });
        validate_facade_call("command", &wait).expect("valid wait");
        assert!(matches!(
            serde_json::from_value::<CommandFacadeCall>(wait).unwrap(),
            CommandFacadeCall::Wait(_)
        ));

        let output_tail = json!({
            "action": "output_tail",
            "job_id": "job_x",
            "max_lines": 30,
            "max_bytes": 8_000,
            "strip_ansi": true
        });
        validate_facade_call("command", &output_tail).expect("valid output_tail");
        assert!(matches!(
            serde_json::from_value::<CommandFacadeCall>(output_tail).unwrap(),
            CommandFacadeCall::OutputTail(_)
        ));

        let deact =
            json!({ "action": "deactivate", "rule_id": "a.b", "scope": { "kind": "global" } });
        validate_facade_call("registry", &deact).expect("valid deactivate");
        assert!(matches!(
            serde_json::from_value::<RegistryFacadeCall>(deact).unwrap(),
            RegistryFacadeCall::Deactivate(_)
        ));

        let health = json!({ "action": "health" });
        validate_facade_call("status", &health).expect("valid health");
        assert!(matches!(
            serde_json::from_value::<StatusFacadeCall>(health).unwrap(),
            StatusFacadeCall::Health
        ));

        let pty_list = json!({ "action": "pty_list" });
        validate_facade_call("session", &pty_list).expect("valid pty_list");
        assert!(matches!(
            serde_json::from_value::<SessionFacadeCall>(pty_list).unwrap(),
            SessionFacadeCall::PtyList
        ));
    }

    #[test]
    fn facade_validator_derives_from_advertised_schema() {
        // Drift guard: the validator's per-action allowed fields are derived
        // from the SAME schemars schema `tools/list` advertises. The union of
        // every action's properties (plus the `action` tag) must equal the flat
        // property set of the advertised compact-surface schema -- proving no
        // hand-maintained field table can drift from the contract.
        for facade in crate::surface_list::COMPACT_TOOL_NAMES {
            let schema = facade_schema(facade).expect("known facade");
            let mut union: BTreeSet<String> = BTreeSet::new();
            for action in schema.actions.values() {
                union.extend(action.properties.iter().cloned());
            }
            union.insert("action".to_owned());

            let tool = crate::surface_list::compact_surface_tools()
                .into_iter()
                .find(|t| t.name.as_ref() == *facade)
                .expect("facade advertised");
            let advertised: BTreeSet<String> = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
                .expect("advertised properties")
                .keys()
                .cloned()
                .collect();

            assert_eq!(
                union, advertised,
                "facade '{facade}': validator fields must equal the advertised flat schema properties"
            );
        }
    }
}
