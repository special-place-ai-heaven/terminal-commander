# Wave 3 — Parse Omni (Auto-Rules + Pack Expansion)

**Goal:** Close the "parse anything" gap so LLMs never need raw scrollback for unknown commands — structured signals or a closed suggest→test→activate loop.

**Depends on:** Wave 0 (stable tail + registry tools). Can parallel Wave 2.

**Acceptance:** O-05.

---

## Workstreams

### WS-3A — `registry_suggest_from_samples` (TC57)

**New MCP tool:**

```json
{
  "samples": ["error: build failed", "warning: unused variable", ...],
  "intent": "extract errors and warnings",
  "max_rules": 5
}
```

**Response:**

```json
{
  "proposed_rules": [
    {"kind": "regex", "pattern": "^error:", "severity": "high", ...},
    ...
  ],
  "confidence": "heuristic",
  "next_steps": ["registry_test", "registry_upsert", "registry_activate"]
}
```

**Implementation tiers:**

1. **v1 (ship first):** Heuristic line classifiers — error prefixes, warning prefixes, `FAILED`, exit-code lines, URL/file path patterns. Pure Rust, no LLM in daemon.
2. **v2 (optional):** Daemon calls local LLM for rule synthesis — behind config flag, off by default (operator trust).

**Invariant:** Suggest never auto-activates. Always requires explicit `registry_test` + `registry_activate`.

---

### WS-3B — Universal always-on extractors (TC58)

Low-severity signals emitted even without custom rules:

| Extractor | Pattern |
|---|---|
| stderr_error_line | `(?i)^(error\|fatal\|panic\|exception)` |
| warning_line | `(?i)^warning` |
| process_exit | synthetic on job exit (already partial) |
| progress_tick | reuse stall/progress sifters globally |

Config: `sifters.universal_extractors = true` (default on for omni profile).

---

### WS-3C — Rule pack expansion (TC59)

**Current:** 8 packs (`generic.terminal`, apt, cargo, npm, pytest, gcc, make, cleanup).

**Target:** 25+ packs covering 90% of agent workflows:

| Pack | Priority |
|---|---|
| docker | P0 |
| kubectl | P0 |
| git | P0 |
| systemd/journal | P1 |
| pip/uv | P1 |
| go/rustc | P1 |
| msbuild/dotnet | P1 (Windows) |
| winget/choco | P1 (Windows) |
| terraform | P2 |
| ansible | P2 |

Each pack: JSON in `crates/store/rules/`, tests in `registry_live_e2e.rs`, docs in `docs/rules/packs/`.

---

### WS-3D — Auto-pack hint in responses (TC60)

When `run_and_watch` or `command_start_combed` starts `docker` and no docker rules active:

```json
{
  "hint": {
    "kind": "pack_available",
    "pack": "docker",
    "action": "registry_import_pack"
  }
}
```

Metadata only — never silent import.

---

## Closed-loop workflow (O-05)

```text
1. run_and_watch argv=["./unknown-tool"] rules=[]
   → receipt + tail_available: true

2. command_output_tail job_id=...
   → bounded lines

3. registry_suggest_from_samples samples=[...tail lines...]
   → proposed_rules

4. registry_test definition=... samples=[...]
   → match proof

5. registry_upsert + registry_activate scope={kind:global}

6. re-run unknown-tool
   → signals only, no tail needed
```

---

## Effort

| Goal | Weeks |
|---|---|
| TC57 suggest v1 | 1.5 |
| TC58 universal extractors | 1 |
| TC59 pack expansion | 1.5 |
| TC60 hints + docs | 0.5 |
| **Total** | **~4.5 weeks** |

---

## Risks

| Risk | Mitigation |
|---|---|
| Bad suggested rules | mandatory registry_test; confidence field |
| Token bloat from universal extractors | severity floor; dedupe; cap rate |
| Pack maintenance burden | community contribution guide; pack versioning |
