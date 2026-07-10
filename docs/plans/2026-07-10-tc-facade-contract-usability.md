# Compact Facade Contract Usability Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Terminal Commander's compact facade advertise the correct per-action arguments and give incomplete `run_and_watch` calls an explicit resumable handoff.

**Architecture:** Preserve the flat facade input schema because Anthropic rejects top-level `oneOf`/`allOf`; runtime strict validation remains the source of truth. Put the action matrix and conditional scope shape in the facade descriptions, and derive a `command.wait(bucket_id, cursor, timeout_ms)` hint in the shared run-and-watch result builder whenever a normal result is incomplete.

**Tech Stack:** Rust, rmcp, schemars, serde_json, cargo-nextest.

---

### Task 1: Lock the advertised compact contract with RED tests

**Files:**
- Modify: `crates/mcp/src/surface_list.rs`
- Modify: `crates/mcp/src/tools.rs`

1. Add assertions that the command facade description names the accepted/required fields for `run_and_watch`, `wait`, and `summary`, including the 60,000 ms cap and the `bucket_id`/`cursor` handoff.
2. Add an assertion that the registry facade description shows `scope={"kind":"global"}` and says it is required when `import_pack` uses `activate=true`.
3. Add a run-and-watch result test that expects an incomplete normal result to carry a non-null `recover_hint` naming `command.wait`, `bucket_id`, `cursor`, and `timeout_ms`.
4. Run the focused tests and confirm they fail only because the new guidance is absent.

### Task 2: Implement the minimal shared fixes

**Files:**
- Modify: `crates/mcp/src/surface_list.rs`
- Modify: `crates/mcp/src/tools.rs`
- Modify only the shared error/remedy source identified by the lane investigation if compact `exec` is mislabeled.

1. Expand `COMMAND_FACADE_DESCRIPTION` and the matching `#[tool]` description with a compact per-action matrix for the reported actions.
2. Expand `REGISTRY_FACADE_DESCRIPTION` and its matching `#[tool]` description with the conditional import scope example.
3. In `run_and_watch_result`, synthesize a concrete wait-resume hint only when `complete=false`, `degraded=false`, and no stronger recovery hint was supplied.
4. Keep legacy `shell_exec` naming on legacy tool surfaces; change only compact-facade guidance that should say `action="exec"`.
5. Run the focused tests and confirm GREEN.

### Task 3: Verify impact and runtime behavior

**Files:**
- Modify: `tasks/todo.md`

1. Run SymForge `analyze_file_impact` for each edited Rust file.
2. Run `cargo fmt --check`.
3. Run focused `cargo nextest` tests for the MCP crate, then the workspace suite using the documented local exclusion if needed.
4. Run `bash scripts/smoke/verify-runtime-smoke.sh` because it is the fastest daemon+MCP end-to-end gate.
5. Dogfood the compact `command` and `registry` schemas/receipts through Terminal Commander.
6. Record exact commands and outcomes in `tasks/todo.md`.
