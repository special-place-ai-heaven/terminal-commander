# D — Hygiene (stale deferred comments + unused enum variant)

**Re-verified 2026-05-28:** stale `deferred to TCxx` comments + unused `ToolStatus::NotImplemented` confirmed against `main`; no content changes. Low priority; do after B1.

---

## D1 — Stale “deferred to TCxx” comments in `crates/**/src`

**Symptom:** Module-level comments reference future TC goals; some goals are **shipped**, misleading readers and agents.

**Verified samples (SymForge `search_text` `deferred` in `crates/**/src`):

| File | Lines | Text (abridged) |
|------|-------|-----------------|
| `crates/core/src/context.rs` | 21 | eviction deferred to TC18 / TC22 |
| `crates/daemon/src/command.rs` | 28 | commands deferred to TC44 |
| `crates/daemon/src/router.rs` | 13 | transport deferred to TC37 |
| `crates/sifters/src/noise.rs` | 20 | persistence deferred to TC12 |
| `crates/probes/src/pty.rs` | 17, 22 | spawn deferred |
| `crates/cli/src/main.rs` | 8 | UDS adapter deferred per TC21 |

**Note:** No literal `TCxx` placeholder string in tree — comments use specific IDs (TC12, TC18, etc.).

**Work:**

1. For each comment, check goal file / implementation status (e.g. TC21 IPC exists — `router.rs` may be stale).
2. Replace with: (a) remove if done, (b) `// Future: …` with link to open goal, or (c) `// See ARCHITECTURE.md §…`.
3. Do **not** mass-delete without per-file verification.

**Effort:** M (triage ~10 files)  
**Test impact:** None (comments only).

---

## D2 — `ToolStatus::NotImplemented` unused

**Symptom:** Enum variant exists; catalogue uses only `Live`.

**Citation:**

```68:73:crates/mcp/src/tools.rs
pub enum ToolStatus {
    Live,
  NotImplemented,
}
```

SymForge: `NotImplemented` appears **only** at definition — no `tool_catalogue()` entry uses it.

**Options:**

| Option | Action |
|--------|--------|
| A | Remove variant + serde consumers if any external doc references |
| B | Reserve for future tools with `#[allow(dead_code)]` + comment |
| C | Use for genuinely stub tools in `tool_catalogue()` if any exist |

**Recommendation:** Option A or B after grep of docs/fixtures for `not_implemented` status string.

**Effort:** S  
**Test impact:** Update contract fixtures if they enumerate status enum.

---

## Execution order

1. Grep fixtures/docs for `not_implemented` / `NotImplemented`.  
2. Decide D2 (remove vs document).  
3. Triage D1 file-by-file against `goals/` and `ARCHITECTURE.md`.  
4. Single PR `chore: hygiene deferred comments and tool status enum`.
