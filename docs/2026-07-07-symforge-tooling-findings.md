# SymForge Tooling Findings

Date: 2026-07-07
Repository: terminal-commander

This is a live issue log for testing SymForge while fixing Terminal Commander. The working rule for this repo is:

- Try SymForge first for code navigation, symbol context, edit planning, and symbol-aware edits.
- If SymForge errors, returns insufficient context, or forces a confusing workflow, report it in the chat before falling back.
- Capture the issue here with enough raw detail for a SymForge coding agent to reproduce it.

SymForge's absolute goal is to reduce noise, save tokens, and increase LLM trust in its tools. Findings below are written against that bar: a bug matters when it adds noise, burns tokens/round-trips, or makes the agent trust raw reads / git diff over SymForge output.

## Current Baseline

- SymForge version observed in-session: 8.13.6.
- Surface after config update: full granular MCP tools are available.
- Full health observed in-session: ready; all indexed files parsed successfully.
- Compact health is only acceptable as a quick liveness check; use full `health` output for diagnostics and report evidence.

## Findings

### 2026-07-07 - Compact surface hid granular tools from Codex

Severity: Integration friction

Intent:
Use SymForge for Terminal Commander code work, including `get_file_context`, `search_text`, `edit_plan`, and symbol-aware edits.

Expected:
The coding agent can discover and call the granular tools described by the repo's AGENTS.md instructions.

Actual:
Only the compact facade tools were visible initially: `symforge`, `status`, and `symforge_edit`. This made the agent appear to skip SymForge or fall back to raw file reads even though the granular tools existed.

Workaround:
Set the SymForge MCP server environment to request the full surface:

```toml
[mcp_servers.symforge.env]
SYMFORGE_SURFACE = "full"
```

Then restart Codex so tool discovery sees the granular tools.

Impact:
High. The compact surface undermines repo-local AGENTS.md instructions that name granular tools directly, and it makes troubleshooting look like agent negligence rather than a surface/config mismatch.

### 2026-07-07 - Compact natural-language routing was a poor edit-planning substitute

Severity: Workflow friction

Intent:
Plan changes to policy defaults and shell handling from a broad natural-language request.

Expected:
The compact facade should route edit-planning intent to a useful code navigation result, or tell the agent which granular tool would have answered the request.

Actual:
The facade routed the request to file search and did not provide the needed symbol/edit context. With the full surface available, direct `edit_plan`, `search_text`, and `get_symbol_context` are the correct path.

Workaround:
Use direct granular tools once `SYMFORGE_SURFACE = "full"` is active.

Impact:
Medium. The facade is useful for discovery, but it is not a drop-in substitute for the repo's documented symbol workflow.

### 2026-07-07 - `edit_plan` did not resolve an impl-method selector

Severity: Workflow friction

SymForge version:
8.13.6

Tool:
`edit_plan`

Intent:
Plan a targeted edit to the `PolicyEngine::new` method after `get_file_context` showed it in `crates/daemon/src/policy.rs`.

Input:

```json
{"target":"crates/daemon/src/policy.rs::PolicyEngine::new"}
```

Expected:
Resolve the impl method listed in the file outline and return an edit plan, or suggest the accepted selector syntax for methods.

Actual:

```text
Target 'crates/daemon/src/policy.rs::PolicyEngine::new' not found.
Try: search_symbols(query="...") to find the correct name.
```

Workaround:
Use `search_symbols` and line-qualified `get_symbol` / `get_symbol_context`, or call `edit_plan` on the file path instead of the qualified method selector.

Impact:
Medium. The repo instructions say to call `edit_plan` before editing a symbol, but the natural selector for an impl method failed after the method appeared in the outline.

### 2026-07-07 - `analyze_file_impact` over-reported a header-only Rust comment edit

Severity: Signal noise

SymForge version:
8.13.6

Tool:
`analyze_file_impact`

Intent:
Refresh the index after editing only the top module doc comment in `crates/mcp/tests/shell_live_e2e.rs`.

Input:

```json
{"path":"crates/mcp/tests/shell_live_e2e.rs","include_co_changes":false}
```

Expected:
Report a file-level/comment-only change or no symbol body changes.

Actual:
Reported every symbol in the file as changed, including `TestClient`, helper functions, and both test functions.

Second reproduction:
After editing only header/leading comments in `crates/daemon/tests/shell_runtime.rs`, `analyze_file_impact` again reported every function in the file as changed.

Third reproduction:
After editing only the leading doc comment before `shell_exec_allowed_on_default_profile_returns_combed_start` in `crates/daemon/tests/ipc_command.rs`, `analyze_file_impact` reported that test plus several following tests as changed.

Fourth reproduction:
After editing only the leading doc comment / tool attribute string for `shell_exec` in `crates/mcp/src/tools.rs`, `analyze_file_impact` reported `impl TerminalCommanderMcpServer`, `shell_exec`, and hundreds of following symbols as changed.

Fifth reproduction:
After editing only shell_exec doc comments in `crates/daemon/src/ipc/handlers/command.rs` and `crates/ipc/src/protocol.rs`, impact reports marked multiple following functions/structs as changed; `protocol.rs` produced hundreds of unrelated changed symbols.

Workaround:
Treat the report as an index refresh receipt for this case and use `what_changed` / git diff for exact textual scope.

Impact:
Medium. The post-edit impact output becomes hard to trust after non-symbol top-of-file comment edits.

### 2026-07-07 - `analyze_file_impact` over-reported a one-string JSON fixture edit

Severity: Signal noise

SymForge version:
8.13.6

Tool:
`analyze_file_impact`

Intent:
Refresh the index after changing exactly one string in `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json`.

Input:

```json
{"path":"tests/fixtures/contracts/mcp-tools/shell_exec.v1.json","include_co_changes":false}
```

Expected:
Report the specific changed JSON key/list item, or a concise file-level JSON change.

Actual:
Reported many unrelated top-level and nested keys as changed, including `tool`, `version`, `group`, `_meta`, `policy`, `denied_by_default`, `output`, and `response_example`, even though the edit only changed one string in the `invariants` array.

Second reproduction:
After changing `_meta.note` and renaming one key in `tests/fixtures/contracts/mcp-tools/system_discover.v1.json`, the impact report also marked much of `response_example_daemon_unavailable` as changed.

Third reproduction:
After changing only `_meta.note` in `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json`, the impact report marked `params` and many schema keys as changed.

Workaround:
Use git diff for exact JSON fixture scope; treat SymForge impact as an index refresh receipt only for this case.

Impact:
Medium. JSON impact reports become too broad to drive code review or targeted follow-up.

### 2026-07-07 - Structural replace preserved stale leading doc comments outside the editable symbol

Severity: Workflow friction

SymForge version:
8.13.6

Tools:
`replace_symbol_body`, then `edit_within_symbol`

Intent:
Rename and replace `shell_exec_denied_on_default_profile_e2e` with `shell_exec_allowed_on_default_profile_e2e`, then update the leading doc comment.

Expected:
Either the leading doc comment should be included in the symbol range for replacement, or `edit_within_symbol` should provide a supported way to edit the preserved leading doc comment.

Actual:
`replace_symbol_body` preserved the old leading doc comment:

```text
/// Default-deny: on the default `developer_local` profile the
/// `allow_shell` capability is OFF...
```

Then `edit_within_symbol` rejected the doc-comment replacement because the comment was not inside the symbol body.

Additional reproductions:
The same preservation happened after replacing `shell_exec_denied_default_profile` in `crates/daemon/tests/shell_runtime.rs` and `shell_exec_denied_on_default_profile_maps_to_policy_denied` in `crates/daemon/tests/ipc_command.rs`.

Workaround:
Use a direct text patch for the preserved leading doc comment, then call `analyze_file_impact`.

Impact:
Medium. After a symbol rename/replacement, stale Rust doc comments can survive outside the symbol-edit range and require a non-SymForge patch.

## Entry Template

### YYYY-MM-DD - Short title

Severity:

SymForge version:

Tool:

Intent:

Input:

Expected:

Actual:

Workaround:

Impact:

Raw evidence:
