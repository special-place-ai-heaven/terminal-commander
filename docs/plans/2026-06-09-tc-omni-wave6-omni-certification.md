# Wave 6 — Omni Certification + Product Realignment

**Goal:** Prove 100% omni acceptance, realign docs with vision, ship agent playbook so any LLM harness can rely on TC alone.

**Depends on:** All prior waves (certification gates on matrix).

**Acceptance:** O-14 + full omni matrix green.

---

## Deliverables

### D6-1 — Omni acceptance test suite (TC70)

Automate O-01 through O-14:

| Script | Platform |
|---|---|
| `scripts/smoke/verify-omni-linux.sh` | Linux native |
| `scripts/smoke/verify-omni-wsl.sh` | WSL |
| `scripts/smoke/verify-omni-windows.ps1` | Windows native |
| `scripts/smoke/verify-omni-macos.sh` | macOS |

Each script:

1. Ensures daemon up.
2. Runs MCP `call_tool` sequence (or CLI equivalent).
3. Exits non-zero on any matrix failure.
4. Writes evidence JSON for CI artifact.

Integrate into GitHub Actions matrix.

---

### D6-2 — Provider harness full parity (TC71)

Extend TC46 smokes to cover omni scenarios per provider:

| Provider | Config | Omni scenarios |
|---|---|---|
| Cursor | `setup harness --provider cursor` | shell, session, pty, run_and_watch |
| Codex CLI | codex-cli stanza | same |
| Claude Code | claude-code stanza | same |
| Claude Desktop | claude-desktop stanza | same |

Document in `docs/testing/omni-trust-test-goal.md` (extend cursor/codex trust goals).

---

### D6-3 — Documentation realignment (TC72)

| Document | Update |
|---|---|
| `README.md` | Primary: omni LLM terminal tool; table row "generic shell bridge" → "opt-in shell capability" |
| `SPEC.md` | Omni program scope; move items from deferred |
| `ROADMAP.md` | TC49–TC72 omni wave |
| `docs/mcp/OMNI_PLAYBOOK.md` | **New** — decision tree for agents |
| MCP server instructions | Update `user-terminal-commander` descriptor |

**OMNI_PLAYBOOK decision tree (outline):**

```text
Need to run something?
├─ Known tool (npm, cargo, ...) → registry_import_pack → run_and_watch
├─ One-shot shell pipeline → shell_exec (if enabled)
├─ Multi-step shell state → shell_session_*
├─ Interactive REPL → pty_command_*
├─ Unknown output → run_and_watch → command_output_tail → registry_suggest
├─ Remote host → target_id on any tool
└─ System install → privileged_exec (if enabled + approved)
```

---

### D6-4 — `system_discover.omni_status` (TC73)

```json
{
  "omni_status": {
    "program_version": "1.0",
    "matrix": {
      "shell_exec": {"available": true, "reason": null},
      "pty": {"available": true, "platform": "windows_conpty"},
      "privileged_helper": {"available": false, "reason": "not_configured"},
      "remote_targets": {"count": 2, "reachable": 1}
    },
    "evidence_ref": "git_sha"
  }
}
```

---

### D6-5 — Version bump + release (TC74)

- Semver: omni complete → **0.2.0** or **1.0.0** (product decision).
- Release notes: omni capability matrix.
- npm + GitHub release.

---

## Certification checklist

Before declaring omni complete:

- [ ] All O-01..O-14 green on Linux
- [ ] All O-01..O-14 green on WSL
- [ ] O-03, O-07 green on Windows native
- [ ] O-08 green on macOS
- [ ] O-09 green (SSH target)
- [ ] O-06 green with test helper in CI VM (or documented manual gate)
- [ ] Four provider harness smokes green
- [ ] No P0 BACKLOG items open
- [ ] README/SPEC reflect omni identity
- [ ] Adversarial security review on shell + privileged paths

---

## Effort

**2–3 weeks** (overlaps with final integration testing from prior waves).

---

## Post-omni (optional v2)

Not required for 100% declaration but valuable:

- Kernel sandbox (Landlock/seccomp) — BACKLOG P2
- Audit hash chain tamper evidence
- LLM-in-daemon rule synthesis (Wave 3 v2)
- Multi-target subscriptions in one pull
