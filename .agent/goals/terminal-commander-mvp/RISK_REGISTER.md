# Risk Register - terminal-commander-mvp

| Risk | Level | Mitigation goal(s) | Status |
|---|---:|---|---|
| LLM-facing interface becomes an unrestricted root shell | High | TC02, TC22, TC23, TC24 | Pending |
| Sensitive files or credentials exposed through file tools | High | TC02, TC22, TC24, TC29 | Pending |
| Raw noisy output leaks through bucket or MCP responses | High | TC05, TC07, TC17, TC23, TC24, TC29 | Pending |
| Regex rules cause runtime denial-of-service | High | TC09, TC10, TC13, TC29 | Pending |
| Large terminal streams overwhelm memory or event stores | High | TC11, TC12, TC17, TC28 | Pending |
| Provider-specific hooks become required for correctness | Medium | TC17, TC23, TC27 | Pending |
| WSL/systemd assumptions are wrong | Medium | TC01, TC26 | Pending |
| PTY prompt handling captures secrets | High | TC19, TC29 | Pending |
| Dynamic registry accepts stale or unsafe rules | Medium | TC13, TC14, TC24 | Pending |
| Goal chain drifts into broad uncontrolled edits | Medium | All goals through branch guard and allowed_files_or_area | Pending |
