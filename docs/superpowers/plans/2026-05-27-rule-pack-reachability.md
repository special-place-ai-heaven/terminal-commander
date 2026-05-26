# Rule-Pack Reachability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an LLM agent get expert cargo (etc.) signal extraction in ONE MCP call by naming a rule pack, instead of authoring rule JSON across ~6 calls.

**Architecture:** A new `registry_import_pack` IPC method resolves a pack by name (pack JSON embedded in the daemon binary via `include_str!`), imports its rules through the existing validated importer, and -- when the caller passes `activate:true` + a scope -- writes each rule to the store as `Active` then reuses the existing `handle_registry_activate` path per rule (no fourth copy of activation logic; the draft-poison gate passes because the rules are genuinely Active). An MCP tool forwards 1:1. The cargo pack is thickened from 2 to 6 rules.

**Tech Stack:** Rust workspace (core, store, daemon, mcp). Tests via `cargo nextest`. Daemon IPC tests are `#![cfg(unix)]` and run under WSL2: `wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p <crate> <filter>"` (login shell `-lc` required for cargo PATH).

---

## Background: verified code facts (do not re-derive)

Confirmed 2026-05-27:

- `crates/store/src/import.rs`: `EventStore::import_rule_pack(path)` and `import_rule_pack_str(json)` exist. `RulePackFile { _meta: RulePackMeta { pack, version, description }, rules: Vec<RuleDefinition> }`. `ImportResult { pack, imported: Vec<String>, skipped: Vec<String> }`. Validates each rule, bounded-compiles regexes (`RULE_PACK_REGEX_SIZE_LIMIT`/`DFA` = 65_536), inserts via `create_rule_version`.
- `crates/store/src/registry.rs:117` `create_rule_version(&mut self, def) -> Result<u32>`: auto-increments version (latest+1), rejects tombstoned, validates. Ignores the JSON's stated `version`. Re-import => new version.
- Pack files at repo root `rules/{generic.terminal,apt,cargo,npm,pytest,gcc,make}.json`. From `crates/store/src/`, relative path is `../../../rules/<name>.json`.
- `rules/cargo.json`: 2 rules (`cargo.compile-error`, `cargo.could-not-compile`), both `status:"draft"`, `_meta.pack = "cargo"`.
- `crates/core/src/rule.rs:78` `RuleStatus { Draft, Active, Disabled, Deprecated, Tombstoned }`; `is_runtime_eligible()` = `matches!(self, Active)`.
- `crates/core/src/activation.rs:41` `ActivationScope { Global (default), Bucket{bucket_id}, Job{job_id}, Probe{probe_id} }`, Serialize/Deserialize, Copy.
- `crates/daemon/src/ipc/protocol.rs`: `IpcRequest` enum has `RegistryUpsert(RegistryUpsertParams)` (~134), `RegistryActivate(RegistryActivateParams)` (~140). `IpcResponse` has `RegistryActivate(RegistryActivateResponse)` (~216). `RegistryActivateParams { rule_id: String, version: Option<u32>, scope: Option<ActivationScope> }` (~714).
- `crates/daemon/src/ipc/server.rs:1088` `handle_registry_activate(state, params: &RegistryActivateParams) -> Result<IpcResponse, IpcError>`: requires scope (`ScopeInvalid` if None), looks up the STORED def, rejects non-eligible (`RuleNotActive`), then activates + records + rebinds. Dispatch arm pattern: `IpcRequest::CommandStatus(p) => match handle_command_status(state, p) { Ok(r) => ("command_status", IpcResult::Ok{response:r}), Err(e) => (...Err...) }` (server.rs ~444).
- `handle_registry_upsert(state, params)` (server.rs:998): validates then `g.create_rule_version(&params.definition)`. Stores whatever `status` the def carries.
- MCP tool body pattern (tools.rs:668 `registry_activate`): parse `Parameters<McpRegistryActivateParams>` -> `params.scope.map(into_ipc_scope)` -> build `RegistryActivateParams` -> `self.daemon.call(IpcRequest::RegistryActivate(ipc))` -> match `IpcResponse`. `McpActivationScope { kind: String, bucket_id/job_id/probe_id: Option<String> }` with `into_ipc_scope() -> Result<ActivationScope, _>`.
- `command_status_payload`-style hand-built JSON payloads live at the bottom of tools.rs.

## File Structure

- **Modify** `crates/store/src/import.rs` -- add pack-name resolver (embed JSON, name->json), `import_rule_pack_by_name`, and an `active`-promotion import variant.
- **Modify** `crates/daemon/src/ipc/protocol.rs` -- `RegistryImportPackParams`, `RegistryImportPackResponse`, `IpcRequest::RegistryImportPack`, `IpcResponse::RegistryImportPack`.
- **Modify** `crates/daemon/src/ipc/server.rs` -- `handle_registry_import_pack` + dispatch arm.
- **Modify** `crates/mcp/src/tools.rs` -- `registry_import_pack` tool + `McpRegistryImportPackParams` + payload.
- **Modify** `rules/cargo.json` -- 2 -> 6 rules.
- **Modify** tests: `crates/store/src/import.rs` (#[cfg(test)]), `crates/daemon/tests/registry_ipc.rs`, `crates/mcp/tests/registry_live_e2e.rs`, `tests/fixtures/contracts/mcp-tools/` (+ fixture-map).

---

## Task 1: Pack-name resolver + active-import in the store

**Files:**
- Modify: `crates/store/src/import.rs`
- Test: `crates/store/src/import.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write failing tests**

Add to `mod tests` in import.rs:

```rust
#[test]
fn known_pack_names_resolve_to_json() {
    assert!(resolve_pack_json("cargo").is_some());
    assert!(resolve_pack_json("pytest").is_some());
    assert!(resolve_pack_json("nope").is_none());
}

#[test]
fn known_pack_names_lists_all_seven() {
    let names = known_pack_names();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"cargo"));
    assert!(names.contains(&"generic.terminal"));
}

#[test]
fn import_by_name_active_promotes_status() {
    let mut s = EventStore::in_memory().unwrap();
    let res = s.import_rule_pack_by_name("cargo", true).unwrap();
    assert_eq!(res.pack, "cargo");
    assert!(!res.imported.is_empty());
    // Every imported cargo rule is stored Active when promote=true.
    for id in &res.imported {
        let got = s.get_latest_rule(id).unwrap().unwrap();
        assert_eq!(got.status, terminal_commander_core::RuleStatus::Active);
    }
}

#[test]
fn import_by_name_draft_keeps_status() {
    let mut s = EventStore::in_memory().unwrap();
    let res = s.import_rule_pack_by_name("cargo", false).unwrap();
    for id in &res.imported {
        let got = s.get_latest_rule(id).unwrap().unwrap();
        assert_eq!(got.status, terminal_commander_core::RuleStatus::Draft);
    }
}

#[test]
fn import_by_unknown_name_is_err() {
    let mut s = EventStore::in_memory().unwrap();
    assert!(s.import_rule_pack_by_name("nope", false).is_err());
}
```

- [ ] **Step 2: Run to verify fail**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-store import_by_name resolve_pack known_pack"
```
Expected: FAIL (functions undefined).

- [ ] **Step 3: Implement resolver + promotion import**

Add to import.rs (module level, above `impl EventStore`):

```rust
/// The seven seed packs, embedded so the daemon needs no repo
/// checkout at runtime. Paths are relative to THIS source file
/// (crates/store/src/import.rs -> repo root is ../../../).
const SEED_PACKS: &[(&str, &str)] = &[
    ("generic.terminal", include_str!("../../../rules/generic.terminal.json")),
    ("apt", include_str!("../../../rules/apt.json")),
    ("cargo", include_str!("../../../rules/cargo.json")),
    ("npm", include_str!("../../../rules/npm.json")),
    ("pytest", include_str!("../../../rules/pytest.json")),
    ("gcc", include_str!("../../../rules/gcc.json")),
    ("make", include_str!("../../../rules/make.json")),
];

/// Resolve a pack name to its embedded JSON, or `None` if unknown.
#[must_use]
pub fn resolve_pack_json(name: &str) -> Option<&'static str> {
    SEED_PACKS.iter().find(|(n, _)| *n == name).map(|(_, j)| *j)
}

/// The list of known pack names (for teaching errors).
#[must_use]
pub fn known_pack_names() -> Vec<&'static str> {
    SEED_PACKS.iter().map(|(n, _)| *n).collect()
}
```

Add to `impl EventStore` (after `import_rule_pack_str`):

```rust
/// Import an embedded pack by NAME. When `promote_active` is true,
/// each rule is stored with `status = Active` (so the caller can
/// activate it through the normal gate). When false, rules keep
/// their on-disk status (typically Draft, the vetting path).
///
/// Returns `InvalidPayload` for an unknown pack name, with the
/// known names listed so the caller can self-correct.
pub fn import_rule_pack_by_name(
    &mut self,
    name: &str,
    promote_active: bool,
) -> Result<ImportResult> {
    let json = resolve_pack_json(name).ok_or_else(|| {
        EventStoreError::InvalidPayload(format!(
            "unknown rule pack '{name}'; known packs: {}",
            known_pack_names().join(", ")
        ))
    })?;
    if !promote_active {
        return self.import_rule_pack_str(json);
    }
    // Promote: parse, set every rule Active, import.
    let mut parsed: RulePackFile = sj::from_str(json)?;
    for rule in &mut parsed.rules {
        rule.status = terminal_commander_core::RuleStatus::Active;
    }
    self.import_parsed_pack(parsed)
}
```

Refactor the body of `import_rule_pack_str` so the post-parse loop lives in a private `import_parsed_pack(&mut self, parsed: RulePackFile) -> Result<ImportResult>`, and `import_rule_pack_str` becomes parse-then-`import_parsed_pack` (DRY; the promotion path reuses the same validate + bounded-regex-compile + insert loop). Move lines 67-102's loop into `import_parsed_pack`.

- [ ] **Step 4: Run to verify pass**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-store"
```
Expected: all PASS (new + existing `import_all_seed_packs_from_repo`).

- [ ] **Step 5: fmt + clippy store**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt -p terminal-commander-store && cargo clippy -p terminal-commander-store --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 6: Commit**

Subject: `feat(store): resolve + import rule packs by name with active promotion`

---

## Task 2: `RegistryImportPack` IPC protocol types

**Files:**
- Modify: `crates/daemon/src/ipc/protocol.rs`

- [ ] **Step 1: Add params + response structs**

Near `RegistryActivateParams` (~714) in protocol.rs:

```rust
/// Import a named, embedded rule pack into the registry. When
/// `activate` is true, `scope` is REQUIRED and every imported rule
/// is promoted to Active and activated in that scope (one-call
/// "give me expert signals for X"). When false, rules import at
/// their on-disk status and nothing is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryImportPackParams {
    pub pack: String,
    #[serde(default)]
    pub activate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ActivationScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryImportPackResponse {
    pub pack: String,
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub activated: Vec<String>,
}
```

- [ ] **Step 2: Add enum variants**

In `IpcRequest`, after `RegistryActivate(RegistryActivateParams)`:
```rust
    /// Import an embedded rule pack by name; optionally promote +
    /// activate its rules in one call.
    RegistryImportPack(RegistryImportPackParams),
```
In `IpcResponse`, after `RegistryActivate(RegistryActivateResponse)`:
```rust
    RegistryImportPack(RegistryImportPackResponse),
```

- [ ] **Step 3: Compile-check**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo check -p terminal-commanderd"
```
Expected: FAIL -- non-exhaustive match in `dispatch` (server.rs) for the new request variant. That is Task 3.

- [ ] **Step 4: Commit**

Subject: `feat(daemon): RegistryImportPack IPC request/response types`

---

## Task 3: `handle_registry_import_pack` + dispatch (reuses activate path)

**Files:**
- Modify: `crates/daemon/src/ipc/server.rs`
- Test: `crates/daemon/tests/registry_ipc.rs`

- [ ] **Step 1: Write the failing IPC test**

Add to `crates/daemon/tests/registry_ipc.rs` (match the file's existing harness for building a daemon/client; mirror an existing `registry_activate` test's setup):

```rust
#[test]
fn import_pack_cargo_activates_and_drives_signal() {
    // Mirror the existing registry_ipc harness: build daemon + client.
    with_daemon(|client| async move {
        let resp = client
            .call(
                1,
                IpcRequest::RegistryImportPack(RegistryImportPackParams {
                    pack: "cargo".to_owned(),
                    activate: true,
                    scope: Some(ActivationScope::Global),
                }),
            )
            .await
            .expect("import_pack");
        let r = match resp {
            IpcResponse::RegistryImportPack(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.pack, "cargo");
        assert!(!r.imported.is_empty());
        assert_eq!(r.activated.len(), r.imported.len());
        assert!(r.skipped.is_empty());
    });
}

#[test]
fn import_pack_requires_scope_when_activating() {
    with_daemon(|client| async move {
        let resp = client
            .call(
                1,
                IpcRequest::RegistryImportPack(RegistryImportPackParams {
                    pack: "cargo".to_owned(),
                    activate: true,
                    scope: None,
                }),
            )
            .await
            .expect("call ok");
        match resp {
            IpcResponse::Error { error } | _ if matches!(&resp, _) => {}
        };
        // Assert the typed scope-required error per this file's
        // error-assertion idiom (match IpcResult::Err / IpcError code
        // == ScopeInvalid). Use the same assertion helper the
        // existing activate-without-scope test uses.
    });
}

#[test]
fn import_pack_unknown_name_is_typed_error() {
    with_daemon(|client| async move {
        let resp = client
            .call(
                1,
                IpcRequest::RegistryImportPack(RegistryImportPackParams {
                    pack: "does-not-exist".to_owned(),
                    activate: false,
                    scope: None,
                }),
            )
            .await
            .expect("call ok");
        // Assert InvalidParams/RuleInvalid carrying the known-pack list.
    });
}
```

NOTE before writing: open `registry_ipc.rs`, find the actual harness (e.g. `with_daemon`/`build_server`) and the error-assertion idiom used by `activate_without_scope_is_rejected_and_audited` (in `registry_scope_required.rs` or this file). Match them EXACTLY; the snippets above show intent, not invented helpers. Use the real client `.call` signature.

- [ ] **Step 2: Run to verify fail**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commanderd --test registry_ipc import_pack"
```
Expected: FAIL (handler missing / non-exhaustive dispatch).

- [ ] **Step 3: Implement the handler**

Add to server.rs near `handle_registry_activate`:

```rust
fn handle_registry_import_pack(
    state: &Arc<DaemonState>,
    params: &RegistryImportPackParams,
) -> Result<IpcResponse, IpcError> {
    // If activating, scope is required up front (mirror activate).
    if params.activate && params.scope.is_none() {
        return Err(IpcError::new(
            IpcErrorCode::ScopeInvalid,
            "scope is required when activate=true; pass {kind:'global'} for explicit global activation",
        ));
    }
    // Import (promote to Active iff we will activate).
    let import = {
        let mut g = state.store.lock();
        g.import_rule_pack_by_name(&params.pack, params.activate)
            .map_err(map_store_error)?
    };
    let mut activated = Vec::new();
    if params.activate {
        let scope = params.scope; // Some, checked above
        for rule_id in &import.imported {
            // Reuse the canonical activate path per rule: it looks up
            // the now-Active stored def, passes the eligibility gate,
            // activates, records, and rebinds. No fourth copy.
            let aparams = RegistryActivateParams {
                rule_id: rule_id.clone(),
                version: None, // latest = the just-imported Active version
                scope,
            };
            handle_registry_activate(state, &aparams)?;
            activated.push(rule_id.clone());
        }
    }
    Ok(IpcResponse::RegistryImportPack(RegistryImportPackResponse {
        pack: import.pack,
        imported: import.imported,
        skipped: import.skipped,
        activated,
    }))
}
```

Map the store's `InvalidPayload` for unknown pack to a caller-fixable code: confirm `map_store_error` maps `InvalidPayload` to `RuleInvalid` or `InvalidParams` (caller-fixable). If it maps to `Internal`, add an explicit unknown-pack check BEFORE the store call using `terminal_commander_store::resolve_pack_json(&params.pack).is_none()` -> `IpcError::new(IpcErrorCode::InvalidParams, format!("unknown rule pack '{}'; known packs: {}", params.pack, terminal_commander_store::known_pack_names().join(", ")))`.

- [ ] **Step 4: Add the dispatch arm**

In the `dispatch` match (server.rs ~444, alongside the other `RegistryX` arms):

```rust
        IpcRequest::RegistryImportPack(p) => match handle_registry_import_pack(state, p) {
            Ok(r) => ("registry_import_pack", IpcResult::Ok { response: r }),
            Err(e) => ("registry_import_pack", IpcResult::Err { error: e }),
        },
```

Match the EXACT tuple/label shape the surrounding arms use (audit action label string + `IpcResult`).

- [ ] **Step 5: Run to verify pass**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commanderd --test registry_ipc import_pack"
```
Expected: PASS.

- [ ] **Step 6: fmt + clippy daemon**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt -p terminal-commanderd && cargo clippy -p terminal-commanderd --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 7: Commit**

Subject: `feat(daemon): registry_import_pack handler reusing the activate path`

---

## Task 4: `registry_import_pack` MCP tool

**Files:**
- Modify: `crates/mcp/src/tools.rs`
- Test: `crates/mcp/tests/registry_live_e2e.rs`

- [ ] **Step 1: Write the failing MCP e2e test**

Add to `crates/mcp/tests/registry_live_e2e.rs` (match its existing live-daemon harness):

```rust
#[tokio::test]
async fn import_pack_cargo_through_mcp_activates_rules() {
    // Mirror this file's existing live-MCP harness (spawn daemon +
    // ToolSurface/server, call a tool, read JSON result).
    let h = harness().await;
    let out = h.call_tool(
        "registry_import_pack",
        serde_json::json!({ "pack": "cargo", "activate": true, "scope": { "kind": "global" } }),
    ).await;
    assert_eq!(out["pack"], "cargo");
    assert!(out["activated"].as_array().unwrap().len() >= 6);
    assert!(out["skipped"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn import_pack_unknown_is_teaching_error() {
    let h = harness().await;
    let err = h.call_tool_expect_err(
        "registry_import_pack",
        serde_json::json!({ "pack": "nope", "activate": false }),
    ).await;
    assert!(err.contains("known packs"));
}
```

NOTE: match the real harness/tool-call helpers in this test file; do not invent `harness()`/`call_tool` if the file uses different names.

- [ ] **Step 2: Run to verify fail**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-mcp --test registry_live_e2e import_pack"
```
Expected: FAIL (tool not registered).

- [ ] **Step 3: Add the MCP param struct**

Near `McpRegistryActivateParams` (tools.rs ~1515):

```rust
/// MCP-facing parameters for `registry_import_pack`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct McpRegistryImportPackParams {
    /// Pack name. One of: generic.terminal, apt, cargo, npm, pytest,
    /// gcc, make.
    pub pack: String,
    /// When true, promote the pack's rules to Active and activate them
    /// in `scope` so they take effect immediately. Requires `scope`.
    #[serde(default)]
    pub activate: bool,
    /// Activation scope (required when activate=true). Omit kind for
    /// none. `{ "kind": "global" }` is the usual choice for a single
    /// agent watching its own commands.
    #[serde(default)]
    pub scope: Option<McpActivationScope>,
}
```

- [ ] **Step 4: Add the tool method**

In the `#[tool_router] impl` block (near `registry_activate`, tools.rs ~668):

```rust
    /// `registry_import_pack` -- one-call expert signal extraction.
    #[tool(
        description = "Import a curated rule pack (cargo, pytest, npm, gcc, apt, make, generic.terminal) so you get expert signal extraction without authoring any rule JSON. Pass activate=true + scope {kind:'global'} to make the rules live immediately for your commands. One call replaces ~6 rule-authoring calls. Unknown pack names return the available list."
    )]
    async fn registry_import_pack(
        &self,
        Parameters(params): Parameters<McpRegistryImportPackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available()?;
        let scope = match params.scope {
            Some(s) => Some(s.into_ipc_scope()?),
            None => None,
        };
        let ipc = RegistryImportPackParams {
            pack: params.pack,
            activate: params.activate,
            scope,
        };
        match self.daemon.call(IpcRequest::RegistryImportPack(ipc)).await {
            Ok(IpcResponse::RegistryImportPack(r)) => json_tool_result(&serde_json::json!({
                "pack": r.pack,
                "imported": r.imported,
                "skipped": r.skipped,
                "activated": r.activated,
            })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }
```

Add `RegistryImportPackParams` to the `use terminal_commanderd::ipc::protocol::{...}` import list at the top of tools.rs.

- [ ] **Step 5: Run to verify pass**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-mcp --test registry_live_e2e import_pack"
```
Expected: PASS.

- [ ] **Step 6: Update the tool catalogue + fixture map**

`registry_import_pack` is a new tool. Update wherever the tool catalogue is enumerated so `fixture_catalogue_contract` stays green:
- Find `tool_catalogue()` / `discovered_tools()` (tools.rs) and add the tool name.
- Add `tests/fixtures/contracts/mcp-tools/registry_import_pack.v1.json` mirroring an existing registry tool fixture (request_example, _meta with schema_anchor + fixture_id + status:live, response_example with pack/imported/skipped/activated, invariants), and add its entry to `tests/fixtures/contracts/mcp-tool-fixture-map.v1.json`.

Run:
```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-mcp --test fixture_catalogue_contract"
```
Expected: PASS.

- [ ] **Step 7: fmt + clippy mcp**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt -p terminal-commander-mcp && cargo clippy -p terminal-commander-mcp --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 8: Commit**

Subject: `feat(mcp): registry_import_pack tool + contract fixture`

---

## Task 5: Thicken the cargo pack 2 -> 6 rules

**Files:**
- Modify: `rules/cargo.json`
- Test: `crates/store/src/import.rs` (extend `import_all_seed_packs_from_repo` count) or a sifter fixture test if this file has one.

- [ ] **Step 1: Add four rules to `rules/cargo.json`**

Keep the two existing rules. Add (all `status:"draft"` on disk, promotion happens at import):

```json
    {
      "id": "cargo.warning",
      "version": 1,
      "kind": "regex",
      "status": "draft",
      "severity": "low",
      "event_kind": "compile_warning",
      "stream": "stderr",
      "description": "rustc/clippy warning line.",
      "pattern": "^warning: (?P<message>.+)$",
      "captures": ["message"],
      "summary_template": "warning: ${message}",
      "tags": ["build", "rust", "cargo"],
      "rate_limit_per_min": 30,
      "redact": [],
      "context_hint": { "before_lines": 0, "after_lines": 2 }
    },
    {
      "id": "cargo.test-failed",
      "version": 1,
      "kind": "regex",
      "status": "draft",
      "severity": "high",
      "event_kind": "test_failed",
      "stream": "stdout",
      "description": "cargo test summary reporting failures.",
      "pattern": "^test result: FAILED\\. (?P<passed>[0-9]+) passed; (?P<failed>[0-9]+) failed",
      "captures": ["passed", "failed"],
      "summary_template": "tests: ${failed} failed, ${passed} passed",
      "tags": ["test", "rust", "cargo"],
      "rate_limit_per_min": 12,
      "redact": [],
      "context_hint": { "before_lines": 1, "after_lines": 0 }
    },
    {
      "id": "cargo.panic",
      "version": 1,
      "kind": "regex",
      "status": "draft",
      "severity": "critical",
      "event_kind": "panic",
      "stream": "stderr",
      "description": "Rust runtime panic line.",
      "pattern": "thread '(?P<thread>[^']+)' panicked at (?P<location>.+)$",
      "captures": ["thread", "location"],
      "summary_template": "panic in '${thread}' at ${location}",
      "tags": ["runtime", "rust", "cargo"],
      "rate_limit_per_min": 12,
      "redact": [],
      "context_hint": { "before_lines": 0, "after_lines": 3 }
    },
    {
      "id": "cargo.aborting",
      "version": 1,
      "kind": "regex",
      "status": "draft",
      "severity": "high",
      "event_kind": "command_failed",
      "stream": "stderr",
      "description": "rustc aggregate failure line.",
      "pattern": "^error: aborting due to (?P<count>[0-9]+) previous error",
      "captures": ["count"],
      "summary_template": "aborting: ${count} error(s)",
      "tags": ["build", "rust", "cargo"],
      "rate_limit_per_min": 6,
      "redact": [],
      "context_hint": { "before_lines": 0, "after_lines": 0 }
    }
```

- [ ] **Step 2: Validate JSON**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import json,sys; json.load(open(\"rules/cargo.json\")); print(\"ok\")'"
```
Expected: `ok`.

- [ ] **Step 3: Update the seed-pack count assertion**

In `import_all_seed_packs_from_repo` (import.rs:222) the assertion `total_imported >= 12` still holds (now ~17). If a per-pack count is asserted elsewhere for cargo, bump it to 6. Verify regexes pass the bounded-compile by running the import test.

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-store import_all_seed_packs_from_repo import_by_name"
```
Expected: PASS (no rule skipped -> all six cargo regexes compile within limits and validate, i.e. no lookaround/backrefs; the patterns above use only named groups + anchors).

- [ ] **Step 4: Add a sifter match fixture (truthfulness)**

If `crates/sifters/tests` or a fixtures-based test asserts representative matches (per TC14 discipline), add one sample per new rule proving it matches a real line and not noise, e.g. `test result: FAILED. 3 passed; 2 failed; 0 ignored` -> `cargo.test-failed`. If no such harness exists, assert matches inside a new store/sifter test using `SifterRuntime::build` on the promoted cargo rules against the four sample lines. Match the existing test idiom; do not invent a framework.

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-sifters && cargo nextest run -p terminal-commander-store"
```
Expected: PASS.

- [ ] **Step 5: Commit**

Subject: `feat(rules): thicken cargo pack to 6 rules (warning/test/panic/aborting)`

---

## Task 6: Full verification + dogfood report

- [ ] **Step 1: Workspace fmt + clippy**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings"
```
Expected: clean.

- [ ] **Step 2: Run touched suites**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo nextest run -p terminal-commander-core -p terminal-commander-store -p terminal-commanderd -p terminal-commander-mcp"
```
Expected: all PASS. (Known pre-existing flake `file_search_rejects_empty_query` -- re-run in isolation if it trips; not caused by this work.)

- [ ] **Step 3: cargo clean**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && cargo clean"
```

- [ ] **Step 4: Dogfood + report**

The behavioral proof. Document in `docs/audits/2026-05-27-cargo-pack-dogfood.md`:
- Build a tiny crate with a deliberate compile error under WSL; run it through the freshly-built TC daemon: `registry_import_pack {pack:"cargo", activate:true, scope:global}` then `command_start_combed cargo build`; record the receipt/signal vs raw `cargo build` token count and call count.
- A noisy long-running command (e.g. a build that warns a lot): did the pack surface only the signals.
- A passing build: did the no-silence receipt read right.
- Honest verdict: where TC won, where Bash was better, where cold-start still bit, and the single next fix.

Note: the live MCP daemon on this host may be the OLD binary; build from main first (`cargo build -p terminal-commanderd -p terminal-commander-mcp`) and point the dogfood at that, or run the flow through the IPC test harness if wiring a live MCP client is heavier than the proof warrants.

- [ ] **Step 5: Report (DSPIVR)**

Objective, changes, files, verification commands + results, evidence (a sample `registry_import_pack` response + a cargo signal event), known gaps (other packs not thickened; scope chicken/egg noted; eval is dogfood not automated).

---

## Spec coverage check

- TCE-ERG-PACK-1 (name resolution, embed) -> Task 1.
- TCE-ERG-PACK-2 (import_pack IPC, promote-before-activate, reuse activate path, scope required) -> Tasks 2-3.
- TCE-ERG-PACK-3 (MCP tool, teaching error, catalogue+fixture) -> Task 4.
- TCE-ERG-PACK-4 (thicken cargo + fixtures) -> Task 5.
- Dogfood activity -> Task 6 Step 4.
- Invariants (no fourth activate copy; draft default on disk; gate honest) -> Task 3 (calls handle_registry_activate per rule; promotion in store first).

## Notes carried from review

- Re-import bumps version (create_rule_version auto-increments) -- harmless; activate targets latest.
- `activate:true` requires scope; global is the pragmatic single-agent default (documented in the tool description).
- `include_str!` embeds at build time from the workspace; the store crate is workspace-internal (not published standalone), so the `../../../rules/` path is stable for this repo's builds.
