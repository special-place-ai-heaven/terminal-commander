# Risk Register - terminal-commander-mvp

**Historical index (frozen).** Canonical open risks: root `RISK_REGISTER.md` (TC48 snapshot). Reconciled via [ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69).

| Risk | Level | Mitigation goal(s) | Status | Pointer |
|---|---:|---|---|---|
| LLM-facing interface becomes an unrestricted root shell | High | TC02, TC22, TC23, TC24 | Mitigated | Root H-04; TC40–TC41; argv-only + policy gates |
| Sensitive files or credentials exposed through file tools | High | TC02, TC22, TC24, TC29 | Mitigated | TC43 path deny; TC38 policy |
| Raw noisy output leaks through bucket or MCP responses | High | TC05, TC07, TC17, TC23, TC24, TC29 | Mitigated | TC39/TC47 bounded output |
| Regex rules cause runtime denial-of-service | High | TC09, TC10, TC13, TC29 | Mitigated | TC42 registry validation + tests |
| Large terminal streams overwhelm memory or event stores | High | TC11, TC12, TC17, TC28 | Mitigated | TC47 retention/drop counters |
| Provider-specific hooks become required for correctness | Medium | TC17, TC23, TC27 | Open | Root R-01; BACKLOG P1.2–P1.4 |
| WSL/systemd assumptions are wrong | Medium | TC01, TC26 | Mitigated | `docs/install/`; TC26 |
| PTY prompt handling captures secrets | High | TC19, TC29 | Mitigated | Root H-06; TC44 |
| Dynamic registry accepts stale or unsafe rules | Medium | TC13, TC14, TC24 | Mitigated | TC42–TC42d |
| Goal chain drift | Medium | All goals | Superseded | [ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69) freeze + runtime chain |
