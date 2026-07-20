# Fable Full-Feature Final Gate Review: Environment and Prerequisite Probe

Independent adversarial commit-gate review of the complete environment-probe
governing milestone, executed per
`docs/reviews/2026-07-20-environment-probe-fable-full-feature-final-brief.md`.
Current disk bytes, current source code, and current Git history were treated as
the authorities. Prior reviews, dispositions, and internal GREEN reports were
inputs to challenge; none of their conclusions was inherited without fresh
derivation from current bytes.

## Verdict: GREEN

No critical or high finding. One medium wording/machine-readability finding and
one bundle of low editorial findings remain; each is demonstrated below to be
unable to alter conformance, unable to produce a false-GREEN checker result,
and unable to yield two divergent conforming implementations. Every mandatory
regression re-derived from fresh bytes verifies corrected. Reviewed bytes were
stable across the entire review. The exact commit-gate conclusion is in the
final section.

## Reviewed-State Fingerprint

- Repository root: `E:/project/terminal-commander`
- Branch: `main`
- HEAD: `40eb16f19cb9b15d36bbf3ddaaa4beaacb86225e`
- Pre/post `git status --short` match: YES (13 entries, byte-identical:
  ` M .specify/feature.json`, ` M .specify/memory/constitution.md`, ten
  untracked `docs/reviews/*environment-probe*` files, and untracked
  `specs/003-environment-probe/`). This review file is the single permitted
  post-review difference.
- Pre/post SHA-256 manifest match: YES. Every digest below was recorded before
  substantive review and re-verified immediately before this verdict; all 17
  are identical. HEAD and branch unchanged.
- The working tree contains no modified code files (only `.specify/` metadata
  and untracked review/spec documents), so on-disk code equals HEAD code for
  every code-verification claim below.

## Complete Input Manifest (pre == post)

| SHA-256 | Bytes | Lines | Path |
|---|---|---|---|
| `d081e0b96fddb4bb165f648c9d00a2bcec8548553d315f124b1e86cddc34082c` | 104420 | 1483 | `specs/003-environment-probe/spec.md` |
| `633be34a21aa4b37f6a8f28de065486710b600aee894f10db2fad7e12ad8fbea` | 397269 | 3899 | `specs/003-environment-probe/scenario-matrix.md` |
| `7a9c19deea43aab636227ebe03e4bcfbd9b7163abdaa35aa3059663740f60d2e` | 2283 | 49 | `specs/003-environment-probe/checklists/requirements.md` |
| `c6e6d35dc14946e4156765647ac64b02bd318405c7c5a410e1f4927d033d52d0` | 58 | 3 | `.specify/feature.json` |
| `594e4e9126681d5fd10d982d91e382224a33499e22a09bb9e0d4781bcb8227b2` | 12756 | 234 | `.specify/memory/constitution.md` |
| `ae645ef0d13cc345e057d875db40f2cf115fb6cdadaec154a5e11bddc853de72` | 7048 | 176 | `docs/reviews/2026-07-17-environment-probe-fable-brief.md` |
| `628207ebd9e50ff9e7bba316531169f756d308c31b21b0e325c2d6c8ff65eca9` | 45729 | 696 | `docs/reviews/2026-07-17-environment-probe-fable-review.md` |
| `2e37fc82b8b76b9f3c8c64b49215cb82d8c4e00ccd743b0380ba1671fcf2a6eb` | 5318 | 66 | `docs/reviews/2026-07-17-environment-probe-review-disposition.md` |
| `9fc0437ae8bfa4b3c8b3bd26b57a9cb93d874dc914acfc16d4f3934eb884cde2` | 9561 | 221 | `docs/reviews/2026-07-20-environment-probe-fable-followup-brief.md` |
| `06bb151bbe9d4391fa767e512339c6bea7aec20c7e7c42b175cb68e73976260a` | 45394 | 690 | `docs/reviews/2026-07-20-environment-probe-fable-followup-review.md` |
| `3705a18bddd2ea369aae689e31505fe1fd9abfd22cb738f03fe2c3029b47f9da` | 15717 | 259 | `docs/reviews/2026-07-20-environment-probe-fable-followup-disposition.md` |
| `2becf614402eca5ba758ff15c272338366116b25e85b2ede6a53264fc1301c32` | 5634 | 114 | `docs/reviews/2026-07-20-environment-probe-fable-rereview-brief.md` |
| `04a7bc95da504fe183cb2ba8656dd039eb3e601f8c8ba178a740715bc2b9634b` | 30554 | 468 | `docs/reviews/2026-07-20-environment-probe-fable-rereview.md` |
| `aef6e9a49efb901457113ff7aba1cc2fe111561381d1e5c6f35ceaa0bf4741ba` | 6102 | 120 | `docs/reviews/2026-07-20-environment-probe-fable-final-brief.md` |
| `e96597534a46da6d45a3e5f35a2f4a2979bc17a0a1cd1a5a95b7b1a3982862c8` | 28171 | 428 | `docs/reviews/2026-07-20-environment-probe-fable-final-review.md` |
| `d04afae5b51f9985cf1ccd917923214f2819c5e99756ed21cf5a6c8c8dceabb7` | 9146 | 157 | `docs/reviews/2026-07-20-environment-probe-fable-full-feature-final-brief.md` |

Those 16 rows constitute the complete mandated input set: the ten prior-review
documents above are the complete `docs/reviews/*environment-probe*.md` set,
and the three `specs/003-environment-probe` files are the complete recursive
regular-file set under that directory. `spec.md` and `scenario-matrix.md`
digests equal the hashes on which the follow-up disposition's final internal
gate reported GREEN; this review is the first independent external gate over
those exact bytes. The matrix confirmed 3,899 lines, ending in a complete
sentence with no truncation.

## Method

### Byte reading

- Read personally, byte-complete: `spec.md` (all 1,483 lines in two windows),
  `scenario-matrix.md` lines 46-79, 183-290, 300-411, 1060-1129, 1290-1398,
  1401-1450, 1472-1501, 1620-1679, 1895-1985, 2125-2148, 2236-2259, 2298-2319,
  2368-2381, 2912-2935, 3145-3160, 3465-3510, 3762-3809, 3860-3899 (every
  region carrying a mandatory regression or a candidate finding),
  `requirements.md` (49), `.specify/feature.json` (3), constitution (234), and
  six prior documents in full: 07-17 brief (176), 07-17 disposition (66),
  followup-brief (221), followup-disposition (259), rereview-brief (114),
  final-brief (120), plus the governing full-feature brief (157).
- Delegated byte-complete ranges (permitted by the brief): three read-only
  delegate readers covered `scenario-matrix.md` lines 1-1300, 1301-2600, and
  2601-3899 (jointly the entire file); two read-only extractors covered the
  four large prior reviews (07-17 review 696, final-review 428,
  followup-review 690, rereview 468) byte-complete. Delegates were forbidden
  to mutate anything and mutated nothing (the post-manifest proves it).
- Every candidate finding from every delegate was personally re-opened and
  adjudicated at its exact current lines before acceptance or refutation; the
  dispositions and refutation bases are recorded below. No delegate
  conclusion was accepted unverified.

### Code verification

SymForge index confirmed live (670 files); source questions used indexed
search plus exact raw reads for configs; Git history via read-only `git log`,
`git show`, `git log -S`. No build, test, daemon, checker, or generator was
executed; no store, lockfile, or index was mutated.

### Structural checks

All executed as read-only stdin scripts over fresh bytes; no temporary files
written inside the checkout:

1. Escaped-pipe-aware GFM table-shape gate (character-scan cell counting,
   escaped pipes treated as literal; the matrix contains zero code fences):
   `scenario-matrix.md` 54 tables / 694 body rows / 0 defects; `spec.md` 5
   tables / 52 body rows / 0 defects. Under the prior gates' counting
   convention (body + header + delimiter rows) this is 864 rows / 59 tables
   versus the 07-20 final review's 863 / 59: the +1 is exactly the N1
   correction re-keying three preservation rows into four winner-keyed
   temporal rows (`scenario-matrix.md:1357-1360`). Independently, the three
   delegate readers verified per-row cell counts across all tables in their
   ranges: zero defects.
2. Domain/reference/link gates: FR-001 through FR-103 each defined exactly
   once, contiguous, no duplicates; SC-001 through SC-021 likewise; zero
   dangling FR/SC references in either file; zero broken relative links; both
   files 100% ASCII.
3. Negative token canaries (the prior gate set, re-run exactly): zero
   `preterminal`, `lease-eligible`, bare `EvidenceModel`, `catalog`
   misspelling, `queue_or_gate`, `snapshot_name_omitted`,
   `execution_name_rejected`; placeholder canaries (TODO/TBD/FIXME/XXX/
   NEEDS CLARIFICATION/WIP/HACK/???): zero. Counter tokens remain distinct:
   `exact_surface_name_scope_limit_and_counter` x4 and
   `exact_scope_limit_and_counter` x5 (drift from the prior 3/5 is the
   post-final-review legacy-surface additions; distinctness preserved).
4. Forbidden content: all three delegate ranges plus my own reads found zero
   environment `NAME=value` pairs, zero secret-shaped strings, zero literal
   redaction markers presented as stored values anywhere in either file. All
   secret references are abstract class names.

## Numbered Dispositions: Complete-Feature Areas (brief items 1-9)

1. **Six-model reconstruction and single classification: VERIFIED.**
   RouteModel: axes closed (`scenario-matrix.md:185-212`), record/verdict
   compatibility closed with explicit residual rejection (227-252), per-gate
   record mandate (189), closed aggregate ladder (317-326), initial winner
   order (330-339), winner-to-event closure (369-385), bridge and projection
   tables total and disjoint (1330-1397; full derivation under Regression 1).
   GoalModel: administration, plan-selection, and aggregation submodels with
   ordered clauses; Goal aggregation clauses 1-9 (3636-3652) walked with
   counterexamples and consistent with the spec entailment table
   (`spec.md:1284-1300`); the suspected "safe route + open safety frontier"
   gap is structurally unrepresentable (matrix:728-744 forces
   `alternative_may_outrank` when a safe Route exists). TransitionModel: all
   40 causes covered by exactly one cause-partition row (1110-1126), all
   events/purposes used, batch compositions with exact member coverage and
   rejection of missing/extra/duplicate members (1060-1093). OperationModel:
   ordered classifier clauses 0-8 with an explicit rejected default (1979);
   start-admission partition disjoint (the one overlap candidate is refuted
   below). SecurityPropertyModel: the nine FR-083 decisions each present with
   a bijective property projection; audit tuple (2236-2251), source-total
   tables (2300-2313), and the explicitly ordered first-matching channel
   combination function with residual row (2370-2379) are total.
   EvidenceBoundaryModel: native-name, census, and independent-bound domains
   closed; wire tagging and the 2x UTF-16 code-unit byte formula exact
   (FR-039/FR-055 mirrored at 3105-3109). No policy, evidence, audit, or
   lifecycle authority is circular: the staged compositions are acyclic (a
   postcommit Operation receipt is never an input to its own Transition), and
   no aggregate, terminal, or outcome field is caller-supplied (214-215;
   caller assertion maps to `unknown` at 272).

2. **FR/SC integrity and anti-false-GREEN strength: VERIFIED.** FR-001..103
   and SC-001..021 contiguous, unique, internally consistent, and linked:
   FR-082/083/084/096 bind the matrix by name, the checklist names the matrix
   as the normative ambiguity gate, and the matrix cites only resolving FR/SC
   ids. The checker/generator cannot report false GREEN by omission: the live
   universe is generated from shipped registries plus the mandatory families
   with stable required-row keys and a failing missing-key report; test-local
   lists cannot shrink it; registry changes require a manifest delta
   (3663-3677); dimension families are generated from the frozen scope-aware
   bound registry, "not a hand-selected subset" (3789-3790); unsupported
   combinations require explicit fixture rows and "must not disappear from
   reports" (3898-3899); coverage reporting is by missing-proof exposure, and
   the corpus contains zero percentage-based coverage language.

3. **One-trigger adaptive campaign LLM utility: VERIFIED AS SPECIFIED.** A
   goal-only request is sufficient (FR-002); bounded waves fan out scouts,
   deepen only evidence-supported convergence points, and retract dead
   branches with bounded terminal reasons (FR-014; SC-015 pins the
   ten-branch/two-survivor fixture); facts carry provenance, freshness,
   grade, and conflict state with exact conflict resolution (FR-064/070/088);
   fan-in returns one bounded map or an `in_progress` receipt (FR-005;
   SC-004's 2,000-token cap with the FR-100 frozen tokenizer measurement);
   decisive uncertainty is never hidden (`missing` requires a complete
   authoritative search scope, straddling freshness intervals demand fresh
   evidence, `unknown` never upgrades) and readiness is never invented
   (READY_SAFE plus the FR-090 real-validator beachhead and SC-016).
   Pre-admission recovery returns a bounded choice set plus one
   schema-validated corrective call while allocating nothing (FR-007 and the
   total recovery submodel at 1401-1449). The quantified savings claim
   (SC-003, 70%) is measurable only against the FR-095 baselines, which are
   absent (recorded under Absences); the specification correctly makes them
   a pre-implementation gate rather than claiming them.

4. **Topology obligations: VERIFIED.** The harness/host/target/workspace/
   policy/transport domains are closed (46-79) with composable isolation
   facets (76-78); per-node/per-edge topology with OS chaining, contiguity,
   and workspace-truth-at-target constraints (3477-3482); Windows/WSL/Linux/
   macOS platform sensing FRs (FR-034..039) with the WSL edge exactly
   host-to-bound-guest or attested reverse interop (3483-3486); container/
   sandbox/VM/Firecracker connectors require their isolation facets, and
   `firecracker_vsock` requires guest identity beyond a CID (3486-3490,
   FR-063). Discovery distinguishes observation, reachability, identity,
   authorization, and an action-valid verified beachhead: FR-032's closed
   connection-failure classes, the upstream-gate projections (254-263),
   FR-012's evidence ladder, and FR-090's requirement that a beachhead pass
   the real request-schema and policy validators. A template, Health reply,
   representative cwd, or policy-neutral probe cannot overclaim: proof
   envelopes are evidence never authority (FR-090/102), a health reply is
   reachability-only (FR-077, 3885-3886), and ambient daemon cwd must not
   masquerade as harness context (FR-020).

5. **Names-only boundary and opaque private-value path: VERIFIED.** FR-053's
   single private-resolver carve-out is closed: frozen requested context with
   exact source/instance/boot/workspace/action match (2130-2136), opaque
   input counted under frozen byte/candidate caps before transcoding with
   destroy-on-stop, and the producer x sink x success/failure/bound-stop
   canary attestation registry that fails closed on any missing or stale key.
   No observed value, value pair, literal redaction marker, value-derived
   hash, or inherited ambient environment may cross a public or audit
   boundary (FR-053/056/058/065-066/103; receipts explicitly ban raw values
   and stable public value digests). Migration, persistence, restore,
   allowlist, purge/quarantine, retry, and operator-repair paths are all
   modeled: the `legacy_overlay_migration` transition row (1090) plus the
   FR-058 family (3842-3856) cover disjoint record-and-index partitioning,
   atomic preserve/delete, quarantine, frozen decreasing attempt budgets,
   signal-driven monotonic wakes, clock-discontinuity terminalization, and
   the operator-only `OverlayMigrationRepair` path. SC-008 canary coverage
   spans every enumerated surface including future registered sinks. Fresh
   forbidden-content scans of both files are clean.

6. **Policy and durable pre-action audit: VERIFIED.** Every observation,
   spawn, dial, forward, private resolution, restore, and remote hop requires
   its positive class-plus-underlying-action authorization and a durable
   pre-action audit admission that fails closed (FR-010/059/060/061/065,
   SC-018). Per-node authority with an ordered authority-chain digest where
   an omitted, swapped, or changed hop is a mismatch (FR-060); multi-hop
   end-to-end and hop-local security with hop-by-hop links explicitly not
   end-to-end confidentiality (FR-062, constitution IV); replay/freshness/
   identity binding via verifier-local monotonic proof at every edge with the
   ordered total combination function (FR-062, 2370-2379); the exact
   distinction among denial, unavailable transport, unobserved state, and
   authoritative unreachability is enforced by the verdict axes, upstream
   gates, and the legacy families' "denied or unobserved is never relabelled
   unreachable" (3885). Audit mismatch/replay are non-authorizing typed
   integrity failures that can never use a policy-denial witness
   (cause-partition row 1119; record row 231; audit tuple rows 2242-2246).

7. **Resource and liveness machinery: VERIFIED.** All FR-072 bound scopes are
   distinct with no counter borrowing; reservations cover the maximum
   lifecycle footprint; `campaign_start` CAS-gates active capacity;
   terminalization is non-increasing; purge releases a partition slot only on
   an exact same-revision empty witness. Every reached bound uses its closed
   role-typed outcome (scope-aware dimension map 3153-3175 plus the ordered
   hierarchical scheduler cut); queue outcomes commit startup-recoverable
   scheduler-owned wait records woken exactly once with duplicate-signal
   coalescing (1479, 1961-1962); no polling anywhere ("never a polling
   loop", FR-058's no-poll recovery, FR-075's single bounded saturated
   wake). Same-incident multi-bound resolves through the exact bundle row
   (2374) and multi-reach composition (3241-3252); receipt replay/mismatch
   through the source-total tables; crash/restart through `engine_loss`
   (1086) and lease re-anchor (1092); protected-retention and
   no-eligible-victim through "protected, continuity-unproved, mismatch, or
   cross-partition selection deletes nothing" (1091, 1093) plus the durable
   saturated no-mutation receipt. Two-stage retention (expiry-to-tombstone,
   then eligibility-proved purge with final-partition fold) is exact,
   including zero-Route results.

8. **Six-surface compatibility and ownership: VERIFIED.** The Product-surface
   domain is exactly `compact_environment`, `full_environment_probe`,
   `legacy_system_discover`, `legacy_target_list`, `legacy_target_probe`,
   `embedded_facade` (53), delivery-constrained at 3473-3476 (legacy surfaces
   MCP-only until an explicit embed contract exists). One engine-owned
   operation across MCP and embed delivery: FR-004/078/080 plus the frozen
   same-case-key equivalence family through compact, full, and embedded
   delivery with "legacy actions receiving the same policy-before-observation
   denial" (3887-3889). No parallel adapter policy/router and no implicit
   dial during registry enumeration: FR-077 plus the legacy families
   (3879-3886) pin registry-only enumeration, policy/capability/remote/audit
   before any forwarded-socket contact, denied-before-dial, and
   Health-cannot-prove-identity. No legacy or embedded bypass: FR-010 ("No
   legacy discovery sentinel is exempt"), FR-081 (embed connectors cannot
   bypass policy or audit), and the invalid-request clause keeping legacy
   surfaces from weakening the recovery submodel (1446-1449).

9. **Current-repository premises: ALL VERIFIED against code and Git.**
   - Toolchain: active pin 1.97.1 (`rust-toolchain.toml`); MSRV
     `rust-version = "1.92"` (`Cargo.toml:20`); rmcp `=1.8.0` with
     server/macros/transport-io (`Cargo.toml:49`).
   - Persistence: refinery deliberately unlinked (`Cargo.toml:52`,
     `crates/store/Cargo.toml:18`, comment at `crates/store/src/lib.rs:56-59`);
     manual SQL migration runner V0001-V0007; rusqlite WAL live
     (`crates/store/src/lib.rs:646`) with the WSL2 9P guard (`:155`). The
     constitution's technology claims match code.
   - Constitution lineage: HEAD carries v1.0.0 in the single commit
     `958c502`; `git log --all -S "2.0.0"` over the file is empty; the
     worktree 2.1.0 Sync Impact Report's "no intermediate 2.0.0" is accurate.
   - Surfaces: compact surface is exactly five facades
     (`crates/mcp/src/surface_list.rs:7-8`), so FR-001's "sixth environment
     facade" is correct; full surface is 51 tools with count anchors
     (`crates/mcp/src/tools.rs:20`, `:41`, count assertion at `:6684`) and a
     fixture-map drift test
     (`crates/mcp/tests/fixture_catalogue_contract.rs:54`), grounding
     FR-001's change-together clause.
   - Discovery: `DaemonState::discover_environment` observes first and only
     filters advertised routes post-observation
     (`crates/daemon/src/state.rs:366-370`,
     `crates/daemon/src/environment/mod.rs:15-20`; commit `cd0d906`), so
     FR-010/FR-077's policy-before-observation is a real migration, correctly
     classified as such by the spec's Assumptions.
   - Legacy targets: `target_list` dials every registered target's forwarded
     socket during listing, ungated by `allow_remote`, from the adapter-side
     router (`crates/mcp/src/tools.rs:1201-1214`); `target_probe` returns
     Health-level reachability plus `daemon_version` under `allow_remote`
     (`:1216-1226`). The matrix's `legacy_target_list`/`legacy_target_probe`
     obligations target exactly this real behavior as migration.
   - Sessions/snapshots: `env_snapshot: Vec<(String, String)>` value pairs
     cross IPC and MCP today (`crates/daemon/src/shell_session.rs:113`,
     `:218`, `:482`; `crates/mcp/src/tools.rs:2530-2537`); workspace snapshot
     rows persist literal `<redacted>` markers as values
     (`crates/store/src/workspace.rs:29-38`); terminal identity is derived
     from `TERM_PROGRAM`/`TERM`/`TERM_PROGRAM_VERSION` values
     (`crates/daemon/src/environment/probe.rs:545-570`). FR-057/FR-058
     migrations therefore target real current surfaces.
   - Audit: every current audit emit is fire-and-forget `let _ = ...`
     (`crates/daemon/src/router.rs:85`, `crates/daemon/src/command.rs:787`,
     `:1541`, `crates/daemon/src/file_watch.rs:214`,
     `crates/daemon/src/pty_command.rs:244`, `:606`), so FR-065's durable
     fail-closed pre-action audit is a demanded change, never claimed
     current.
   - Embedding/platform: public daemon-library engine modules
     (`crates/daemon/src/lib.rs:17-35`) with
     `crates/daemon/examples/embed_in_process.rs`; `EnvironmentSpec` lives in
     core and rides two request types (`crates/ipc/src/protocol.rs:1221`,
     `:2068`) with WSL routing at
     `crates/daemon/src/environment/router.rs:39`; zero vsock/firecracker
     code repo-wide. Missing implementation is classified as migration or
     absent future artifact throughout (see Absences); nothing absent was
     reported as implemented.

## Numbered Dispositions: Mandatory Regressions

1. **N1 (later hard exclusion from prior `denied|exhausted|truncated`):
   VERIFIED CORRECTED, personally re-derived.** The closed temporal
   composition (matrix:341-367) applies the cause's exact per-record effects
   in one atomic `fresh_unique` batch (retain requires per-record stable
   identity plus canonical evidence digest; a no-change cause cannot
   invalidate, demote, or delete an unchanged record; malformed effect sets
   reject), derives the complete current post-batch closure, and runs the
   unchanged ordinary winner algebra over precisely that closure (1338-1350).
   I re-derived all four counterexample classes from the prior reviews:
   (a) class shift after an authorized context change (rotation invalidates
   the denial's dependents, a persisting fresh `unreachable` record wins, row
   1359 accepts, outcome `unreachable`); (b) partial same-class persistence
   (a two-record denial set with one member invalidated: per-record retain
   plus post-batch winner `denied`, row 1358 accepts); (c) non-change replay
   with a persisting denial (record retained, winner `denied`, row 1358; the
   event conflict is resolved by the sole later-terminal exception at
   380-383: the batch receipt supplies `hard_excluded` and the cause, the
   winner supplies only the outcome); (d) no persisting higher winner (row
   1360 derives `blocked` from the exclusion record). The projection table
   (1352-1366) is total and disjoint over the accepted temporal domain: the
   bridge's full `excluded` prior set is exactly partitioned across rows
   1357-1360 (terminal-negative priors, winner-keyed with explicit
   precedence constraints), 1361 (non-terminal priors; reachable only with a
   class-4 winner because the initial winner-to-event closure sends any
   higher-class winner to a different event), and 1362 (prior `ready`, with
   the exact cause-to-proof mapping at 1368-1381). Lifecycle `excluded`,
   event `hard_excluded`, and the exact new cause are independent invariants;
   supersession keeps the old Route provenance-only, forbids digest reuse
   after a member-subset or winner-class change, and guarantees exactly one
   current replacement Route (1383-1397). The batch-table row (1076) states
   the identical algebra, so the three texts agree. Stale/mismatched negative
   twins, no-safe-output, and no-work are pinned by the generated fixture
   family (3762-3781), including every nonempty same-class persisting subset,
   full persistence, full invalidation to `blocked`, the three downward class
   shifts, conflict recomputation, prior `evidence_unavailable|bound_reached`
   with no higher winner, and the complete negative-twin set (zero-or-two
   replacement Routes, non-change evidence demotion, digest reuse). The
   delegate's upward-class-shift concern was refuted by derivation: a
   higher-priority record existing pre-batch would have produced a different
   prior state/cause, and no hard-exclusion cause can mint new
   unsupported/denied/unreachable records (policy and representability
   changes route through recoverable invalidation), so the downward-only
   MUST-cover set is exactly the reachable set.

2. **Campaign deadline/completion: VERIFIED CORRECTED, personally
   re-derived, with one MEDIUM wording finding (M-1 below).** The
   `campaign_bound_stop` row (1088) carries four arms: open (each open branch
   commits `truncated/bound_reached`, Goal `ready_with_warnings|unknown` as
   applicable), Route-bearing all-terminal (retains the exact terminal Route
   set and exhaustive terminal Goal unchanged), accepted-empty (legal only
   for the exact zero-branch/zero-Route/zero-live-resource queue-gated case;
   any work member rejects, matching FR-101), and the exclusive zero-Route
   arm (Goal `unknown` from `bound_before_route`, no synthetic Route,
   explicit for cancelled-only and cancelled-root-plus-already-retracted
   ownership, every Route effect exactly `none`). Arm guards are disjoint by
   open-branch count, Route count, and lifecycle state, with the zero-Route
   arm's exclusivity clause closing the only overlap candidate. The six
   orderings all verify: completion (1089) requires an exact same-revision
   hierarchical cut with `total_time=within`; a reached receipt at that cut
   or an earlier still-current revision selects `campaign_bound_stop`
   (same-cut reached wins; covers reached-after-terminalization-but-before-
   completion); a receipt after completion is stale and rejects; duplicates
   fail the exact campaign-decisive receipt correlation and the root
   lifecycle member; restart resolves through `engine_loss` and
   startup-recoverable wait records. Branch, Route, Goal, receipt, snapshot,
   cleanup, retention, and campaign effects are enumerated per arm and
   conjunctive with the queue-wait coverage witness (1095-1105); the
   dedicated race family carries the six disjoint orderings (3810-3823).
   Totality of the Route-bearing arm's member dispositions holds under the
   uniquely consistent reading documented in finding M-1.

3. **Legacy target list/probe: VERIFIED, personally re-derived.** Current
   bytes pin, at 3879-3886 with the surface constraints at 53, 1446-1449, and
   3473-3476: local and forwarded delivery; registry-only enumeration with no
   implicit liveness dial; every requested reachability/health observation
   requiring the exact connector sensor class, connector capability,
   underlying remote decision, and durable pre-action audit before any
   forwarded-socket contact; fixture coverage for denied-before-dial,
   authorized execution, audit denial/failure, unavailable transport, and
   successful liveness; denied or unobserved never relabelled `unreachable`;
   and a health response never upgrading reachability into target identity or
   a beachhead proof. Both legacy surfaces are MCP delivery projections of
   the one engine-owned operation ("legacy actions receiving the same
   policy-before-observation denial", 3888), with the code premise (the
   current adapter-side dialing `target_list`) verified real and correctly
   classified as migration.

4. **M1, L1, L2, L3, and prior clean-room items 2-9: ALL VERIFIED on fresh
   bytes.**
   - M1: the single-source combination row forbids only a hard-exclusion
     Transition ("no hard-exclusion Transition", 2375), consistent with the
     source-total rows ("no connector hard exclusion is fabricated", 2304;
     "no hard-exclusion Transition is fabricated", 2306) and compatible with
     the ordinary terminal sensor-result commit; the fixture family demands
     "zero hard-exclusion Transition" at every owning property (3782-3788).
   - L1: the bound-dimension family is generated from the frozen scope-aware
     bound registry with the prose list explicitly marked "Representative
     families include" (3789-3795), so omitted examples cannot narrow
     conformance.
   - L2: clause 7 names "an authorized existing-campaign attachment",
     overrides only `bound_effect`, preserves `campaign_state_after`
     (1952-1958), the wake-registration axis forbids Operation-owned wait
     state (1479), and the family requires matching-key attachments to an
     existing `accepted` campaign to reuse the exact pending
     `campaign_start` wait/wake with zero new registration, reservation,
     Transition, spawn, campaign mutation, or key mutation (3800-3803).
   - L3: `bound_reached/bound_reached` requires the exact branch- or
     campaign-decisive bound receipt selected by the closed bound-effect
     composition and bound to the creating Transition; missing, mismatched,
     replayed, stale, or cross-Transition receipts cannot select it
     (374-378, 384-385); the `exhausted/evidence_unavailable` projection row
     accepts only `probe_failed` (1365; dead `timed_out` disjunct absent)
     while `truncated/bound_reached` retains `timed_out|truncated` with the
     receipt (1366).
   - Items 2-9 (rereview matrix): `unproved` is exclusively an
     evidence/observation state with identical seven-value preliminary/final
     domains and a total `surface_name_application` tuple (2142-2148 plus
     residual rows; delegate-verified full coverage); endpoint-binding
     activity is consistent (campaign rows structurally unreachable,
     2311-2313); the ordered combination function accepts exactly the
     complete same-incident bundle (2374) and rejects
     independent/conflicting/incomplete shapes (2373); private-input counter
     replay derives the deliberate no-receipt rejection distinct from
     structural `rejected` (2306, 2308, replay-class partition); non-audit
     receipt mismatch has one property-local disposition with no fabricated
     connector exclusion (2304); the mandatory live families retain
     retention purge/saturation, `retired` dispatch denial (3892-3896),
     bound-registry dimensions with admission `rejected_bound` and queue
     wait/wake (3796-3803), targeted/full census with consent states, and
     `operator_local_composite`; constitution lineage and persistence-stack
     claims match Git and code (area 9); and the machine-readability gates
     re-ran green from fresh bytes (Method, Structural checks). The
     platform-clock table binds guest-kernel `CLOCK_BOOTTIME` for WSL
     verifiers, requires target-native registered sources for other
     guests/remotes with parent/sender/wall clocks forbidden, and fails
     closed to `clock_continuity_unproved` (3142-3147); no unsupported
     inheritance is claimed.

## New Findings

No critical findings. No high findings.

1. **[medium] `campaign_bound_stop` Route-bearing arm uses "all-terminal" /
   "each terminal branch" across two conflicting declared vocabularies.**
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:1088`
     (Route-bearing arm: "a nonempty all-terminal ownership set ... each
     terminal branch becomes `retracted`"); `:210` declares Terminal branch
     state = {ready, denied, excluded, exhausted, truncated} (includes
     `ready`, excludes `cancelled`); `:1063` declares `A_terminal` =
     {denied, excluded, exhausted, truncated, cancelled} (includes
     `cancelled`, excludes `ready`); `:1312-1315` (Route effects: bound stop
     retains "every existing ready/terminal Route"); `:1089`
     (campaign_completion enumerates `{ready, denied, excluded, exhausted,
     truncated, cancelled} -> retracted` explicitly).
   - Reproducible counterexample: a `running` campaign with zero open
     branches, one `ready` branch holding a current Route, and one
     `cancelled` branch, when the total-time receipt arrives at the same cut
     that bars completion (1089: "a reached receipt at that cut ... selects
     `campaign_bound_stop`"). Under the `:210` vocabulary the `cancelled`
     member is not a "terminal branch", so its disposition is unenumerated
     and the descendant column's "no branch omitted" rejects the batch;
     under the `:1063` vocabulary the `ready` member breaks the
     "all-terminal" guard and no arm matches at all. Either strict reading
     strands a reachable member class in a state where the stop is
     mandatory.
   - Consequence: an FR-096 checker keying these tokens to either declared
     domain reports a loud totality failure on a reachable
     mandatory-liveness path. It cannot produce false GREEN, and it cannot
     produce two divergent conforming implementations: any reading other
     than the intended one dead-ends into behavior that explicit text
     forbids (unbounded life past total-time violates FR-019/FR-073;
     completion is barred at the reached cut), so no alternative reading is
     implementable as a conforming whole. The intended semantics are forced
     by four independent texts: the zero-Route arm's exclusivity clause
     presupposes cancelled/retracted ownership can otherwise enter the
     Route-retention arm; the Route-effects paragraph handles "every
     existing ready/terminal Route" in bound stop; `cancelled -> retracted`
     is the sole legal continuation (spec branch-state table); and row 1089
     shows the exact enumeration pattern. This demonstration is why the
     finding is nonblocking.
   - Smallest root-cause correction: in row 1088's Route-bearing arm,
     replace "a nonempty all-terminal ownership set" with "a nonempty
     ownership set containing no open branch" and "each terminal branch
     becomes `retracted`" with "every owned branch in `{ready, denied,
     excluded, exhausted, truncated, cancelled}` becomes `retracted`"
     (mirroring row 1089).
   - Verification boundary: FR-096 checker totality over the
     `campaign_bound_stop` arms; the deadline/completion race family
     (3810-3823); SC-017 transition fixtures.

2. **[low] Editorial/machine-readability bundle** (shared root cause:
   hand-maintained prose beside closed tables; each item is demonstrated
   unable to alter conformance because a closed table, domain typing, or an
   unambiguous unique referent forces one reading):
   - (a) Route "Terminal branch cause" tokens (`:211` `policy_denial`,
     `route_proved`) differ from Transition cause tokens (`policy_decision`,
     `none`); the bridge's fourth column (1332-1336) is the explicit closed
     rename, and domain typing makes the alternative reading
     unrepresentable; the phrase "that exact terminal state and cause"
     (387-388) could name the bridge mapping explicitly.
   - (b) `:271` points "Delivery + product surface" at the delivery/role
     compatibility table (127-138), which has no product-surface column; the
     actual closed rule is Structural Constraint 2 (3473-3476). Wrong
     pointer, unique existing content; fix the pointer.
   - (c) The combined-axis map rows (`:279`, `:280`, `:282`) state no
     intra-row precedence; uniqueness is forced by the one-record-per-gate
     mandate (`:189` separates policy, lane/shell, identity, and workspace
     gates) plus the closed aggregate ladder (`:317-322`), so e.g. host
     `unknown` + shell `policy_denied` yields separate records aggregating
     to `denied`. The residual lane-`none`-plus-schema-value tuple affects
     only the choice between two causes that proof-map identically to
     `requirement_mismatch` (1379-1381) with identical Route outcome
     `blocked`. Stating "each gate contributes its own record; the aggregate
     ladder resolves" would close it.
   - (d) The connector-cap function (`:2933`) uses `not_applicable`, absent
     from the line-63 Connector-kind axis; the function table's own first
     column is its closed ten-value domain and mirrors `spec.md:1239-1249`
     exactly; the edge-domain versus action-connector-reference distinction
     should be stated once.
   - (e) `IndependentBoundModel` (`:1040`) names no declared model; the
     unique referent is the "Independent bound domain" (`:3113`).
   - (f) Naming nits a checker must be told about: `queue_gate` reused as
     role (`:3163`, `:3166`) and effect (`:3233`); the receipt-name triplet
     `surface_name_application` / `security_surface_name_application` /
     `exact_security_surface_name_application` (`:2973`, `:3132`, `:3053`);
     spec-taxonomy CamelCase action names at `:2134` resolved only via the
     spec table; winner-table rows 1363-1366 omit the prior tuple component
     (the bridge supplies it); "one status record for each finite dimension"
     (`:3149`) versus the scope-keyed rows of its own table (the frozen
     scope-aware registry is the authoritative key).
   - (g) Two defensive-clarity opportunities verified safe as written: audit
     row `:2245`'s fail-closed `denied` emits no receipt and no Transition,
     and `:236` (Route `denied` requires exactly `complete_policy_receipt`)
     structurally bars it from ever being evidenced as a policy denial
     (FR-065 holds); private-resolution rows `:2135`/`:2136` are disjoint
     because a pair with a `not_applicable` axis is not a "cross-pair", and
     their effects coincide regardless.
   - (h) Style: escaped-pipe convention in prose code spans is inconsistent
     (e.g. `:462` raw, `:1435` and `:1734-1751` mixed); zero table impact
     (gate: 0 defects). Minor list-grammar slips at `:1377`/`:1380`,
     `:1748`/`:1750`, and the interrupted colon-promise at `:2452-2455`.

## Delegate Candidates Adjudicated and Refuted

Recorded for auditability; each was personally re-derived at its exact lines:
start-admission rows 1633/1635 do not overlap (authorization is evaluated
before capacity per 1626-1628/1653, and the constraint block at 1646-1648
forces the denial arm; observable outcome identical either way);
`branch_progress` Goal-witness scope does not bind `queue_wait_registration`
rows (different purpose; the queue-wait witness paragraph governs); an
initial `hard_excluded` event with an unsupported/denied/unreachable-class
winner is unreachable (the winner-to-event closure sends those winners to
different events), so no extra non-temporal projection rows are needed;
authorized-stopped transport terminalizes through the bounded-attempts
clauses rather than needing its own row; upward N1 class shifts are
structurally unreachable (derivation in Regression 1); the Goal-aggregation
"safe route + open safety frontier" arm is structurally unrepresentable
(728-744); the same-cut-denial-plus-replay case resolves by winner order with
the replay record retained in provenance.

## Missing Implementation and External Evidence (stated as absence, never success)

- The environment-probe feature itself is not implemented:
  `crates/daemon/src/environment/` implements the legacy discovery being
  migrated; there is no campaign/planner/sensor/goal-plan engine, no
  `allow_environment_*` cap, no `Environment*` policy action, no connector
  attestation, no fixed helper, no typed decoder, no request-key/campaign
  registry. This is the expected pre-`/speckit-plan` state.
- FR-095 manual onboarding baselines: absent (no baselines directory or
  transcript fixtures exist). Pre-implementation gate.
- FR-100 frozen response-cap/tokenizer/fixture/measurement artifacts: absent.
  Pre-implementation gate.
- FR-050 independent official comparator reference corpus: absent; external
  sourcing unverified.
- Shipped registries that generate the live universe (support/capability
  manifests, sensor/plan catalogues, producer registry, sink inventory,
  private-store inventory): absent; the matrix defines their required shape.
- FR-096 checker, generator, and fixture corpus: not built and not executed;
  machine-checkability was assessed analytically plus by the mechanical gates
  above.
- AAP Firecracker/vsock consumer: external repository, not inspected; zero
  vsock code in this repo is consistent with, not proof of, the integration
  gate. SC-013's AAP row remains AAP-owned.
- Platform clock vendor contracts (Windows QPC suspend semantics, Linux
  `CLOCK_BOOTTIME`, macOS `mach_continuous_time`): cited by URL in the matrix
  but not re-fetched here; the registry's fail-closed
  `clock_continuity_unproved` design bounds the risk, and the mappings remain
  implementation-test obligations.
- No live probe/daemon/test execution occurred in this review; all behavior
  claims are code-read-backed and labeled as such.

## Gate Conclusion

**GREEN - the complete governing specification milestone
(`specs/003-environment-probe/spec.md` at `d081e0b9...`,
`specs/003-environment-probe/scenario-matrix.md` at `633be34a...`,
`specs/003-environment-probe/checklists/requirements.md` at `7a9c19de...`,
`.specify/feature.json` at `c6e6d35d...`, and
`.specify/memory/constitution.md` at `594e4e91...`) is SAFE TO COMMIT
directly to `main` at exactly these fingerprints.**

Basis: stable reviewed bytes (pre/post fingerprints identical); every
mandatory regression re-derived and verified corrected from fresh bytes; no
unresolved correctness, security, privacy, totality, liveness, compatibility,
machine-checkability, or evidence-authority defect that could change a
conforming implementation (the one medium and all low findings are
demonstrated loud-failure-only wording items with a single forced reading);
no unclassified or multiply classified reachable assignment under those
demonstrated readings; all structural, canary, and content gates green; every
current-code premise verified against the repository at `40eb16f`; and every
absent artifact stated as absence.

Conditions and scope:

1. GREEN authorizes committing exactly the fingerprinted bytes above. If any
   reviewed byte is edited before the commit (including applying finding
   M-1), these fingerprints no longer attest that state and a fresh narrow
   verification of the edited region is required first. The recommended path
   is: commit the reviewed bytes, then land M-1's one-clause correction plus
   the low editorial bundle as a follow-up amendment with its own narrow
   gate.
2. GREEN authorizes the specification commit only. It is not implementation
   completion, not SC satisfaction, and not a claim that any absent artifact
   exists.
3. `/speckit-plan` and implementation remain blocked until that commit exists
   on `main`; once it does, the Development Review Gate's independent
   adversarial review requirement is satisfied by this document and planning
   may begin.
