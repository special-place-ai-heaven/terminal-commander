# TC Compact Facade — Plan 1: Foundation + `command` facade

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an env-gated **compact** MCP surface to Terminal Commander that advertises the `command` facade (one verb-dispatched tool over the existing run/observe operations) instead of the granular tools — proving the full pattern end-to-end while keeping the 50-tool `full` surface and every parity test green.

**Architecture:** Register the facade as an ordinary rmcp `#[tool]` method on the *existing* `#[tool_router]` impl (so dispatch reuses the generated router and the existing handlers). Replace the macro-generated `#[tool_handler]` with a hand-written `impl ServerHandler` whose `list_tools` returns the compact facade list under `TC_SURFACE=compact` (else the unchanged 50) and whose `call_tool` adds an admission gate then delegates to `self.tool_router`. The facade's input is a typed discriminated union (`#[serde(tag="action")]`) whose variants reuse the existing `Mcp*Params` structs, so per-action schema validation is preserved. Daemon/IPC untouched.

**Tech Stack:** Rust, `rmcp = "=1.7.0"` (features `server, macros, transport-io`), `schemars` (JsonSchema derive), `serde`. Crate: `crates/mcp`.

## Global Constraints

- **rmcp is pinned `=1.7.0`** (`crates/mcp/Cargo.toml`). All `ServerHandler` / `Tool` / `ListToolsResult` / `CallToolRequestParams` signatures MUST match rmcp **1.7.0**, NOT the 1.1.0 reference snippets quoted in this plan. Confirm each signature against the 1.7.0 source of truth (see Task 3, Step 0).
- **Env var:** `TC_SURFACE`, values `compact` | `full` (case-insensitive). **Default = `full`** (compact is opt-in this milestone; it is incomplete until facades 2–5 land).
- **No daemon/IPC changes.** Do not touch `crates/ipc` or `crates/daemon`. The facade only forwards to existing adapter methods.
- **`full` surface must stay byte-for-byte behaviorally identical:** the 50 legacy tools, same names, same schemas, same dispatch. Every existing test must pass unchanged except the three parity tests, which are updated to account for the added facade handler in the router (Task 4).
- **`run_and_watch` has no IPC variant** — it is an adapter-side composition (`CommandStartCombed` + `CommandStatus` + `BucketWait`). The facade calls the existing `run_and_watch` method; do not reimplement it.
- **Plain ASCII** in code, comments, and docs.
- **Honesty contracts (day 1):** (1) every facade `action` maps to exactly one existing handler with its exact param type — enforced by the conformance test in Task 4; (2) no fabricated numbers in any output (the facade only forwards existing handler output verbatim).

---

### Reference: the `command` facade action map

Each `action` reuses the **existing** `Mcp*Params` struct and calls the **existing** `#[tool]` method (verify each param type name against the method's `Parameters<...>` signature in `crates/mcp/src/tools.rs` — they follow the `Mcp<Name>Params` convention; several were confirmed during design: `McpCommandStartParams`, `McpRunAndWatchParams`, `McpCommandStatusParams`, `McpCommandOutputTailParams`, `McpCommandStopParams`).

| `action` (snake_case) | existing method | param type |
|---|---|---|
| `run` | `command_start_combed` | `McpCommandStartParams` |
| `run_and_watch` ⭐ | `run_and_watch` | `McpRunAndWatchParams` |
| `exec` | `shell_exec` | `McpShellExecParams` |
| `status` | `command_status` | `McpCommandStatusParams` |
| `output_tail` | `command_output_tail` | `McpCommandOutputTailParams` |
| `stop` | `command_stop` | `McpCommandStopParams` |
| `events` | `bucket_events_since` | `McpBucketEventsSinceParams` |
| `wait` | `bucket_wait` | `McpBucketWaitParams` |
| `summary` | `bucket_summary` | `McpBucketSummaryParams` |
| `event_context` | `event_context` | `McpEventContextParams` |
| `sub_open` | `subscription_open` | `McpSubscriptionOpenParams` |
| `sub_pull` | `subscription_pull` | `McpSubscriptionPullParams` |
| `sub_seek` | `subscription_seek` | `McpSubscriptionSeekParams` |
| `sub_close` | `subscription_close` | `McpSubscriptionCloseParams` |
| `sub_list` | `subscription_list` | `McpSubscriptionListParams` |

---

## Task 1: `Surface` enum + `TC_SURFACE` resolver

**Files:**
- Create: `crates/mcp/src/surface.rs`
- Modify: `crates/mcp/src/lib.rs` (add `mod surface;` / `pub mod surface;` — match the crate's existing module-declaration style)
- Test: inline `#[cfg(test)]` in `crates/mcp/src/surface.rs`

**Interfaces:**
- Produces: `pub enum Surface { Full, Compact }`; `pub fn surface_from_env() -> Surface` (reads `TC_SURFACE`, default `Full`, case-insensitive; mirrors the existing `notify_enabled()` precedent at `tools.rs:2737`).

- [ ] **Step 1: Write the failing test**

```rust
// crates/mcp/src/surface.rs  (append at bottom)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_surface_defaults_and_values() {
        assert_eq!(Surface::parse(None), Surface::Full);          // unset -> Full
        assert_eq!(Surface::parse(Some("full")), Surface::Full);
        assert_eq!(Surface::parse(Some("compact")), Surface::Compact);
        assert_eq!(Surface::parse(Some("COMPACT")), Surface::Compact); // case-insensitive
        assert_eq!(Surface::parse(Some("garbage")), Surface::Full);    // unknown -> Full
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-mcp surface::tests::parse_surface_defaults_and_values`
Expected: FAIL — `cannot find type Surface` / module `surface` does not exist.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/mcp/src/surface.rs  (top of file)
//! Tool-surface selection. `TC_SURFACE=compact` advertises the verb-dispatched
//! facade tools; default `full` keeps the granular 50-tool surface. Read once
//! per `tools/list` / `tools/call` so an operator can flip it without rebuild.

/// Which MCP tool surface to advertise + admit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The granular legacy tools (default; the escape hatch).
    Full,
    /// The verb-dispatched facade tools.
    Compact,
}

impl Surface {
    /// Parse a raw `TC_SURFACE` value. `None` or any unrecognized value => `Full`.
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("compact") => Surface::Compact,
            _ => Surface::Full,
        }
    }
}

/// Resolve the active surface from the live `TC_SURFACE` env var.
#[must_use]
pub fn surface_from_env() -> Surface {
    Surface::parse(std::env::var("TC_SURFACE").ok().as_deref())
}
```

Then add the module declaration in `crates/mcp/src/lib.rs` next to the other `mod` lines (e.g. `pub mod surface;`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commander-mcp surface::tests::parse_surface_defaults_and_values`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/mcp/src/surface.rs crates/mcp/src/lib.rs
git commit -m "feat(mcp): TC_SURFACE env resolver (full default)"
```

---

## Task 2: `command` facade type + dispatch method

**Files:**
- Create: `crates/mcp/src/facades.rs` (the `CommandFacadeCall` discriminated union)
- Modify: `crates/mcp/src/lib.rs` (add `mod facades;`)
- Modify: `crates/mcp/src/tools.rs` (add the `command_facade` `#[tool]` method INSIDE the existing `#[tool_router] impl TerminalCommanderMcpServer` block, ~`tools.rs:691`)
- Test: inline `#[cfg(test)]` in `crates/mcp/src/facades.rs` (deserialization) + an integration check in Task 5

**Interfaces:**
- Produces: `pub enum CommandFacadeCall { Run(McpCommandStartParams), RunAndWatch(McpRunAndWatchParams), … }` (internally tagged by `action`); and the registered tool `command` → `command_facade(&self, Parameters<CommandFacadeCall>) -> Result<CallToolResult, McpError>`.
- Consumes: the existing `Mcp*Params` structs and the existing `#[tool]` methods listed in the action map above.

- [ ] **Step 1: Write the failing test (deserialization shape)**

```rust
// crates/mcp/src/facades.rs  (append at bottom)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_call_deserializes_flat_action() {
        // status: flat { action, ...fields }
        let v: CommandFacadeCall =
            serde_json::from_value(serde_json::json!({"action":"status","job_id":"job_abc"}))
                .expect("status must parse");
        assert!(matches!(v, CommandFacadeCall::Status(p) if p.job_id == "job_abc"));

        // run_and_watch: the promoted happy path
        let v: CommandFacadeCall = serde_json::from_value(
            serde_json::json!({"action":"run_and_watch","argv":["echo","hi"]}),
        )
        .expect("run_and_watch must parse");
        assert!(matches!(v, CommandFacadeCall::RunAndWatch(_)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-mcp facades::tests::command_call_deserializes_flat_action`
Expected: FAIL — `cannot find type CommandFacadeCall`.

- [ ] **Step 3: Write the discriminated union**

```rust
// crates/mcp/src/facades.rs  (top of file)
//! Verb-dispatched facade input types. Each variant reuses an existing
//! `Mcp*Params` struct so per-action schema validation is preserved; the
//! facade method forwards to the existing `#[tool]` handler unchanged.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::tools::{
    McpBucketEventsSinceParams, McpBucketSummaryParams, McpBucketWaitParams,
    McpCommandOutputTailParams, McpCommandStartParams, McpCommandStatusParams,
    McpCommandStopParams, McpEventContextParams, McpRunAndWatchParams,
    McpShellExecParams, McpSubscriptionCloseParams, McpSubscriptionListParams,
    McpSubscriptionOpenParams, McpSubscriptionPullParams, McpSubscriptionSeekParams,
};

/// `command` facade — run + observe + stream a one-shot command. Internally
/// tagged by `action`; the action's remaining fields are the chosen op's params.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CommandFacadeCall {
    Run(McpCommandStartParams),
    RunAndWatch(McpRunAndWatchParams),
    Exec(McpShellExecParams),
    Status(McpCommandStatusParams),
    OutputTail(McpCommandOutputTailParams),
    Stop(McpCommandStopParams),
    Events(McpBucketEventsSinceParams),
    Wait(McpBucketWaitParams),
    Summary(McpBucketSummaryParams),
    EventContext(McpEventContextParams),
    SubOpen(McpSubscriptionOpenParams),
    SubPull(McpSubscriptionPullParams),
    SubSeek(McpSubscriptionSeekParams),
    SubClose(McpSubscriptionCloseParams),
    SubList(McpSubscriptionListParams),
}
```

If any `Mcp*Params` struct is not currently `pub` / not re-exported from `crate::tools`, make it `pub` (it is already `JsonSchema + Deserialize`). Confirm the exact `McpShellExecParams` name against `shell_exec`'s `Parameters<...>` signature with: `rg "fn shell_exec" -A3 crates/mcp/src/tools.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p terminal-commander-mcp facades::tests::command_call_deserializes_flat_action`
Expected: PASS.

- [ ] **Step 5: Add the `command_facade` tool method (dispatch)**

Inside the `#[tool_router] impl TerminalCommanderMcpServer` block in `crates/mcp/src/tools.rs` (same block as the 50 existing tools), add:

```rust
    /// `command` — run + observe + stream a one-shot command (compact surface).
    #[tool(
        description = "Run and observe a one-shot command. To run a command and \
get its result in ONE call, use action=\"run_and_watch\" (it does start + bounded \
wait + collect for you). Other actions: run, exec, status, output_tail, stop, \
events, wait, summary, event_context, sub_open, sub_pull, sub_seek, sub_close, \
sub_list. For an interactive shell, see the `session` facade."
    )]
    pub(crate) async fn command_facade(
        &self,
        Parameters(call): Parameters<crate::facades::CommandFacadeCall>,
    ) -> Result<CallToolResult, McpError> {
        use crate::facades::CommandFacadeCall as C;
        match call {
            C::Run(p) => self.command_start_combed(Parameters(p)).await,
            C::RunAndWatch(p) => self.run_and_watch(Parameters(p)).await,
            C::Exec(p) => self.shell_exec(Parameters(p)).await,
            C::Status(p) => self.command_status(Parameters(p)).await,
            C::OutputTail(p) => self.command_output_tail(Parameters(p)).await,
            C::Stop(p) => self.command_stop(Parameters(p)).await,
            C::Events(p) => self.bucket_events_since(Parameters(p)).await,
            C::Wait(p) => self.bucket_wait(Parameters(p)).await,
            C::Summary(p) => self.bucket_summary(Parameters(p)).await,
            C::EventContext(p) => self.event_context(Parameters(p)).await,
            C::SubOpen(p) => self.subscription_open(Parameters(p)).await,
            C::SubPull(p) => self.subscription_pull(Parameters(p)).await,
            C::SubSeek(p) => self.subscription_seek(Parameters(p)).await,
            C::SubClose(p) => self.subscription_close(Parameters(p)).await,
            C::SubList(p) => self.subscription_list(Parameters(p)).await,
        }
    }
```

Confirm each callee method name with `rg "async fn (command_start_combed|run_and_watch|shell_exec|command_status|command_output_tail|command_stop|bucket_events_since|bucket_wait|bucket_summary|event_context|subscription_open|subscription_pull|subscription_seek|subscription_close|subscription_list)\b" crates/mcp/src/tools.rs`. (`#[tool]`-annotated methods remain directly callable.)

- [ ] **Step 6: Build (the router now has 51 tools; parity tests will fail — expected, fixed in Task 4)**

Run: `cargo build -p terminal-commander-mcp`
Expected: compiles. (`cargo test` parity tests will fail until Task 4 — that is expected and called out there.)

- [ ] **Step 7: Commit**

```bash
git add crates/mcp/src/facades.rs crates/mcp/src/lib.rs crates/mcp/src/tools.rs
git commit -m "feat(mcp): command facade type + dispatch (registered on tool_router)"
```

---

## Task 3: Hand-written `ServerHandler` (compact `list_tools` + `call_tool` gate)

**Files:**
- Modify: `crates/mcp/src/tools.rs` — replace `#[tool_handler]` (`tools.rs:2585`) with a hand-written `impl ServerHandler`; add a `compact_surface_tools()` builder + `enforce_surface()` gate (new small functions in `tools.rs` or a new `crates/mcp/src/surface_list.rs` — keep `surface.rs` for the enum, `surface_list.rs` for the rmcp `Tool` construction to keep files focused).
- Create: `crates/mcp/src/surface_list.rs`
- Modify: `crates/mcp/src/lib.rs` (`mod surface_list;`)
- Test: inline `#[cfg(test)]` in `tools.rs` / `surface_list.rs`

**Interfaces:**
- Produces: `pub fn compact_surface_tools() -> Vec<rmcp::model::Tool>` (the facade Tools; this milestone returns the single `command` Tool); `pub fn enforce_surface(surface: Surface, tool_name: &str) -> Result<(), McpError>`; the hand-written `list_tools` / `call_tool`.

- [ ] **Step 0: Confirm rmcp 1.7.0 signatures (DO THIS FIRST — version-specific)**

The reference snippets below are from symforge on **rmcp 1.1.0**. TC is on **1.7.0**. Confirm the exact 1.7.0 trait method signatures and types before writing code:

```bash
# What the macro currently generates (the thing you are replacing):
cargo expand -p terminal-commander-mcp --lib 2>/dev/null | rg -n "fn list_tools|fn call_tool|ListToolsResult|CallToolRequestParam|PaginatedRequestParam|ToolCallContext"
# Trait + types as defined by the pinned rmcp:
rg -n "fn list_tools|fn call_tool" "$(cargo metadata --format-version=1 | jq -r '.packages[]|select(.name=="rmcp")|.manifest_path' | xargs dirname)/src" 2>/dev/null | head
```
Record the exact: `list_tools` params (`Option<PaginatedRequestParam>` vs `...Params`), `call_tool` request type (`CallToolRequestParam` vs `CallToolRequestParams`), error type (`rmcp::ErrorData` aliased as `McpError`), the `ToolCallContext::new(...)` path, and the `ListToolsResult { tools, .. }` field set. Use the confirmed forms in Steps 2–3 (adjust the reference snippets accordingly).

- [ ] **Step 1: Write the failing test (surface filtering + gate)**

```rust
// crates/mcp/src/surface_list.rs  (append)
#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::Surface;

    #[test]
    fn compact_list_is_only_facades() {
        let names: Vec<_> = compact_surface_tools().iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&"command".to_string()));
        // No granular legacy tool leaks into the compact list this milestone:
        assert!(!names.iter().any(|n| n.starts_with("command_") || n == "run_and_watch"));
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p terminal-commander-mcp surface_list::tests`
Expected: FAIL — `compact_surface_tools` / `enforce_surface` not found.

- [ ] **Step 3: Implement the builder + gate, then replace `#[tool_handler]`**

`crates/mcp/src/surface_list.rs` (adapt types to the rmcp 1.7.0 forms confirmed in Step 0; reference is rmcp 1.1.0):

```rust
//! Compact-surface `tools/list` construction + the admission gate.
use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::Tool;
use schemars::{schema_for, JsonSchema};
use serde_json::{Map, Value};

use crate::surface::Surface;

/// The facade tool names advertised + admitted on the compact surface.
/// This milestone: `command` only (session/files/registry/status land in
/// follow-on plans). KEEP IN SYNC with `compact_surface_tools()`.
pub const COMPACT_TOOL_NAMES: &[&str] = &["command"];

fn schema_object<T: JsonSchema>() -> Arc<Map<String, Value>> {
    let schema = schema_for!(T);
    let value = serde_json::to_value(schema).expect("facade input schema must serialize");
    Arc::new(value.as_object().expect("schema root must be a JSON object").clone())
}

fn surface_tool(name: &'static str, description: &'static str, input: Arc<Map<String, Value>>) -> Tool {
    let mut tool = Tool::default();
    tool.name = Cow::Borrowed(name);
    tool.description = Some(Cow::Borrowed(description));
    tool.input_schema = input;
    tool
}

/// `tools/list` for `TC_SURFACE=compact`.
#[must_use]
pub fn compact_surface_tools() -> Vec<Tool> {
    vec![surface_tool(
        "command",
        "Run and observe a one-shot command. Prefer action=\"run_and_watch\" to \
run and get the result in ONE call.",
        schema_object::<crate::facades::CommandFacadeCall>(),
    )]
}

/// Admission gate: under `Compact`, reject any tool name not in the facade set.
pub fn enforce_surface(surface: Surface, tool_name: &str) -> Result<(), rmcp::ErrorData> {
    if surface == Surface::Compact && !COMPACT_TOOL_NAMES.contains(&tool_name) {
        return Err(rmcp::ErrorData::invalid_request(
            format!("tool '{tool_name}' not on compact surface; set TC_SURFACE=full"),
            None,
        ));
    }
    Ok(())
}
```

Then in `crates/mcp/src/tools.rs`, **remove `#[tool_handler]`** from the `impl ServerHandler` block (`tools.rs:2585`) and add the two methods alongside the existing `get_info` / `initialize` (signatures per Step 0; reference forms shown):

```rust
    // (rmcp 1.1.0 reference signature — CONFIRM against 1.7.0 in Step 0)
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = match crate::surface::surface_from_env() {
            crate::surface::Surface::Compact => crate::surface_list::compact_surface_tools(),
            crate::surface::Surface::Full => self
                .tool_router
                .list_all()
                .into_iter()
                .filter(|t| !crate::surface_list::COMPACT_TOOL_NAMES.contains(&t.name.as_ref()))
                .collect(),
        };
        Ok(ListToolsResult { tools, next_cursor: None, ..Default::default() })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        crate::surface_list::enforce_surface(
            crate::surface::surface_from_env(),
            request.name.as_ref(),
        )?;
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }
```

Add the needed imports to the `use rmcp::{...}` block (`PaginatedRequestParam`, `CallToolRequestParam`, `ListToolsResult`, `RequestContext`, `RoleServer`, model items) per the confirmed 1.7.0 names. Note `full` filters the facade names OUT of `list_all()` so the full surface stays exactly the 50 legacy tools.

- [ ] **Step 4: Run tests + build**

Run: `cargo test -p terminal-commander-mcp surface_list::tests` then `cargo build -p terminal-commander-mcp`
Expected: the two surface_list tests PASS; crate compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/mcp/src/surface_list.rs crates/mcp/src/lib.rs crates/mcp/src/tools.rs
git commit -m "feat(mcp): hand-written ServerHandler with compact list_tools + call_tool gate"
```

---

## Task 4: Update parity tests + add the no-silent-drop conformance test

**Files:**
- Modify: `crates/mcp/src/tools.rs` — the three parity tests: `tool_router_exposes_all_live_tools` (`tools.rs:5351`), `system_discover_tools_explain_daemon_unavailable` (`tools.rs:5417`), and leave `catalogue_lists_fifty_live_tools` (`tools.rs:5279`) asserting the 50 *legacy* names (the facade is NOT a catalogue entry).
- Modify: `crates/ipc/src/protocol.rs` — correct the stale `33 methods / 32 live tools` doc comment (`protocol.rs:251-259`) to the real counts.
- Sweep: `crates/mcp/tests/` for any hard-coded tool-count / name-list anchors and update them.
- Test: add `command_facade_action_map_is_total` in `tools.rs` (conformance).

**Interfaces:**
- Consumes: `CommandFacadeCall`, `command_facade`, `tool_router`.

- [ ] **Step 1: Add the conformance test (no-silent-drop / total action map)**

```rust
// crates/mcp/src/tools.rs  (in the existing #[cfg(test)] mod)
#[test]
fn command_facade_action_map_is_total() {
    // Every CommandFacadeCall action parses and is a known variant. If a new
    // action is added without a dispatch arm, the match in `command_facade`
    // fails to compile — this test guards the *schema* side: every advertised
    // action name round-trips into a variant (no silent drop of an action).
    let actions = [
        "run","run_and_watch","exec","status","output_tail","stop","events",
        "wait","summary","event_context","sub_open","sub_pull","sub_seek",
        "sub_close","sub_list",
    ];
    let schema = serde_json::to_value(schemars::schema_for!(crate::facades::CommandFacadeCall))
        .expect("schema serializes");
    let blob = schema.to_string();
    for a in actions {
        assert!(blob.contains(a), "action '{a}' missing from CommandFacadeCall schema");
    }
}
```

- [ ] **Step 2: Run it — expect PASS (schema already lists them)**

Run: `cargo test -p terminal-commander-mcp command_facade_action_map_is_total`
Expected: PASS.

- [ ] **Step 3: Update `tool_router_exposes_all_live_tools`**

The router now contains the 50 legacy names **plus** `command`. Add `"command".to_owned()` to the asserted sorted vector in `tool_router_exposes_all_live_tools` (`tools.rs:5351`), keeping alphabetical order (between `bucket_*`/`command_*` — `command` sorts before `command_output_tail`). Do NOT change `catalogue_lists_fifty_live_tools` (the catalogue stays the 50 legacy; the facade is a surface view, not a catalogue tool).

- [ ] **Step 4: Update `system_discover_tools_explain_daemon_unavailable`**

This test asserts `discovered_tools(false).len() == tool_catalogue().len()` (50). Since `system_discover` returns the catalogue (still 50) and the facade is not a catalogue entry, this should still hold. Run it; if `discovered_tools` is derived from the router (55→51), adjust the assertion to compare against the router's live set minus facade names, matching how `discovered_tools` is actually built (inspect `discovered_tools` at the cited line before editing).

- [ ] **Step 5: Fix the stale protocol doc comment**

In `crates/ipc/src/protocol.rs:251-259`, replace the `All 33 methods` / `32 live tools` wording with the accurate counts (49 IPC methods; 50 legacy MCP tools + facade tools on the compact surface). Wording only — no code change.

- [ ] **Step 6: Run the full crate test suite**

Run: `cargo test -p terminal-commander-mcp -- --test-threads=1`
Expected: PASS (all parity tests green with the facade accounted for).

- [ ] **Step 7: Commit**

```bash
git add crates/mcp/src/tools.rs crates/ipc/src/protocol.rs crates/mcp/tests
git commit -m "test(mcp): account for command facade in parity tests; fix stale protocol counts"
```

---

## Task 5: End-to-end surface behavior tests

**Files:**
- Create: `crates/mcp/tests/compact_surface.rs` (integration test using the in-process rmcp client already used by other `crates/mcp/tests/`)
- Test only.

**Interfaces:**
- Consumes: the server handler, `TC_SURFACE`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/mcp/tests/compact_surface.rs
// Follow the existing in-process client setup in crates/mcp/tests/ (rmcp
// ClientHandler / RoleClient) — copy the harness from a sibling test file.

// 1. Default (full): tools/list contains the 50 legacy names and NOT "command".
// 2. TC_SURFACE=compact: tools/list contains "command" and NOT "command_status".
// 3. TC_SURFACE=compact: tools/call "command" {action:"run_and_watch", argv:[...]}
//    reaches run_and_watch (assert the daemon-unavailable envelope when no daemon,
//    OR a real result against the test daemon the harness already spins up).
// 4. TC_SURFACE=compact: tools/call "command_status" {...} is REJECTED (gate).
//
// Set the env per-test with std::env::set_var BEFORE building the server, and
// run this file single-threaded (env is process-global): add to the test a
// serialization guard or run with --test-threads=1.
```

Write the four assertions concretely against the harness copied from the sibling test (the existing `crates/mcp/tests/` files show the exact client build + `list_tools()` / `call_tool()` calls).

- [ ] **Step 2: Run to verify they fail (then pass after wiring)**

Run: `cargo test -p terminal-commander-mcp --test compact_surface -- --test-threads=1`
Expected: compile/fail first, then PASS once the harness is filled in.

- [ ] **Step 3: Full gate**

Run:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p terminal-commander-mcp -- --test-threads=1
```
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add crates/mcp/tests/compact_surface.rs
git commit -m "test(mcp): end-to-end compact vs full surface behavior"
```

---

## Done-when (Plan 1 acceptance)

- `TC_SURFACE` unset/`full` → `tools/list` is exactly the 50 legacy tools; every existing test green.
- `TC_SURFACE=compact` → `tools/list` advertises `command`; `tools/call command {action:"run_and_watch", …}` runs start+wait+collect in one call; a legacy name is rejected with a clear "set TC_SURFACE=full" message.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test -p terminal-commander-mcp -- --test-threads=1` all pass.
- **Compact stays opt-in** (incomplete until facades 2–5). Do NOT flip the default.

## Follow-on plans (not this plan)
- Plan 2 `session` (pty_* + shell_session_*), Plan 3 `files` (file_* + snapshot_*), Plan 4 `registry` (registry_*), Plan 5 `status` (health/self_check/policy_status/runtime_state/probe_*/system_discover/target_*) — each adds a facade type + dispatch method + extends `COMPACT_TOOL_NAMES`/`compact_surface_tools()` + cross-ref descriptions (seams S2–S4) + the same conformance/parity updates.
- Plan 6: the live-dogfood loop (npm install, reconnect MCP under `TC_SURFACE=compact`, drive all 5 facades, record friction vs AC1–AC3) and only then the default flip to `compact`.
