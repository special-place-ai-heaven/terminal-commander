# Adversarial review: draft-poison fix

**Date:** 2026-05-26  
**Scope:** Uncommitted fix in `C:\Users\poslj\terminal-commander`  
**Bug:** Draft rules accepted by `registry_activate` poisoned the activation registry; every subsequent `command_start_combed` in scope failed at sifter build with `SifterError::NotActive` until manual deactivation.

**Files changed (5, not 3):**

| File | Change |
|------|--------|
| `crates/daemon/src/ipc/protocol.rs` | `IpcErrorCode::RuleNotActive` |
| `crates/daemon/src/ipc/server.rs` | Up-front eligibility gate in `handle_registry_activate` |
| `crates/daemon/src/activation.rs` | Skip non-eligible rows in `from_entries` (restart defense) |
| `crates/mcp/src/tools.rs` | Map `RuleNotActive` → `invalid_params` |
| `crates/daemon/tests/registry_ipc.rs` | IPC rejection + restart rehydration tests |

**Verdict:** Directionally correct; closes the main IPC and restart paths. Remaining risk is legacy state, unguarded `activate()`, and residual `command_start` error mapping.

---

## Critical

*None.* New IPC activations of non-`Active` rules are rejected before bind/persist/rebind; restart rehydration skips non-eligible definitions.

---

## High

### 1. Already-poisoned running daemon is not healed in-memory

The gate runs only on new `registry_activate` calls. A daemon that already bound a Draft rule (pre-fix) keeps that entry in `ActivationRegistry` until restart or explicit `registry_deactivate`.

`command_start_combed` still builds the sifter from the poisoned snapshot and fails in `SifterRuntime::build` (`SifterError::NotActive`), mapped to **`IpcErrorCode::Internal`**, not `RuleNotActive`:

```869:892:crates/daemon/src/ipc/server.rs
fn map_command_error(e: CommandError) -> IpcError {
    match e {
        // ...
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}
```

Operators on a poisoned process see “internal error” on every command start until restart/deactivate, while new activates get the clear `RuleNotActive` remedy.

**Recommend:** Document “restart or deactivate after upgrade,” or filter non-eligible defs in `snapshot_for_job` / `merge_active_and_inline` as a hotfix for live poison.

---

## Medium

### 2. Orphan `rule_activations` rows: skipped at runtime, never closed

`from_entries` skips non-eligible defs but does **not** call `deactivate_rule_scoped`:

```72:75:crates/daemon/src/activation.rs
            if !e.definition.status.is_runtime_eligible() {
                continue;
            }
```

Pre-fix DB rows (`deactivated_at IS NULL` + Draft/Deprecated definition via JOIN) remain “open” in SQLite forever. Runtime is safe; DB/audit hygiene is not. No migration in this diff.

`draft_activation_row_does_not_rehydrate_on_restart` (`registry_ipc.rs:510–557`) covers the restart path; there is **no** test that bootstrap auto-deactivates ghost rows (and none is implemented).

**Recommend:** Optional bootstrap pass: for each skipped row, close the activation record (or one-shot migration tool).

### 3. `ActivationRegistry::activate()` has no eligibility check

```95:98:crates/daemon/src/activation.rs
    pub fn activate(&self, def: RuleDefinition, scope: ActivationScope) {
        let key = (def.id.clone(), def.version, scope);
        self.by_key.write().insert(key, def);
```

Grep shows **one** production caller after lookup: `server.rs:1130`. Today the IPC gate is sufficient. Any future caller of `.activate()` (or `record_activation_scoped` without IPC) can reintroduce poison.

**Recommend:** `activate()` should assert `is_runtime_eligible()` (or return `Result`) for defense in depth.

### 4. `ToolSurface::registry_activate` still bypasses the daemon gate

```219:232:crates/mcp/src/lib.rs
    pub fn registry_activate(
        &self,
        store: &mut terminal_commander_store::EventStore,
        rule_id: &str,
        version: u32,
        profile: Option<&str>,
    ) -> Result<(), McpError> {
        // ...
        store
            .record_activation(rule_id, version, profile, Some("mcp"))
```

Live MCP tools use daemon IPC (`tools.rs:651`) and are gated. The in-process path is used by `crates/mcp/tests/e2e.rs` and can persist activation rows **without** updating in-memory registry—but bootstrap would still skip Draft defs on restart.

**Risk:** Tests/tools that write activations directly; not the production MCP path.

### 5. TOCTOU: lookup → check → activate without holding store lock

`lookup_rule_def` locks the store, copies the definition, then drops the lock before the eligibility check and `activate()`.

- **Draft poison via TOCTOU:** Unlikely—another thread cannot slip a Draft into activation after an `Active` snapshot without going through `activate()` with a Draft clone.
- **Stale Active clone:** If the definition is demoted to Draft/Deprecated **after** lookup but **before** `activate`, you can still bind an **old Active snapshot** while `rule_versions` now says otherwise. Restart fixes via JOIN; a long-lived daemon can run stale Active text until re-activate. Pre-existing pattern, not introduced by this fix.

### 6. Inline `command_start_combed` rules can still trigger `NotActive`

`handle_command_start_combed` does not validate rule status on `params.rules`; `merge_active_and_inline` passes them through; `SifterRuntime::build` rejects non-eligible (`sifters/src/lib.rs:192–194`). That fails **one** start as `Internal`, not scope-wide registry poison. The `registry_activate` remedy mentions `rules_json`; the command path still surfaces `internal_error` in MCP.

---

## Low

### 7. `registry_test` force-active is fine; UX footgun only

```1039:1044:crates/daemon/src/ipc/server.rs
    let mut def = lookup_rule_def(state, &params.rule_id, params.version)?;
    // Force-active so a Draft rule can still be evaluated against
    // samples without persisting an activation. Read-only.
    def.status = RuleStatus::Active;
```

Does not touch `ActivationRegistry` or `record_activation`. No bad interaction with the gate. LLMs might infer “test worked ⇒ safe to activate” without reading status—that is documentation/ergonomics, not a correctness bug.

### 8. Wire-compat / TC05 / exhaustive matches

- `IpcErrorCode` uses `#[serde(rename_all = "snake_case")]`; new variant serializes as `"rule_not_active"`.
- No TC05 golden fixtures enumerate IPC error codes (fixtures are rule/MCP tool shapes).
- `into_mcp_error` maps `RuleNotActive` to `invalid_params` (`tools.rs:1135–1144`). Safe within this repo.
- Older binary + newer daemon: extra enum variant on the wire is usually fine for clients that treat `code` as string-like JSON. Strict deserializers to a closed enum elsewhere could break—deploy daemon + MCP together.

### 9. Tests

| Test | Status |
|------|--------|
| `registry_activate_rejects_draft_rule_with_typed_error` | Added (`registry_ipc.rs:404–461`) |
| `draft_activation_row_does_not_rehydrate_on_restart` | Added (`registry_ipc.rs:510–557`) |
| `registry_activations_survive_daemon_restart` | Unchanged; still valid for Active path |
| `activation.rs` unit tests | No `from_entries` skip-Draft unit test |
| All `registry_ipc` tests | `#![cfg(unix)]` — do not run on Windows hosts |

---

## Verification checklist

| Question | Verdict |
|----------|---------|
| Gate at right layer? | **Yes** for IPC + MCP mapping; **yes** for restart via `from_entries`. |
| Draft via rehydration / rebind / scoped activate? | **Rehydration:** blocked. **Rebind:** only after successful activate. **Scoped:** same handler, same gate. |
| `registry_test` force-active? | **Safe** (read-only eval). |
| TOCTOU? | **No draft poison**; possible **stale Active** bind (medium #5). |
| `activate()` enforce eligibility? | **Not on `activate()`**; only IPC + `from_entries`. |
| Other `.activate()` callers? | **Only** `handle_registry_activate` + `from_entries`. |
| TC05 / serde round-trip? | **No fixture break** observed; add `rule_not_active` to contract docs if IPC errors are versioned. |
| Draft restart sibling test? | **Present** (`draft_activation_row_does_not_rehydrate_on_restart`). |
| Legacy poisoned rows? | **Runtime ignored; DB rows linger.** **Running daemon** needs restart/deactivate. |

---

## Alternate activation paths (trace)

```
registry_activate (IPC)
  → lookup_rule_def
  → is_runtime_eligible?  [NEW GATE]
  → activation.activate + record_activation_scoped + rebind_jobs_in_scope

Daemon bootstrap
  → list_active_rule_defs_scoped (JOIN rule_versions)
  → ActivationRegistry::from_entries
  → skip !is_runtime_eligible  [NEW]

registry_test
  → force Active in-memory only; no activation row

command_start_combed inline rules
  → merge_active_and_inline → SifterRuntime::build (NotActive → Internal)

ToolSurface::registry_activate (in-process MCP tests only)
  → record_activation_scoped (no IPC gate)
```

---

## Summary

The fix correctly blocks the footgun at `handle_registry_activate` and prevents restart re-poison via `ActivationRegistry::from_entries`, with good regression tests on Unix.

Main gaps:

1. No cleanup of legacy DB activation rows.
2. No in-memory heal for already-poisoned daemons.
3. `activate()` still trusts all callers.
4. Command-start still surfaces residual poison as `Internal`.

Addressing high #1 and medium #2/#3 would make this production-solid; the current diff is a sound minimal fix.
