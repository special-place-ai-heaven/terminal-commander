# Risk Register - terminal-commander-mvp

Owner: this file is the canonical register. Each TC goal that adds
behavior touching a row MUST surface its mitigation status in its
final report. Severity uses High / Medium / Low. Status uses Pending
/ Active mitigation / Mitigated / Accepted / Deferred (post-MVP).

| Risk | Level | Mitigation goal(s) | Status |
|---|---:|---|---|
| LLM-facing interface becomes an unrestricted root shell | High | TC02, TC22, TC23, TC24 | Active mitigation (TC02 doctrine locked; engine in TC22) |
| Sensitive files or credentials exposed through file tools | High | TC02, TC22, TC24, TC29 | Active mitigation (TC02 default-deny list; engine in TC22) |
| Raw noisy output leaks through bucket or MCP responses | High | TC05, TC07, TC17, TC23, TC24, TC29 | Pending |
| Regex rules cause runtime denial-of-service (ReDoS) | High | TC09, TC10, TC13, TC29 | Active mitigation (TC02 names compile/step time limits in policy) |
| Large terminal streams overwhelm memory or event stores | High | TC11, TC12, TC17, TC28 | Pending |
| Provider-specific hooks become required for correctness | Medium | TC17, TC23, TC27 | Pending |
| WSL/systemd assumptions are wrong | Medium | TC01, TC26 | Mitigated for TC01 (no-systemd assumption locked); TC26 implements |
| PTY prompt handling captures secrets | High | TC19, TC29 | Pending |
| Dynamic registry accepts stale or unsafe rules | Medium | TC13, TC14, TC24 | Pending |
| Goal chain drifts into broad uncontrolled edits | Medium | All goals through branch guard and allowed_files_or_area | Active mitigation (branch guard + allowed_files enforced) |
| Command execution lacks policy mediation | High | TC02, TC22, TC25, TC29 | Active mitigation (TC02 locks B1-B3 + commands.deny list) |
| Root access path enters the codebase via sudo/polkit/setuid | High | TC02, TC22, TC26, TC29 | Active mitigation (TC02 deny `sudo`/`doas`/`su`/`pkexec`/`kexec`; no setuid; helper deferred) |
| File exfiltration outside policy-allowed paths | High | TC02, TC18, TC20, TC22, TC24, TC29 | Active mitigation (TC02 cap-std Dir + default-deny + per-profile allow-set) |
| Raw-output leakage past bounded context windows | High | TC02, TC05, TC07, TC08, TC17, TC23, TC29 | Active mitigation (TC02 B5: structured events + bounded event_context only) |
| Audit log retention growth or rotation gap | Medium | TC02, TC12, TC22, TC25 | Active mitigation (TC02 retention defaults: 30d audit / 24h buckets / 1h spool) |
| Audit log tamper or loss | Medium | TC02, TC22 (post-MVP hash chain) | Accepted for MVP (operator-side filesystem ACL + disk encryption) |
| Privileged helper added without doctrine update | High | TC02 (this file forbids), TC22, TC26 | Active mitigation (TC02 section 5 of PRIVILEGE_MODEL.md pre-binds the constraint) |
| MCP server gains direct process-spawn or socket capability | High | TC02, TC22, TC23, TC29 | Active mitigation (TC02 PRIVILEGE_MODEL.md section 9 + TC29 grep tests) |
| Cross-process trust (MCP <-> daemon IPC) not yet pinned | Medium | TC21 | Deferred (TC21 picks transport; constrained to local-only per TC02) |
| Telemetry or network egress added without policy extension | Medium | TC02, TC22 | Active mitigation (TC02 SECURITY.md section 8: no telemetry; POLICY.md section 7) |
| Profile drift: runtime mutation of active profile | Medium | TC02, TC22, TC25 | Active mitigation (TC02 POLICY.md section 3: restart-boundary only) |
| Default-deny path silently overridden | High | TC02, TC22, TC29 | Active mitigation (TC02 POLICY.md section 5: explicit i_understand_risk + audit emission) |

## Notes

- Severity reflects MVP-window impact, not absolute worst case.
- "Accepted" entries (e.g. audit-log tamper) MUST be revisited at
  TC32 (evidence review and backlog refinement).
- Adding a new high-risk row requires a corresponding doctrine
  update in `SECURITY.md`, `POLICY.md`, or
  `docs/security/PRIVILEGE_MODEL.md`. Risk additions without
  doctrine support are out-of-doctrine and must be rejected at
  review.

## TC02 attribution

The rows added or upgraded in TC02 (2026-05-21):

- Command execution lacks policy mediation
- Root access path enters the codebase via sudo/polkit/setuid
- File exfiltration outside policy-allowed paths
- Raw-output leakage past bounded context windows
- Audit log retention growth or rotation gap
- Audit log tamper or loss
- Privileged helper added without doctrine update
- MCP server gains direct process-spawn or socket capability
- Cross-process trust (MCP <-> daemon IPC) not yet pinned
- Telemetry or network egress added without policy extension
- Profile drift: runtime mutation of active profile
- Default-deny path silently overridden

Pre-existing rows had their Status field upgraded to reflect
TC02's doctrine landing.
