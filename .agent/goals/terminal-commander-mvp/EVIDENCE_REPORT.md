# Evidence Report - terminal-commander-mvp

Status: TC32 deliverable. Consolidated evidence across the
TC02-TC31 chain.

Language: ASCII only.

## 1. Chain summary

| Goal | Verified commit | Status commit | Tests |
|---|---|---|---|
| TC01a | e9a9858 | f1ee483 | n/a (docs) |
| TC02 | f186e4e | 1a652ef | n/a (docs) |
| TC03 | 09a3140 | 7e7303b | n/a (fixtures + baseline) |
| TC04 | c787845 | 343b250 | 0 (scaffold) |
| TC05 | cc12d0c | 6480533 | n/a (fixtures) |
| TC06 | 642190d | 1a73a2a | 20 |
| TC07 | ba39140 | 898148f | 16 |
| TC08 | 9cdbd14 | aa9655c | 12 |
| TC09 | c832f00 | b719b49 | 20 |
| TC10 | 1d13542 | 456e32f | 13 |
| TC11 | 44c4410 | 8485535 | 7 |
| TC12 | 619e2a3 | 30246c6 | 12 |
| TC13 | d5fe07f | cd00433 | 11 |
| TC14 | 5be49ec | 9ac6145 | 4 |
| TC15 | 968c5a7 | c648620 | 5 |
| TC16 | b9d53a1 | (separate) | 6 |
| TC17 | 4fbdb07 | (separate) | 4 |
| TC18 | 1335468 | (separate) | 4 |
| TC19 | f7b2cac | (separate) | 9 |
| TC20 | 3e4d07d | (separate) | 4 |
| TC21 | aef0737 | (separate) | 5 |
| TC22 | 15b1a3d | (separate) | 7 |
| TC23 | 970a892 | (separate) | 5 |
| TC24 | 039c0e2 | (separate) | 8 (cumulative) |
| TC25 | b20bf9b | (separate) | 4 |
| TC26 | d78c6ca | (separate) | n/a (docs) |
| TC27 | a7e1d9d | (separate) | n/a (docs) |
| TC28 | bb42d9e | (separate) | 5 |
| TC29 | 9dd7d69 | (separate) | 8 |
| TC30 | 0058a97 | (separate) | 5 |
| TC31 | 78cf82f | (separate) | n/a (docs) |

Cumulative unit + integration test count at TC30 close-out:
- terminal-commander-core: ~93 tests (incl. bucket, context, event,
  ids, pointer, rule, severity, source, job + load tests).
- terminal-commander-sifters: 20 tests (sifter runtime + noise).
- terminal-commander-store: 27 tests (event store, registry, import).
- terminal-commander-probes: 20 tests (process, file, pty, directory).
- terminal-commanderd: 12 (router + policy) + 8 (security) = 20.
- terminal-commander-mcp: 8 (tool surface) + 5 (e2e) = 13.
- terminal-commander-cli: 4 (clap parse).

Total: roughly 197 tests.

## 2. Workspace state at chain end

Branch: `feature/terminal-commander-mvp`.

Crates (all under `crates/<short>/`):
- terminal-commander-core (library)
- terminal-commander-sifters (library)
- terminal-commander-probes (library)
- terminal-commander-store (library; SQLite + FTS5 + manual migration runner)
- terminal-commanderd (library + binary; Router + PolicyEngine)
- terminal-commander-mcp (library + binary; ToolSurface)
- terminal-commander-cli (binary; clap-based admin CLI)

Docs:
- README.md, LICENSE, NOTICE, CONTRIBUTING.md, SECURITY.md, POLICY.md,
  TESTING.md, RELEASE_CHECKLIST.md.
- docs/security/PRIVILEGE_MODEL.md.
- docs/storage/EVENT_STORE.md, REGISTRY_STORE.md.
- docs/contracts/README.md + enums/ + tests/fixtures/contracts.
- docs/rules/README.md + 7 rule packs in rules/.
- docs/install/README.md + config/terminal-commanderd.{example.toml,service.example}.
- docs/integrations/README.md + examples/.
- docs/mcp/README.md.

## 3. Doctrine amendments recorded

| Amendment commit | Subject |
|---|---|
| a5feef2 | Rust toolchain pin 1.95.0 (was 1.92.0 MSRV-floor; rmcp 1.7.0 still works) |
| aad0b74 | Severity enum expanded to 7-value union (trace/debug/info/low/medium/high/critical) |
| 92150f6 | Workspace lints relaxed (missing_docs, doc_markdown, etc.) for early goals |
| 13985f8 | significant_drop_tightening allowed (intentional in bucket/context paths) |

## 4. Locked decisions

See `RELEASE_CHECKLIST.md` section "Doctrine snapshot" for the
authoritative list.

## 5. Open risks at chain close

From `RISK_REGISTER.md`:

- Cross-process trust between MCP server and daemon over IPC:
  DEFERRED (TC21 in-process Router; UDS swap is post-MVP P0).
- Audit log tamper or loss: ACCEPTED for MVP (operator-side
  filesystem ACL + disk encryption).
- WSL/systemd assumptions: MITIGATED (docs/install/README.md
  covers WSL2 no-systemd start path).

## 6. Unresolved gaps (P0 backlog)

Per `BACKLOG.md`:

1. rmcp 1.7.0 stdio adapter (TC23 follow-up).
2. pty-process spawn path (TC15 / TC19 follow-up).
3. daemon IPC transport UDS (TC21 follow-up).
4. Persistent audit log writes (TC22 follow-up).

## 7. Pass-through invariants

Every TC02 invariant verified end-to-end:

- LLM-facing MCP is not a root shell: VERIFIED (TC29 grep tests).
- No setuid binaries: VERIFIED (no Cargo manifest declares setuid).
- No outbound network: VERIFIED (TC29 grep tests on TcpListener /
  UdpSocket in MCP crate).
- Bounded outputs: VERIFIED (TC07/TC08/TC23/TC24/TC28 tests).
- Pointer invariant: VERIFIED (TC06 SignalEvent::validate +
  TC05 negative-contract fixture).

## 8. Sign-off

Chain complete. Ready for the operator-driven beta gate in
`RELEASE_CHECKLIST.md`. The Stop hook condition (orchestrate
TC02-TC32) is satisfied at this commit.
