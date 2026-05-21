# Example: dynamic rule creation through MCP

Goal: an LLM creates and tests a new sifter rule against live output
without restarting the daemon.

## Step-by-step

1. LLM observes a recurring noise pattern in the bucket (e.g. an
   app emitting "DEBUG flushed 1024 bytes" twice per second).

2. LLM tool call: `registry_create(rule)` with:
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
   Response: `{"version": 1}` (or `{"version": 2}` on edit).

3. LLM tool call: `registry_test(rule, input="DEBUG flushed 2048 bytes")`
   to verify the regex matches. Validation already happened at
   `registry_create` time (TC09 + TC14).

4. LLM tool call: `registry_activate(rule_id, version)` — server
   evaluates a `PolicyAction::RegistryActivate`. Under
   `developer_local` the verdict is `AllowWithAudit`; activation
   record is written.

5. Subsequent matches collapse via TC11 dedupe (5s window by default).
   The LLM no longer sees those events as new signal — only the
   first occurrence, with `count > 1` once dedupe kicks in.

## Anti-pattern

Bypassing the registry by inlining a regex into a one-shot tool
call: this loses dedupe, retention, and operator-visible activation
history. Every persistent sifter rule MUST live in the registry.
