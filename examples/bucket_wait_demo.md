# Example: bucket_wait avoids polling

Goal: an LLM kicks off a build, waits for a matching signal event,
and pulls bounded context around it — without ever reading raw
terminal output directly.

## Step-by-step

1. LLM tool call: `system_discover()` -> confirms `bucket_wait`,
   `bucket_events_since`, `event_context` are available.

2. (Out of scope at MVP) LLM kicks off the build via a future
   `command_start_combed` tool. For now, the operator starts the
   command via the CLI and shares the resulting `bucket_id` with
   the LLM.

3. LLM tool call: `bucket_wait(bucket_id="build_42", cursor=0,
   severity_min="high", timeout_ms=30000)`.

4. Response cases:

   - **Events arrived:**
     ```json
     {
       "bucket_id": "build_42",
       "next_cursor": 1842,
       "heartbeat": false,
       "events": [{
         "kind": "compile_error",
         "summary": "rustc E0432: unresolved import...",
         "pointer": { "frame_id": "frm_...", "context_available": true }
       }]
     }
     ```
     The LLM responds to the user with the summary. It DOES NOT
     ask for raw stdout.

   - **Timeout:**
     ```json
     {
       "bucket_id": "build_42",
       "next_cursor": 0,
       "heartbeat": true,
       "events": []
     }
     ```
     The LLM tells the user "build is still running, no high-
     severity signals yet" and EITHER waits again or asks the user
     whether to keep waiting.

5. If the event has `pointer.context_available = true`, the LLM may
   call `event_context(probe_id, anchor=pointer.frame_id, before=3,
   after=5)` to retrieve bounded context for diagnosis.

6. The LLM NEVER calls `event_context` with unbounded `before`/`after`
   — the server caps the response at `MAX_WINDOW_BYTES` (64 KiB).

## Anti-pattern

```text
LLM: "Let me read the entire stdout to see what's happening..."
```

This burns tokens and races the build. The bucket_wait path returns
ONLY structured signal events; raw frames live in the context ring
and are retrieved on demand via `event_context`.
