# Example: dynamic rule creation through MCP

Goal: an LLM creates and tests a new sifter rule against live output
without restarting the daemon.

## Step-by-step

1. LLM observes a recurring noise pattern in the bucket (e.g. an
   app emitting "DEBUG flushed 1024 bytes" twice per second).

2. LLM tool call: `registry_upsert(definition_json)` where
   `definition_json` is the JSON-encoded string of:
   ```json
   {
     "id": "myapp.debug-flush",
     "version": 1,
     "kind": "regex",
     "status": "draft",
     "severity": "low",
     "event_kind": "noise",
     "stream": "stdout",
     "pattern": "^DEBUG flushed [0-9]+ bytes$",
     "summary_template": "debug flush noise",
     "tags": ["myapp", "noise"]
   }
   ```
   The store assigns the version (latest + 1); the response returns it.

3. LLM tool call: `registry_test(rule_id, samples=[{"text": "DEBUG flushed
   2048 bytes"}])` to verify the regex matches. Validation already
   happened at `registry_upsert` time.

4. LLM tool call: `registry_activate(rule_id, version, scope={"kind":
   "global"})` (`scope` is required) — server
   evaluates a `PolicyAction::RegistryActivate`. Under
   `developer_local` the verdict is `AllowWithAudit`; activation
   record is written.

5. Subsequent matches collapse via TC11 dedupe (5s window by default).
   The LLM no longer sees those events as new signal — only the
   first occurrence, with `count > 1` once dedupe kicks in.

## Anti-pattern

Bypassing the registry by inlining a regex into a one-shot tool
call: this loses dedupe, retention, and operator-visible activation
history. Every persistent sifter rule MUST live in the registry. (One-off inline
`rules` on `run_and_watch` / `command_start_combed` are a supported
per-job shortcut, but they are not persisted or activated.)
