# Fable Clean-Room Follow-up Review: Environment Probe

Reviewer: Claude (Fable 5), 2026-07-20. Clean-room follow-up per
`docs/reviews/2026-07-20-environment-probe-fable-followup-brief.md`. Findings
were derived from the stable checkout and the complete current specification
before any historical review material could be consulted; no prior finding
list or disposition seeded this review.

## Reviewed-State Fingerprint

- Repository root: `E:/project/terminal-commander`
- Branch: `main`
- Commit: `40eb16f19cb9b15d36bbf3ddaaa4beaacb86225e`
- Pre/post worktree status match: YES. Both observations returned exactly:
  - ` M .specify/feature.json`
  - ` M .specify/memory/constitution.md`
  - `?? docs/reviews/2026-07-17-environment-probe-fable-brief.md`
  - `?? docs/reviews/2026-07-17-environment-probe-fable-review.md`
  - `?? docs/reviews/2026-07-17-environment-probe-review-disposition.md`
  - `?? docs/reviews/2026-07-20-environment-probe-fable-followup-brief.md`
  - `?? specs/003-environment-probe/checklists/requirements.md`
  - `?? specs/003-environment-probe/scenario-matrix.md`
  - `?? specs/003-environment-probe/spec.md`
- Pre/post specification manifest match: YES. Identical enumerated file set and
  identical SHA-256 digests before analysis and immediately before this verdict.
- External checkout fingerprints, if any: none used. No AAP checkout was
  consulted; AAP-side claims are recorded under Missing Verification.

Stability caveat recorded (not a spec-logic finding by itself, but material):
the three specification files are untracked, and `.specify/feature.json` plus
`.specify/memory/constitution.md` are uncommitted worktree modifications. The
reviewed state is therefore a worktree state, not a committed tree. See
Finding 10.

## Complete Specification Input Manifest

| Relative path | SHA-256 |
|---|---|
| `specs/003-environment-probe/checklists/requirements.md` | `5331e94b68697e73a65ce996df80d0de7d0f59956c838cc54a7e27c14c029485` |
| `specs/003-environment-probe/scenario-matrix.md` | `f8028c8b7c85f7c273af9cef298c8f21449cd7184488843dc085cec7c9c895bd` |
| `specs/003-environment-probe/spec.md` | `6cefb297cb412e037218edf40a41f6dba65487a645b8d1ef79838c4480c710d7` |

This is the complete recursive regular-file set under
`specs/003-environment-probe` (including subdirectories); no other file exists
there.

Reading method (auditable): the lead reviewer read spec.md (all 1476 lines),
checklists/requirements.md (all 49 lines), and scenario-matrix.md lines 1-1080,
1269-1315, 1622-1643, 1680-1695, 1720-1781, 1890-2000, 2047-2060, 2127-2136,
2205-2218, 2255-2290, 2355-2368, 2556-2565, 2624-2631, 2885-2910, and 3537-3707
directly. The remaining scenario-matrix ranges (1081-2000, 2001-2900,
2901-3536) were byte-completely read by three read-only delegate readers
quarantined from `docs/reviews/`, whose candidate observations were then
re-verified by the lead against the exact cited lines before acceptance. Every
HIGH finding below is lead-verified end-to-end at its cited lines; MEDIUM
findings are lead-verified except where explicitly marked; LOW items carry
their provenance inline. Several delegate candidates were REFUTED by lead
verification and are recorded as cleared in the coverage record.

## Supporting Evidence Files Consulted

Governance and worktree state:
- `.specify/memory/constitution.md` (worktree v2.1.0; `git diff` vs HEAD v1.0.0)
- `.specify/feature.json` (worktree; `git diff` vs HEAD)

Code, config, tests (current implementation at 40eb16f):
- `crates/mcp/src/surface_list.rs` (compact facade list :68, count anchor :310,
  gate :277-279), `crates/mcp/src/tools.rs` (tool catalogue :134-384, anchors
  :41, :3076, :3100; `target_probe` gating :516-537), `crates/mcp/src/surface.rs`,
  `crates/mcp/src/facades.rs`, `crates/mcp/src/facade_strict.rs`,
  `crates/mcp/src/target_router.rs`, `crates/mcp/src/main.rs`
- `crates/mcp/tests/compact_surface.rs`, `crates/mcp/tests/fixture_catalogue_contract.rs`,
  `crates/mcp/tests/mcp_live_daemon.rs` (:260), `crates/mcp/tests/daemon_unavailable_envelope.rs` (:246)
- `tests/fixtures/contracts/` (13 schema fixtures, `mcp-tools/` per-tool set,
  `mcp-tool-fixture-map.v1.json`, `forbidden/` negative oracles)
- `crates/daemon/src/policy.rs` (profiles :64-75, presets :168-186, caps
  :157-162, actions :79-140, discovery cwd anchor :705-708, containment tests
  :1583-1675), `crates/daemon/src/config.rs` (caps overlay :244-252, resolution
  :444-462, remote targets :589-616, retention/audit config :311, :329-333)
- `crates/daemon/src/state.rs` (:366-370 discover-then-filter), `crates/daemon/src/ipc/server.rs`
  (:550-552, :881-898), `crates/daemon/src/environment/probe.rs` (:14-16, :41-93,
  :218-319, :449-487, :543-587, :589-661), `crates/daemon/src/environment/mod.rs`
  (:17-69), `crates/daemon/src/environment/router.rs` (:34-70),
  `crates/daemon/src/environment/wsl.rs` (:15-90), `crates/core/src/environment.rs` (:11-19)
- `crates/ipc/src/protocol.rs` (envelope :192-195, AccessRoute :696-706,
  HostEnvironment :710-724, DiscoverResponse :730-738, PolicyCapsView :750-756,
  command env :1230-1232, dedup nonce :1267, session env snapshot :2297-2312)
- `crates/daemon/src/command.rs` (runtime :528+, dedup :558-566/:2261-2279,
  receipts :567-573/:1375-1399, redaction :2195-2750, canary tests :2849-3033),
  `crates/daemon/src/shell_session.rs` (:321-332, :487-527),
  `crates/daemon/src/audit.rs` (:25-95), `crates/daemon/src/runtime.rs` (:470-531),
  `crates/daemon/src/ipc/peer.rs`, `crates/daemon/src/ipc/handlers/{command,session,audit,common,file}.rs`
- `crates/store/src/lib.rs` (:57-59 manual migration runner), `crates/store/src/audit.rs`,
  `crates/store/src/registry.rs`, `crates/store/src/workspace.rs` (:8-13, :29-39, :70-88),
  `crates/store/migrations/V0001..V0007` (V0002 registry, V0003 audit,
  V0006 workspace_snapshot, V0007 job_receipt), `crates/core/src/rule.rs`,
  `crates/core/src/job.rs`, `crates/core/src/platform.rs` (:23-31)
- `crates/daemon/tests/` (environment_discovery.rs, security.rs, shell_session_ipc.rs :306),
  `crates/daemon/examples/embed_in_process.rs` (:5-34), `crates/daemon/src/lib.rs` (:1-15)
- Workspace `Cargo.toml` (:6-16, nine crates)

Code verification was executed by two read-only delegate investigators
quarantined from `docs/reviews/` and `specs/003-environment-probe/`; the lead
accepted their reports as evidence with the citations above, and every claim
used in a finding or confirmation below carries its exact file:line.

## Intent Restatement

The specification adds one goal-directed `environment_probe`: a single
low-cost trigger from any supported harness or isolation boundary that runs a
staged, policy-gated sensor campaign (Physarum-style bounded waves), discovers
topology and workspace, proves routes with target-native evidence, and returns
one bounded trustworthy readiness map (or one resumable `in_progress` receipt)
with provenance, without ever exposing environment values or raw probe noise.
It must be complete, internally executable (the six-model normative matrix must
be totally classifiable and checkable), and faithful to Terminal Commander's
current working model (engine-owned authority, policy-before-observation,
bounded combed output, append audit, compact/full/embed parity). Required
capability is not rejected for scope; machinery is challenged only where it
does not buy correctness, robustness, trust, or LLM utility.

## Verdict: CONTESTED

Four HIGH findings (all internal formal-model contradictions or coverage holes
that block the FR-096 machine-readable checker or a mandated runtime path)
remain. No CRITICAL finding: no unsafe disclosure, authority bypass, or
foundational contradiction invalidating the six-model architecture was found;
the security intent is coherent and every code-facing premise checked was true.
The HIGH findings are localized and correctable.

## Findings

1. **[high] Terminal evidence-projection table is incomplete against its own branch-state bridge**
   - Model/domain: TransitionModel -> RouteModel bridge (terminal evidence projections)
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:1284-1294` (projection rows),
     `:1309-1310` ("Every other terminal state/cause/evidence tuple is a rejected
     checker constraint"), vs `:1277` (bridge: `excluded` accepts prior states
     including `denied`, `exhausted`, `truncated`), `:1040` (batch row
     `{..., denied, exhausted, truncated} -> excluded`), `:1083` (hard-exclusion
     causes MANDATE `terminal_branch_commit/hard_excluded` for those non-ready
     priors), `:1042` + `:1279` (truncation batch requires an exact
     `timed_out|truncated` sensor-result member), `:1294` (the only truncated row
     accepts "hard observation `truncated` or hard freshness non-fresh");
     spec.md:1199-1202 (branch table: `denied`/`exhausted`/`truncated` ->
     `excluded` on a later hard exclusion).
   - Concrete counterexample or sequence: (a) a branch reaches terminal `denied`;
     a `connector_clone_detected` receipt then arrives; matrix:1083 mandates
     `terminal_branch_commit/hard_excluded`, and matrix:1277 accepts prior
     `denied` -> `excluded`; but projection rows :1289 ("non-`ready`
     preterminal / excluded") and :1290 ("`ready` / excluded") cover no terminal
     prior, so the mandated Route record is rejected by :1309-1310.
     (b) a per-sensor deadline fires `timed_out` on an otherwise fresh branch;
     matrix:1042 commits `terminal_branch_commit/bound_reached` with the
     `timed_out` sensor result; the branch's hard observation derives
     `timed_out` (aggregate order matrix:321-322) and freshness `fresh`, which
     satisfies neither disjunct of row :1294 -> rejected by :1309-1310.
   - Incorrect/undefined result and consequence: the same transition is
     simultaneously must-accept (cause table, bridge, batch rows) and
     must-reject (projection closure). The FR-096 exhaustive checker cannot be
     built to satisfy both; an implementation must silently pick a side,
     which is exactly the destructive ambiguity the matrix exists to prevent.
   - Exact required specification correction: add projection rows for
     `excluded` created from terminal priors `denied|exhausted|truncated`
     (same blocked subordinate set as :1289, proof column "missing or immutable
     pre-existing non-current proof", plus the :1296-1309 cause-to-proof mapping
     where the root owned a proof), and extend row :1294 to accept hard
     observation `timed_out` (or add an explicit sensor-result-kind disjunct
     for the timed_out bound composition). Also define "preterminal" as a term
     or replace it with the explicit state set.
   - Verification boundary: FR-096 machine-readable checker totality plus
     SC-017 exhaustive branch-transition fixtures.

2. **[high] `unproved` exists in SecurityPropertyModel's preliminary native-name verdict domain but EvidenceBoundaryModel can never emit it**
   - Model/domain: SecurityPropertyModel (surface_name_application input axis)
     x EvidenceBoundaryModel (native-name identity domain); staged cross-model
     composition matrix:39-44, :2869-2872.
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:1910` (preliminary
     verdict domain includes `unproved`, "emitted privately by EvidenceBoundary
     name classification"), `:2054-2055` (Security rows consuming preliminary
     `unproved`), vs `:2893-2897` (the preliminary verdict has "exactly the same
     value domain as the final admission", enumerated closed as seven values
     WITHOUT `unproved`).
   - Concrete counterexample or sequence: a caller-supplied overlay name whose
     bound/counter infrastructure cannot be proved. If the classifier can be
     unproved, the EvidenceBoundary closed domain cannot represent its own
     output; if it cannot, Security rows :2054-2055 quantify assignments that
     can never occur while the FR-096 checker must still classify the axis
     value that :1910 declares.
   - Incorrect/undefined result and consequence: the two models enumerate the
     SAME quantity with different closed domains. The mandated machine-readable
     model cannot encode both; whichever side an implementer picks, either dead
     normative rows or an unrepresentable classifier state results, and an
     unproved classification risks being rounded into `unknown` or `rejected`
     ad hoc.
   - Exact required specification correction: align the domains. Either add
     `unproved` to both the preliminary and final `native_name_admission`
     enumerations at :2893-2897 (with its classification rule), or remove
     `unproved` from :1910 and rewrite rows :2054-2055 to consume only values
     EvidenceBoundary can emit.
   - Verification boundary: FR-096 checker cross-model composition; the staged
     name-classification -> surface application -> final receipt fixtures.

3. **[high] Private endpoint binding is simultaneously required-active and required-neutral for authenticated-local overlay transport**
   - Model/domain: SecurityPropertyModel (overlay_transport channel bundle).
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:1953` (axis
     definition: "active for a cross-boundary overlay"), `:1979-1984` ("every
     input axis not explicitly named active ... is exactly `not_applicable` ...
     an arbitrary value in an unselected bundle rejects the assignment"), vs
     `:2135` (authenticated local-process overlay positive tuple REQUIRES
     "endpoint binding `exact_origin_and_final_target_route`"; crossing
     `authenticated_local_private` is not `cross_boundary`, domain :1956);
     corroborated by `:2215-2217` (endpoint-binding rows: "this axis is active
     only for non-campaign overlay transport/persistence" - i.e. NOT
     cross-boundary-only) and the delegate-verified rows `:2378` and
     `:2723-2725` (authenticated_local_private requires exact origin/final
     endpoint binding).
   - Concrete counterexample or sequence: the mainline flow - an MCP client
     supplies an env overlay for command execution; adapter-to-daemon is
     exactly an authenticated local-process boundary. The positive tuple at
     :2135 requires an active endpoint binding; the activity rule at :1953 +
     :1979-1984 rejects that same tuple for carrying an active value in a
     non-active axis.
   - Incorrect/undefined result and consequence: the most common local overlay
     transport is both valid and structurally rejected. An implementation that
     follows :1953 ships local overlays without final-target binding (weakening
     the misdirection defense the axis exists for); one that follows :2135
     fails the structural gate.
   - Exact required specification correction: rewrite :1953 to "active for
     authenticated-local and cross-boundary overlay transport/persistence
     operations" (matching :2135/:2215-2217/:2378/:2723-2725), or make :2135's
     endpoint binding `not_applicable` and justify why local final-target
     binding is unnecessary. The first direction matches the rest of the text.
   - Verification boundary: FR-096 checker; SC-020 cross-boundary and
     local-overlay channel fixtures.

4. **[high] The exact same-incident bound-effect bundle is accepted by the causal-source rules but has no accepting row in the ordered channel combination function**
   - Model/domain: SecurityPropertyModel causal accounting x TransitionModel
     `bound_effect_bundle_commit`; spec FR-062.
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:1994-1998`
     (closed accepted form: `many + exact_same_incident_bound_effect_bundle`,
     members are distinct sources with "no duplicate or projection"),
     spec.md:761-763 (FR-062: "Multiple sources are valid only as either exact
     projections of one primary causal source or distinct complete members of
     the exact same-incident `bound_reached` Transition bundle"), vs
     `:2274-2282` (ordered total combination function: row :2277 rejects
     multi-source only for the bad correlations, row :2278 accepts only
     "several exact same-source/same-incident/same-disposition projections",
     and the residual row :2282 structurally rejects everything else) and
     `:2286-2287` ("The function is applied before every property-specific
     success rule; no later derivation may select a different result").
     Bundle commit is mandated at `:1085` and `:3160-3174`.
   - Concrete counterexample or sequence: one private resolver invocation
     reaches both its opaque-input-byte cap and its resolver-candidate cap in
     the same scheduler cut -> two distinct `reached_and_stopped` sources with
     exact counters -> cardinality `many`, correlation
     `exact_same_incident_bound_effect_bundle` -> accepted by :1994-1998 and
     required by the IndependentBoundModel multi-reach rows, but no row of
     :2274-2282 accepts it, so it falls to :2282 structural `rejected`.
   - Incorrect/undefined result and consequence: the mandated
     `bound_effect_bundle_commit` can never obtain its Security receipts for a
     legal double-cap stop; simultaneous bound reaches become unclassifiable
     (reachable deadlock/undefined behavior on a mandatory safety path).
   - Exact required specification correction: add a combination-function row
     between :2277 and :2278 accepting `many +
     exact_same_incident_bound_effect_bundle` with complete member-key
     coverage, preserving each member's `bound_reached` disposition and
     issuing/consuming the bundle-bound receipts of the exact Transition
     bundle; keep every other multi-source shape rejected.
   - Verification boundary: FR-096 checker; IndependentBoundModel /
     `bound_effect_bundle_commit` fixtures (multi-reach dominance rows
     matrix:3117-3129).

5. **[medium] OperationModel publishes two different `retry` values for a successfully committed keyed admission**
   - Model/domain: OperationModel (derived outputs; request-key lifecycle).
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:1728-1729`
     (clause 2 total classification: `present_new -> retry=safe`; clause 4
     success at :1760-1762 changes key state/effect but never retry), vs
     `:1776-1779` ("A committed `present_new` key remains `present_matching`
     with effect `bound_atomically` and retry `resolved`"), with `:1629-1634`
     defining `retry=resolved` as promised only through the persisted lease and
     `retry=safe` as an unbound-key property.
   - Concrete counterexample or sequence: keyed new-campaign start, authorized,
     committed, no wait. Clause chain yields `retry=safe` (never overridden);
     the postcommit paragraph asserts the same response carries
     `retry=resolved`.
   - Incorrect/undefined result and consequence: the wire `retry` field for the
     most common keyed-success row is ambiguous; fixtures and clients will
     disagree on blind-retry semantics.
   - Exact required specification correction: add an explicit retry override to
     clause 4's success text (committed `present_new` -> `retry=resolved`
     together with `bound_atomically` and the returned horizon), or correct
     :1777-1778 to `retry=safe`; state exactly one.
   - Verification boundary: OperationModel fixtures (SC-021) and the FR-068
     idempotency contract tests.

6. **[medium] Decoder private-input counter-observation `replayed` maps to two different receipt effects**
   - Model/domain: SecurityPropertyModel (replay causality partition x
     fixed_helper_decode derivation).
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:2260`
     ("private-input counter-observation enum `replayed` ... deliberate
     no-receipt rejection ... `receipt_effect = no_receipt` (or `rejected` for
     a structural projection)"), vs `:2627-2629` ("`exceeded`, missing,
     mismatched, replayed, or `not_applicable` required bound/counter evidence
     derives `decoder_admission = rejected` and `receipt_effect = rejected`").
     `replayed` is runtime evidence, not a structural projection, so the
     parenthetical cannot reconcile them.
   - Concrete counterexample or sequence: fixed-helper decode whose private
     input counter observation is `replayed` -> :2260 requires
     `no_receipt`, :2628 requires `rejected`.
   - Incorrect/undefined result and consequence: receipt-effect accounting (the
     basis of the causal-cardinality rules and SC-008/SC-018 receipt canaries)
     is contradictory for this cell; both sides reject the input, so the risk
     is checker/fixture inconsistency rather than unsafe behavior.
   - Exact required specification correction: exempt the decoder's
     bound/counter `replayed` from :2628's `receipt_effect = rejected` list
     (mapping it to the :2260 no-receipt class), or name
     `fixed_helper_decode` as an explicit exception inside :2260; one
     authoritative mapping.
   - Verification boundary: FR-096 checker; receipt-effect accounting fixtures.

7. **[medium] surface_name_application tuple table has no row for preliminary `canonical_*`/`rejected` with observation `unproved`**
   - Model/domain: SecurityPropertyModel (surface_name_application).
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:2049-2057`
     (rows: canonical+`not_reached`; omitted+counter; rejected+`not_reached`;
     `unknown|unproved`+`not_reached|unproved`; omitted+`unproved`;
     any+`missing|mismatch|replayed`; enumerated structural remainder). The
     pairs (`canonical_*`, `unproved`) and (`rejected`, `unproved`) appear in
     no row, while observation `unproved` is in-domain at `:1911`. Unlike
     :2282 and :2328 this table carries no "every remaining assignment"
     residual row.
   - Concrete counterexample or sequence: preliminary verdict `canonical_utf8`,
     private surface-name observation receipt `unproved` (counter
     infrastructure unproved), consumer `command_request` -> no row selects a
     decision.
   - Incorrect/undefined result and consequence: undefined classification in a
     table that FR-083 requires to derive "exactly one" application decision;
     an implementer must guess between `canonical` (unsafe: authorizes
     transport on an unproved observation) and `unknown`/structural reject.
   - Exact required specification correction: extend row :2054 to cover any
     non-`not_applicable` preliminary verdict paired with observation
     `unproved` (deriving `unknown`, `no_receipt`), or add an explicit
     residual "every remaining assignment -> structural rejected" row; state
     which.
   - Verification boundary: FR-096 checker totality for the
     surface_name_application property.

8. **[medium] Non-audit receipt-correlation `mismatch` is routed to a disposition table that has no row for it**
   - Model/domain: SecurityPropertyModel (private_environment_resolution ->
     source-total disposition table).
   - Evidence: `specs/003-environment-probe/scenario-matrix.md:2560-2561`
     ("Receipt mismatch/replay follows the exact source-total disposition table
     and never authorizes"), vs `:2205-2217` (the table's mismatch rows cover
     only the action-audit correlation subclasses (:2207) and private endpoint
     binding (:2215); non-audit rows cover `replayed` (:2209-2210) and
     `expired` (:2211) only). The correlation domain at `:1903` tracks sensor/
     resolver/spawn/decoder correlations independently, so e.g. a sensor
     receipt `sensor_class_mismatch` inside private resolution has no row.
   - Concrete counterexample or sequence: private resolution whose bound sensor
     verdict is `allowed` but whose sensor-receipt correlation is
     `sensor_class_mismatch` -> :2560 directs to a table with no matching row.
   - Incorrect/undefined result and consequence: dangling normative
     cross-reference; the receipt effect (no_receipt vs rejected) and
     campaign/non-campaign treatment for non-audit mismatches are undefined at
     the referenced authority.
   - Exact required specification correction: add a source-total row for
     non-audit immutable-receipt correlation `mismatch` (property-local
     `rejected`, exact treatment and receipt effect stated), or change
     :2560-2561 to route mismatch to the property-local rules and reserve the
     table for replay/expiry.
   - Verification boundary: FR-096 checker; private-resolution receipt fixtures.

9. **[medium] The Mandatory Live Conformance Families omit normative minimums they exist to gate**
   - Model/domain: Mandatory Live Conformance Families (matrix:3537-3707) vs
     FR-072/FR-075/FR-102/FR-054/FR-055/SC-014. All bullets lead-read.
   - Evidence (each an absence from the family list at
     `specs/003-environment-probe/scenario-matrix.md:3555-3704`, verified
     against the normative sources cited):
     (a) retention stage 2: only expiry is a family (`:3649-3650`); tombstone
     purge, global-store victim selection, empty-partition audit fold, and the
     saturated no-victim wake path (`:3050-3051`, `:3176-3193`; spec.md:965-981
     FR-075) have no family;
     (b) beachhead proof states: the dispatch family `:3700-3704` enumerates
     fifteen states but omits `retired`, whose dispatch outcome is normative
     (`:3451`; spec.md:1168-1172 FR-102);
     (c) capacity-ledger dimensions: the bounds family `:3641-3648` covers six
     dimensions; admission/queue/branch dimensions of FR-072 (spec.md:914-929 -
     queued/active campaigns, aliases and key indexes, run-registry records and
     bytes, retention partitions, helper processes, concurrency, topology
     nodes/edges, sensors, requirements, names, result detail) have no live
     family at below/reached states;
     (d) names-only census: no family exercises census counters, truncation
     markers, or full-census consent rejection (census domain `:2956-2999`;
     spec.md:620-627 FR-054/FR-055);
     (e) operator-local plans: the plan family bullet `:3571-3572` covers the
     ten built-ins plus typed custom requirements only; selection/composition
     of an `operator_local_composite` plan (domain member `:513`; SC-014)
     appears in no live family (administration-only coverage at `:3557-3570`).
   - Concrete counterexample or sequence: an implementation that leaks
     tombstones and key indexes forever (never purges), dispatches on retired
     proofs, or ships an unenforced census consent gate still passes the
     generated mandatory live universe, because FR-084 generates the minimum
     live universe from these families.
   - Incorrect/undefined result and consequence: the "minimum live gates" green
     signal overstates coverage on exactly the destructive-deletion,
     stale-authority, and consent boundaries. Severity is medium, not high,
     because the exhaustive symbolic checker and concrete witness corpus
     (FR-096, SC-007, SC-012, SC-018) still mandate these paths outside the
     live gate.
   - Exact required specification correction: extend the family list with (a) a
     retention purge/saturation family (timer purge, ceiling purge with
     deterministic victim and protected/continuity-unproved rejection, final
     partition fold, saturated no-mutation wake), (b) `retired` in the
     beachhead-proof family, (c) admission/queue/branch capacity dimensions at
     below/reached in the bounds family (including `rejected_bound` and queue
     wait/wake), (d) a census family (counters, markers, consent present/
     absent/mismatched/replayed, unrequested-name rejection), (e) an
     operator-local composite selection/composition row.
   - Verification boundary: FR-084 generated required-key set and its CI
     missing-key report.

10. **[medium] The specification's constitutional authority exists only as an uncommitted worktree amendment**
    - Model/domain: Development Review Gate (spec.md:1422-1428); FR-066's
      "constitution-authorized private bounded typed decoder" (spec.md:812-813);
      sensor gating, channel authentication, and new-cap-defaults language.
    - Evidence: `.specify/memory/constitution.md` worktree version 2.1.0
      (private-decoder exception L91-97, environment-sensor/connector gating and
      new-cap-resolves-false L63-76, authenticated-channel/E2E-confidentiality
      L111-116, connector/sensor audit L123-128) is an UNCOMMITTED modification;
      `git diff` shows HEAD carries constitution v1.0.0 with none of those
      articles, and the worktree Sync Impact Report claims "2.0.0 -> 2.1.0"
      while no 2.0.0 was ever committed. `.specify/feature.json` retarget to
      `specs/003-environment-probe` is likewise uncommitted. Additionally the
      worktree constitution's own claim "persistence is rusqlite + refinery"
      (L181-182) contradicts code: migrations use a manual runner because
      refinery pins an older rusqlite (`crates/store/src/lib.rs:57-59`).
    - Concrete counterexample or sequence: a planner starts from committed
      HEAD (fresh clone) and finds a spec whose decoder exception, sensor
      classes, and channel requirements contradict the committed constitution
      v1.0.0; the review gate's consistency claim is unanchored.
    - Incorrect/undefined result and consequence: the spec's governing
      authority is unstable worktree state; the "no unresolved contradiction
      among this specification, the project constitution, current code" gate
      cannot be durably evaluated until the amendment is committed.
    - Exact required specification correction: commit constitution 2.1.0 (with
      a corrected version lineage or an honest 1.0.0 -> 2.1.0 impact note) and
      the feature.json retarget before planning; fix the refinery claim at
      L181-182 to match the actual manual migration runner.
    - Verification boundary: git history at the planning gate; constitution
      text vs `crates/store/src/lib.rs:57-59`.

11. **[low] Editorial/machine-readability defects in normative text (bundled; shared root cause: hand-maintained closed tables)**
    - Model/domain: cross-cutting.
    - Evidence and items (lead-read unless marked):
      (a) `scenario-matrix.md:2360-2367`: rows :2365-:2367 collapse the two
      input axes into one cell under a three-column header; a column-indexed
      consumer of the normative table mis-reads the result cell as the proof
      axis;
      (b) `scenario-matrix.md:1017`: sentence begins mid-thought ("whole bundle
      uses cause `bound_reached`...") after ":1016 ...leave Route/Goal
      unchanged." - a lost sentence subject in a normative paragraph;
      (c) `scenario-matrix.md:1911` `exact_scope_limit_counter` vs `:1917`
      `exact_scope_limit_and_counter`: two near-identical receipt tokens for
      different receipts; `:1954`/`:1996` reference the :1917 token - fragile
      for literal-string checker generation;
      (d) delegate-read, lead-accepted: `:2434` blanket rule "`unproved` is
      unknown or omitted" is contradicted by `:2145` (audit-correlation
      unproved -> `unavailable`) and the diagnostic rows `:2315-2326`
      (unproved -> `rejected`); `:1190-1222` proof-invalidation table is
      declared "exact" yet omits the `proof_validation_failed|unavailable`
      transitions that live inline at `:1087-1088` while including the equally
      inline `proof_expired`; `:3092` uses "EvidenceModel" for
      EvidenceBoundaryModel (sole occurrence); `:3110-3111` effect ids
      (`queue_or_gate`, `snapshot_name_omitted`, `execution_name_rejected`)
      differ from their role names (`queue_gate`, `snapshot_name_omit`,
      `execution_name_reject` at `:3036`, `:3040`); `:1381` "catalog" vs
      "catalogue" elsewhere; `:3122` uses the undefined predicate
      "lease-eligible"; `:3039` writes the Transition cause `bound_reached`
      where the Goal-evidence value `bound_before_route` (`:626`) is meant;
      `:2969` vs `:3016` reuse the axis name "Reached-dimension set" for two
      different domains; goal-aggregation clause 1's second disjunct
      (`:3512-3513`) is unreachable because `:685-694` rejects
      `safety_or_executability_open` alongside a safe route (lead-read at
      :685-694); census marker matching for `raw_entries` vs `emitted_items`
      is not individually defined (`:2969-2988`).
    - Concrete counterexample or sequence: any generator that derives the
      FR-096 machine-readable model mechanically from these tables/enums
      mis-parses or fails on each listed cell.
    - Incorrect/undefined result and consequence: no wrong runtime behavior is
      forced; the cost lands on the mandated machine-readable model and
      checker, plus reviewer/implementer confusion.
    - Exact required specification correction: normalize the listed cells,
      names, and enums; give every closed table an explicit residual row; add
      the missing sentence subject at :1017; define or remove "lease-eligible";
      align effect ids with role names; pick one spelling of catalogue.
    - Verification boundary: FR-096 model generation from the corrected file.

## Model-by-Model Coverage Record

- RouteModel (matrix:183-376): lead-read in full. Domains, record-kind/verdict
  compatibility, upstream-gate and axis projections, freshness algebra,
  conflict resolution, joint-winner order, and terminal-event mapping audited.
  Totality holds except as broken by Finding 1's projection gaps. The
  connector-`unknown` route (`unsupported`, never policy-denied) is consistent
  across spec.md:1242, matrix:157-158, :256. Delegate candidate "row 1293
  admits an all-satisfied exhausted branch" was REFUTED by the lead: the
  derived-joint-winner column plus the winner order at matrix:330-348 confines
  that row to the missing/non-current-proof case.
- GoalModel (matrix:377-715): lead-read in full, including the goal-plan
  administration submodel, plan selection/comparator submodel, and terminal
  aggregation. Ordered classification is disjoint and total; comparator
  trusted-revision gating matches FR-050; the staged-maxima freshness
  comparator avoids fabricated interval orders. Entailment spot-checks
  (spec.md:1277-1293) passed for {denied,blocked}, {denied,unknown},
  {unreachable,denied}, and the zero-route `bound_before_route` case. One dead
  defensive disjunct recorded in Finding 11.
- TransitionModel (matrix:716-1315): lead-read 716-1090 and 1269-1315 plus
  verification reads; 1092-1268 delegate byte-read with lead spot-verification
  of the cause table (:1081-1090) and bridge (:1269-1315). Batch atomicity,
  capacity ledger, queue waits, deadline registrations, overlay migration
  member, retention lifecycle members, and the simple-progress rows audited;
  every purpose/event/cause used in 1081-2000 resolves to its domain (:728,
  :733, :734). Findings 1 and 11 apply.
- OperationModel (matrix:1317-1890): delegate byte-read; lead-verified the
  horizon/lease semantics (:1622-1643, :1680-1692) and clauses 2-4
  (:1720-1781). Pre-admission recovery (FR-007) allocates nothing; the
  engine-loss horizon path is correctly gated on `retention_lease_reanchor`
  (:1687-1688) - a delegate candidate to the contrary was REFUTED. Finding 5
  applies; two delegate-observed lower-confidence items (fresh-alias-on-
  terminal falling to the untyped default; queue-gate wording scoped
  ambiguously at :1482-1483 vs :1811-1812) were not independently verified by
  the lead and are recorded as Missing Verification rather than findings.
- SecurityPropertyModel (matrix:1892-2831): lead-read the complete input-axis
  domain (:1894-2000) and the load-bearing tables (:2047-2060, :2127-2136,
  :2205-2218, :2255-2290, :2355-2368, :2556-2565, :2624-2631); remainder
  delegate byte-read. Nine properties, pin provenance, resolver context
  binding, producer/sink attestation, channel algebra, overlay
  transport/persistence, and diagnostic egress audited. Findings 2, 3, 4, 6,
  7, 8 apply. Anti-circularity statements are consistent wherever a property
  both emits and could consume a receipt.
- EvidenceBoundaryModel (matrix:2832-3535): lead-read :2885-2910 and the
  structural-constraint/outcome-function tails via targeted reads; remainder
  delegate byte-read. Native-name identity, census, independent bounds,
  boundary receipt composition, and the 22 structural constraints audited.
  Findings 2 and 11 apply; census marker underdefinition recorded.
- Cross-model compositions: the two declared staged compositions (matrix:39-44)
  are acyclic as written; the pending-boundary one-way order (:941-1001)
  contains no receipt cycle. The compositions are where Findings 2 and 4 bite.
- Mandatory conformance coverage: matrix:3537-3707 lead-read in full; FR-050,
  FR-053 producer x sink canaries, FR-058 migration, FR-062 channel families,
  FR-066 decoder caps, and FR-084 universe generation are covered; the five
  omissions in Finding 9 are not.

## Missing Verification

- AAP-side Firecracker/vsock consumer: no AAP checkout was fingerprinted; the
  pinned-revision integration gate (FR-081, SC-013, matrix:3605-3607,
  :3695-3699) is an external obligation. Zero vsock/firecracker code exists in
  this repository (repo-wide grep), consistent with the spec assigning that
  connector to AAP - but consumer compatibility is UNVERIFIED.
- FR-095 manual onboarding baselines and FR-100 frozen artifacts (response byte
  cap, tokenizer suite, fixture corpus, measurement command): do not exist yet;
  they are pre-implementation obligations the spec itself imposes. Not a
  defect; recorded here as unfulfilled gates.
- Suspend-inclusive monotonic clock (matrix:3054-3059; FR-070/FR-073): the
  existence of an approved per-OS clock source whose monotonic contract
  includes system suspend was not verified against platform APIs.
- Comparator "independent official reference fixtures" (FR-050): external
  corpus sourcing unverified.
- Shipped registries the live universe generates from (harness/surface support
  manifest, per-surface schema registries, sensor catalogue, plan/comparator
  registries, private-store inventory, producer registry, sink inventory;
  matrix:3539-3553): none exist in code yet; they are new deliverables. Their
  feasibility was not further verified.
- Two delegate-observed OperationModel candidates (fresh-alias-on-terminal
  untyped fallthrough at :1541-1550; queue-gate scope wording at :1482-1483 vs
  :1811-1812) were not lead-verified and remain unadjudicated.
- The 2026-07-17 review disposition's claim of recorded adjudication
  (requirements.md:47-49) points into quarantined historical material and was
  deliberately not verified (see Historical Material Consultation).

Absence of evidence in every item above is recorded as absence, not as
confirmation.

## Claims Independently Confirmed Against Code

All confirmations at HEAD 40eb16f via the evidence files listed above:

1. FR-001's premise: the compact surface has exactly five facades
   (`crates/mcp/src/surface_list.rs:68`), a hard count anchor
   (`surface_list.rs:310` asserts 5), an admission gate (`:277-279`), and
   fixture-map drift tests (`crates/mcp/tests/fixture_catalogue_contract.rs:53-96`);
   the full surface is exactly 51 tools with its own anchors. The "count
   source, count anchors, admission tests, and discovery fixtures change
   together" requirement names real, existing artifacts.
2. FR-077/FR-010's "legacy discovery sentinel": real. `system_discover`
   dispatch performs no policy evaluation
   (`crates/daemon/src/ipc/server.rs:550-552`);
   `DaemonState::discover_environment` (`crates/daemon/src/state.rs:366-370`)
   spawns probe processes first (`crates/daemon/src/environment/probe.rs:41-93`,
   `:449-487`, `:589-661`) and applies policy only to filter advertised routes
   (`crates/daemon/src/environment/mod.rs:17-69`, commit cd0d906). The spec's
   migration demand is grounded.
3. FR-057's legacy value-bearing public fields: real.
   `ShellSessionStatusResponse.env_snapshot: Vec<(String, String)>`
   (`crates/ipc/src/protocol.rs:2297-2312`) carries NAME=VALUE pairs, and
   terminal identity reads environment VALUES (`TERM_PROGRAM`, `TERM`,
   `TERM_PROGRAM_VERSION`) into public fields
   (`crates/daemon/src/environment/probe.rs:543-587`).
4. FR-058's migration target: real. Workspace snapshots persist overlay env in
   SQLite (`crates/store/src/workspace.rs:29-39`, `:70-88`; migration V0006)
   with capture-time redaction that stores the literal `<redacted>` marker
   (`crates/daemon/src/shell_session.rs:321-332`,
   `crates/daemon/src/command.rs:2528-2542`), and snapshot apply restores env
   into a live session (`shell_session.rs:497-527`) - i.e., today redaction
   markers are persistable and restorable, exactly what FR-058 forbids and
   migrates.
5. Profile table names match code exactly: `developer_local`, `repo_only`,
   `read_only_observer`, `admin_debug`, `full_access`
   (`crates/daemon/src/policy.rs:64-75`). Current caps are exactly four
   (`policy.rs:157-162`); `full_access` presets all four true (`:168-186`),
   which is why the spec's explicit "environment caps absent from config remain
   false" carve-out for `full_access` (spec.md:1252) is a real, needed
   divergence from the preset pattern - and matches the worktree constitution's
   new-cap rule.
6. None of the proposed `allow_environment_*` caps, `Environment*`/
   `HarnessContextIngest`/`OverlayMigrationRepair` actions, campaign object,
   durable request keys, attestation, protocol-version negotiation, connector
   nonce/replay protection, fixed helpers, or typed decoder exist in code
   (grep-verified absences) - the spec correctly treats all of them as new.
   The existing action enum (14 variants incl. `CommandStart`, `FileRead`;
   `policy.rs:79-140`) contains the underlying actions the taxonomy references.
7. FR-065's fail-closed audit is a real change: today every audit emit is
   fire-and-forget (`let _ = ...emit(...)` at `crates/daemon/src/router.rs:85`,
   `command.rs:787`, `:1541`, `file_watch.rs:214`, `pty_command.rs:244`, `:606`,
   ipc handlers), and the sink trait documents non-blocking intent
   (`crates/daemon/src/audit.rs:25-43`).
8. FR-080/FR-081's embed premise: in-process embedding already exists as the
   public `terminal_commanderd` library (`crates/daemon/src/lib.rs:1-15`,
   `DaemonState::bootstrap` at `state.rs:181`,
   `examples/embed_in_process.rs:5-34`); no narrow semver-stable facade exists
   yet, which is precisely what FR-081 introduces.
9. FR-043's separation target: the current rule registry (TC13) is
   `rules`/`rule_versions`/`rule_tags`/`rule_activations` + FTS5
   (`crates/store/migrations/V0002__registry.sql`,
   `crates/store/src/registry.rs`); "sifter rule" is informal commentary, and
   goal plans reusing none of it is implementable as specified.
10. WSL/remote reality matches the spec's starting assumptions: WSL routes
    exist (`environment/router.rs:34-50`, runner forwarding with a Windows-arm
    stub at `environment/wsl.rs:21-45`), remote is operator-forwarded SSH only
    (`config.rs:589-616`) with adapter-side target routing gated on
    `allow_remote` (`crates/mcp/src/tools.rs:537`), `EnvironmentSpec::SshHost`
    is unimplemented (`environment/router.rs:66-68`), and no identity pinning
    exists - consistent with FR-062/FR-063/FR-094 being new machinery.
11. Existing safeguards the spec builds on are real: secret-value canary tests
    (`command.rs:2849-3033`), WSLENV allowlist rebuild
    (`environment/wsl.rs:72-90`, `core/src/platform.rs:23-31`), structural
    privilege denials (`daemon/tests/security.rs`), forbidden-output negative
    fixtures (`tests/fixtures/contracts/forbidden/`), and the store's
    single-writer WAL actor (`crates/store/src/lib.rs:4-23`).

## Historical Material Consultation

`docs/reviews/2026-07-17-environment-probe-fable-review.md` and
`docs/reviews/2026-07-17-environment-probe-review-disposition.md` were NOT
consulted at any point, by the lead or by any delegate (all delegates were
instructed to and did avoid `docs/reviews/`). Every finding above was derived
solely from the specification files, the constitution/feature worktree state,
and current code. No conclusion is inherited from, compared against, or
adjusted toward the 2026-07-17 material.

## Final Planning Gate: NOT READY

- Blocking corrections, if any (all in `specs/003-environment-probe/scenario-matrix.md`):
  1. Finding 1: add terminal-prior `excluded` evidence-projection rows and
     admit `timed_out` in the truncation row (and define "preterminal").
  2. Finding 2: reconcile the preliminary native-name verdict domain
     (`unproved`) between matrix:1910/2054-2055 and matrix:2893-2897.
  3. Finding 3: reconcile private-endpoint-binding activity (matrix:1953)
     with the authenticated-local overlay tuple (matrix:2135, :2215-2217,
     :2378, :2723-2725).
  4. Finding 4: add the `many + exact_same_incident_bound_effect_bundle`
     accepting row to the ordered channel combination function
     (matrix:2274-2282).

The four blocking corrections are narrow and mechanical relative to the size
of the model; none undermines the feature's architecture, security posture, or
LLM-utility case, and every code-facing premise sampled proved true. After the
four HIGH corrections (and ideally the MEDIUM batch, especially committing the
constitution amendment), this specification is a strong candidate for GREEN on
a re-fingerprinted state.
