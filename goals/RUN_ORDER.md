# Run Order - terminal-commander-mvp

Frozen 2026-05-28 as historical record. Live status tracked in `.agent/goals/terminal-commander-runtime/` and `docs/release/MVP_EVIDENCE_REVIEW.md`. Do not edit.

Target branch: `feature/terminal-commander-mvp`

The list below is archival run order only. Do not skip dependencies unless the skipped goal is explicitly marked `Deferred`, `Superseded`, or `Cancelled` with a reason.

TC01. `.agent/goals/terminal-commander-mvp/TC01-research-product-baseline-and-source-map.md` — Research Product Baseline And Source Map — depends_on: none
TC02. `.agent/goals/terminal-commander-mvp/TC02-security-privilege-and-policy-doctrine.md` — Security Privilege And Policy Doctrine — depends_on: TC01
TC03. `.agent/goals/terminal-commander-mvp/TC03-test-methodology-and-fixture-plan.md` — Test Methodology And Fixture Plan — depends_on: TC01, TC02
TC04. `.agent/goals/terminal-commander-mvp/TC04-rust-workspace-and-toolchain-scaffold.md` — Rust Workspace And Toolchain Scaffold — depends_on: TC03
TC05. `.agent/goals/terminal-commander-mvp/TC05-contract-schemas-and-golden-fixtures.md` — Contract Schemas And Golden Fixtures — depends_on: TC04
TC06. `.agent/goals/terminal-commander-mvp/TC06-core-identifiers-events-and-source-pointers.md` — Core Identifiers Events And Source Pointers — depends_on: TC05
TC07. `.agent/goals/terminal-commander-mvp/TC07-in-memory-bucket-manager.md` — In Memory Bucket Manager — depends_on: TC06
TC08. `.agent/goals/terminal-commander-mvp/TC08-context-ring-and-bounded-context-windows.md` — Context Ring And Bounded Context Windows — depends_on: TC06
TC09. `.agent/goals/terminal-commander-mvp/TC09-rule-model-validation-and-templates.md` — Rule Model Validation And Templates — depends_on: TC06, TC08
TC10. `.agent/goals/terminal-commander-mvp/TC10-keyword-and-regex-sifter-runtime.md` — Keyword And Regex Sifter Runtime — depends_on: TC09
TC11. `.agent/goals/terminal-commander-mvp/TC11-noise-suppression-dedupe-and-progress-basics.md` — Noise Suppression Dedupe And Progress Basics — depends_on: TC10
TC12. `.agent/goals/terminal-commander-mvp/TC12-persistent-event-store-and-bucket-cursors.md` — Persistent Event Store And Bucket Cursors — depends_on: TC07
TC13. `.agent/goals/terminal-commander-mvp/TC13-registry-store-and-rule-crud.md` — Registry Store And Rule Crud — depends_on: TC09, TC12
TC14. `.agent/goals/terminal-commander-mvp/TC14-seed-rule-packs-and-registry-import.md` — Seed Rule Packs And Registry Import — depends_on: TC10, TC13
TC15. `.agent/goals/terminal-commander-mvp/TC15-process-probe-streaming-stdout-stderr.md` — Process Probe Streaming Stdout Stderr — depends_on: TC10, TC12
TC16. `.agent/goals/terminal-commander-mvp/TC16-job-manager-and-command-exit-events.md` — Job Manager And Command Exit Events — depends_on: TC15
TC17. `.agent/goals/terminal-commander-mvp/TC17-realtime-bucket-waiter.md` — Realtime Bucket Waiter — depends_on: TC07, TC12, TC16
TC18. `.agent/goals/terminal-commander-mvp/TC18-file-probe-follow-create-and-rotate.md` — File Probe Follow Create And Rotate — depends_on: TC10, TC12
TC19. `.agent/goals/terminal-commander-mvp/TC19-terminal-pty-probe-and-prompt-detection.md` — Terminal Pty Probe And Prompt Detection — depends_on: TC15, TC16
TC20. `.agent/goals/terminal-commander-mvp/TC20-directory-and-artifact-probes.md` — Directory And Artifact Probes — depends_on: TC18
TC21. `.agent/goals/terminal-commander-mvp/TC21-daemon-local-api-and-router.md` — Daemon Local Api And Router — depends_on: TC13, TC16, TC17, TC18
TC22. `.agent/goals/terminal-commander-mvp/TC22-policy-engine-and-audit-log.md` — Policy Engine And Audit Log — depends_on: TC02, TC21
TC23. `.agent/goals/terminal-commander-mvp/TC23-mcp-server-discovery-jobs-and-buckets.md` — Mcp Server Discovery Jobs And Buckets — depends_on: TC21, TC22
TC24. `.agent/goals/terminal-commander-mvp/TC24-mcp-registry-probe-and-file-tools.md` — Mcp Registry Probe And File Tools — depends_on: TC22, TC23
TC25. `.agent/goals/terminal-commander-mvp/TC25-admin-cli-and-doctor-commands.md` — Admin Cli And Doctor Commands — depends_on: TC21, TC22
TC26. `.agent/goals/terminal-commander-mvp/TC26-installer-service-and-wsl-startup-docs.md` — Installer Service And Wsl Startup Docs — depends_on: TC22, TC23, TC25
TC27. `.agent/goals/terminal-commander-mvp/TC27-provider-harness-integration-examples.md` — Provider Harness Integration Examples — depends_on: TC23, TC24, TC26
TC28. `.agent/goals/terminal-commander-mvp/TC28-load-performance-and-backpressure-tests.md` — Load Performance And Backpressure Tests — depends_on: TC11, TC17, TC21
TC29. `.agent/goals/terminal-commander-mvp/TC29-security-hardening-and-fuzz-like-tests.md` — Security Hardening And Fuzz-Like Tests — depends_on: TC22, TC24
TC30. `.agent/goals/terminal-commander-mvp/TC30-end-to-end-mvp-demo-scenarios.md` — End To End Mvp Demo Scenarios — depends_on: TC24, TC27, TC28, TC29
TC31. `.agent/goals/terminal-commander-mvp/TC31-beta-packaging-and-release-checklist.md` — Beta Packaging And Release Checklist — depends_on: TC26, TC30
TC32. `.agent/goals/terminal-commander-mvp/TC32-evidence-review-and-backlog-refinement.md` — Evidence Review And Backlog Refinement — depends_on: TC31
