# Work order: include command completion status in bucket_wait

Status: Open
Created: 2026-10-01
Repository: terminal-commander
Priority: Minor usability improvement

## Prompt for the implementation agent

Implement this work item in terminal-commander. Read and follow repository AGENTS.md and the current development instructions. Investigate the existing contracts first, implement the smallest compatible change, run the required checks, and report the actual results. Do not publish or push unless separately authorized.

### Problem and desired behavior

A Codex user reports that TC is reliable, but after waiting for command output with `bucket_wait`, the agent needs a separate `command_status` call to confirm the job's final state and exit code. Make it possible for the wait response to include that authoritative completion information for the command being watched. `run_and_watch` already provides a useful combined result for shorter commands.

The earlier reported slowdown was inside WSL: TC returned promptly and correctly reported commands as still running. Restarting WSL and adding a Malwarebytes exclusion happened together, after which package repair finished in seconds. The improvement cannot be attributed to either change individually. This work item concerns the extra status round trip; it is not evidence of a TC performance fault.

### Investigation and implementation scope

- Trace `bucket_wait` from its MCP input schema and response payload through the IPC contract and daemon handler. It currently returns bucket, cursor, heartbeat, dropped-event, and event information.
- Inspect the existing `command_status` authority, including live execution lanes and persisted/reconstructed outcomes. Reuse its semantics and preserve `outcome_trust`; do not infer successful completion or an exit code from silence, a heartbeat, or an output event alone.
- Compare `run_and_watch`, which currently obtains command status separately.
- Resolve command association explicitly. `bucket_wait` is generic and accepts a bucket ID, while buckets may be shared or unrelated to commands. Investigate whether a reliable unique association already exists. Optional `job_id` input and optional status metadata are a proposed approach, not a predetermined requirement. Avoid guessing a job for an ambiguous bucket.
- Preserve existing event delivery, cursor advancement, timeout behavior, and compatibility for callers that only wait on buckets. Keep the change additive where practical.
- Ensure returned status and completion claims remain correct during completion races, daemon restart, unavailable jobs, and persisted outcomes. Document the observation semantics rather than promising an atomic event/status snapshot unless the implementation guarantees it.
- Update the relevant tool description, schema, and usage documentation so an agent can use the combined response without routinely calling `command_status` afterward.

### Acceptance criteria

1. A caller watching a specific command can receive its authoritative final state and exit code from `bucket_wait` when completion is observed, without an additional client-side `command_status` request.
2. Successful and nonzero command exits are both represented correctly. Cancellation, failure, or other terminal states use existing command-status semantics; unavailable exit codes remain unavailable rather than becoming zero.
3. A timeout or heartbeat while the command is running does not falsely report completion. Existing output and cursor behavior remains correct.
4. Generic buckets and shared buckets retain usable behavior. Any supplied command association is validated consistently with existing API error conventions and cannot silently report a different command's outcome.
5. Live and persisted/reconstructed results retain the existing trust information. Test relevant completion races and restart behavior where the implementation touches them.
6. Existing callers remain compatible, and the MCP response schema accurately describes the new result.
7. Add focused behavioral tests for the cases above, run the relevant existing tests and required platform gates, and report any failures or checks that could not run.

### Verification and handoff

Use README.md, CONTRIBUTING.md, and TESTING.md for current commands. Workspace tests require cargo-nextest. Follow CONTRIBUTING.md section 6.1 for OS gates when changing tests or platform-specific code. Run the daemon/MCP runtime smoke check when appropriate to the contract change; use its documented environment and target-directory requirements.

Before declaring completion, review the actual diff against this scope, verify the behavior with tooling, and provide a concise summary of the final input/output contract, checks run, and any remaining limitations. Do not expand this into WSL tuning, antivirus configuration, or a general performance overhaul.
