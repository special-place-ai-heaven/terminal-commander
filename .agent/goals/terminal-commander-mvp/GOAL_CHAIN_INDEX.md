# Goal Chain Index - terminal-commander-mvp

## Goal Chain Overview

- Chain name: `terminal-commander-mvp`
- Goal prefix: `TC`
- Target repository: `https://github.com/special-place-administrator/terminal-commander.git`
- Target output folder: `.agent/goals/terminal-commander-mvp/`
- Target branch: `feature/terminal-commander-mvp`
- Prohibited branches: `["main", "master"]`
- Total goals: 32
- Mode: mixed planning + implementation
- Destructive-change policy: no destructive changes, system installs, privileged helper behavior, or broad filesystem operations unless a specific goal explicitly allows and verifies safe dry-run behavior.

## Source Evidence Used

- User request: Terminal Commander / live terminal-stream signal-combing abstraction for LLMs, 2026-05-21
- Repository: https://github.com/special-place-administrator/terminal-commander.git
- User note: repository is initially empty except the generated README.md already added by user
- Planning source: Terminal Commander product specification v0.1 from ChatGPT session

## Assumptions Register Summary

- See `ASSUMPTIONS.md` for confirmed facts, assumptions, and correction protocol.

## Dependency Graph

- TC01 depends_on: none
- TC01a depends_on: TC01
- TC02 depends_on: TC01
- TC03 depends_on: TC01, TC02
- TC04 depends_on: TC01a, TC03
- TC05 depends_on: TC04
- TC06 depends_on: TC05
- TC07 depends_on: TC06
- TC08 depends_on: TC06
- TC09 depends_on: TC06, TC08
- TC10 depends_on: TC09
- TC11 depends_on: TC10
- TC12 depends_on: TC07
- TC13 depends_on: TC09, TC12
- TC14 depends_on: TC10, TC13
- TC15 depends_on: TC10, TC12
- TC16 depends_on: TC15
- TC17 depends_on: TC07, TC12, TC16
- TC18 depends_on: TC10, TC12
- TC19 depends_on: TC15, TC16
- TC20 depends_on: TC18
- TC21 depends_on: TC13, TC16, TC17, TC18
- TC22 depends_on: TC02, TC21
- TC23 depends_on: TC21, TC22
- TC24 depends_on: TC22, TC23
- TC25 depends_on: TC21, TC22
- TC26 depends_on: TC22, TC23, TC25
- TC27 depends_on: TC23, TC24, TC26
- TC28 depends_on: TC11, TC17, TC21
- TC29 depends_on: TC22, TC24
- TC30 depends_on: TC24, TC27, TC28, TC29
- TC31 depends_on: TC26, TC30
- TC32 depends_on: TC31

## Run Order

| Order | Goal ID | Title | Status | Depends On | Target Branch | Intended Outcome |
|---:|---|---|---|---|---|---|
| 1 | TC01 | Research Product Baseline And Source Map | Pending | none | `feature/terminal-commander-mvp` | Create the verified product baseline, source map, architecture notes, and assumptions register for Terminal Commander before implementation begins. |
| 1a | TC01a | Readme License Contributing Reconcile | Pending | TC01 | `feature/terminal-commander-mvp` | Reconcile README, LICENSE, NOTICE, and CONTRIBUTING with TC01 locked decisions (Apache-2.0; 7 crates) so TC04 has a consistent foundation. |
| 2 | TC02 | Security Privilege And Policy Doctrine | Pending | TC01 | `feature/terminal-commander-mvp` | Define the security, privilege, policy, audit, and data-retention doctrine before any command execution or file access implementation exists. |
| 3 | TC03 | Test Methodology And Fixture Plan | Pending | TC01, TC02 | `feature/terminal-commander-mvp` | Create the project test methodology, fixture taxonomy, verification commands, and evidence rules that all later implementation goals must use. |
| 4 | TC04 | Rust Workspace And Toolchain Scaffold | Pending | TC01a, TC03 | `feature/terminal-commander-mvp` | Initialize the Rust workspace, toolchain files, crate skeletons, and baseline build configuration without adding product behavior. |
| 5 | TC05 | Contract Schemas And Golden Fixtures | Pending | TC04 | `feature/terminal-commander-mvp` | Define versioned JSON contract examples for events, buckets, rules, probes, jobs, source pointers, context windows, and policy decisions before implementation. |
| 6 | TC06 | Core Identifiers Events And Source Pointers | Pending | TC05 | `feature/terminal-commander-mvp` | Implement the core typed identifiers, severity enum, event model, source descriptor, and source pointer types with serialization and tests. |
| 7 | TC07 | In Memory Bucket Manager | Pending | TC06 | `feature/terminal-commander-mvp` | Implement an in-memory signal bucket manager with monotonic cursors, bounded reads, severity filtering, summaries, and tests. |
| 8 | TC08 | Context Ring And Bounded Context Windows | Pending | TC06 | `feature/terminal-commander-mvp` | Implement the bounded context ring model that stores normalized frames and returns explicit before/after context windows around source pointers. |
| 9 | TC09 | Rule Model Validation And Templates | Pending | TC06, TC08 | `feature/terminal-commander-mvp` | Implement the rule definition model, validation rules, capture mapping, summary template rendering, and rule test input/output types. |
| 10 | TC10 | Keyword And Regex Sifter Runtime | Pending | TC09 | `feature/terminal-commander-mvp` | Implement the first sifter runtime that evaluates keyword and regex rules against normalized frames and emits structured signal event drafts. |
| 11 | TC11 | Noise Suppression Dedupe And Progress Basics | Pending | TC10 | `feature/terminal-commander-mvp` | Add basic deduplication, suppression, repeated-event collapse, and progress-noise classification to the sifter runtime. |
| 12 | TC12 | Persistent Event Store And Bucket Cursors | Pending | TC07 | `feature/terminal-commander-mvp` | Implement persistent event storage and bucket cursor queries using the chosen storage backend while preserving bounded reads. |
| 13 | TC13 | Registry Store And Rule Crud | Pending | TC09, TC12 | `feature/terminal-commander-mvp` | Implement persistent rule registry CRUD, versioning, search, test metadata, and activation records without live probe binding yet. |
| 14 | TC14 | Seed Rule Packs And Registry Import | Pending | TC10, TC13 | `feature/terminal-commander-mvp` | Create initial rule packs and an import path that seeds the registry with validated generic, apt, cargo, npm, pytest, gcc, make, and terminal rules. |
| 15 | TC15 | Process Probe Streaming Stdout Stderr | Pending | TC10, TC12 | `feature/terminal-commander-mvp` | Implement a non-interactive process probe that starts a command, continuously reads stdout/stderr, normalizes frames, writes context, and feeds the sifter runtime. |
| 16 | TC16 | Job Manager And Command Exit Events | Pending | TC15 | `feature/terminal-commander-mvp` | Implement the job manager that tracks process lifecycle, job status, cancellation, exit codes, non-zero exit events, and command runtime metadata. |
| 17 | TC17 | Realtime Bucket Waiter | Pending | TC07, TC12, TC16 | `feature/terminal-commander-mvp` | Implement bucket_wait semantics so clients can block for matching signal events by cursor, severity, kind filter, and timeout without polling raw output. |
| 18 | TC18 | File Probe Follow Create And Rotate | Pending | TC10, TC12 | `feature/terminal-commander-mvp` | Implement a file probe that can scan or follow files, handle creation after watch start, truncation, and rotation while emitting signal events and bounded context. |
| 19 | TC19 | Terminal Pty Probe And Prompt Detection | Pending | TC15, TC16 | `feature/terminal-commander-mvp` | Add PTY/terminal-style command support with ANSI/carriage-return normalization, prompt detection, and bounded stdin writing for interactive workflows. |
| 20 | TC20 | Directory And Artifact Probes | Pending | TC18 | `feature/terminal-commander-mvp` | Implement directory watching and initial artifact detectors for generated reports without dumping whole artifacts to the LLM. |
| 21 | TC21 | Daemon Local Api And Router | Pending | TC13, TC16, TC17, TC18 | `feature/terminal-commander-mvp` | Implement the daemon router and local API surface that coordinates jobs, probes, registry, buckets, context, and policy placeholders without exposing MCP yet. |
| 22 | TC22 | Policy Engine And Audit Log | Pending | TC02, TC21 | `feature/terminal-commander-mvp` | Implement the first real policy engine and audit log for command execution, file access, registry edits, probe creation, and privileged-operation denial. |
| 23 | TC23 | Mcp Server Discovery Jobs And Buckets | Pending | TC21, TC22 | `feature/terminal-commander-mvp` | Implement the initial MCP server exposing system discovery, command start/status, bucket events_since, bucket_wait, bucket_summary, and event_context tools. |
| 24 | TC24 | Mcp Registry Probe And File Tools | Pending | TC22, TC23 | `feature/terminal-commander-mvp` | Expose dynamic registry, probe, and bounded file tools through MCP so an LLM can search/create/test/activate rules and observe files without terminal access. |
| 25 | TC25 | Admin Cli And Doctor Commands | Pending | TC21, TC22 | `feature/terminal-commander-mvp` | Implement a human admin CLI for status, doctor, rules, buckets, jobs, probes, policy, and audit inspection without bypassing daemon policy. |
| 26 | TC26 | Installer Service And Wsl Startup Docs | Pending | TC22, TC23, TC25 | `feature/terminal-commander-mvp` | Create safe installer, service, configuration, and WSL startup artifacts for local development use without enabling unintended privileged behavior. |
| 27 | TC27 | Provider Harness Integration Examples | Pending | TC23, TC24, TC26 | `feature/terminal-commander-mvp` | Create provider-neutral MCP integration examples and harness notes for Claude Code, Codex CLI, and generic MCP clients without hardcoding secrets or machine-specific paths. |
| 28 | TC28 | Load Performance And Backpressure Tests | Pending | TC11, TC17, TC21 | `feature/terminal-commander-mvp` | Add deterministic load, scale, and backpressure tests proving Terminal Commander can comb large noisy streams without flooding buckets or callers. |
| 29 | TC29 | Security Hardening And Fuzz-Like Tests | Pending | TC22, TC24 | `feature/terminal-commander-mvp` | Add security hardening tests for policy enforcement, regex safety, path denial, bounded outputs, prompt secrecy, and malformed input handling. |
| 30 | TC30 | End To End Mvp Demo Scenarios | Pending | TC24, TC27, TC28, TC29 | `feature/terminal-commander-mvp` | Create and verify end-to-end MVP demo workflows showing command execution, realtime bucket waiting, dynamic registry rule creation, file watching, and bounded context retrieval. |
| 31 | TC31 | Beta Packaging And Release Checklist | Pending | TC26, TC30 | `feature/terminal-commander-mvp` | Prepare beta packaging metadata, release checklist, versioning notes, and installation verification without publishing or deploying anything automatically. |
| 32 | TC32 | Evidence Review And Backlog Refinement | Pending | TC31 | `feature/terminal-commander-mvp` | Review all completed goals, consolidate evidence, identify unresolved gaps, and produce the next backlog without implementing new features. |
