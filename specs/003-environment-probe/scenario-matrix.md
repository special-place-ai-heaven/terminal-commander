# Normative Scenario Matrix: Goal-Directed Environment Probe

**Status**: Draft normative domain for feature 003

## Purpose

This file closes the verification domain. Implementers do not choose comfortable
rows. The machine-readable model required by FR-096 MUST encode every value and
derived function below and prove total classification through exhaustive
symbolic or partitioned evaluation. Impossible combinations remain explicit
`unsupported` constraint classes with a reason; they are never deleted.

The logical matrix is exhaustive without materializing an astronomical row
file. Concrete fixtures witness every domain value, decision partition,
transition, failure class, supported boundary, and unsupported constraint. Live
conformance is additional evidence; it does not replace the symbolic proof.

The model has six related but independently total layers:

1. `RouteModel` classifies one bounded connector sequence as one terminal route
   outcome.
2. `GoalModel` aggregates the finite set of route outcomes plus the remaining
   frontier into one goal outcome.
3. `TransitionModel` proves every branch, topology, identity, policy, proof, and
   run-lifecycle transition.
4. `OperationModel` classifies admission, retry, attachment, cancellation, and
   transport behavior without fabricating a goal outcome.
5. `SecurityPropertyModel` exhaustively proves capability derivation, executable
   trust, non-campaign surface-name application, channel
   binding/confidentiality, overlay persistence, typed decoding, and diagnostic
   sanitization.
6. `EvidenceBoundaryModel` exhaustively proves platform-native environment-name
   identity, names-only census consent/encoding, and every independent sensing,
   output, retention, and concurrency bound.

No layer may borrow an outcome from another layer merely to achieve totality.
Declared immutable receipt dependencies are not borrowing: each consumer binds an
exact producer result and cannot override it. The only staged cross-layer
compositions beyond ordinary receipt consumption are (1) preliminary
non-campaign name classification -> Security surface application -> final
EvidenceBoundary receipt and (2) private soft/informational bound observation ->
conditional Security admission -> Transition effect/cleanup -> final
EvidenceBoundary receipt -> Transition publication. Both are explicitly acyclic; neither final receipt can
feed an effect that it is supposed to prove.

## Common Finite Domains

| Axis | Required values |
|---|---|
| Harness | `codex`, `claude_code`, `cursor`, `generic_mcp`, `aap_embed`, `unknown_mcp`, `unknown_embed` |
| Harness version state | `known_compatible`, `known_incompatible`, `newer_unknown`, `missing` |
| Delivery | `mcp_local_ipc`, `mcp_forwarded_ipc`, `embedded_engine` |
| Product surface | `compact_environment`, `full_environment_probe`, `legacy_system_discover`, `legacy_target_list`, `legacy_target_probe`, `embedded_facade` |
| Harness context | `adapter_observed`, `caller_asserted`, `mixed`, `absent`, `conflicting` |
| Root capability | `one_fresh_root`, `several_roots`, `stale_root`, `no_roots`, `unsupported` |
| Operating system | `windows`, `linux`, `macos`, `unknown` |
| CPU/ABI relation | `native_match`, `supported_emulation`, `incompatible`, `unknown` |
| Virtualization facet | `no_boundary_observed`, `wsl1`, `wsl2`, `vm`, `firecracker`, `unknown` |
| Container facet | `no_boundary_observed`, `docker`, `podman`, `generic_container`, `unknown` |
| Sandbox facet | `no_boundary_observed`, `harness_managed`, `os_native`, `restricted`, `unknown` |
| Automation facet | `interactive`, `headless_service`, `ci`, `unknown` |
| Privilege facet | `standard`, `elevated`, `restricted`, `unknown` |
| Connector kind | `local_direct`, `wsl`, `forwarded_remote`, `container`, `sandbox`, `vm_guest`, `firecracker_vsock`, `embedded`, `unknown` |
| Connector direction | `outbound`, `inbound_forward`, `bidirectional_local`, `not_applicable` |
| Execution lane | `direct_argv`, `shell`, `persistent_session`, `none` |
| Lane schema state | `exact_valid`, `placeholder_only`, `invalid_action_shape`, `unsupported_by_peer` |
| Windows command host | `not_applicable`, `direct_win32`, `cmd_builtin`, `powershell_desktop`, `powershell_core`, `unknown` |
| Shell state | `not_required`, `allowed`, `policy_denied`, `interpreter_denied`, `edition_or_version_mismatch`, `unknown` |
| Workspace source | `explicit_override`, `tc_session_cwd`, `client_root`, `policy_root`, `discovered_marker`, `none`, `ambiguous` |
| Workspace source authority | `authoritative`, `candidate`, `stale`, `conflicting`, `unavailable` |
| Workspace mapping | `same_path_verified`, `translated_identity_verified`, `reachable_identity_mismatch`, `unreachable`, `unknown` |
| Workspace identity | `verified`, `mismatch`, `unknown`, `not_applicable` |
| Goal family | `automatic`, `build`, `test`, `run`, `develop`, `diagnose`, `custom_requirements` |
| Goal-plan state | `one_valid`, `several_composed`, `conflict`, `missing`, `invalid`, `unknown`, `registry_head_changed_after_admission`, `frozen_snapshot_unavailable`, `frozen_digest_mismatch` |

Isolation fields are composable facets, not a mutually exclusive label. A node
may therefore be, for example, `ci` + `harness_managed` + `wsl2` + `docker` +
`restricted` at the same time.

## Per-Node and Per-Edge Topology Domain

For every connector sequence of length zero through eight, the checker creates a
bounded typed graph. It uses equality classes rather than raw identifiers.

`Node(i)` contains:

| Field | Required values |
|---|---|
| Role set | every non-empty subset of `harness`, `adapter`, `engine`, `session`, `runner`, `shell`, `forwarder`, `target`, `guest` |
| Process equivalence | `new`, `same_as_node_j` for each `j < i`, `unknown` |
| OS and ABI | values from the common domains |
| Isolation | one value from every composable isolation facet |
| Persistent identity relation | `new`, `same_as_node_j` for each `j < i`, `changed`, `unknown` |
| Boot identity relation | `new`, `same_as_node_j` for each `j < i`, `changed`, `unknown` |
| Workspace context | source, authority, mapping, and identity values from the common domains |

`Edge(i)` contains:

| Field | Required values |
|---|---|
| Endpoints | `source = Node(i)`, `target = Node(i+1)` |
| Connector and direction | values from the common domains |
| Governing policy authority | `origin`, `forwarder`, `target`, `multiple`, `unavailable` |
| Governing decision | `allowed`, `denied`, `unavailable` |
| Transport | `not_configured`, `stopped`, `connecting`, `reachable`, `name_failure`, `refused`, `timed_out`, `protocol_skew`, `replaced` |
| Protocol relation | `exact`, `compatible`, `peer_too_old`, `peer_newer_unknown`, `malformed`, `unavailable` |
| Identity verdict | `not_applicable`, `structural_local_verified`, `pinned_verified`, `unverified`, `mismatch`, `changed`, `unavailable` |
| Identity mismatch cause | `target_persistent_identity_mismatch`, `target_boot_mismatch`, `connector_binding_mismatch`, `connector_clone_detected`, `not_applicable`; exactly one non-`not_applicable` cause is required only for verdict `mismatch`, and every other verdict requires `not_applicable` |
| Target workspace reference | `reference_to_target_node`, `contradictory_copy`, `unavailable` |

Topology transition facets independently range over `stable\|changed` for policy
revision, catalogue trust/security revision, connector instance, target persistent
identity, target boot identity, workspace identity, cwd/action schema, and
no other generic topology state. Each facet carries one change disposition from
`not_applicable`, `authoritative_revision`, `approved_pre_execution`,
`unapproved_pre_execution`, or `mid_run`. `stable` requires `not_applicable`.
Policy/catalogue-trust-security/cwd/action changes require
`authoritative_revision` and are recoverable. Connector/target/workspace changes require one of the three
execution-phase dispositions: approved pre-execution is recoverable; unapproved
pre-execution and mid-run are hard exclusions. Every other state/disposition pair
is a rejected structural constraint. Cycle detection is derived from persistent
and boot equality classes; target replacement is fully represented by the exact
identity change and its execution-phase disposition, not a second graph enum.

Role and process compatibility is a closed derived table:

| Delivery or connector | Required roles/process relation |
|---|---|
| `mcp_local_ipc` | distinct adapter and engine processes; authenticated local-peer edge |
| `mcp_forwarded_ipc` | origin adapter, forwarding role, and target engine in distinct processes |
| `embedded_engine` / `embedded` | approved host and engine share one process; no manufactured adapter role |
| zero-hop passive sensing | campaign-owning engine is present; no execution target is invented |
| `local_direct` execution | engine source to distinct runner/target process |
| `wsl` | engine/forwarder source to the distinct counterpart runner/target on its attested Windows-host/Linux-distro pair |
| `forwarded_remote` | forwarding source to distinct target-engine process |
| `container` or `sandbox` | source engine/forwarder to distinct runner/target with the matching isolation facet |
| `vm_guest` | source engine/forwarder to distinct VM runner/target |
| `firecracker_vsock` | source engine/forwarder to distinct node containing both guest and target roles |

Connector OS and direction compatibility is also closed. `known` below means one
of Windows, Linux, or macOS; an `unknown` endpoint keeps the route unknown until
attested and never silently satisfies the row.

| Connector/delivery edge | Exact source OS -> target OS | Exact direction |
|---|---|---|
| zero-hop / `embedded` | same process and therefore the same known OS | `not_applicable` |
| `mcp_local_ipc` delivery prelude | same known OS | `bidirectional_local` |
| `mcp_forwarded_ipc` delivery prelude | any known -> any known with exact forwarder/target roles | `inbound_forward` at the target-facing socket; its originating connector edge remains `outbound` |
| `local_direct` | same known OS; supported emulation changes ABI, not OS identity | `outbound` |
| `wsl` host-to-guest | Windows host -> its attested Linux target with target `wsl1\|wsl2` | `outbound` |
| `wsl` guest-to-host interop | Linux source with source `wsl1\|wsl2` -> its attested Windows host target; arbitrary Linux -> Windows is not WSL | `outbound` |
| `forwarded_remote` | any known -> any known | `outbound` or `inbound_forward` exactly as fixed by the configured forward direction; never `bidirectional_local` |
| `container` | any known host -> known target with matching container facet | `outbound` |
| `sandbox` | any known host -> known target with matching sandbox facet | `outbound` |
| `vm_guest` | any known host -> known guest with VM facet | `outbound` |
| `firecracker_vsock` | Linux host -> Linux Firecracker guest | `outbound` |
| `unknown` connector | no accepted OS pair; Route representability is unsupported | no accepted direction; Route representability is unsupported |

Every known connector assignment outside its row is an unsupported structural
class. A reverse route is represented by a separately admitted edge with its own
source/target roles and authority, never by relabelling an outbound edge. This
table composes at every hop, so Windows -> WSL -> container -> Firecracker and
other nested paths are checked edge by edge rather than granted by the root
harness label.

The campaign execution graph begins at a node containing `engine`; the delivery
prelude supplies the required harness/adapter/engine roles above. Role subsets or
process equivalence assignments that violate this table remain explicit
unsupported classes.

Before RouteModel, a closed ownership projection separates stable evidence from
events. `Goal-plan = frozen_snapshot_unavailable\|frozen_digest_mismatch`, any node persistent/boot relation
`changed`, any edge identity `changed`, transport `replaced`, or any topology
transition facet `changed` with its exact disposition derives `transition_event` and is owned exclusively by
TransitionModel with its exact authorization/cause mapping. All Route output axes
are then `not_applicable`; fabricating terminal `unknown` or `unsupported` is
rejected. Every other structurally valid snapshot derives `route_snapshot` and
enters RouteModel. Thus the full common-domain assignment is still classified,
while RouteModel totality quantifies only stable snapshots and every change must
commit through an accepted TransitionBatch before a replacement snapshot can be
evaluated.

## RouteModel Domains

| Axis | Required values |
|---|---|
| Execution hop count | every integer from `0` through `8`, plus rejected `over_limit` |
| Connector sequence | every sequence from the connector-kind domain for that hop count |
| Mandatory topology/policy records | one engine-derived `structural_representability` hard record for every relevant node/edge role/process, lane/shell, policy, transport/protocol, identity, workspace, platform, and goal-plan gate |
| Terminal Goal prerequisite/fact records | complete frozen-plan set of `goal_prerequisite` or `observed_fact` records; each carries stable requirement equality class, relevance `hard`, `soft`, or `informational`, verdict `satisfied`, `missing`, `incompatible`, `unknown`, `denied`, `unreachable`, or `not_applicable`, observation status `complete`, `probe_failed`, `timed_out`, `truncated`, or `unsupported`, freshness `fresh`, `stale`, `invalidated`, or `fresh_required_but_unavailable`, observed evidence grade, frozen minimum required grade, and conflict state `none`, `resolved_by_grade`, `resolved_by_freshness`, or `equal_grade_fresh_conflict`. A public live prerequisite record may instead carry observation `in_progress`, but it belongs to the Transition/Operation frontier and is structurally excluded from a terminal RouteModel assignment. For these two record kinds, `not_applicable` is accepted only for an informational or conditional requirement proved irrelevant on this target. |
| Fact freshness clock proof | per terminal fact/attestation exactly one of `exact_owner_monotonic_observation_interval`, `exact_same_boot_cache_continuity`, `exact_authenticated_cross_boot_continuity_with_bounded_uncertainty`, `continuity_unproved`, `missing`, `mismatch`, `replayed`, or `not_applicable`; exact proofs bind campaign-owner boot/clock identity, dispatch lower bound, owner receive/commit upper bound, sampled revision, and cache provenance. Target/guest wall time is provenance-only and never an ordering or age input |
| Freshness evaluation receipt | `exact_owner_cut_and_frozen_max_age`, `missing`, `mismatch`, `replayed`, `not_applicable`; the exact receipt binds one campaign-owner monotonic evaluation cut, boot/clock identity, frozen per-fact max-age rule, and observation interval revision |
| Fact source-set partition | per record `zero`, `one`, `many_within_admitted_bound`, or rejected `over_limit`; source identities use only finite `same`/`different` equality classes in the symbolic model |
| Evidence/source witness | `complete_nonzero_observation`, `complete_authoritative_absence_search`, `complete_policy_receipt`, `complete_integrity_failure_receipt`, `complete_bounded_attempt_receipt`, `complete_frozen_plan_irrelevance_receipt`, `incomplete`, `zero`, `mismatch`, `not_applicable` |
| Requirement applicability proof | `applicable`, `static_informational_plan`, `conditional_irrelevance_proved`, `unproved`, `not_applicable` |
| Observed evidence grade | `attested_authoritative`, `local_authoritative`, `corroborated`, `single_source`, `none`, `not_applicable` |
| Frozen minimum evidence grade | `attested_authoritative`, `local_authoritative`, `corroborated`, `single_source`, `not_applicable` |
| Requirement/fact count partition | `zero`, `one`, `many_within_admitted_bound`, rejected `over_limit`; hard `zero` is valid only when the goal declares no hard prerequisite beyond mandatory topology records |
| Hard-requirement aggregate | derived exactly once as `all_satisfied`, `missing`, `incompatible`, `unknown`, `denied`, `unreachable`, or `not_applicable` |
| Soft-requirement aggregate | derived exactly once as `all_satisfied`, `non_satisfied`, `unknown`, or `not_applicable` |
| Hard observation status | derived exactly once from hard records as `complete`, `probe_failed`, `timed_out`, `truncated`, or `unsupported` |
| Hard freshness | derived exactly once from hard records as `fresh`, `stale`, `invalidated`, or `fresh_required_but_unavailable` |
| Beachhead proof state | `current`, `retired`, `expired`, `evidence_stale`, `validation_failed`, `validation_unavailable`, `ancestor_invalidated`, `policy_changed`, `engine_boot_changed`, `target_persistent_identity_changed`, `target_persistent_identity_mismatch`, `target_boot_changed`, `target_boot_identity_mismatch`, `workspace_changed`, `workspace_mismatch`, `cwd_changed`, `connector_changed`, `connector_replay_detected`, `connector_unapproved_rotation`, `connector_clone_detected`, `connector_binding_mismatch`, `connector_mid_run_replacement`, `action_audit_mismatch`, `action_audit_replay`, `requirement_mismatch`, `route_cycle_detected`, `catalogue_trust_changed`, `action_schema_or_digest_changed`, `outside_substitution`, `missing` |
| Safe-route evidence grade | `attested_authoritative`, `local_authoritative`, `corroborated`, `single_source`, `not_applicable` |
| Safe-route comparator completeness | `complete`, `partial`, `not_applicable` |
| Safe-route freshness anchor | required campaign-owner monotonic observation interval for safe routes and `not_applicable` otherwise; the symbolic domain uses pairwise `newer`, `equal`, `older`, and `overlap_incomparable` classes. No target/guest wall timestamp is an anchor |
| Frozen operator preference | `preferred`, `neutral`, `disfavored`, `not_applicable` |
| Stable route identity | canonical identity is required on every Route record; the symbolic domain uses only pairwise `same`, `lex_lt`, `lex_gt` classes |
| Terminal branch prior state | exact accepted from-state of the creating transition |
| Terminal branch state | `ready`, `denied`, `excluded`, `exhausted`, `truncated` |
| Terminal branch cause | `route_proved`, `policy_denial`, `connector_replay_detected`, `connector_unapproved_rotation`, `connector_clone_detected`, `connector_binding_mismatch`, `connector_mid_run_replacement`, `action_audit_mismatch`, `action_audit_replay`, `target_persistent_identity_mismatch`, `target_boot_mismatch`, `workspace_mismatch`, `architecture_or_requirement_mismatch`, `required_prerequisite_missing`, `route_cycle_detected`, `no_representable_candidate`, `authoritative_unreachable`, `evidence_unavailable`, `bound_reached` |
| Route outcome | derived exactly once as `unsupported`, `denied`, `unreachable`, `blocked`, `unknown`, `ready_with_warnings`, or `ready` |

None of the aggregate, observation, freshness, terminal-event, terminal-cause, or
Route-outcome fields is caller-supplied. Mandatory topology records are projected
from every Common/Node/Edge assignment before goal prerequisites are aggregated:

Each frozen goal-plan version supplies a finite relevance map for every Common
axis and declared predicate: `hard`, `soft`, or `informational`. Structural
delivery/process, policy, route transport, identity, and workspace-safety gates
are always hard and cannot be downgraded by a plan. Informational values project
to `not_applicable`; hard/soft values use the exact map below. A plan missing a
relevance entry is invalid, not a wildcard.

Record-kind and verdict compatibility is closed before aggregation:

| Record kind / verdict | Exact accepted observation and source projection |
|---|---|
| `structural_representability / not_applicable` | applicability proof `not_applicable`; relevance hard; observation `unsupported`; source set nonzero; witness `complete_nonzero_observation`; freshness `fresh`; this tuple alone represents an unrepresentable mandatory gate |
| `structural_representability / satisfied` or non-audit `incompatible` | applicability proof `not_applicable`; observation `complete`; source set nonzero; witness `complete_nonzero_observation`; freshness `fresh`; observed grade meets the frozen minimum when the gate requires one |
| `structural_representability / incompatible` from action-audit integrity | applicability proof `not_applicable`; observation `complete`; source set nonzero; witness `complete_integrity_failure_receipt` bound to exactly `action_audit_mismatch\|action_audit_replay`; freshness `fresh`; no policy receipt may substitute |
| `structural_representability / missing` | applicability proof `not_applicable`; observation `complete`; source set nonzero; witness `complete_authoritative_absence_search`; freshness `fresh`; observed grade meets the frozen minimum |
| `structural_representability / unknown` | applicability proof `not_applicable`; incomplete/zero/mismatched witness, unavailable or insufficient grade, non-fresh evidence, or observation `probe_failed\|timed_out\|truncated`; exact typed reason retained |
| `goal_prerequisite\|observed_fact / satisfied\|incompatible` | applicability proof `applicable`; observation `complete`; source set nonzero; witness `complete_nonzero_observation`; freshness `fresh`; observed grade meets the frozen minimum |
| `goal_prerequisite\|observed_fact / missing` | applicability proof `applicable`; observation `complete`; source set nonzero because the authoritative absence-search receipt is itself a source; witness `complete_authoritative_absence_search`; freshness `fresh`; observed grade meets the frozen minimum |
| `structural_representability / denied` | applicability proof `not_applicable`; observation `complete`; source set nonzero; witness exactly `complete_policy_receipt`; freshness `fresh` |
| `structural_representability / unreachable` | applicability proof `not_applicable`; observation `complete`; source set nonzero; witness exactly `complete_bounded_attempt_receipt`; freshness `fresh` |
| `goal_prerequisite\|observed_fact / denied` | applicability proof `applicable`; observation `complete`; source set nonzero; witness exactly `complete_policy_receipt`; freshness `fresh`; observed grade meets minimum when declared |
| `goal_prerequisite\|observed_fact / unreachable` | applicability proof `applicable`; observation `complete`; source set nonzero; witness exactly `complete_bounded_attempt_receipt`; freshness `fresh`; observed grade meets minimum when declared |
| `goal_prerequisite\|observed_fact / unknown` | applicability proof `applicable` with evidence failure or `unproved`; incomplete/zero/mismatched witness, insufficient grade, freshness ambiguity, or observation `probe_failed\|timed_out\|truncated\|unsupported`; the exact typed reason is retained |
| `goal_prerequisite\|observed_fact / not_applicable` | static informational: source set nonzero, applicability `static_informational_plan`, witness `complete_frozen_plan_irrelevance_receipt`, freshness `fresh`, grades `not_applicable`; conditional: source set nonzero, applicability `conditional_irrelevance_proved`, witness `complete_nonzero_observation`, freshness `fresh`, observed grade meets minimum |

Every other combination is a rejected checker constraint. In particular,
`missing\|satisfied\|incompatible` can never accompany failed, timed-out,
truncated, unsupported, zero-source, or incomplete evidence. A hard safe result
can never be supported by source set `zero`; an absence-search receipt counts as
one authoritative source rather than pretending that no evidence exists.
Applicability is also exact: every structural record uses proof
`not_applicable`; definitive non-NA goal/fact verdicts use `applicable`; unknown
uses either `applicable` with an evidence failure or `unproved`; verdict
`not_applicable` uses only the two proved static/conditional rows above. Every
other applicability/verdict pairing rejects.

| Upstream gate | Exact projection |
|---|---|
| invalid role/process/endpoint/direction relation, unknown connector, invalid/unsupported lane schema, unsupported OS/ABI/platform, malformed or unsupported protocol, or missing/invalid goal plan | hard `not_applicable` with observation `unsupported`, leading to `no_representable_candidate` |
| governing decision `denied` or shell `policy_denied` | hard `denied` with complete policy evidence |
| governing authority/decision `unavailable` | hard `unknown` with `fresh_required_but_unavailable` |
| transport `not_configured`, unauthorized `stopped`, `name_failure`, `refused`, or terminal `timed_out` | hard `unreachable`; once bounded attempts end, the synthesized reachability gate observation is `complete` while retaining per-attempt timeout reasons; `connecting` before its bound is hard `unknown`; `reachable` is `satisfied`; `protocol_skew` is unsupported; `replaced` is Transition-only and cannot enter this projection |
| identity `mismatch` | hard `incompatible` with the exact identity exclusion cause; `unverified` or `unavailable` is hard `unknown` until attestation resolves it; `changed` is Transition-only; verified/not-applicable identity is satisfied only where the structural rules permit |
| workspace mismatch/contradictory copy | hard `incompatible`; unreachable mapping is hard `unreachable`; ambiguous, stale, conflicting, unavailable, or unknown mapping is hard `unknown`; authoritative verified mapping is satisfied |
| shell `interpreter_denied` or edition/version mismatch | hard `incompatible`; shell `unknown` is hard `unknown`; an allowed or structurally unnecessary shell is satisfied |
| every declared architecture, capability, requirement, and prerequisite predicate | compare the observed typed value with the frozen predicate: match -> `satisfied`, proved mismatch -> `incompatible` or `missing` as declared by the predicate, unavailable/ambiguous -> `unknown`; no omitted gate defaults to satisfied |

Every Common/Node/Edge value is routed through this finite axis map before the
projection above:

| Axis | Exact required-value mapping |
|---|---|
| Harness + version | known harness with `known_compatible` -> satisfied; `known_incompatible` -> unsupported; `newer_unknown` or `missing` version -> unknown; unknown harness is satisfied only through validated generic MCP/embed schema, otherwise unknown |
| Delivery + product surface | combinations in the delivery/role compatibility table -> satisfied; every other combination -> unsupported |
| Harness context | `adapter_observed` -> satisfied; non-conflicting source-labelled `mixed` -> satisfied at its weakest evidence grade; `caller_asserted`, `absent`, or `conflicting` -> unknown for a required fact |
| Root capability | a selected `one_fresh_root` or identity-resolved member of `several_roots` -> satisfied; unresolved several roots or `stale_root` -> unknown; `no_roots` -> missing when required; `unsupported` -> unsupported |
| Operating system | `windows`, `linux`, or `macos` satisfies an allowed plan value; a proved disallowed OS -> unsupported; `unknown` -> unknown |
| CPU/ABI relation | `native_match` or plan-approved `supported_emulation` -> satisfied; `incompatible` -> `architecture_or_requirement_mismatch`; `unknown` -> unknown. CPU/ABI mismatch is blocked, never platform-unsupported |
| Virtualization, container, sandbox facets | `unknown` -> unknown when relevant; otherwise exact plan predicate match -> satisfied and proved mismatch -> incompatible; informational facets -> not applicable |
| Automation + privilege facets | exact plan predicate match -> satisfied; `unknown` -> unknown when relevant; proved restricted/incompatible state -> incompatible; informational values -> not applicable |
| Connector kind + direction | known connector with the exact structural direction/role relation -> satisfied; `unknown` connector or impossible direction -> unsupported |
| Execution lane + schema | required lane present with `exact_valid` -> satisfied; `none` -> missing when execution is required; `placeholder_only` -> unknown; `invalid_action_shape` -> incompatible; `unsupported_by_peer` -> unsupported |
| Windows command host + shell | non-Windows/not-required `not_applicable` -> satisfied; supported known host plus `allowed`/`not_required` shell -> satisfied; unknown host/shell -> unknown; policy denial -> denied; interpreter or edition/version mismatch -> incompatible |
| Workspace source + authority | explicit/session/client/policy/marker source with `authoritative` proof -> satisfied; `candidate`, `stale`, `conflicting`, `unavailable`, `none`, or `ambiguous` -> unknown, except proved required `none` -> missing |
| Workspace mapping + identity | same-path/translated verified + identity verified -> satisfied; reachable identity mismatch or identity mismatch -> workspace mismatch/incompatible; unreachable -> unreachable; unknown -> unknown; identity not applicable is valid only for a plan-declared workspace-free route |
| Goal family + plan state | declared family with `one_valid`, conflict-free `several_composed`, or `registry_head_changed_after_admission` while the exact admitted snapshot remains available and digest-valid -> satisfied; `unknown` -> unknown; `conflict`, `missing`, or `invalid` -> unsupported at admission; `frozen_snapshot_unavailable\|frozen_digest_mismatch` are Transition-only campaign failures and never enter a stable Route snapshot. A registry activation-head change affects future admissions only |
| Node roles/process equivalence | exact role/process compatibility -> satisfied; structural mismatch -> unsupported; unknown process relation -> unknown |
| Node persistent/boot identity relation | fresh non-cyclic `new` relation -> satisfied; repeated equality class -> route-cycle exclusion; `changed` is Transition-only with its disposition-owned cause; stable mismatch -> exact hard exclusion; unknown -> unknown |
| Edge endpoints/target reference | contiguous source/target + `reference_to_target_node` -> satisfied; contradictory copy -> workspace mismatch; unavailable reference -> unknown; any non-contiguous endpoint relation -> unsupported |
| Edge governing authority/decision | all required node decisions allowed -> satisfied; any denied -> denied; any unavailable -> unknown |
| Edge transport + protocol | reachable + exact/compatible -> satisfied; terminal transport failures -> unreachable/complete; connecting -> unknown; protocol skew, peer too old, or malformed -> unsupported; peer newer unknown or protocol unavailable on reachable transport -> unknown; `replaced` is Transition-only; contradictory transport/protocol pairs are rejected constraints |
| Edge identity verdict | structurally/pinned verified or structurally valid not-applicable -> satisfied; mismatch -> the exact required `Identity mismatch cause` hard exclusion; unverified/unavailable -> unknown; `changed` is Transition-only; a pin mismatch maps to `target_persistent_identity_mismatch` |

Freshness is anchored only by the campaign owner. A live local, remote, guest, or
connector observation carries the owner-monotonic interval from dispatch through
receive/atomic fact commit; a target clock value remains typed provenance. A
same-boot cached fact is reusable only with exact owner-clock continuity. A
cross-boot fact may preserve freshness only with an independently authenticated
old-to-new clock continuity receipt and bounded uncertainty; approval or re-anchor
alone creates no time evidence. When continuity cannot be proved, it is `stale` or
`fresh_required_but_unavailable`, never fresh. Clock rollback/jump, boot mismatch,
missing bounds, replay, or mixed owner-clock domains cannot be normalized. Two
fresh intervals are ordered only when one lies strictly after the other, equal
only when their exact bounds are identical, and otherwise
`overlap_incomparable`. At an exact evaluation cut `T`, observation interval
`[dispatch_lower, receive_commit_upper]` yields age interval
`[T - receive_commit_upper, T - dispatch_lower]`. It is `fresh` only when the
worst-case age is within the frozen limit, `stale` only when the best-case age is
past it, and `fresh_required_but_unavailable` when the interval straddles the
limit. Missing/mismatched/replayed evaluation evidence derives unknown. No caller
or implementation may choose the younger endpoint.

Fact conflict resolution is exact before aggregation: higher evidence grade wins;
at equal grade a strictly fresher owner-clock interval wins; contradictory
equal-grade facts with identical or overlap-incomparable freshness remain an
`unknown` record with `equal_grade_fresh_conflict`; superseded facts and all of
their derived facts are removed transitively. An otherwise satisfied record
whose observed grade is below its frozen minimum is rewritten to `unknown`;
identity/transport gates that require attestation cannot be satisfied by
single-source or caller-asserted evidence. Aggregate priority is closed. Hard
records derive `denied`, then `unreachable`, then `missing`, then `incompatible`,
then `unknown`, then `all_satisfied`; an entirely inapplicable hard goal derives
`not_applicable`. Soft records derive `unknown`, then `non_satisfied`, then
`all_satisfied`, or `not_applicable`. Hard observation derives `unsupported`,
then `probe_failed`, `timed_out`, `truncated`, then `complete`; hard freshness
derives `invalidated`, then `fresh_required_but_unavailable`, `stale`, then
`fresh`. Soft observation/freshness failures affect only the soft aggregate and
safe-route comparator completeness; they never downgrade a hard-safe Route to
Route `unknown`.
Every source record is consumed exactly once; a projection disagreement is a
rejected checker constraint.

For an initial terminalization, the joint terminal winner consumes all
independently derived axes before a branch event is selected. Its closed order
is: (1) any hard representability or
hard-observation `unsupported`; (2) hard aggregate `denied`; (3) hard aggregate
`unreachable`; (4) hard aggregate `missing\|incompatible`; (5) hard aggregate
`unknown`, any remaining hard observation `probe_failed\|timed_out\|truncated`, any
hard freshness other than `fresh`, or a required non-current proof; (6) hard-safe
with soft warning/uncertainty; (7) fully safe. Lower-priority hard and all soft
records remain in provenance but cannot override the winner. This winner is
derived and never supplied.

A candidate later `terminal_branch_commit/hard_excluded` from exact prior
terminal state `denied\|exhausted\|truncated` uses one closed temporal
composition. The accepted Transition batch first applies the exact per-record
effects required by its cause. A `retain` effect MUST match the current
pre-batch record's stable identity and canonical evidence digest. An accepted
authoritative-context or fact change invalidates exactly its dependent members,
retains every proved-independent member, and recomputes the complete dependent
closure; a cause with no such change cannot invalidate, demote, or delete an
unchanged source/topology record. Missing, extra, duplicate, stale-revision,
hybrid-revision, or unauthorized `add` effects reject the batch.

After those effects, the checker derives the complete current post-batch
source/topology and derived-record closure from only unchanged or explicitly
retained current sources and any exact accepted authoritative-context
replacement when applicable, then runs the
unchanged ordinary winner algebra above over that closure. The resulting current
winner alone selects replacement Route outcome `unsupported`, `denied`, or
`unreachable`; when no such current winner exists, the exact hard-exclusion
record selects `blocked`. A newly recomputed derived record is eligible only
when its exact dependency root/count and the same batch's `fresh_unique`
transaction identity bind it into that exact closure; the temporal composition
itself grants no record admission authority.
Independently, the candidate batch owns lifecycle state `excluded`, its exact
accepted hard-exclusion cause, and event `hard_excluded`. The superseded Route is
provenance-only before closure/winner derivation. This composition can never
derive a safe outcome, reuse stale/invalidated provenance as current, or
authorize work.

The derived initial terminal transition is likewise closed: representability failure ->
`sensor_exhausted/no_representable_candidate`; hard denial ->
`policy_denied/policy_decision`; hard missing ->
`hard_excluded/required_prerequisite_missing`; hard incompatibility ->
`hard_excluded` with the exact typed mismatch/cycle/security cause; hard
unreachable -> `sensor_exhausted/authoritative_unreachable`; terminal unknown
with an exact branch- or campaign-decisive bound receipt selected by the closed
bound-effect composition and bound to the creating Transition ->
`bound_reached/bound_reached`; every other bounded terminal
unknown -> `sensor_exhausted/evidence_unavailable`; and only all hard gates plus
complete current proof -> `route_proved/none`. No Common/Node/Edge assignment can
be paired with a different initial terminal event or bridge projection. The sole
later-terminal exception is the exact hard-exclusion temporal composition above;
its batch receipt supplies `hard_excluded` and the accepted cause while the
current winner supplies only the replacement Route outcome.
Missing, mismatched, replayed, stale, or cross-Transition bound receipts cannot
select `bound_reached`; they retain their exact ordinary unknown/rejection class.

Every branch that reaches a terminal evidence state feeds exactly one
`RouteModel` record carrying that exact terminal state and cause. The bridge
constraints below reject any state/cause/evidence/outcome combination that was
not produced by the corresponding branch transition.
Nonterminal `discovered`/`gated`/`scouting`/`attesting`/`viable`/`deepening`
branches feed `frontier_effect` instead. A cancelled branch retains a bounded
cancellation reason but has no fabricated route verdict. `over_limit` is a
rejected ninth frontier; the ninth hop is never attempted.

Safe-route comparator aggregates are deterministic. Evidence grade is the
weakest grade among every required hard fact, identity attestation, and proof
component, using the order declared in GoalModel. Comparator completeness is
`complete` only when all hard and applicable soft comparator inputs are complete;
otherwise it is `partial`. From the complete finite required-fact/attestation set,
the route freshness interval is the associative conservative-oldest envelope
`[min_i(observation_lower_i), min_i(observation_upper_i)]` in the campaign-owner
clock domain. Internal component-interval overlap does not change comparator
completeness; it is evidence coverage, not a fabricated time order. Two derived
route intervals may themselves be overlap-incomparable, in which case the staged
maximal-set comparator below retains both until later deterministic dimensions. Operator
preference comes only from the run's frozen plan mapping, defaulting to `neutral`.
Stable route identity is the canonical digest of connector sequence, persistent
target identity, workspace identity, and route schema; unequal content with the
same identity is rejected. Safe outcomes require non-`not_applicable` comparator
fields; non-safe outcomes use `not_applicable` fields but retain stable identity.

## GoalModel Domains

### Goal-plan administration submodel

Goal-plan administration is a finite, independently total pre-admission
submodel for FR-043, FR-045, and FR-092. It owns only declarative plan
records. It never creates a campaign, invokes a sensor, changes an admitted
run, or enters the sifter-rule registry.

| Administration axis | Required values |
|---|---|
| Action | `add`, `store`, `search`, `validate`, `test`, `activate`, `audit` |
| Authority | `operator_plan_admin`, `authorized_read_only`, `caller_or_llm_only`, `sifter_rule_authority`, `unauthorized`, `unknown` |
| Plan structural input | `exact_new_declarative`, `exact_stored_identity_and_version`, `malformed`, `unsupported_schema_version`, `not_applicable` |
| Forbidden-content classification | `exact_none`, `exact_nonempty_set`, `unproved`, `not_applicable`; the nonempty set is complete over the disjoint syntax classes `command_invocation`, `script_body`, `shell_text`, `executable_path_or_locator`, `credential_or_secret_material`, `environment_value_material`, `permission_change`, `authority_grant`, and `side_effect_or_runtime_behavior` |
| Version relation | `new_identity_and_version`, `new_version_of_exact_identity`, `exact_existing`, `version_conflict`, `missing`, `not_applicable` |
| Dedicated-registry proof | `exact_separate_types_tables_migrations_authority_audit_and_operations`, `sifter_type_alias`, `sifter_storage_alias`, `sifter_identity_or_lifecycle_alias`, `unproved` |
| Registry service | `available`, `read_unavailable`, `write_unavailable` |
| Validation/test result | `passed`, `failed`, `unavailable`, `not_run`, `not_applicable` |
| Registry transaction | `committed`, `rolled_back`, `failed`, `unavailable`, `not_attempted`, `not_applicable` |
| Audit append | `committed`, `failed`, `unavailable`, `not_attempted`, `not_applicable` |
| Activation visibility | `future_runs_only`, `attempted_current_admitted_run_mutation`, `no_activation`, `not_applicable` |

`add` creates one new inactive operator-local plan identity and immutable first
version. `store` appends one immutable version to an exact operator-local
identity using an expected revision; it never overwrites a version. `search`,
`validate`, and `test` are bounded read-only operations. `test` proves schema,
sensor references, comparator references, composition, and conflict behavior
without running a sensor. `activate` changes one activation head atomically.
`audit` is a bounded read of the dedicated plan audit trail and does not
recursively audit itself.

Action shape is exact. `add\|store` accept any document-bearing structural input
(`exact_new_declarative\|malformed\|unsupported_schema_version`), registry service
`available\|write_unavailable`, and activation visibility `no_activation`.
Their success shape alone requires `exact_new_declarative`, the corresponding
new-version relation, forbidden-content classification `exact_none`, validation
`passed`, and a write transaction; malformed/unsupported input pairs with
forbidden classification `not_applicable`, while a parseable new document may
carry `exact_none\|exact_nonempty_set\|unproved`. An invalid
document pairs with version `not_applicable`, validation `not_run`, and
transaction `not_attempted`. `validate\|test` accept any document-bearing input
or exact stored version; a new input requires a non-`not_applicable` forbidden
classification and a stored version requires `not_applicable`. Malformed,
unsupported, forbidden-nonempty, or unproved input uses validation/test
`not_run`; an exact-none new input or exact stored version requires
`passed\|failed\|unavailable`. They require registry service
`available\|read_unavailable`, registry transaction `not_applicable`, and
activation visibility `not_applicable`. `activate` requires an exact stored
identity/version reference, version relation `exact_existing|version_conflict|
missing`, validation `not_applicable`, service
`available\|write_unavailable`, and activation visibility
`future_runs_only\|attempted_current_admitted_run_mutation`, with forbidden
classification `not_applicable`. `search\|audit`
require structural input, forbidden classification, version relation,
validation/test, registry transaction, and
activation visibility all `not_applicable` except that service is
`available\|read_unavailable`. Every other action/neutral-field pair is an
explicit `unsupported_constraint`, not an ignored field.

The bounded validator derives the forbidden set before persistence from a
versioned field grammar. Each field or syntax location has exactly one class, so
a plan containing several forbidden forms yields one complete nonempty set
without precedence ambiguity. Validation is private and bounded. No plan body,
credential, environment value, permission, authority grant, command, executable
path, or script text may enter diagnostics, registry state, or audit. A rejection
may append only the action digest, schema revision, and closed typed cause set
after the ordinary audit-authority check; it grants no authority and records no
caller content.

The checker applies the following ordered, disjoint classification to every
structurally valid assignment:

1. Any sifter alias proof derives `registry_alias_violation`; `unproved` derives
   `administration_unknown`. Both have no plan-state effect. The exact
   dedicated-registry proof is required for every success and ordinary failure.
2. `add\|store\|activate` require `operator_plan_admin`.
   `search\|validate\|test` accept operator administration or
   `authorized_read_only`. `audit` requires operator administration.
   `caller_or_llm_only\|sifter_rule_authority\|unauthorized` derive `denied`;
   `unknown` derives `administration_unknown`. Thus an LLM may request
   read-only plan operations through a separately authorized read capability,
   but can never gain mutation authority from either registry.
3. `malformed` or `exact_nonempty_set` forbidden content derives `invalid`;
   `unproved` forbidden classification derives `administration_unknown`;
   `unsupported_schema_version` derives `unsupported`. These outcomes occur
   before plan persistence and expose only the closed typed cause set. They
   require audit append `committed`; `failed\|unavailable` derives `audit_failed`
   with no persistence, and `not_attempted\|not_applicable` is a structural
   rejection. A required
   read/write service that is unavailable then derives `unavailable`;
   `version_conflict` derives `version_conflict`; and `missing` derives
   `not_found`. None mutates state.
4. Validation/test `failed` derives `invalid` and `unavailable` derives
   `unavailable`. A required result of `not_run\|not_applicable` is a structural
   rejection.
5. For `add\|store\|activate`, registry `failed` with audit `committed` derives
   `commit_failed`; registry `unavailable` derives `unavailable`. For every
   non-`audit` action, audit `failed\|unavailable` derives `audit_failed` with no
   visible mutation: a mutating action requires registry `rolled_back`, while a
   read-only action retains transaction `not_applicable`. Registry `committed`
   with any audit value other than `committed` is an invariant violation, never
   a successful response. `rolled_back` with committed audit, or `not_attempted`
   without an earlier classified rejection, is a structural rejection.
6. Successful `add\|store` require registry and audit both `committed` and derive
   `added_inactive\|stored_inactive_version`.
   Successful `activate` requires `future_runs_only` and derives
   `activated_for_future_runs` only with registry and audit both `committed`.
   `attempted_current_admitted_run_mutation` is
   rejected before commit. Existing admitted runs retain their frozen versions
   and digests. Successful `search\|validate\|test` require a committed audit
   append and derive their matching read-only result; `audit` requires audit
   append `not_applicable` and derives `audit_result`.
7. Every remaining assignment derives `unsupported_constraint`. No transaction,
   audit, validation, or neutral-field tuple falls through.

Clauses 1 through 4 require registry transaction `not_attempted\|unavailable`
and a non-mutating activation value; a committed or rolled-back plan mutation
in any such row is `unsupported_constraint` rather than a concealed side
effect.

Plan mutation and its audit append therefore publish atomically or not at all.
All error rows preserve the previous activation head and immutable versions.
Plan identity is typed and namespace-separated; textual equality with a sifter
rule or pack name has no lookup, precedence, authority, or lifecycle effect.

### Plan selection and prerequisite submodel

PlanModel is a finite admission-time submodel for FR-044, FR-046, FR-047,
FR-049, FR-050, and FR-051. Its selected versions, digests, sensor catalogue
revision, comparator catalogue plus conformance-corpus/oracle revisions, and
manifest-evidence set are frozen
before campaign admission.

| PlanModel axis | Required values |
|---|---|
| Plan-family member domain | `python`, `rust`, `node`, `go`, `dotnet`, `java`, `c_cpp_build`, `container`, `git`, `github`, `typed_custom_requirements`, `operator_local_composite`; this is a domain for set members, never a standalone scalar axis; an empty set has no member row |
| Selection mode | `automatic`, `explicit_plan_identity`, `typed_custom_requirements_only` |
| Selection-source record set | one complete bounded record for every applicable manifest candidate or explicit typed selection input, each carrying family member, typed source identity, and state `exact_builtin`, `exact_operator_local_active_identity`, `exact_typed_custom_requirements`, `missing`, `malformed`, `untrusted`, or `inactive`; empty is valid only for automatic exact-no-relevant-manifest selection |
| Workspace-manifest evidence | `exact_no_relevant_manifest`, `exact_one_family`, `exact_multi_family_compatible`, `conflicting`, `stale`, `unavailable`, `unproved`, `not_applicable` |
| Manifest/workspace binding | `exact_canonical_workspace`, `mismatch`, `unproved`, `not_applicable` |
| Manifest-family candidate-set proof | `exact_empty_set`, `exact_single_set`, `exact_bounded_set`, `over_limit`, `missing_member`, `extra_member`, `duplicate_member`, `root_or_count_mismatch`, `unproved`, `not_applicable`; exact forms bind the canonical sorted unique family set, count/root, complete manifest evidence, workspace, and plan-catalogue revision |
| Selected typed-plan record set | a complete bounded set of typed plan identity, immutable version, digest, source, and shipped-family member records; empty is valid before selection |
| Selection-input-to-selected set proof | `exact_empty_automatic_set`, `exact_automatic_single_bijection`, `exact_explicit_single_identity`, `exact_typed_custom_single_identity`, `exact_duplicate_collapse_map`, `exact_compatible_merge_map`, `exact_conflict_resolution_map`, `missing_candidate`, `extra_selected`, `duplicate_selected`, `identity_version_digest_or_family_mismatch`, `root_or_count_mismatch`, `unproved`, `not_applicable`; exact forms bind equality among the applicable manifest-candidate or explicit typed input set, the complete selection-source record set, selected count/root, and every mapping edge |
| Selected-plan cardinality | engine-derived from the selected record set as `zero`, `one`, `many_within_bound`, or rejected `over_limit` |
| Composition proof | `single`, `exact_duplicate_collapse`, `explicit_compatible_merge`, `explicit_conflict_resolution`, `unresolved_conflict`, `unproved`, `not_applicable` |
| Frozen plan snapshot | `exact_selected_set_versions_digests_count_and_root`, `activation_head_changed_after_admission_with_exact_selected_set_snapshot`, `missing`, `mismatch`, `not_applicable` |
| Comparator ecosystem | `python`, `rust`, `node`, `go`, `dotnet`, `java`, `c_cpp`, `container`, `git`, `github`, `name_presence_only` |
| Comparator evidence source | `exact_catalogue_bound_ecosystem_semantics`, `exact_target_metadata_bound_to_catalogue`, `generic_string_or_lexical`, `missing`, `unproved`, `not_applicable` |
| Comparator catalogue and conformance-corpus proof | `exact_trusted_frozen_catalogue_and_complete_corpus_revision`, `head_changed_after_admission_with_exact_frozen_catalogue_and_corpus_revision`, `missing_required_case_key`, `observed_or_oracle_mismatch`, `missing`, `mismatch`, `untrusted`, `not_applicable` |
| Derived comparison | `satisfied`, `missing`, `incompatible`, `unknown`, `not_applicable` |
| Python observation method | `distribution_metadata_only`, `import_attempted`, `package_code_execution_attempted`, `unproved`, `not_applicable` |

Every built-in family above is a required shipped identity, not a heuristic
label. Automatic selection derives candidates only from fresh manifest and
toolchain evidence bound to the exact canonical workspace. Explicit selection
requires the typed plan identity; operator-local precedence never follows
name shadowing. Typed custom requirements add no executable behavior.

Source shape is exact. `automatic` accepts built-in plans and requires a
non-`not_applicable` manifest observation and workspace binding plus the exact
candidate-set proof matching that observation: empty for no relevant manifest,
single for one family, and a bounded exact set for compatible or conflicting
multi-family evidence.
`explicit_plan_identity` accepts an exact built-in or active operator-local
identity and may carry exact manifest specialization or both manifest fields
`not_applicable`; absent manifest specialization requires candidate-set proof
`not_applicable`. `typed_custom_requirements_only` requires the matching typed
source and all manifest fields/proof `not_applicable`. Cardinality is recomputed
from the selected record set. Zero is valid only for automatic selection and
requires `exact_empty_automatic_set` plus composition `not_applicable`; one
requires the mode-matched `exact_automatic_single_bijection`,
`exact_explicit_single_identity`, or `exact_typed_custom_single_identity` plus
`single`; bounded many requires the operation-matched
exact map and matching composition proof. Every candidate appears exactly once
in the map unless an explicit duplicate-collapse equivalence class names all of
its members; every selected record has exactly one justified map image. Every
other source/mode/manifest/set/cardinality/composition pairing is
`unsupported_constraint`.

The selection function is ordered and total. Structurally inconsistent
not-applicable fields are `unsupported_constraint`. A malformed or untrusted
source record is `invalid`. Manifest/workspace mismatch is `conflict`; conflicting
manifest evidence or unresolved composition is `conflict`. Missing/inactive
source record, automatic selection with exact no-relevant-manifest evidence, or
selected cardinality `zero` is `missing`. Stale, unavailable, or unproved
manifest evidence, unproved workspace binding/composition, or a missing frozen
snapshot is `unknown`. Cardinality `over_limit` or frozen digest `mismatch` is
`invalid`. Candidate-set `over_limit`, any missing/extra/duplicate/mismatched
candidate or selected member, count/root mismatch, or map mismatch is `invalid`;
an unproved set or map is `unknown`. Exact one-plan selection derives
`one_valid` only with the exact mode-matched single selection proof and a frozen snapshot whose
selected identity/version/digest/family set and count/root are identical. Exact bounded
multi-plan selection requires duplicate collapse or an explicit compatible/
conflict-resolved merge, the corresponding complete map, and the same exact
selected-set frozen snapshot, and then derives `several_composed`. An activation-head
change after admission derives `registry_head_changed_after_admission` only
when that exact frozen selected-set snapshot remains available; it never
reselects the run.
Every remaining selection assignment is `unsupported_constraint`; none falls
through.

Comparator classification is independently total per prerequisite. An exact
catalogue proof is authorizing only when it binds the complete mandatory
ecosystem conformance-corpus key set and frozen corpus revision below. Each case
records required and observed parse class (`valid_canonical`,
`valid_normalizable`, `malformed`, `ambiguous_or_unsupported`), required and
observed relation (`less`, `equal`, `greater`, `incomparable`, `rejected`), and
an `exact_independent_official_reference_fixture` oracle or one of `missing`,
`not_independent`, `untrusted`. Required case roles cover ordinary releases,
normalization aliases, prerelease/development/postrelease ordering,
epoch-or-ecosystem-equivalent ordering, local/build metadata semantics,
malformed input, and ambiguous/unsupported input; ecosystems that lack a role
must carry an exact `not_applicable_to_ecosystem` oracle fixture. The corpus is
versioned, canonically keyed, and complete over every comparator ecosystem.
Missing/duplicate keys, observed/required disagreement, a non-independent
oracle, or an untrusted fixture prevents the catalogue revision from becoming
trusted.

With no
version constraint, `name_presence_only + not_applicable` derives only presence
or absence. A versioned check may derive `satisfied\|missing\|incompatible` only
from an exact approved ecosystem comparator, exact target metadata where that
ecosystem requires it, and an exact trusted frozen catalogue plus complete
conformance-corpus revision. Missing,
unproved, mismatched, untrusted, or generic string/lexical comparison derives
`unknown` and cannot be laundered into an ordering. Missing required corpus keys,
oracle mismatch, or catalogue/corpus mismatch is also `unknown` and fails the
trusted-revision gate. A changed catalogue head does not affect an admitted run
when the exact frozen catalogue and corpus revisions remain bound.
For Python, only `distribution_metadata_only` may produce a comparison;
`import_attempted\|package_code_execution_attempted` is denied before execution,
and `unproved` derives `unknown`. Every non-Python prerequisite requires Python
method `not_applicable`. Comparator source, catalogue revision, and outcome are
engine-derived and recorded in every prerequisite receipt. The checker
recomputes the comparison and rejects any supplied or stored outcome that does
not equal that derivation.

### Terminal-route aggregation

Goal aggregation receives the information it actually quantifies; it never
refers to an undeclared "other route."

| Input record set | Required values |
|---|---|
| Current terminal-route records | the complete finite set of current Route records, each keyed by unique branch identity and carrying exact terminal state, terminal cause, Route outcome, and bridge evidence; empty is valid |
| Safe-candidate comparator records | derived, never caller-supplied, from terminal records whose outcome is `ready` or `ready_with_warnings`; each carries goal satisfaction, evidence grade, completeness, campaign-owner freshness observation interval, operator preference, and stable route identity |
| Open-branch role records | a complete record for every nonterminal branch, each exactly `required_for_safe_route`, `alternative_may_outrank`, or `proved_irrelevant_to_current_selection`; the projected presence vector covers every subset, including empty |
| Terminal/open record-count partitions | each independently `zero`, `one`, `many_within_admitted_bound`, or rejected `over_limit`; the admitted route/branch bounds are frozen finite run inputs |
| Terminal campaign evidence | `none`, `bound_before_route`; `bound_before_route` is engine-derived only from an exact accepted-or-running-campaign total-time bound with zero terminal Routes and zero open branches, and never manufactures a Route |

The comparator field domains and order are exact: goal satisfaction is
`clean > soft_warning`; evidence grade is `attested_authoritative >
local_authoritative > corroborated > single_source`; completeness is `complete >
partial`; freshness compares campaign-owner monotonic observation intervals after
both rows have passed the Route freshness gate; `overlap_incomparable` cannot be
rewritten to newer/equal/older and retains both rows in the freshness-maximal set
when earlier rank fields tie; operator preference is
`preferred > neutral > disfavored`; and stable route identity uses ascending
canonical bytes as the final tie-break after the freshness-maximal set is formed. The symbolic checker reduces each
pair to finite relations: `better`, `equal`, or `worse` for the enumerated rank
fields; `newer`, `equal`, `older`, or `overlap_incomparable` for freshness; and `same`, `lex_lt`, or
`lex_gt` for stable identity. Raw timestamps and identity bytes exist only in
concrete fixtures and are not symbolic axes. Scalar ranks and equal-freshness
stable-identity ordering are total, antisymmetric, and transitive. Freshness is a
partial interval order: strict newer/older is asymmetric and transitive, exact
equality is an equivalence, and overlap-incomparable is symmetric and need not be
transitive. The checker enforces those separate laws, admitted cardinality, and
every first-decisive dimension. Deterministic selection is total without asserting
a false pairwise time order: filter to the maximum goal-satisfaction, evidence-grade,
and completeness classes; remove every interval strictly older than another
survivor; retain all overlap-incomparable maximal intervals; then filter by
operator preference and stable identity. Duplicate stable
identity with unequal record content is rejected; byte-identical duplicates are
deduplicated before comparison.

The model derives, rather than accepts, all projections: the seven-value
route-outcome presence vector, duplicate multiplicity, safe-candidate set,
selected stable route identity, selected class (`clean`, `soft_warning`, or
`not_applicable`), first decisive dimension (`no_safe_candidate`,
`sole_safe_candidate`, `goal_satisfaction`, `evidence_grade`, `completeness`,
`freshness`, `operator_preference`, `stable_route_identity`, or
`ranking_incomplete`), frontier effect, and alternative-rank proof. Because goal
satisfaction is first, Route `ready` outranks Route `ready_with_warnings`; later
dimensions select identity only within the same terminal class. The staged-maxima
algorithm always produces one deterministic best-known candidate without claiming
that overlap-incomparable intervals are equal or ordered. Freshness is the first
decisive dimension only when strict interval dominance removes a survivor;
otherwise later preference/identity select within the exact maximal set and the
receipt records whether incomparability was present. A
selection is final only when the alternative-rank proof is `complete` or
`proved_cannot_outrank`; it is provisional for `open` and bounded best-known for
`bounded_incomplete`.

Alternative-rank proof is derived exactly after that projection. With no safe
Route it is `not_applicable`. With a safe Route, any
`alternative_may_outrank` open record yields `open`; otherwise any terminal
Route `unknown` yields `bounded_incomplete`; otherwise any still-open record
proved irrelevant yields `proved_cannot_outrank`; otherwise it is `complete`.
It is never a caller assertion. `ranking_incomplete` is the first decisive
dimension exactly when a safe best-known candidate exists and the rank proof is
`open` or `bounded_incomplete`; for the two final proof states the first unequal
lexicographic field remains decisive. Thus every declared decisive value is
reachable without inventing a total order. The model then derives exactly
one Goal outcome:
`in_progress`, `ready`, `ready_with_warnings`, `blocked`, `unknown`, `denied`,
`unreachable`, or `unsupported`.

Open-branch role records are exhaustive, not caller summaries. When no current
safe Route exists, every open branch MUST be `required_for_safe_route`; when a
current `ready` or `ready_with_warnings` Route exists, every open branch MUST be
`alternative_may_outrank` or `proved_irrelevant_to_current_selection`. The latter
requires bounded evidence that the branch cannot change safety, executability, or
the lexicographic winner. Any missing, duplicate, or oppositely classified open
branch is a rejected checker constraint. Frontier aggregation is exact: any
required record derives `safety_or_executability_open`; otherwise any
`alternative_may_outrank` record derives `rank_changing_open`; records proved
irrelevant contribute no frontier; otherwise the result is `none`.

The presence vector, multiplicity, selected identity/class, decisive dimension,
frontier, and rank proof MUST equal those derived from the complete record sets.
No independent projection input exists. Every missing record, inconsistent
projection, or unproved irrelevant role is a rejected checker constraint.
`bound_before_route` additionally requires the exact campaign-bound Transition
receipt and otherwise-empty record sets and derives Goal `unknown`; every other
record-set shape requires terminal campaign evidence `none`.

GoalModel classifies only the current projection of an active campaign or the
immutable terminal result retained by a completed campaign. Cancellation, engine
loss, and failure do not run empty record sets through GoalModel: their exact
Transition batch retires the current projection into bounded terminal-status
history, so Operation reports no terminal Goal. Retention expiry deletes a
completed terminal result and every heavy campaign-owned evidence/output artifact
into one fixed unavailable tombstone, including the zero-Route
`bound_before_route` case; cancelled/failed status is reduced the same way.
Neither tombstone is a GoalModel input. Retention purge later deletes the tombstone,
bounded key bindings, and remaining indexes atomically; tagged `absent` has no
Goal or lifecycle record.

## TransitionModel Domains

The transition checker accepts only an atomic `TransitionBatch`. Branch,
campaign, proof, Route-record, Goal-snapshot, cleanup, and Operation-trace changes
that share one cause are members of that batch; member projections are never
accepted independently. A one-member batch is permitted only for the simple
progress rows below. Tagged member states prevent one subject kind from being
mistaken for another.

| Input axis | Required values |
|---|---|
| Transition record kind | `atomic_batch`; every other or missing kind is rejected |
| Batch purpose | `branch_progress`, `queue_wait_registration`, `sensor_admission`, `sensor_observation_commit`, `soft_evidence_bound_commit`, `soft_evidence_bound_finalize`, `bound_effect_bundle_commit`, `route_proof_commit`, `terminal_branch_commit`, `recoverable_invalidation`, `hard_exclusion`, `proof_failure`, `irrelevant_branch_pruning`, `caller_cancellation`, `engine_loss`, `campaign_admission`, `request_key_binding`, `campaign_start`, `campaign_bound_stop`, `campaign_completion`, `campaign_failure`, `legacy_overlay_migration`, `retention_expiry`, `retention_lease_reanchor`, `retention_purge` |
| Member subject | `branch`, `campaign`, `queue_wait`, `sensor_invocation`, `request_key`, `capacity_ledger`, `deadline_registration`, `terminal_retention_eligibility`, `purge_eligibility`, `authoritative_context`, `frozen_plan_integrity`, `legacy_overlay_migration`, `action_audit`, `sensor_result`, `fact_admission`, `source_fact`, `derived_fact`, `pending_boundary_transaction`, `bound_effect_bundle`, `beachhead_proof`, `route_record`, `goal_snapshot`, `campaign_result_snapshot`, `retention_artifact_set`, `retention_audit`, `audit_history`, `campaign_summary_audit`, `cleanup`, `operation_trace` |
| Branch from-state | `absent`, `discovered`, `gated`, `scouting`, `attesting`, `viable`, `deepening`, `ready`, `denied`, `excluded`, `exhausted`, `truncated`, `cancelled`, `retracted`, `not_applicable` |
| Campaign from-state | `none`, `accepted`, `running`, `completed`, `cancelled`, `failed`, `expired`, `not_applicable` |
| Proof from-state | every Beachhead proof state in `RouteModel`, plus `not_applicable` |
| Event | `candidate_found`, `gates_started`, `scout_started`, `attestation_started`, `capacity_wait_registered`, `sensor_started`, `sensor_observation_admitted`, `sensor_result_recorded`, `sensor_private_input_stopped`, `soft_bound_finalized`, `bound_effect_bundle_committed`, `deepening_started`, `route_proved`, `policy_denied`, `hard_excluded`, `sensor_exhausted`, `bound_reached`, `branch_pruned`, `caller_cancelled`, `shutdown_or_engine_lost`, `hard_invalidation`, `campaign_admitted`, `request_key_bound`, `campaign_started`, `campaign_completed`, `campaign_failed`, `legacy_overlay_migration_committed`, `retention_expired`, `retention_lease_reanchored`, `retention_purged`, `proof_validation_failed`, `proof_validation_unavailable`, `proof_expired` |
| Invalidation, exclusion, or exhaustion cause | `none`, `ancestor`, `proved_irrelevant_to_final_goal`, `policy_decision`, `policy_revision`, `connector_instance_change_approved`, `target_persistent_identity_change_approved`, `target_persistent_identity_mismatch`, `target_boot_change_approved`, `target_boot_mismatch`, `workspace_change_approved`, `workspace_mismatch`, `cwd`, `architecture_or_requirement_mismatch`, `connector_replay_detected`, `connector_unapproved_rotation`, `connector_clone_detected`, `connector_binding_mismatch`, `connector_mid_run_replacement`, `action_audit_mismatch`, `action_audit_replay`, `route_cycle_detected`, `required_prerequisite_missing`, `no_representable_candidate`, `authoritative_unreachable`, `evidence_unavailable`, `bound_reached`, `proof_expired`, `fact_freshness_expired`, `fact_superseded_by_grade`, `fact_superseded_by_freshness`, `equal_grade_fresh_conflict`, `proof_validation_failed`, `proof_validation_unavailable`, `action_schema_or_digest`, `catalogue_trust_revision`, `frozen_plan_integrity_failure`, `deadline_registration_integrity_failure`, `substitution_outside_envelope`, `engine_boot` |
| Invalidation scope | `none`, `self`, `descendants`, `self_and_descendants`, `whole_campaign` |
| Proof validation result | `not_applicable`, `all_passed`, `failed`, `unavailable` |
| Proof instance action | `not_applicable`, `same_instance`, `new_instance` |
| Transition transaction identity | `fresh_unique`, `duplicate`, `missing` |
| Root-branch count | `zero`, `one`, `many_within_admitted_bound`, `over_limit` |
| Discovered-descendant count | `zero`, `one`, `many_within_admitted_bound`, `over_limit` |
| Descendant coverage | `exact_complete`, `missing_member`, `extra_member`, `duplicate_member`, `not_applicable` |
| Per-branch Route-effect records | complete finite set assigning each affected branch exactly one of `none`, `install_terminal`, `replace_terminal`, `invalidate_current`, `retire_to_historical`, `retain_final_snapshot`, `install_and_retain_terminal_snapshot`, `delete_retained_snapshot`; `missing`, `duplicate`, or `inconsistent` rejects the batch |
| Queue-wait member | kind `campaign_start\|sensor_admission`; exact campaign and optional branch/sensor-edge identity, reached capacity-ledger revision, state `absent\|pending\|released\|cancelled`, and one deduplicated wake keyed to the next capacity-ledger revision plus owning campaign total-time deadline. Effects are `register_or_refresh_wait\|consume_on_admission\|cancel_on_terminal\|delete_retained_wait\|not_applicable`. It is durable, startup-recoverable, and scheduler-owned; Operation may project it but never creates it |
| Queue-wait coverage witness | `exact_complete_owned_set`, `exact_empty_owned_set`, `missing`, `extra`, `duplicate`, `stale_or_mismatch`, `not_applicable`; every transition that removes/replaces a pending sensor edge or makes a branch/campaign non-runnable requires the exact complete owned wait set and maps each `pending\|released -> cancelled` as applicable in that batch. `retention_expiry` instead deletes every retained cancelled/released record and wake/revision index. No generic cleanup receipt may stand in for this durable state transition |
| Sensor-invocation member | exact campaign/branch/plan/sensor identity and resource-ownership set; state `absent\|admitted_live\|terminal`, effect `admit_before_spawn\|terminalize_and_release\|cancel_and_release\|not_applicable`. Admission requires current policy/action audit, exact live-capacity reservation, and durable deadline arm; every terminal sensor result consumes the one live member. Duplicate admission/result or work without `admitted_live` rejects |
| Request-key member | from `unbound` or `bound_to_this_campaign` to `bound_to_this_campaign`, `unbound`, `no_transition`, or `not_applicable`; effect `bind_atomically`, `retain`, `reanchor_lease`, `delete_binding`, or `not_applicable`. Each bounded alias has its own immutable binding identity and persisted minimum-not-before resolution lease: configured duration, campaign-owner clock domain, originating boot id, monotonic anchor, remaining duration, and receipt. A matching-key read reports the existing remaining guarantee and never silently extends it. `delete_binding` is valid only inside exact `retention_purge` coverage after every alias lease and the tombstone minimum have elapsed; it removes both forward and reverse indexes |
| Capacity-ledger member | one exact conditional delta set over per-request, campaign, immutable admission-partition, and daemon-store counters/revisions plus one reservation identity. Effects are `reserve_unkeyed_admission_all_caps`, `reserve_keyed_admission_all_caps`, `reserve_alias_all_caps`, `reserve_sensor_live_capacity`, `release_sensor_live_capacity`, `move_queue_to_active`, `release_queue_or_active_to_terminal`, `reduce_terminal_to_tombstone`, `purge_tombstone_keys_and_indexes`, or `not_applicable`. Admission reserves one queued slot, a possible new retention-partition slot, and the maximum serialized run-registry/lifecycle footprint through terminal and tombstone states; keyed admission additionally reserves its alias and key indexes. Later alias binding reserves the alias/index deltas **and** the exact maximum current-to-terminal/tombstone run-registry, retained-byte, and key-proof delta caused by that alias; it may not borrow unused admission headroom. Sensor admission reserves its helper/concurrency slots before spawn and every sensor terminal/cancellation batch releases them. `campaign_start` alone CAS-checks active capacity and moves queued to active. Terminalization is non-increasing within the admission reservation and therefore cannot fail for capacity. A purge uses partition delta zero unless an exact same-revision final-record/refcount witness proves the owning partition becomes empty; only that final purge may apply delta minus one together with audit/metadata fold-and-delete. Every increment is reserved before and committed with its owning state/key write; every decrement/move commits with the lifecycle transition. Missing, stale, negative, split, double-counted, or cross-scope deltas reject |
| Terminal-retention-eligibility member | exact terminal campaign identity, immutable admission partition, configured minimum-retention duration, campaign-owner boot/suspend-inclusive monotonic clock, terminal-commit anchor, remaining duration, continuity proof, and receipt; state `absent\|armed\|reanchored\|consumed\|not_applicable`, effect `construct_at_terminal_commit\|reanchor_conservatively\|consume_for_expiry\|retain\|not_applicable`. Every transition into `completed\|cancelled\|failed` constructs it in the same batch. Expiry requires proved elapsed eligibility on the current owner clock. A boot/clock change with unproved continuity permits only `retention_lease_reanchor`, which preserves lifecycle/artifacts and persists the full remaining configured window under the current clock before any expiry decision |
| Deadline-registration member | exact owner campaign and optional sensor identity, owner boot/suspend-inclusive monotonic clock, start anchor, frozen duration, durable registration revision, and arm-health receipt; state `absent\|armed\|terminal`, effect `persist_and_arm_before_work\|clear_atomically_on_terminal\|force_clear_on_integrity_failure\|retain\|not_applicable`. Total-time registration commits with campaign admission before accepted publication; each per-sensor registration and action audit commit before spawn. Every sensor/campaign terminal or cancellation batch clears its complete owned registration set in the same transaction; external timer disarm is idempotent cleanup. Missing/failed arm starts no work. A missing/extra/stale clear can never block cleanup or strand work: it selects typed `deadline_registration_integrity_failure`, force-disarms by owner identity, terminalizes through campaign failure with complete cleanup, and records an anomaly receipt without claiming a valid deadline cut |
| Purge-eligibility member | exact bounded alias-set root/count and complete per-alias lease coverage, exact tombstone minimum, derived `protected_until = max(all alias minimum-not-before leases, tombstone minimum)`, campaign-owner clock-domain identity, originating boot ids, persisted monotonic anchors/remaining durations, current continuity proofs, and verdict `elapsed_eligible\|protected_not_elapsed\|continuity_unproved`; effect `authorize_purge\|retain_and_reanchor_conservatively\|not_applicable`. Missing/extra alias coverage or a non-max aggregate rejects. Same-boot daemon restart uses proved monotonic continuity. A new/unproved clock or boot can only select `retention_lease_reanchor`, which conservatively persists the full remaining configured window under the current boot; wall time is informational only. Jump/rollback, missing anchor, mismatch, replay, or `protected_not_elapsed\|continuity_unproved` can never authorize purge |
| Authoritative-context member | kind `policy`, `catalogue_trust_security`, `cwd`, `action_schema`, `connector_instance`, `target_persistent_identity`, `target_boot`, or `workspace`; exact old/new revision or equality class; disposition from the closed pre-Route change table; effect `replace_authoritative`, `hard_exclude`, or `not_applicable`. A registry activation-head change is future-admission state and is never a current-run member; topology is the derived set of these exact members plus branch transitions, never an independent context kind |
| Frozen-plan-integrity member | exact admitted snapshot identity and expected digest plus observed `snapshot_unavailable\|digest_mismatch`; effect `fail_campaign`; valid only for `campaign_failure/frozen_plan_integrity_failure` with no replacement plan and never as recoverable authoritative context |
| Legacy-overlay-migration member | one exact entry for every store in the authoritative intentional value-bearing private-store inventory; source state `legacy_or_unproved_value_bearing`, `current_private_value_schema_with_exact_allowed_provenance`, `mixed_current_and_legacy_or_unproved`, `current_names_only`, `quarantined_retryable`, `quarantined_requires_operator_repair`, `unavailable`, or `unknown`; result `purged_legacy`, `preserved_verified_current_private_values`, `purged_legacy_and_preserved_verified_current`, `verified_current_names_only`, `quarantined_retryable`, `quarantined_requires_operator_repair`, or `failed_closed`; and effect `delete_legacy_records_atomically`, `preserve_verified_current_opaque_records`, `delete_legacy_and_preserve_verified_current_atomically`, `record_idempotent_current`, `quarantine_whole_store_and_schedule_bounded_retry`, `terminalize_quarantine_for_operator_repair`, or `block_store`. Before classification the store is inaccessible. Using the frozen current schema, exact operator persistence/allowlist policy, canonical-name metadata, and immutable provenance metadata, the member proves a complete disjoint partition of every value-bearing record and every secondary index into (a) current private records whose canonical name is currently allowlisted and whose exact provenance is caller-supplied, or (b) legacy, literal-redaction-marker, stale-policy, malformed, or unproved records. Classification MUST NOT open, export, compare, hash, decode, or log value material. Verified current material remains opaque and byte-untouched; legacy/unproved material and only its indexes are deleted. A mixed store commits preservation and deletion atomically. Every mutation requires exact engine/store policy plus append-only action-audit admission before execution. Exact success proves complete record/index coverage, all legacy/unproved records and their indexes deleted, and exact equality of every verified-current opaque record-identity set plus its current index set before/after commit; only all-store success may release the migration gate, after which preserved records remain subject to ordinary restore authorization. Exact quarantine proves restore/list/status access was disabled before classification and remains disabled. Every retryable quarantine carries durable crash-safe control state: frozen finite attempt budget, strictly decreasing attempts-remaining counter, attempt ordinal, unique retry/deduplication identity, owner boot and suspend-inclusive monotonic clock, frozen bounded backoff schedule revision, not-before anchor/duration, total recovery deadline, and wake state `armed\|due`. Each failed authorized attempt consumes exactly one remaining attempt atomically with its result; restart, duplicate wake, or commit retry MUST NOT reset or increment the budget. Wake is signal-driven with the total deadline as fail-safe, never polling. Zero remaining attempts or elapsed total deadline atomically derives `quarantined_requires_operator_repair`, removes every automatic wake, and remains terminal until a distinct operator-authorized/audited repair starts a new migration generation and budget. Audit denial/failure starts no mutation and leaves the default-closed migration gate in force. Failed/crashed transactions prove the verified-current opaque identity/index sets unchanged. Missing/extra/duplicate stores or records/indexes, overlapping partitions, partial deletion/preservation, retry-state mismatch, re-enabled access, or any content-bearing audit field rejects |
| Action-audit member | one immutable append-only admission record for every newly authorized gated action at each authorizing/executing node; binds peer, action digest, sensor/action class, policy revision and verdict, campaign/branch, target/boot/workspace, connector route, and local audit identity; effect `commit_before_action` or `not_applicable`; missing, failed, duplicate, or mismatched coverage rejects and starts no action |
| Sensor-result member | exact producer/sensor/campaign/branch identity, result `trusted_fact`, `denied`, `unavailable`, `rejected`, `unknown`, `probe_failed`, `timed_out`, `truncated`, `input_bound_reached`, `unsupported`, `cancelled_by_branch_transition`, `cancelled_by_campaign_transition`, or `interrupted_by_engine_loss`, its exact immediate producer/security/bound receipt when applicable, and effect `record_terminal`; each lifecycle-cancellation result instead binds the exact winning Transition purpose/event/cause; missing, duplicate, mismatched, replayed, or nonterminal results reject |
| Fact-admission member | exactly one of: final Security `fact_admission=trusted` plus composed EvidenceBoundary receipt and relation, effect `admit_exact_source_set`; an exact finite sensor-to-private-observation-to-conditional-Security-receipt-to-source-set map whose every receipt is `fact_admission=trusted_pending_effect_commit`, effect `stage_exact_source_set_pending_boundary`; or that identical map plus its exact pending Transition identity and subsequently composed final EvidenceBoundary receipt, effect `publish_staged_source_set`; otherwise `not_applicable`. The ordinary single-sensor case is cardinality one. The finalizer is not a second Security admission. Missing/extra/duplicate map keys, source/receipt mismatch, replay, rejection, or unknown/unproved admission commits no fact |
| Source-fact member | exact old/new source identity, evidence grade, freshness, value equality class, sensor/boundary receipt, visibility `public_current\|pending_boundary\|not_applicable`, and effect `add`, `stage_pending`, `publish_pending`, `supersede`, `invalidate`, `delete_owned`, `retain`, or `not_applicable` |
| Derived-fact member | exact dependency identity, visibility `public_current\|not_applicable`, plus effect `recompute`, `remove`, `delete_owned`, `retain`, or `not_applicable`; pending-bound staging never snapshots a derived closure |
| Pending-boundary transaction member | exact pending Transition identity, owner campaign/branch, and exact finite sensor-to-conditional-Security-receipt-to-source-set map (cardinality one for the ordinary case), plus optional already-composed final boundary receipt over that complete map, state `staged\|published\|discarded_invalidated`, and effect `publish_exact\|discard_and_invalidate_receipts\|retain_unaffected\|not_applicable`; missing/extra/duplicate map entries reject, and a published/discarded identity is terminal and cannot be reused |
| Bound-effect-bundle member | exact complete independent-bound reached-dimension set, per-dimension disposition/correlation map, unsuppressed primary-effect set, affected ownership union, conditional Security receipts, frozen proposed Operation-output descriptor when presentation/queue effects apply, and shared cleanup identity; valid only for two or more independently applicable reached dimensions and effect `commit_exact_bundle`. A postcommit Operation receipt is never an input to the state transition it presents; missing, duplicate, extra, scalar-only, caller-selected dominance, or descriptor/committed-state mismatch rejects |
| Campaign-result snapshot member | exact campaign-bound state `current_goal_projection`, `retained_terminal_result`, `retained_terminal_status`, `unavailable_tombstone`, or `not_applicable`; effect `retain_terminal_result`, `retire_current_goal_to_terminal_status`, `delete_result_to_unavailable_tombstone`, `reduce_status_to_tombstone`, or `not_applicable`; it owns zero-Route results as well as ordinary Route-backed results |
| Retention-artifact-set member | exact complete ownership closure over the campaign row; bounded request-key aliases/indexes; branch/Route/proof/Goal/result snapshots including zero-Route state; source/derived facts; pending candidates and receipts; sensor invocation/result records; cleared deadline registrations; cancelled/released queue-wait records plus their wake/revision indexes; terminal-retention eligibility; detailed action/audit/operation traces; output/bucket/cursor stores; frozen plan payload; and every secondary index. `retention_expiry` effect `reduce_to_fixed_tombstone` deletes every heavy artifact--including terminal invocation/deadline/queue-wait records and wake indexes--while retaining only the fixed tombstone, bounded key lookup set, complete per-key lease proofs, newly constructed tombstone-minimum lease, and exact max protected-until proof; `retention_purge` effect `delete_tombstone_keys_and_indexes` deletes those last records and leaves campaign state tagged `absent`. A separately owned shared fact may survive only with an exact owner/refcount transfer into its own bounded partition |
| Retention-audit member | exact fixed-size per-partition monotonic digest plus a daemon-global fixed-size monotonic digest accumulator and bounded recent-maintenance tail. Effects are `append_partition_and_compact`, `record_lease_reanchor`, or `fold_final_partition_and_delete`. Every expiry, re-anchor, and purge updates the exact owning-partition digest. When a purge removes the final record, `fold_final_partition_and_delete` atomically folds its terminal digest into the bounded global accumulator and deletes all per-partition audit/metadata, so audit state cannot keep an empty partition alive. Global/partition tail caps and deterministic compaction are frozen; no maintenance proof creates an unbounded replacement log |
| Dependency-coverage witness | `exact_transitive_closure`, `not_applicable`, `missing`, `extra`, `stale_or_mismatch` |
| Pending-boundary coverage witness | `exact_complete`, `not_applicable`, `missing`, `extra`, `duplicate`, `stale_or_mismatch`; covers every pending transaction owned by each affected branch/campaign before any competing state change |
| Goal recomputation witness | `exact_current_records`, `published_pending_boundary_from_exact_current_records`, `terminal_final_snapshot`, `cleared_to_terminal_status`, `deleted_to_unavailable_tombstone`, `not_applicable`, `missing`, `stale_or_inconsistent` |
| Audit-history witness | `exact_append_set`, `not_applicable`, `missing`, `duplicate_or_mismatch`; every retiring branch preserves prior terminal state/cause, Route identity/outcome/evidence digest when present, cancellation reason when Route-less, and retirement cause |
| Campaign-summary audit witness | `exact_committed`, `not_applicable`, `missing`, `failed_or_mismatch`; the exact terminal campaign verdict/evidence/bounds/cleanup summary is append-only at the owning engine and cannot claim authority for another node |
| Forwarded terminal-summary authentication | independently for every expected executing node: `not_forwarded_local_owner`, `exact_authenticated_executing_node_and_bound_local_audit`, `unauthenticated`, `missing_local_audit`, `mismatch`, `unproved`, `not_applicable`; the exact form binds executing-node persistent identity/boot, action digest, policy revision, terminal verdict, and the immutable local audit receipt without importing that node's authority |
| Cleanup witness | `complete_no_live_resources`, `incomplete`, `not_applicable` |
| Causal receipt correlation | `exact_start_admission`, `exact_sensor_fact_admission`, `exact_sensor_terminal_result`, `exact_sensor_private_input_stop`, `exact_soft_bound_finalization`, `exact_bound_effect_bundle`, `exact_action_audit_integrity_failure`, `exact_route_proof`, `exact_policy_decision`, `exact_hard_exclusion`, `exact_exhaustion`, `exact_bound_observation`, `exact_legacy_overlay_migration`, `exact_retention_partition_bound`, `exact_tombstone_partition_bound`, `exact_purge_eligibility`, `exact_retention_lease_reanchor`, `exact_recoverable_invalidation`, `exact_proof_event`, `exact_scheduler_prune`, `exact_cancel_request`, `exact_engine_event`, `exact_scheduler_completion`, `exact_failure`, `exact_frozen_plan_integrity_event`, `exact_retention_timer`, `exact_tombstone_timer`, `internal_progress`, `missing`, `mismatch`, `not_applicable` |
| Candidate to-state | the same tagged state domain as the subject, plus campaign deletion state `absent`, `no_transition`, and `not_applicable`; `absent` is an effect result only, never a durable tombstone or legal transition from-state |

Migration retry clock continuity is closed: proved continuity on the same
suspend-inclusive monotonic domain resumes the persisted not-before and total
deadline with the already-decreased budget. A boot/clock change without exact
continuity atomically selects `quarantined_requires_operator_repair`; it MUST NOT
re-anchor a fresh recovery window or reset attempts. A duplicate wake consumes no
attempt, while an authorized attempt consumes exactly one in the same transaction
as its retry result. These rules make attempts remaining a strict finite
decreasing measure even across crash/restart.

Leaving the terminal repair state requires a separate `OverlayMigrationRepair`
storage-maintenance request, exact pre-existing `operator_store_admin` authority,
and committed pre-action audit. LLM/caller, plan, sensor, migration, or retry
authority is structurally rejected and cannot create a new generation or budget.

Derived outputs are `transition = accepted \| rejected`, the exact post-commit
per-branch Route-effect record set, and the immutable accepted-batch receipt.
Every accepted batch has `fresh_unique` transaction identity and exact
member correlation. Duplicate and missing identities, partial writes, or a member
visible without the batch commit receipt are rejected.

Forwarded terminal-summary contribution is a separate closed provenance function:

| Expected summary source | Authentication input | Exact contribution |
|---|---|---|
| campaign-owning engine | `not_forwarded_local_owner` | local provenance from its own committed audit only; never imported authority |
| forwarded executing node | `exact_authenticated_executing_node_and_bound_local_audit` | provenance-only terminal summary bound to that node and audit receipt; never authority |
| forwarded executing node | `unauthenticated\|missing_local_audit\|mismatch\|unproved` | typed `unknown`; no policy, action, fact, or authority claim from that summary |
| no forwarded executing node is expected | `not_applicable` | no contribution |
| campaign owner with any forwarded input; forwarded node with an invalid local-role marker; absent expected-node entry; duplicate entry; or any other unmatched source | any forwarded value for the campaign owner; `not_forwarded_local_owner\|not_applicable` for a forwarded node; otherwise missing, duplicate, or mismatched | structural rejection of the campaign-summary member |

The set is exact and complete over the executing-node set frozen by the terminal
Transition. A typed-unknown forwarded contribution may be recorded only as unknown;
it cannot block cleanup or fabricate authority. Environment names, values, raw
output, and secret-shaped request data are forbidden in every contribution.
This axis is active exactly for a batch that owns a `campaign_summary_audit`
member; every other batch requires `not_applicable`. A required summary batch must
classify the owner and every expected forwarded executing node even when the
campaign-summary audit witness is `missing\|failed_or_mismatch`; that witness still
rejects the batch and none of the classified contributions is persisted.

Every branch creation/progress, sensor-result, fact, proof, Route, pruning, or
branch-scoped invalidation batch binds its owning campaign and requires that
campaign's committed pre-state to be exactly `running`. State `accepted` means
durably admitted but no branch, Route, sensor/helper, proof, or other live member
has started. From `accepted`, the only legal next effects are request-key binding,
`campaign_start`, cancellation/failure/engine loss, Operation-owned queue/output
annotation, or a campaign total-time stop with exactly zero branches, zero Routes,
and zero live resources. Any work member under `accepted`, or any branch work
whose owning-campaign guard is missing, terminal, or mismatched, rejects the
entire batch.

Capacity ownership is equally atomic. `campaign_admission` consumes one conditional
all-caps reservation and records the accepted campaign in the bounded queue/run
registry; it does **not** reserve active capacity. The reservation includes the
maximum serialized footprint of every later live, zero-Route terminal, retained,
and tombstone form plus a possible immutable admission-partition slot, so mandatory
terminalization is non-increasing and cannot fail for storage. A keyed reservation
also includes the initial alias and both key indexes. `campaign_start` separately
uses an exact fresh active-capacity receipt and CAS to decrement queued and increment
active; at the active cap it commits no start transition and the campaign remains
bounded `accepted` for a later capacity signal or total-time stop. Alias binding
consumes one all-caps reservation covering its bounded alias/key-index deltas plus
the exact maximum current-to-terminal/tombstone run-registry, retained-byte, and
key-proof delta introduced by that alias; no unused earlier reservation is borrowed. Cancellation/failure/
bound-stop/completion release the queued-or-active slot and transform the already
reserved registry footprint to its terminal class. Expiry reduces it to the fixed
tombstone; purge releases the last run/key reservations and uses a zero partition
delta unless the same-revision post-delete refcount proves this was the final record,
in which case exactly one partition slot is released with the audit/metadata fold. No state/key
transition may omit, defer, split, or independently repair its exact capacity-ledger
member. If any admission cap cannot be reserved--including when every historical
record is lease-protected--the start returns precommit `rejected_bound`; it never
creates an unbounded accepted campaign.

Queue waits are state, not client subscriptions. Every capacity-ledger release
emits one store-revision signal; the scheduler atomically consumes each matching
pending wait into `campaign_start\|sensor_admission` or refreshes it at the new
revision if capacity remains reached. Startup recovery enumerates pending waits.
Any winning branch/campaign terminal, cancellation, failure, engine-loss, or
total-time batch includes the complete owned wait set as `cancel_on_terminal`.
Missing, duplicate, or orphaned wait coverage rejects ordinary progress but cannot
block safety cleanup; an integrity mismatch escalates to campaign failure and
force-cancels by owner identity.

The following are the only simple progress/admission compositions. They are still
atomic batches, require `internal_progress` except admission/key-binding/start as
shown, use coverage `not_applicable` because they do not affect descendants, have
no proof/Route/cleanup/audit-history/campaign-summary member, and assign Route
effect `none`. Every `branch_progress` row carries the exact post-commit
open-branch-role set and Goal recomputation witness `exact_current_records` in the
same batch; campaign admission, key binding, and campaign start have no Goal
member because their rows do not add, remove, or reclassify a branch.
`scout_started`, `attestation_started`, and `deepening_started` carry exact
stage-admission `action_audit` members; every `sensor_admission/sensor_started`
carries a distinct exact concrete sensor/action-digest audit that cannot be
substituted by the stage record or another sensor. Each append commits before any
observation, connector call, or spawn. Every other simple row has action-audit
`not_applicable`. The request-key member is present
exactly in the keyed rows stated below.
Every time-bounded sensor/helper is admitted only by the `sensor_admission` row,
which commits its exact `deadline_registration/persist_and_arm_before_work` beside
the action audit before spawn. Every ordinary sensor-result, bound stop, branch/campaign
terminalization, cancellation, or engine-loss batch covers and clears the complete
owned deadline-registration set atomically with terminal result and cleanup. A
timer callback alone is never a transition authority.

| Purpose / member | Event | Accepted from -> to | Cause/scope/correlation |
|---|---|---|---|
| `branch_progress` / branch + exact Goal snapshot | `candidate_found` | `absent -> discovered` | cause/scope `none`; `internal_progress` |
| `branch_progress` / branch + exact Goal snapshot | `gates_started` | `discovered -> gated` | cause/scope `none`; `internal_progress` |
| `branch_progress` / branch + exact action-audit set + exact Goal snapshot | `scout_started` | `gated -> scouting` | cause/scope `none`; `internal_progress`; all action audits commit before work |
| `branch_progress` / branch + exact action-audit set + exact Goal snapshot | `attestation_started` | `scouting -> attesting` | cause/scope `none`; `internal_progress`; all action audits commit before work |
| `branch_progress` / branch + exact action-audit set + exact Goal snapshot | `deepening_started` | `viable -> deepening` | cause/scope `none`; `internal_progress`; all action audits commit before work |
| `queue_wait_registration` / unchanged branch + sensor queue wait | `capacity_wait_registered` | running campaign; branch in `{gated, scouting, attesting, viable, deepening}` retains exact state; exact sensor-admission helper/concurrency reach creates or refreshes one `pending` wait | cause `bound_reached`; scope `self`; `exact_bound_observation`; no action audit, sensor invocation, deadline registration, or spawn. The wait survives client disconnect and wakes on capacity revision or campaign total-time deadline |
| `queue_wait_registration` / unchanged campaign + campaign-start queue wait | `capacity_wait_registered` | campaign remains `accepted`; exact active-capacity reach creates or refreshes one `pending` wait | cause `bound_reached`; scope `whole_campaign`; `exact_bound_observation`; no branch/work/action audit or spawn. The wait survives client disconnect and wakes on capacity revision or total-time deadline |
| `sensor_admission` / unchanged branch + optional pending queue wait + sensor invocation + exact action audit + capacity ledger + deadline registration | `sensor_started` | branch in `{gated, scouting, attesting, viable, deepening}` retains exact state; sensor `absent -> admitted_live`; exact `reserve_sensor_live_capacity` and `persist_and_arm_before_work`; an existing pending wait becomes `released` in the same batch | cause `none`; scope `self`; `internal_progress`; current campaign/plan/policy/action digest and frozen fallback edge bind the batch. No spawn precedes commit |
| `campaign_admission` / campaign + capacity ledger + total-time deadline registration + operation trace, plus request key iff supplied | `campaign_admitted` | campaign `none -> accepted`; keyless uses one `reserve_unkeyed_admission_all_caps`, keyed uses one `reserve_keyed_admission_all_caps`; both reserve queue, maximum lifecycle registry footprint, and a new immutable admission-partition slot iff absent; the keyed effect additionally reserves alias/key indexes and binds the supplied key `unbound -> bound_to_this_campaign` with its own lease. Keyless has no key member. The durable total-time deadline is persisted and armed before accepted publication. Active capacity is not reserved | cause `none`; scope `whole_campaign`; `exact_start_admission`; the complete conditional delta set, deadline, key, and campaign become visible in one durable batch or none changes; failed arm disarms/cleans and admits nothing |
| `request_key_binding` / immutable campaign reference + request key + capacity ledger + operation trace | `request_key_bound` | campaign in `{accepted, running}` has `no_transition`; new alias key `unbound -> bound_to_this_campaign`, effect `bind_atomically` with its own lease; one `reserve_alias_all_caps` covers alias/forward/reverse indexes and the exact maximum current-to-terminal/tombstone run-registry, retained-byte, and key-proof delta caused by that alias; terminal/history campaigns reject a fresh alias | cause `none`; scope `whole_campaign`; `exact_start_admission`; exact authorized shareability digest; failure leaves key, campaign, and every ledger scope unchanged |
| `campaign_start` / campaign + optional pending queue wait + capacity ledger | `campaign_started` | `accepted -> running`; one exact current-revision active-capacity receipt authorizes `move_queue_to_active`, atomically decrementing queued and incrementing active; an existing pending wait becomes `released` in the same batch | cause `none`; scope `whole_campaign`; `exact_start_admission` consuming the accepted admission receipt and fresh active-capacity receipt |

On the ordinary producer-return path, every admitted sensor invocation terminates
through exactly one `sensor_observation_commit` batch. Both subrows carry one exact terminal
`sensor_result`, the complete producer-owned resource set, cleanup witness
`complete_no_live_resources` (including an exact empty-set witness for a purely
in-process producer), exact sensor invocation `admitted_live -> terminal`,
`release_sensor_live_capacity`, atomic deadline-registration clear, and exact
post-commit Goal/open-frontier recomputation. The
result and cleanup become visible together or neither does:

- `sensor_observation_admitted` requires result `trusted_fact`, one exact final
  `fact_admission=trusted` member, the complete nonempty admitted `source_fact`
  set, and `exact_transitive_closure` over every dependent derived fact. The
  branch is in `gated\|scouting\|attesting\|viable\|deepening`; it retains that state
  or moves `scouting\|attesting\|deepening -> viable` only when the newly committed
  facts make the frozen viability predicate exact. Correlation is
  `exact_sensor_fact_admission`; there is no proof or Route effect.
- `sensor_result_recorded` requires exactly one non-fact result from
  `denied\|unavailable\|rejected\|unknown\|probe_failed\|unsupported`,
  its exact immediate producer/security/bound receipt when one exists, no
  fact-admission/source/derived member, and correlation
  `exact_sensor_terminal_result`. It retains the branch state, adds no fact, and
  atomically unlocks only the fallback edges declared by the frozen plan before
  recomputing the frontier/Goal snapshot. Missing, duplicate, mismatched, replayed,
  or nonterminal producer evidence rejects the batch.
- `sensor_private_input_stopped` requires result `input_bound_reached`, the exact
  private counter observation, the matching Security non-authorizing private-input
  stop receipt, and the sole shared cleanup receipt over the complete top-level
  sensor/helper ownership set. It terminalizes the invocation, releases live
  capacity, and clears its deadline registration. It records no fact, atomically unlocks the frozen
  fallback edges and recomputes Goal/frontier, and uses
  `exact_sensor_private_input_stop`. A nested decoder/resolver typed failure is
  bound evidence inside this one top-level result, never a second result or cleanup.
  EvidenceBoundary consumes this exact Transition/cleanup receipt to issue the
  incomplete boundary receipt; it performs no second cleanup. Missing, duplicate,
  or cross-sensor ownership rejects.

`timed_out\|truncated` sensor results are reserved for the exact feature-003 bound
compositions below. They can never enter `sensor_result_recorded`: a hard or
structural relevance closure must use the branch-decisive terminal bound row,
while a soft/informational closure must use `soft_evidence_bound_commit`.

Across all paths, every admitted ordinary sensor has exactly one durable terminal
disposition: an ordinary producer-return result above, a `timed_out\|truncated`
result in its exact bound composition, or one lifecycle-cancellation result in the
winning transition that owns its cleanup. None may be synthesized later or appear
in two batches.

`soft_evidence_bound_commit/bound_reached` is the staging half of the sole
nonterminal bound composition. It accepts one `scouting\|attesting\|deepening`
branch, the exact per-sensor-time private bound-observation receipt whose complete
frozen relevance closure is `soft_only\|informational_only`, and an exact Security
`fact_admission=trusted_pending_effect_commit` receipt bound to the same typed
incomplete `timed_out\|truncated` sensor-result member and proposed batch digest.
Those inputs form the cardinality-one conditional receipt/source map; the bundle
form may supply a larger exact map but never a loose receipt list.
The batch retains the public branch state exactly, stages only the exact source
candidate with visibility `pending_boundary`, terminalizes and cleans only that
sensor, releases its live capacity, clears its deadline registration,
and emits the exact pending Transition identity. It snapshots no derived closure
or Goal projection. Pending members are transaction-private and inert: no
scheduler, fallback selector, Route/Goal classifier, Operation response, public
read, or later sensor may observe or consume them.

EvidenceBoundary then deterministically composes the final evidence-incomplete
receipt from the pending Transition/cleanup receipt. A distinct
`soft_evidence_bound_finalize/soft_bound_finalized` batch must consume that exact
final receipt, the exact pending Transition identity, and the identical complete
conditional Security receipt/source map. It atomically changes every mapped staged
source to `public_current`,
recomputes the complete transitive derived closure and Goal/open frontier from the
exact then-current public record set, may then advance
`scouting\|attesting\|deepening -> viable` only when that recomputation makes the
hard viability predicate exact, and uses
`published_pending_boundary_from_exact_current_records` plus
`exact_soft_bound_finalization`. It performs no
new Security admission and no second sensor cleanup. The ordinary
`final_boundary_receipt` admission path rejects a candidate already bound to a
pending Transition, preventing re-admission. A crash between staging and
finalization leaves only inert recovery-owned state; same-boot recovery resumes
deterministic composition/finalization, while engine loss retires it without ever
publishing it. This one-way order contains no receipt cycle.
`soft_only` can later produce Route `ready_with_warnings`; `informational_only`
cannot change Route or Goal. Any hidden hard/structural dependency, relevance
mismatch, missing cleanup, or absent exact admission rejects the batch.

Pending-boundary disposition is exhaustive. The finalizer publishes exactly one
still-`staged` identity with coverage `exact_complete`. Any competing accepted
terminalization, hard/recoverable invalidation, pruning, caller cancellation,
engine loss, campaign failure, campaign bound stop, or campaign completion whose
scope owns a pending identity must instead include every such pending transaction
and its staged source member, discard them atomically, and
invalidate both the conditional and any composed final receipt. A later finalizer
for `discarded_invalidated` rejects. Ordinary branch progress, new sensor work,
proof/Route publication, or scheduler completion lacking that exact discard
disposition while a same-owner pending identity exists is rejected rather than
racing it. Unaffected identities outside
the batch scope are covered exactly once as `retain_unaffected`; missing, extra,
duplicate, or stale coverage rejects. Thus every pending identity reaches exactly
one terminal disposition--published or discarded--and can never become orphaned or
publish across a newer branch/campaign state.

At most one pending publication barrier may exist per branch. A second soft-bound
stage or any ordinary sensor/proof/Route work on that branch rejects until the
barrier is published or discarded. Other branches may progress, but the finalizer
must recompute against their exact current public records; it never publishes a
staged Goal or derived snapshot. A current-record mismatch in any receipt-bound
policy/identity/plan dependency discards or retries the pending source under a new
identity and can never publish stale state.

`bound_effect_bundle_commit/bound_effect_bundle_committed` is legal only when the
IndependentBoundModel proves two or more independently applicable reached
dimensions in one scope. Its exact bundle member carries the closed dominance map;
each unsuppressed Transition-owned subeffect then uses the same branch/campaign,
fact, pending, Route/Goal, sensor-result, and cleanup semantics as its corresponding
single-effect row below. Safety-subsumed soft candidates create no source member;
coalesced soft candidates share one pending transaction. Conditional Security
receipts and any frozen proposed Operation-output descriptor are inputs but never
become independent public state. Operation derives its exact queue/output receipt
only after the Transition commit, from that committed state and matching descriptor. The
per-branch Route-effect set and Goal witness are the exact union after dominance:
a branch/campaign terminal effect supplies the sole terminal Route/Goal projection;
otherwise only the unsuppressed soft pending set may later affect Goal at its
finalizer, while queue/output/private-only effects leave Route/Goal unchanged.
The whole bundle uses cause `bound_reached`, correlation
`exact_bound_effect_bundle`, and one fresh transaction identity; partial subeffect
commits, a bundle for a single dimension, or a caller-selected dominance map
reject. This composition is the sole multi-reach exception to the single-purpose
rows; it does not weaken any subeffect's guards.

Every state-changing composition not listed above is one of the following exact
multi-member batches. `A` means the complete finite descendant set owned by the
root at the transaction snapshot and is partitioned exactly into `A_active =
{discovered, gated, scouting, attesting, viable, deepening, ready}`,
`A_terminal = {denied, excluded, exhausted, truncated, cancelled}`, and
`A_historical = {retracted}`. A root with `A = empty` uses scope `self`,
descendant count `zero`, and coverage `exact_complete`. A nonempty root-scoped
row uses exactly `self_and_descendants`; a campaign-wide lifecycle row uses
exactly `whole_campaign`. Each declares its exact admitted count and carries
every member once. No row may choose between those scopes. `missing_member`,
`extra_member`, `duplicate_member`, `over_limit`, or an
unmatched transaction identity rejects the entire batch.

| Purpose / event | Root branch member | Descendant members | Proof member | Route/Goal/other mandatory members |
|---|---|---|---|---|
| `route_proof_commit` / `route_proved` | one `{viable, deepening} -> ready` | none | `missing -> current`, `new_instance`, validation `all_passed` | install the exact `ready` or `ready_with_warnings` Route and recompute Goal from `exact_current_records` in the same commit; `exact_route_proof` |
| `terminal_branch_commit` / `policy_denied` | one `{gated, scouting, attesting, viable, deepening} -> denied`; cause `policy_decision` | partitioned `A` disposition below | root has no current proof; descendant proof coverage follows the rule below | install exact terminal Route `denied`; exact Goal recomputation; `exact_policy_decision` |
| `terminal_branch_commit` / `hard_excluded` | one `{discovered, gated, scouting, attesting, viable, deepening, denied, exhausted, truncated} -> excluded`; exact hard-exclusion cause | partitioned `A` disposition below | root has no current proof; descendant proof coverage follows the rule below | from prior `denied\|exhausted\|truncated`, apply the cause's exact per-record effects, derive the complete current post-batch closure, run the ordinary winner algebra, and replace the terminal Route as `unsupported`, `denied`, or `unreachable` from that current winner, otherwise `blocked`; from a nonterminal prior state, install only the ordinary exact `blocked` projection; retain the new exclusion cause and `hard_excluded` event exactly; exact Goal recomputation; action-audit mismatch/replay uses only `exact_action_audit_integrity_failure`, every other hard-exclusion cause uses only `exact_hard_exclusion` |
| `terminal_branch_commit` / `sensor_exhausted` | one `{gated, scouting, attesting, viable, deepening} -> exhausted`; cause `no_representable_candidate`, `authoritative_unreachable`, or `evidence_unavailable` | partitioned `A` disposition below | root has no current proof; descendant proof coverage follows the rule below | install exact mapped terminal Route; exact Goal recomputation; `exact_exhaustion` |
| `terminal_branch_commit` / `bound_reached` | one `{gated, scouting, attesting, viable, deepening} -> truncated`; cause `bound_reached`; exact `timed_out\|truncated` sensor-result member | partitioned `A` disposition below | root has no current proof; descendant proof coverage follows the rule below | consume exact private counter/limit/ownership receipt, complete producer cleanup, install exact Route `unknown`, and recompute Goal; `exact_bound_observation` |
| `recoverable_invalidation` / `hard_invalidation` | one `{discovered, gated, scouting, attesting, viable, deepening, ready, denied, exhausted, truncated} -> gated`; exact recoverable cause | partitioned `A` disposition below | if and only if the root owns `current`, map it by the cause function; otherwise no proof member | invalidate any current Route for the root and descendants; exact Goal recomputation; no spawn; `exact_recoverable_invalidation` |
| `proof_failure` / `proof_expired` | proof-owning `ready -> gated` | partitioned `A` disposition below | `current -> expired` for `proof_expired`, or `current -> evidence_stale` for `fact_freshness_expired` | invalidate current Route; exact Goal recomputation; no spawn; `exact_proof_event` |
| `proof_failure` / `proof_validation_failed` | proof-owning `ready -> gated` | partitioned `A` disposition below | `current -> validation_failed`; validation `failed` | invalidate current Route; exact Goal recomputation; no spawn; `exact_proof_event` |
| `proof_failure` / `proof_validation_unavailable` | proof-owning `ready -> gated` | partitioned `A` disposition below | `current -> validation_unavailable`; validation `unavailable` | invalidate current Route; exact Goal recomputation; no spawn; `exact_proof_event` |
| `hard_exclusion` / `hard_excluded` | proof-owning `ready -> excluded`; exact hard-exclusion cause | partitioned `A` disposition below | `current ->` exact cause-mapped non-current state | atomically replace the old safe Route with exact Route `blocked`; exact Goal recomputation; no spawn; action-audit mismatch/replay uses only `exact_action_audit_integrity_failure`, every other hard-exclusion cause uses only `exact_hard_exclusion` |
| `irrelevant_branch_pruning` / `branch_pruned` | one `{discovered, gated, scouting, attesting, viable, deepening, ready} -> cancelled`; cause `proved_irrelevant_to_final_goal` | partitioned `A` disposition below | root `current -> retired` when present; descendant current proofs -> `ancestor_invalidated` | invalidate current Routes for the pruned set, require exhaustive Goal proof that none can change safety/executability/final rank, complete their cleanup, recompute Goal, and require `exact_scheduler_prune` |
| `caller_cancellation` / `caller_cancelled` | every owned nonterminal or `ready` branch -> `cancelled`; `denied`, `excluded`, `exhausted`, `truncated`, and already `cancelled` retain bounded terminal state; `retracted` remains historical | exact complete ownership set; active descendants -> `cancelled`, cause `ancestor` | every `current -> retired` | campaign `{accepted, running} -> cancelled`, invalidate safe current Routes, atomically retire any current Goal projection to exact retained terminal-status history with witness `cleared_to_terminal_status`, construct exact terminal-retention eligibility at this commit, complete cleanup, commit exact owning-engine campaign-summary audit, and require `exact_cancel_request`; later reads consume this receipt and expose no terminal Goal result |
| `engine_loss` / `shutdown_or_engine_lost` | every owned nonterminal or `ready` branch -> `cancelled`; already-terminal/cancelled branches retain bounded evidence; `retracted` remains historical | exact complete ownership set; active descendants -> `cancelled`, cause `ancestor` | every `current -> engine_boot_changed` | campaign `{accepted, running} -> failed`, invalidate safe current Routes, atomically retire any current Goal projection to exact retained terminal-status history with witness `cleared_to_terminal_status`, construct exact terminal-retention eligibility at this commit under the current owner clock, complete cleanup, commit exact owning-engine campaign-summary audit, and require headless `exact_engine_event`; no live client is required and no terminal Goal result is exposed |
| `campaign_failure` / `campaign_failed` | every owned nonterminal or `ready` branch -> `cancelled`; already-terminal/cancelled branches retain bounded evidence; `retracted` remains historical | exact complete ownership set | every `current -> retired` | campaign `{accepted, running} -> failed`, atomically retire any current Goal projection to exact retained terminal-status history with witness `cleared_to_terminal_status`, construct exact terminal-retention eligibility at this commit, complete cleanup, commit exact owning-engine campaign-summary audit, and require headless `exact_failure`; cause `frozen_plan_integrity_failure` instead requires `exact_frozen_plan_integrity_event`, the exact `frozen_plan_integrity` member, and no replacement plan; no terminal Goal result is exposed |
| `campaign_bound_stop` / `bound_reached` | from `running` with at least one owned open branch, each open branch commits terminal `truncated/bound_reached`, all exact terminal Routes enter the final snapshot, and every owned branch ends `retracted`; from `running` with zero open branches, a nonempty all-terminal ownership set, and at least one exact current terminal Route, each terminal branch becomes `retracted`, already `retracted` branches remain unchanged, and the exact nonempty existing terminal Route set and exhaustive terminal Goal are retained unchanged in the final snapshot; from `accepted` only the exact zero-branch/zero-Route/zero-live-resource queue-gated case is legal; from either accepted or running with zero Routes and zero open branches, no synthetic Route is created and terminal campaign evidence is `bound_before_route`; this zero-Route arm is exclusive regardless of cancelled or already-retracted ownership and cannot enter the Route-retention arm; in its running case every owned branch in `{ready, denied, excluded, exhausted, truncated, cancelled}` becomes `retracted`, every already-`retracted` branch remains unchanged, and every Route effect is exactly `none`, while its accepted-empty case has no branch member; an accepted row with any work member rejects this purpose | exact complete ownership set; no branch omitted | every `current -> retired` | consume the exact campaign-decisive total-time receipt selected by `exact_hierarchical_campaign_cut`; for the open arm atomically derive Goal `ready_with_warnings\|unknown` as applicable, for the exclusive zero-Route arm derive Goal `unknown` from `bound_before_route`, and for the Route-bearing all-terminal arm retain the already exhaustive terminal Goal without changing any Route outcome; retain the exact Route/Goal/audit snapshot, campaign `{accepted, running} -> completed`, construct exact terminal-retention eligibility at this commit, complete cleanup, commit exact owning-engine campaign-summary audit, and require `exact_bound_observation`; snapshot plus cleanup is indivisible |
| `campaign_completion` / `campaign_completed` | every owned branch in `{ready, denied, excluded, exhausted, truncated, cancelled} -> retracted`; already `retracted` remains unchanged but covered | exact complete ownership set; at least one terminal Route or a proved representability terminal record is required | every `current -> retired` | campaign `running -> completed`, atomically retain the immutable terminal Goal in the exact campaign-result snapshot, retain final Route/Goal snapshot, construct exact terminal-retention eligibility at this commit, complete cleanup, commit exact owning-engine campaign-summary audit, and require headless `exact_scheduler_completion` plus an exact same-revision hierarchical cut with `total_time=within`; a reached receipt at that cut or an earlier still-current lifecycle revision selects `campaign_bound_stop`, while a receipt after this completion is stale and rejects; later Operation calls consume the completion receipt |
| `legacy_overlay_migration` / `legacy_overlay_migration_committed` | `not_applicable` | `not_applicable` | `not_applicable` | before any snapshot restore/list/status or environment surface becomes available, freeze the authoritative private-store inventory, current persistence/allowlist policy, schema revisions, finite retry budget/backoff revision, and total recovery deadline, then commit one complete migration-member set under exact engine/store policy and pre-action audit authority. Each store proves a complete disjoint record-and-index partition. `legacy_or_unproved_value_bearing` deletes every legacy, literal-redaction-marker, stale-policy, malformed, or unproved record and only its indexes without reading value material. `current_private_value_schema_with_exact_allowed_provenance` preserves byte-untouched opaque records only when current schema, canonical allowlist membership, exact caller-supplied provenance, and complete current indexes are metadata-proved. `mixed_current_and_legacy_or_unproved` atomically performs both effects and proves exact verified-current opaque identity/index equality across the commit; `current_names_only` records an idempotent verified-current result. Audit denial/failure or transaction commit failure accepts no mutation, proves the verified-current opaque identity/index sets unchanged, and leaves the pre-existing default-closed migration gate in force. Crash/restart, unavailable/unknown state, prior quarantine, incomplete/overlapping classification, or record/index coverage failure keeps the store inaccessible. A retryable failure atomically consumes one durable attempts-remaining unit and arms exactly one deduplicated verifier-monotonic not-before/deadline wake; restart resumes the same attempt identity and remaining budget, and no periodic polling is permitted. Zero attempts or the total deadline atomically selects `quarantined_requires_operator_repair`, clears automatic wake state, and cannot retry until a distinct operator-authorized/audited repair creates a new migration generation. Only an all-store exact success changes the versioned migration gate to current; preserved records then remain governed by ordinary operation-matched restore authorization. The public/audit receipt binds inventory/schema/policy/store identities, typed bounded partition counts, result classes, opaque-preservation/deletion-or-quarantine proofs, and retry state--never environment names, values, literal secret-shaped data, or value-derived hashes; correlation `exact_legacy_overlay_migration` |
| `retention_expiry` / `retention_expired` | exact complete campaign ownership closure; every branch/Route/proof/Goal/fact/pending/sensor/output/detail artifact is deleted rather than retained | none; exact artifact coverage proves no omitted descendant or secondary index | no current proof; every non-current proof payload is deleted, retaining only its permitted digest in the tombstone | campaign `{completed, cancelled, failed} -> expired`; exact current-clock `terminal_retention_eligibility/consume_for_expiry` proves its minimum-not-before lease elapsed. Exact `retention_artifact_set/reduce_to_fixed_tombstone` then deletes every heavy campaign-owned artifact, including zero-Route results, terminal sensor/deadline records, and every cancelled/released queue-wait record plus wake/revision index, and `reduce_terminal_to_tombstone` is non-increasing inside the admission reservation. In that same commit, a frozen configured tombstone-minimum duration is converted into a full lease proof containing campaign-owner boot/clock identity, expiry-commit monotonic anchor, remaining duration, and immutable receipt. The fixed-shape tombstone contains campaign id, immutable admission partition, terminal origin, exact bounded key-set root/count, every per-key lease proof, that tombstone-minimum lease, their exact max protected-until proof, terminal/expiry instants, typed unavailable/status code, and cleanup/summary receipt digests--no Route, Goal, fact, raw payload, output, or cursor. Request-key bindings remain in the bounded tombstone index until purge. Age expiry requires headless `exact_retention_timer`; a per-partition or global store ceiling may select only a terminal record whose terminal-retention eligibility is proved elapsed, and requires exact eligible-set digest, deterministic deadline/terminal-time/stable-ID victim selection, the owning-partition receipt, and `exact_retention_partition_bound`; protected, continuity-unproved, mismatch, or cross-partition selection deletes nothing. The owning retention-audit digest is compacted in the same batch |
| `retention_lease_reanchor` / `retention_lease_reanchored` | exact campaign in `{accepted, running, completed, cancelled, failed, expired}` with at least one lease-bearing component: a nonempty alias set, terminal-retention eligibility, or an expired tombstone minimum; its bounded request-key set is exact complete and may be empty; no lifecycle or artifact deletion | none | none | campaign state remains unchanged. When lease-clock continuity is unproved, one headless batch consumes the exact old boot/clock evidence and applies `reanchor_lease` to every still-bound alias using the conservative full remaining configured window under the current campaign-owner monotonic clock. A terminal pre-expiry campaign also applies `terminal_retention_eligibility/reanchor_conservatively`; an expired campaign instead reanchors the tombstone minimum. Only applicable components are reanchored, and at least one must exist. The batch proves every retained component comparable in one current clock domain, recomputes the applicable max protected-until proof, records `retention_audit/record_lease_reanchor`, and emits `exact_retention_lease_reanchor`. Matching-key Operation responses and retention expiry/purge are gated until this commit. Capacity counters are unchanged and serialized size cannot grow. Missing/extra keys, partial re-anchor, mixed clock domains, wall-clock inference, lifecycle change, or any deletion rejects |
| `retention_purge` / `retention_purged` | exact expired tombstone ownership closure; no heavy campaign artifact may remain | none | none | campaign `expired -> absent`; exact `purge_eligibility/authorize_purge` MUST prove complete key-set root/count coverage and elapsed `max(all per-key leases, tombstone minimum)`. In the same batch, `retention_artifact_set/delete_tombstone_keys_and_indexes` deletes the tombstone, every request-key forward/reverse binding, and every remaining campaign/secondary index; `purge_tombstone_keys_and_indexes` releases their reserved counters. A non-final purge has partition delta zero. Exactly when a same-revision final-record/refcount witness proves the post-delete partition empty, the batch applies partition delta minus one and `fold_final_partition_and_delete` atomically folds the terminal digest into the bounded global accumulator and deletes all per-partition audit/metadata; any other minus-one delta rejects. A key is unbound only after this commit. Timer purge uses `exact_tombstone_timer` plus `exact_purge_eligibility`; per-partition/global ceilings additionally require exact eligible-set digest, deterministic protected-until/terminal-time/stable-partition-and-campaign-ID victim selection, owning-partition receipt, and `exact_tombstone_partition_bound`. Protected, continuity-unproved, unknown, mismatch, non-max, replayed, or cross-partition proof deletes nothing; `absent` is not retained state |

The table's "other mandatory members" column is conjunctive with the
`Queue-wait coverage witness`; it is never optional shorthand. `route_proof_commit`,
every terminal-branch/proof/invalidation/pruning row, every campaign terminal or
engine-loss row, and every transition that removes or replaces a queued sensor edge
must carry `exact_complete_owned_set\|exact_empty_owned_set`. Every applicable
`pending\|released` wait becomes `cancelled` in that same batch. Retention expiry
instead consumes the exact complete retained wait set with `delete_retained_wait`.
A missing/extra/duplicate wait or wake/revision index rejects ordinary progress;
safety cleanup may force-cancel by owner identity while recording the integrity
failure, but may never leave a runnable orphan or claim that generic cleanup changed
durable wait state.

Cause selection is a closed function; the phrases "recoverable cause" and "hard
exclusion cause" in the rows above mean exactly this partition:

| Cause partition | Sole accepted purpose/event use |
|---|---|
| `none` | simple branch progress, sensor admission, ordinary trusted-fact or non-bound sensor-result commit subrows, route proof commit, campaign admission, request-key binding, campaign start, store-level legacy-overlay migration, and campaign-wide caller cancellation/failure/completion/retention-expiry/lease-reanchor/purge whose exact causal receipt and event are the cause; never an invalidation, exclusion, or exhaustion row |
| `ancestor` | descendant member only inside root-scoped terminal/invalidation/proof-failure/hard-exclusion/pruning or campaign-wide cancellation/engine-loss/failure; never the selecting root cause |
| `proved_irrelevant_to_final_goal` | only `irrelevant_branch_pruning/branch_pruned` with `exact_scheduler_prune` |
| `policy_decision` | only `terminal_branch_commit/policy_denied` |
| `policy_revision`, `connector_instance_change_approved`, `target_persistent_identity_change_approved`, `target_boot_change_approved`, `workspace_change_approved`, `cwd`, `action_schema_or_digest`, `catalogue_trust_revision`, `substitution_outside_envelope`, `fact_superseded_by_grade`, `fact_superseded_by_freshness`, `equal_grade_fresh_conflict` | only `recoverable_invalidation/hard_invalidation`; `catalogue_trust_revision` re-gates executable/sensor trust while retaining the exact frozen plan; any current proof becomes the exact cause-mapped non-current state and descendants use `ancestor` |
| `frozen_plan_integrity_failure` | only `campaign_failure/campaign_failed` with exact frozen-plan snapshot identity, expected/observed digest class, no replacement plan, and `exact_frozen_plan_integrity_event` |
| `deadline_registration_integrity_failure` | only `campaign_failure/campaign_failed`; exact owner-set anomaly receipt, forced idempotent disarm, every live sensor invocation terminalized, and complete cleanup are mandatory. It never claims a deadline was reached |
| `target_persistent_identity_mismatch`, `target_boot_mismatch`, `workspace_mismatch`, `architecture_or_requirement_mismatch`, `connector_replay_detected`, `connector_unapproved_rotation`, `connector_clone_detected`, `connector_binding_mismatch`, `connector_mid_run_replacement`, `action_audit_mismatch`, `action_audit_replay`, `route_cycle_detected`, `required_prerequisite_missing` | `hard_exclusion/hard_excluded` exactly for a `ready` root with current proof; otherwise `terminal_branch_commit/hard_excluded` for the listed non-ready prior states; never recoverable. The two action-audit causes require the exact non-authorizing Security integrity-failure receipt and `exact_action_audit_integrity_failure`; they can never use `policy_decision` or a policy-denial witness |
| `no_representable_candidate`, `authoritative_unreachable`, `evidence_unavailable` | only `terminal_branch_commit/sensor_exhausted` |
| `bound_reached` | `queue_wait_registration/capacity_wait_registered` for an active or sensor-admission queue gate; `sensor_observation_commit/sensor_private_input_stopped` for a single private resolver/decoder input stop; `terminal_branch_commit/bound_reached` for a single branch-decisive private bound receipt; the exact paired `soft_evidence_bound_commit/bound_reached` staging batch then `soft_evidence_bound_finalize/soft_bound_finalized` publication batch for a single soft/informational evidence-only relevance; campaign-wide `campaign_bound_stop/bound_reached` for one campaign-decisive receipt; or `bound_effect_bundle_commit/bound_effect_bundle_committed` for the exact closed multi-reach map. Scope/role mismatch, missing bundle/pair identity, partial subeffect commit, or duplicate finalization rejects |
| `proof_expired` | `proof_failure/proof_expired`, `current -> expired`, for a proof-owning ready root; otherwise `recoverable_invalidation/hard_invalidation` re-gates the pre-ready or non-proof root without fabricating a proof |
| `proof_validation_failed` | only `proof_failure/proof_validation_failed`, `current -> validation_failed` |
| `proof_validation_unavailable` | only `proof_failure/proof_validation_unavailable`, `current -> validation_unavailable` |
| `fact_freshness_expired` | `proof_failure/proof_expired` with `current -> evidence_stale` for a proof-owning ready root; otherwise `recoverable_invalidation/hard_invalidation` for a pre-ready or non-proof root |
| `engine_boot` | only campaign-wide `engine_loss/shutdown_or_engine_lost` with `exact_engine_event`; never a root recoverable row |

Every cause/purpose/event/state combination outside this table is rejected before
member effects are evaluated. Grade/freshness supersession and equal-grade fresh
conflict additionally require the exact old/new fact members and derived-fact
removal in the same batch; a caller label cannot assert the cause.

Member projection is exact for that cause partition. Every
policy/catalogue-trust-security/cwd/action revision and every
connector/target/boot/workspace change carries the
corresponding `authoritative_context` old/new member with its pre-Route
disposition and exact engine-derived revision receipt. Every
`fact_superseded_by_grade`, `fact_superseded_by_freshness`,
`equal_grade_fresh_conflict`, or `fact_freshness_expired` batch carries all
affected old/new `source_fact` members, every transitively dependent
`derived_fact` recompute/removal, and dependency coverage
`exact_transitive_closure`. Sensor observation and soft/informational bound
commits use their exact admission rows below. All other unrelated cause families
require these members and coverage `not_applicable`. Missing, extra, stale, contradictory, or caller-
asserted change/fact members reject the whole batch. Thus a cause label cannot
stand in for the authoritative state change it claims.

Fact member effects are not caller choices:

| Cause family | Exact source-fact effects | Exact derived-fact effects |
|---|---|---|
| `sensor_observation_commit` with `trusted_fact` | every exact newly admitted source `add`; existing independent sources `retain`; no unadmitted source | recompute the complete dependent closure and retain unrelated values under `exact_transitive_closure` |
| `sensor_observation_commit` with any non-fact terminal result | source and derived members plus dependency coverage `not_applicable`; the durable sensor-result and cleanup members are not facts | `not_applicable` |
| `soft_evidence_bound_commit` | `stage_pending` only the exact conditionally admitted soft/informational incomplete observation carrying the identical bound marker; retain independent public sources; staged source remains inert | derived members and dependency coverage `not_applicable`; no closure or Goal snapshot is staged |
| `soft_evidence_bound_finalize` | `publish_pending` exactly the staged source set after matching final boundary receipt; no add or second admission | recompute the complete dependent closure and Goal from the exact then-current public records under `exact_transitive_closure`; retain unrelated public conclusions |
| `bound_effect_bundle_commit` | stage as one pending set exactly the unsuppressed compatible soft/informational sources selected by the dominance map; safety-subsumed or private-stopped candidates have no source member. When the map has no unsuppressed soft/informational effect, source facts and dependency coverage are `not_applicable` | derived members and dependency coverage `not_applicable` until the one pending finalizer; branch/campaign/queue/output/private-only bundles never stage a derived closure |
| `fact_superseded_by_grade` or `fact_superseded_by_freshness` | losing old source `supersede`; newly admitted winning source `add`; unrelated sources `retain` | remove every value derived from the loser, recompute from the winner, retain unrelated closure |
| `equal_grade_fresh_conflict` | prior source `retain`; contradictory newly admitted source `add`; neither is a winner | remove any single-valued conclusion and recompute the typed conflict/unknown closure |
| `fact_freshness_expired` | every expired contributing source `invalidate`; current independent sources `retain` | remove stale conclusions and recompute the remaining/unknown closure |
| governing policy/catalogue-trust-security/cwd/action/connector/identity/boot/workspace change | invalidate exactly the sources whose receipts depend on the changed context; retain proved-independent sources | remove/recompute exactly the transitive dependent closure |
| `retention_expiry` | every campaign-owned source `delete_owned`; a separately shared source survives only through an exact owner/refcount transfer to its independently bounded owner | every campaign-owned derived value `delete_owned`; no Goal recomputation occurs because the terminal result is being reduced to a tombstone |
| `retention_purge` | source members and dependency coverage `not_applicable`, with artifact coverage proving expiry left none | derived members `not_applicable`; delete only the fixed tombstone/key/index remainder |
| every other unrelated cause | source/derived members and dependency coverage `not_applicable` | `not_applicable` |

A later hard exclusion that is not itself an authoritative-context or fact
change cannot invalidate or demote an unchanged source/topology record. Its
source/derived members and dependency coverage remain `not_applicable`, so the
unchanged current store is the post-batch closure input. When the exact accepted
cause does carry an authoritative-context or fact change, every pre-batch member
uses its own closed effect domain: the authoritative-context member uses its
exact `replace_authoritative\|hard_exclude` effect; each source-fact member uses
exactly its required `retain\|invalidate`, with `retain` preserving the same
stable identity and canonical evidence digest; each dependent derived-fact
member uses exact `remove\|recompute`; and every proved-independent derived fact
uses `retain`. Invalidation/removal covers every and only dependent member, and
derived records are recomputed from the resulting complete current source set
plus the exact replacement context. Topology is re-derived from those context
and branch members and is not an independent effect member. The temporal
projection admits no source `add`.

Any `retain` where supersede/invalidate is required, any changed retained
identity/digest, any omitted or independently invalidated member, any
recomputation from a removed source, any unproved dependency root/count, or any
missing/extra/duplicate effect rejects the batch. Only the prior Route record is
replaced; it cannot supply a current closure member.

For every root-scoped row, partitioned descendant disposition is exact:
`A_active -> cancelled` with cause `ancestor`; `A_terminal -> retracted`, using
Route effect `retire_to_historical` when it owns a current Route and `none` when
it owns only a cancellation reason. Retirement atomically removes the current
Route and appends the same bounded evidence to the batch's immutable
`audit_history` member before Goal recomputation. Every such batch requires
`Audit-history witness = exact_append_set`; missing, duplicate, or mismatched
history rejects. `A_historical` remains `retracted` with
no state mutation and no current Route/proof. All three partitions remain present
in descendant coverage. No already-terminal or historical descendant is rewritten
to `cancelled`, and no member is omitted merely because it does not transition.

For every root-scoped terminal commit, invalidation, proof failure, or hard
exclusion, proof and Route coverage follows branch membership, not merely the
named root. Each affected branch that owns a `current` proof contributes exactly
one proof member. The root maps by the row's exact cause; every descendant
cancelled for `ancestor` maps `current -> ancestor_invalidated`. Each such branch
also contributes `invalidate_current` for its current Route. Campaign-wide
cancellation, engine loss, failure, and completion instead apply their explicit
row-wide proof mapping to every current proof. A missing proof/Route member, an
extra proof for a branch without current proof, or members with different
transaction/cause identities rejects the whole batch.

Every root-scoped route-proof, terminal, invalidation, proof-failure,
hard-exclusion, or pruning batch requires
`Cleanup witness = complete_no_live_resources` over the exact root plus affected
descendant ownership set before receipt publication, even when `A` is empty or
the resource set is proved empty. The set includes every process, PTY, watch,
subscription, sensor, temporary file, output handle, and runtime registration.
Missing, partial, duplicate, or extra resource coverage rejects the batch; no
deferred cleanup transition can repair an accepted incomplete batch. Simple
branch progress/admission/start/key-binding rows alone use cleanup
`not_applicable`; campaign-wide rows apply their explicit complete-cleanup rule.

Any accepted cleanup batch that stops an admitted active ordinary sensor not
already carrying its terminal producer/bound result MUST include exactly one
terminal `sensor_result` member for that sensor in the same batch. Root-scoped
terminalization, invalidation, proof failure, hard exclusion, or pruning uses
`cancelled_by_branch_transition`; caller cancellation, campaign failure, or
campaign bound stop uses `cancelled_by_campaign_transition`; engine loss uses
`interrupted_by_engine_loss`. Each member binds the batch's exact purpose, event,
cause, transaction identity, and cleanup resource member and carries no fact or
fallback effect beyond the branch/campaign effects already selected by that
winning transition. Campaign completion instead requires its existing proved-empty
live-resource witness and rejects an active sensor. Missing, duplicate, late, or
cross-batch terminal disposition rejects, so cleanup can never orphan an admitted
sensor or permit a second result.

Campaign completion or campaign-bound stop is accepted only with a
machine-checkable completion witness:
the exhaustive Goal record is terminal (never `in_progress`), its open frontier
is empty, every owned branch appears exactly once in the cleanup set, there are
no live jobs/PTYs/watches/subscriptions/sensors for the campaign, the final result
and audit receipt committed durably, and the retained snapshot exactly matches
the pre-cleanup current Route and Goal records. The snapshot is historical only;
`retired` proof and `retracted` branches can never authorize a later dispatch.
A later environment request starts a new campaign and creates a new proof.

The proof invalidation function is exact:

| Cause | Proof to-state |
|---|---|
| `ancestor` | `ancestor_invalidated` |
| `policy_revision` | `policy_changed` |
| `connector_instance_change_approved` | `connector_changed` |
| `target_persistent_identity_change_approved` | `target_persistent_identity_changed` |
| `target_persistent_identity_mismatch` | `target_persistent_identity_mismatch` |
| `target_boot_change_approved` | `target_boot_changed` |
| `target_boot_mismatch` | `target_boot_identity_mismatch` |
| `workspace_change_approved` | `workspace_changed` |
| `workspace_mismatch` | `workspace_mismatch` |
| `connector_replay_detected` | `connector_replay_detected` |
| `connector_unapproved_rotation` | `connector_unapproved_rotation` |
| `connector_clone_detected` | `connector_clone_detected` |
| `connector_binding_mismatch` | `connector_binding_mismatch` |
| `connector_mid_run_replacement` | `connector_mid_run_replacement` |
| `action_audit_mismatch` | `action_audit_mismatch` |
| `action_audit_replay` | `action_audit_replay` |
| `architecture_or_requirement_mismatch` | `requirement_mismatch` |
| `required_prerequisite_missing` | `requirement_mismatch` |
| `route_cycle_detected` | `route_cycle_detected` |
| `proof_expired` | `expired` |
| `proof_validation_failed` | `validation_failed` |
| `proof_validation_unavailable` | `validation_unavailable` |
| `fact_freshness_expired` | `evidence_stale` |
| `fact_superseded_by_grade` | `evidence_stale` |
| `fact_superseded_by_freshness` | `evidence_stale` |
| `equal_grade_fresh_conflict` | `evidence_stale` |
| `cwd` | `cwd_changed` |
| `action_schema_or_digest` | `action_schema_or_digest_changed` |
| `catalogue_trust_revision` | `catalogue_trust_changed` |
| `substitution_outside_envelope` | `outside_substitution` |
| `engine_boot` | `engine_boot_changed` |

Every cause that changes proof, branch, Route, Goal, campaign, or operation state
exists only in its complete batch row. Approved/recoverable causes re-gate the
root and cancel its full discovered-descendant set; hard exclusions replace the
root Route and cancel descendants; engine loss cancels all owned work; and
proof/freshness/validator failures re-gate the proof owner. The batch emits the
exact typed reason (including `proof_stale` where applicable) and permits no
spawn. No proof, branch, Route, Goal, campaign, cleanup, or operation-trace half
is accepted alone. Confirmed FR-062 security exclusions never enter the
recoverable path.

Every composition not listed is `rejected`; legal member adjacency with the wrong
event, cause, scope, coverage, witness, or correlation is still rejected.
Route effects are total per affected branch. Simple progress, admission, start,
sensor-result commit, and both soft-bound staging/finalization batches derive
`none`. Route proof derives root `install_terminal`. A terminal
commit derives root `install_terminal` or the atomic invalidation-plus-install
effect `replace_terminal` according to whether a current row exists, plus
`invalidate_current` for every `A_active` descendant current Route and
`retire_to_historical` for every `A_terminal` Route. Recoverable/proof
invalidation derives `invalidate_current` for the root and `A_active` prior
Routes, plus `retire_to_historical` for `A_terminal`. Hard
exclusion derives root `replace_terminal` when a prior row exists (otherwise
`install_terminal`), `invalidate_current` for `A_active` current Routes, and
`retire_to_historical` for `A_terminal`. Irrelevant-branch pruning derives
`invalidate_current` for root/`A_active` current Routes, `none` for those without
one, and `retire_to_historical` for `A_terminal`; historical terminal evidence is
retained only in the Goal/campaign audit snapshot.
Cancellation, engine loss, and failure assign `invalidate_current` to every safe
current Route and `retain_final_snapshot` to every bounded non-safe terminal
Route; active branches without a Route receive `none`; their campaign-result
member atomically retires any current Goal into terminal-status history.
Campaign bound stop assigns `install_and_retain_terminal_snapshot` to each open
branch newly closed as truncated, `retain_final_snapshot` to every existing
ready/terminal Route, and `none` to already-historical branches; its
campaign-result member retains the terminal Goal even when there is no Route.
Completion derives `retain_final_snapshot` for every final Route record and
retains the campaign-level terminal Goal. Retention expiry derives
`delete_retained_snapshot` for each retained Route plus the declared campaign-
result deletion/tombstone effect; zero Route records do not waive that member.
Unaffected branches derive `none`. Any missing or contradictory effect record
rejects the batch. Every active branch/Route change regenerates exhaustive
open-branch roles and the current Goal before another sensor can start; terminal
cancel/failure clears that projection instead, and expiry deletes/reduces the
retained campaign snapshot exactly as its row declares.

An accepted terminal Route-effect record is not permission to invent a Route
row. The accepted batch that created the terminal state supplies its state and
cause through this exact bridge:

| Terminal state | Accepted prior state | Creating event/cause | Route terminal cause |
|---|---|---|---|
| `ready` | `viable`, `deepening` | `route_proved` / `none` | `route_proved` |
| `denied` | `gated`, `scouting`, `attesting`, `viable`, `deepening` | `policy_denied` / `policy_decision` | `policy_denial` |
| `excluded` | `discovered`, `gated`, `scouting`, `attesting`, `viable`, `deepening`, `ready`, `denied`, `exhausted`, `truncated` | `hard_excluded` / its accepted exclusion cause | that same exclusion cause |
| `exhausted` | `gated`, `scouting`, `attesting`, `viable`, `deepening` | `sensor_exhausted` / its accepted exhaustion cause | that same exhaustion cause |
| `truncated` | `discovered`, `gated`, `scouting`, `attesting`, `viable`, `deepening` | `bound_reached` / `bound_reached`; `discovered` is accepted only inside `campaign_bound_stop` | `bound_reached` |

The resulting Route record is valid only for one of these exact evidence
projections. A set in a cell is an enumerated set, not a wildcard. For every
later-terminal temporal row, "exact post-batch closure" means the fresh accepted
Transition's complete per-record effect partition is evaluated in that same
atomic batch and defines its post-batch state; every
retained source matches its pre-batch stable identity and canonical evidence
digest; every authorized invalidation and recomputation has exact dependency
root/count coverage; and the complete current source/topology/derived set is
bound to the same atomic snapshot under one `fresh_unique` Transition transaction
identity. Stale, invalidated, provenance-only, unadmitted,
missing, extra, duplicate, or hybrid-revision records are ineligible. The
ordinary winner order is then derived over precisely that closure; neither the
old Route nor an asserted replacement outcome is an input.

| Terminal prior/state/cause | Derived joint winner | Exact current evidence constraints | Proof state | Derived Route outcome |
|---|---|---|---|---|
| `{viable, deepening}` / `ready` / `route_proved` | fully safe | hard `all_satisfied`; hard observation `complete`; hard freshness `fresh`; soft `all_satisfied\|not_applicable` | `current` | `ready` |
| `{viable, deepening}` / `ready` / `route_proved` | hard-safe soft warning | same exact hard-safe tuple; soft `non_satisfied\|unknown` | `current` | `ready_with_warnings` |
| `{gated, scouting, attesting, viable, deepening}` / `denied` / `policy_denial` | denied | no unsupported record; hard aggregate `denied`; hard observation `complete\|probe_failed\|timed_out\|truncated`; hard freshness any derived value; soft aggregate any derived value | `missing` or immutable pre-existing non-current proof | `denied` |
| prior `{denied, exhausted, truncated}` / `excluded` / accepted hard-exclusion cause | current post-batch winner `unsupported` | exact post-batch closure; at least one current hard representability/observation unsupported record; every subordinate current record admitted; exact new exclusion receipt/cause | `missing` or immutable pre-existing non-current proof | `unsupported` |
| prior `{denied, exhausted, truncated}` / `excluded` / accepted hard-exclusion cause | current post-batch winner `denied` | exact post-batch closure; no current unsupported record; current hard aggregate `denied`; every subordinate current record admitted; exact new exclusion receipt/cause | `missing` or immutable pre-existing non-current proof | `denied` |
| prior `{denied, exhausted, truncated}` / `excluded` / accepted hard-exclusion cause | current post-batch winner `unreachable` | exact post-batch closure; no current unsupported/denied winner; current hard aggregate `unreachable`; every subordinate current record admitted; exact new exclusion receipt/cause | `missing` or immutable pre-existing non-current proof | `unreachable` |
| prior `{denied, exhausted, truncated}` / `excluded` / accepted hard-exclusion cause | blocked | exact post-batch closure; no current unsupported/denied/unreachable winner; exact current accepted hard-exclusion receipt/cause | `missing` or immutable pre-existing non-current proof | `blocked` |
| `{discovered, gated, scouting, attesting, viable, deepening}` / `excluded` / accepted hard-exclusion cause | blocked | no unsupported/denied/unreachable winner; hard aggregate `missing\|incompatible`; hard observation `complete\|probe_failed\|timed_out\|truncated`; hard freshness any derived value; soft aggregate any derived value | `missing` or immutable pre-existing non-current proof | `blocked` |
| `ready` / `excluded` / accepted hard-exclusion cause | blocked | same blocked subordinate set; hard freshness includes `invalidated` | exact cause-to-proof mapping below | `blocked` |
| `exhausted` / `no_representable_candidate` | unsupported | at least one hard representability/observation unsupported record; every subordinate hard/soft aggregate, observation, and freshness value admitted | `missing` or immutable pre-existing non-current proof | `unsupported` |
| `exhausted` / `authoritative_unreachable` | unreachable | no unsupported/denied winner; hard aggregate `unreachable`; hard observation `complete\|probe_failed\|timed_out\|truncated`; hard freshness any derived value; soft aggregate any derived value | `missing` or immutable pre-existing non-current proof | `unreachable` |
| `exhausted` / `evidence_unavailable` | unknown | no higher winner; hard aggregate `unknown\|all_satisfied\|not_applicable`, or hard observation `probe_failed`, or hard freshness `invalidated\|fresh_required_but_unavailable\|stale`; soft aggregate any derived value | `missing` or immutable pre-existing non-current proof | `unknown` |
| `truncated` / `bound_reached` | unknown | no higher winner; hard observation `timed_out\|truncated` or hard freshness non-fresh; soft aggregate any derived value | `missing` or immutable pre-existing non-current proof | `unknown` |

The exact exclusion proof mapping is
`connector_replay_detected -> connector_replay_detected`,
`connector_unapproved_rotation -> connector_unapproved_rotation`,
`connector_clone_detected -> connector_clone_detected`,
`connector_binding_mismatch -> connector_binding_mismatch`,
`connector_mid_run_replacement -> connector_mid_run_replacement`,
`action_audit_mismatch -> action_audit_mismatch`,
`action_audit_replay -> action_audit_replay`,
`target_persistent_identity_mismatch -> target_persistent_identity_mismatch`,
`target_boot_mismatch -> target_boot_identity_mismatch`, and
`workspace_mismatch -> workspace_mismatch`,
`architecture_or_requirement_mismatch -> requirement_mismatch`,
`required_prerequisite_missing -> requirement_mismatch`, and
`route_cycle_detected -> route_cycle_detected`. Every other terminal
state/cause/evidence tuple is a rejected checker constraint and cannot enter Goal
aggregation. Each branch has at most one current Route record. A later accepted
terminal transition atomically invalidates and replaces that branch's prior Route
record before the Goal presence vector is recomputed; superseded `ready` evidence
cannot coexist with a later `excluded`, `denied`, `exhausted`, or `truncated`
record from the same branch.
An `excluded` transition from prior `denied\|exhausted\|truncated` supersedes that
prior terminal Route in the same batch; the old Route record remains provenance
only and cannot enter the current closure, winner, or recomputed Goal presence
vector. Every retained source/topology member matches its own pre-batch stable
identity and canonical evidence digest; every invalidated member is absent from
the current closure. Derived records may be newly recomputed only from the exact
retained current sources and accepted replacement context under complete
dependency coverage. The replacement Route's evidence digest is recomputed from
that current closure and cannot reuse the superseded digest after a member subset
or winner class changes.

## OperationModel Domains

### Invalid-request recovery pre-admission submodel

FR-007 is classified before the ordinary OperationModel axes exist. This
submodel accepts only requests rejected for an invalid goal, plan, field, or
action on the compact, full, or embedded surface. It is total over the finite
product below and is exclusive with campaign admission.

| Recovery axis | Required values |
|---|---|
| Surface | `compact_environment`, `full_environment_probe`, `embedded_facade` |
| Invalid-request class | `invalid_goal`, `invalid_plan`, `invalid_or_forbidden_field`, `invalid_or_unavailable_action` |
| Versioned recovery-catalogue proof | `exact_current_surface_schema_and_plan_head`, `missing`, `mismatch`, `stale`, `unproved` |
| Supported-choice namespace | `goals`, `plans`, `fields`, `actions` |
| Supported-choice encoding | `exact_complete_within_cap`, `exact_prefix_at_cap_with_total_and_omitted_count`, `empty`, `over_cap`, `duplicate_or_unsorted`, `unproved` |
| Corrective call | `exact_schema_valid_current_surface`, `wrong_surface`, `schema_invalid`, `advertises_unsupported_choice`, `absent` |
| Pre-admission effects | `none`, `campaign_or_job_created`, `registry_or_session_mutated`, `external_observation_started`, `authority_changed`, `unproved` |

The invalid class determines exactly one namespace: goal -> goals, plan ->
plans, field -> fields, and action -> actions. Choices are synthesized from the
same immutable versioned schema and frozen plan-head snapshot that validates
the request, deduplicated and canonically sorted. If the complete set exceeds
the public response cap, the bounded prefix carries the exact total,
omitted-count, and at least one supported choice without allocating cursor
state. A hard-coded or stale example is never accepted. The corrective call is
generated from that same schema, selects one
advertised supported choice (or removes the invalid field), and is
schema-validated on the current surface before publication.

Classification priority is closed and disjoint:

1. Any pre-admission effect other than `none` derives
   `pre_admission_safety_violation`. No recovery response may hide it.
2. A non-exact recovery-catalogue proof derives `recovery_contract_failure`;
   stale choices are never presented as current.
3. A wrong namespace, `empty\|over_cap\|duplicate_or_unsorted\|unproved` choice
   encoding, or any corrective-call value other than
   `exact_schema_valid_current_surface` derives `recovery_contract_failure`.
4. Every remaining assignment derives `invalid_request_recovery` with the
   typed invalid class, bounded supported choices, and the one validated
   corrective call.

Neither failure nor successful recovery allocates a campaign identifier,
request-key binding, job, PTY, watch, deadline, capacity slot, continuation
state, or audit authority. The caller's next corrective call is a new request
and enters normal admission only after independent schema and policy checks.
`legacy_system_discover`, `legacy_target_list`, and `legacy_target_probe`
compatibility are outside this three-surface submodel and cannot be used to
weaken it; their invalid requests remain governed by their existing versioned
surface contracts until an explicit migration adds closed recovery rows.

### Admitted-operation submodel

| Input axis | Required values |
|---|---|
| Requested operation | `start`, `resume`, `detail`, `cancel` |
| Campaign state before | `none`, `accepted`, `running`, `completed`, `cancelled`, `failed`, `expired` |
| Campaign reference lookup | `not_requested`, `found`, `not_found`; `not_found` is a store lookup result after purge, not a lifecycle state |
| Registry transaction snapshot | `exact_same_store_revision`, `changed_before_commit`, `missing`, `mismatch`, `replayed`; the exact receipt binds one serializable read/CAS snapshot of campaign-id lookup, request-key forward and reverse lookup, tombstone terminal origin, per-key remaining resolution lease, immutable admission partition, and every capacity-ledger counter used by the decision |
| Idempotency-lease clock receipt | `exact_current_clock`, `exact_reanchor_committed_same_revision`, `continuity_unproved`, `missing`, `mismatch`, `replayed`, `not_applicable`; every successful keyed commit and every authorized matching-key resolution requires one exact value binding the complete key set, current owner boot/clock, and registry snapshot. A different/unproved persisted boot requires the durable no-lifecycle-change `retention_lease_reanchor` before Operation classification; the response may never compute a horizon from wall time or stale clock state |
| Engine continuity | `current`, `lost`, `replaced`, `not_applicable`; only an exact `not_found` row may use `not_applicable` |
| Observable request context | `start_request`, `resume_same_peer`, `resume_other_peer`, `detail_same_peer`, `detail_other_peer`, `cancel_requested` |
| Request key | `present_new`, `present_matching`, `present_conflicting`, `absent` |
| Attachment authorization | `same_authorized_peer`, `different_authorized_peer`, `unauthorized`, `policy_changed`, `identity_changed`, `not_requested` |
| Concurrency relation | `single`, `identical_shareable`, `same_shape_different_tenant_or_peer`, `same_shape_different_normalized_goal`, `same_shape_different_policy`, `same_shape_different_topology_revision_or_route`, `same_shape_different_target_or_boot`, `same_shape_different_workspace`, `same_shape_different_plan`, `same_shape_different_sensor_revision`, `same_shape_different_requested_detail`, `same_shape_different_freshness_or_bounds`, `not_applicable` |
| Frozen shareability digest | exact equality or one typed mismatch for tenant/peer scope, normalized goal, topology and route revision, policy revision, target persistent/boot identity, canonical workspace, admitted frozen plan snapshot/digest, sensor/catalogue revision, requested detail, freshness, or every bound; `not_applicable` only when concurrency is `single\|not_applicable` |
| Independent admission authority | `allowed`, `denied`, `unavailable`, `not_applicable` |
| Durable admission commit | `committed`, `failed`, `unavailable`, `not_attempted`, `not_applicable` |
| Durable cancellation commit | `committed`, `proved_not_committed`, `unavailable`, `not_attempted`, `not_applicable`; active authorized cancel alone may use a non-`not_applicable` value. `committed` binds the exact caller-cancellation Transition identity, pre-state, post-state `cancelled`, campaign/store revision, and request; `proved_not_committed\|unavailable` proves no cancellation mutation |
| Admission-capacity reservation receipt | `exact_reserved_all_caps`, `reached_no_reservation`, `missing`, `mismatch`, `replayed`, `not_applicable`; every new/independent start reserves bounded per-request and daemon-wide queued-campaign capacity, one maximum-footprint run/lifecycle registry slot, and a retention-partition slot iff the immutable admission partition is new. Keyed starts additionally reserve one alias and forward/reverse key-index records/bytes. Active capacity is deliberately absent and is CAS-gated only by `campaign_start`; an active-campaign alias reserves its alias/index deltas plus the exact maximum current-to-terminal/tombstone run-registry, retained-byte, and key-proof delta caused by that alias. One engine-derived aggregate receipt binds its exact request-or-campaign scope, serialized maximum sizes, and every partition/global subreceipt; it commits atomically with admission/key binding or not at all |
| Bounded start-wait outcome | `not_requested`, `active_accepted`, `active_running`, `completed_with_result`, `cancelled`, `failed`, `expired` |
| Active-wait observation receipt | `exact_accepted_no_mutation`, `exact_running_no_mutation`, `exact_running_after_start_transition`, `missing`, `mismatch`, `not_applicable` |
| Observed wait terminal origin | `completed`, `cancelled`, `failed`, `not_applicable` |
| Retained result state | `available`, `unavailable`, `not_applicable` |
| Expired terminal origin | `completed`, `cancelled`, `failed`, `not_applicable` |
| Admission-batch correlation | `exact`, `missing`, `mismatch`, `not_applicable` |
| Observed lifecycle-trace correlation | `exact_complete_ordered_set`, `missing`, `mismatch`, `not_applicable` |
| Private bound-observation input | `exact_queue_gate`, `exact_output_continuation`, `exact_queue_and_output`, `missing`, `mismatch`, `replayed`, `not_applicable` |
| Bound-effect application correlation | `exact_applied`, `missing`, `mismatch`, `not_applicable` |
| Queue wake registration | `exact_deduplicated_capacity_release_or_deadline_signal`, `missing`, `mismatch`, `replayed`, `not_applicable`; exact only for a queue gate and projects an already committed Transition-owned `queue_wait_registration` receipt binding campaign, pending sensor edge when present, capacity-ledger revision, and total-time deadline. Operation never creates or owns this state. It is signal-driven with one bounded deadline fail-safe, never a polling loop |

Every public Operation classification requires registry snapshot
`exact_same_store_revision`. `changed_before_commit` is an internal serializable-CAS
retry signal, not a stale public classification: it starts/mutates nothing and is
retried only within the frozen request time/attempt bound. Exhaustion returns the
complete typed store-unavailable envelope before OperationModel is constructed.
`missing\|mismatch\|replayed` is an integrity rejection. Admission, alias binding,
retention expiry/re-anchor/purge, and reads linearize on this same store revision;
no clause may combine projections from different revisions.

Retained result `available\|unavailable` is valid only with a completed campaign,
`completed_with_result`, or wait `expired` with observed origin `completed`;
every other active/non-completing row requires `not_applicable`.
`completed_with_result` requires `available`; expired wait from completed
requires `unavailable`. A new or
independent admission requires prior campaign state `none`; same-campaign
admission requires a non-`none` prior state. These are checker constraints, not
values an implementation may silently normalize. Expired terminal origin is
non-`not_applicable` exactly for campaign state `expired` and preserves its
bounded tombstone; every other state requires `not_applicable`. Durable admission
commit is non-`not_applicable` for every new/independent campaign attempt,
keyed or keyless, and for an authorized `present_new` alias-binding attempt.
Admission-batch correlation is `exact` only for a committed batch and otherwise
`not_applicable`. Observed wait terminal
origin is non-`not_applicable` exactly for wait outcome `expired`; it names the
terminal transition observed during that bounded wait and is independent of a
prior expired tombstone. A `committed` admission entails correlation `exact`;
`failed\|unavailable\|not_attempted` entails correlation `not_applicable`.
Campaign reference lookup is `found` for every existing-campaign path and
`not_requested` for a new/independent start with no explicit campaign id. Exact
`not_found` requires prior state `none`, continuity/attachment/retained-result/
expiry/wait/active/lifecycle/admission/bound axes `not_applicable` or their neutral
shape, and can never be normalized into a new campaign while the deleted id remains
in the request. A purged request key alone is simply `present_new`; with lookup
`not_requested` it may start a new campaign under a new bounded horizon.
The exact snapshot makes the purge boundary closed: before purge, an expired
campaign plus matching key is `found + present_matching` with its retained terminal
origin and exact remaining lease; after purge, an explicit old campaign id is
`not_found`, while an id-free old key is `present_new`. Hybrids such as
`expired + present_new`, `not_found + present_matching`, a tombstone without its
reverse key set, or a horizon from another revision reject as store-integrity
assignments. Admission and purge use mutually exclusive CAS commits, so neither
may publish from a snapshot the other changed.
Idempotency-lease clock receipt is `exact_current_clock` for a newly committed key
whose anchor is created under the current owner boot. An authorized matching key
requires `exact_current_clock\|exact_reanchor_committed_same_revision`; every
keyless, unbound, conflicting, denied, unavailable, bound-rejected, commit-failed,
not-found, or non-start row requires `not_applicable`. `continuity_unproved` is an
internal re-anchor gate, not a public Operation classification, and
`missing\|mismatch\|replayed` rejects.
Committed plus missing/mismatched correlation is a rejected integrity constraint,
never an unchanged or safely retryable non-commit. The lifecycle-trace field is a complete ordered receipt set, so a
single start-and-wait response can prove admission followed by running/completion
without collapsing them into one receipt. It is non-`not_applicable` only when
the operation observes or causes a post-admission lifecycle mutation; read-only
attachment and already-terminal mapping require `not_applicable`.
The active-wait receipt is non-`not_applicable` exactly for
`active_accepted\|active_running`. `active_accepted` requires
`exact_accepted_no_mutation`, lifecycle trace `not_applicable`, and proves the
campaign remained durably accepted for the bounded wait. `active_running`
requires either `exact_running_no_mutation` with lifecycle trace
`not_applicable`, or `exact_running_after_start_transition` with the exact ordered
`accepted -> running` Transition receipt. `missing\|mismatch`, an active receipt on
a terminal/not-requested outcome, or a lifecycle trace that claims a mutation for
a no-mutation receipt rejects. This receipt is bounded liveness observation, not
a fabricated state transition.
The active-state relation is exact:
an existing `running` campaign permits only `active_running +
exact_running_no_mutation`; an existing `accepted` campaign permits
`active_accepted + exact_accepted_no_mutation` or `active_running +
exact_running_after_start_transition`; and a successful new/independent admission
permits the same accepted-no-mutation or running-after-start pairs, never
`exact_running_no_mutation`. Any running-to-accepted projection or invented prior
running state rejects.
Private bound-observation input and application correlation are both
`not_applicable` when no Operation-owned bound effect is selected. A queue gate
requires `exact_queue_gate\|exact_queue_and_output + exact_applied` plus
`exact_deduplicated_capacity_release_or_deadline_signal` projected from the exact
durable Transition-owned pending queue-wait at the applicable capacity-ledger
revision. It is valid either for
(a) a successful new/independent start whose campaign is durably accepted but
active or next-sensor capacity is reached, (b) an authorized attachment to an
existing `accepted` campaign carrying the exact still-pending `campaign_start`
wait at the same registry/capacity revision, or (c) an authorized existing
running campaign whose exact `sensor_admission` capacity check reached before
spawn. The accepted forms permit only wait `not_requested` with active-wait/lifecycle
receipts `not_applicable`, or `active_accepted` with
`exact_accepted_no_mutation`; the running form requires unchanged running state
and, for start wait, `active_running + exact_running_no_mutation`, while
resume/detail use their ordinary no-wait read projection. An existing-accepted
attachment reuses the exact durable wait and wake registration; it creates no
registration, reservation, Transition, spawn, campaign mutation, or key
mutation. None of the forms spawns the pending sensor. Terminal outcomes reject
a queue gate. When no queue gate exists,
queue wake registration is `not_applicable`. Output continuation
requires `exact_output_continuation\|exact_queue_and_output + exact_applied` and is
valid only after a campaign has been successfully admitted or reauthorized for
reuse and the current caller is authorized to receive that campaign response.
Pre-admission conflict, denial, unavailability, bound rejection, and commit-failure responses are
complete fixed-shape typed receipts whose mandatory fields fit the public
response cap; they have no Operation bound input, no application correlation,
and no continuation cursor. A fixed receipt that cannot fit is an invariant
failure and is not silently truncated. A continuation may remove only optional
detail and never add undisclosed state. Before a durable admission/key/cancel commit,
missing, mismatched, replayed, cross-scope, or wrong-effect pairs are rejected
checker constraints. After such a commit, the applicable postcommit integrity-preservation
rule below applies; no receipt failure may rewrite or conceal the already
committed campaign/key/cancellation.

Before classification, the following compatibility function runs. A structural or
precommit row outside these exact sets retains the complete rejected default tuple
and no later clause may accept it. The sole carve-out is an enumerated
`missing\|mismatch\|replayed\|wrong-effect` lifecycle or Operation-bound receipt
discovered only after exact campaign-admission/key-binding/cancellation commit; that tuple is
routed exclusively to the postcommit integrity-preservation rule and can never
fall back to the precommit default.

| Requested operation | Required observable context | Reference lookup | Request key | Wait outcome | Attachment input | Admission-only inputs |
|---|---|---|---|---|---|---|
| `start` | `start_request` | governed by the start-admission partition below | governed by the start-admission partition below | governed by the prior-state/admission compatibility function below; never an ignored free value | governed by the start-admission partition below | governed by the start-admission partition below; durable cancellation commit `not_applicable` |
| `resume` | `resume_same_peer`, `resume_other_peer` | `found` | `absent` | `not_requested` | current-peer authorization required | frozen shareability digest, concurrency, independent authority, durable admission commit, admission-batch correlation, admission capacity, and durable cancellation commit are all `not_applicable` |
| `detail` | `detail_same_peer`, `detail_other_peer` | `found` | `absent` | `not_requested` | current-peer authorization required | frozen shareability digest, concurrency, independent authority, durable admission commit, admission-batch correlation, admission capacity, and durable cancellation commit are all `not_applicable` |
| `cancel` | `cancel_requested` | `found` | `absent` | `not_requested` | current-peer authorization required | admission/shareability/capacity axes `not_applicable`; durable cancellation commit is governed by clause 6 |
| `resume\|detail\|cancel` missing reference | matching non-start context | `not_found` | `absent` | `not_requested` | `not_requested` | every admission/shareability/capacity axis and durable cancellation commit `not_applicable`; continuity `not_applicable`; every result/trace/bound axis neutral |

For a found `resume_same_peer` and `detail_same_peer`, attachment is exactly
`same_authorized_peer`, `unauthorized`, `policy_changed`, or `identity_changed`.
For `resume_other_peer` and `detail_other_peer`, it is exactly `different_authorized_peer`,
`unauthorized`, `policy_changed`, or `identity_changed`. `cancel_requested` may
use either authorized-peer value or a denial value. A found non-start operation
never uses `not_requested` attachment; a not-found row requires it. Every
non-start operation uses an absent start-idempotency key.
A non-start operation also has no admission candidate: its frozen shareability
digest, concurrency relation, independent admission authority, durable admission
commit, admission-batch correlation, and admission-capacity reservation receipt MUST all
be `not_applicable`. A phantom
digest, authority, or commit receipt on `resume\|detail\|cancel` rejects before
lifecycle mapping; it can never be ignored as ambient start state.
Durable cancellation commit is separately `not_applicable` for every start,
resume, detail, missing-reference, denied cancel, and already-terminal cancel row;
only an authorized active cancel uses clause 6.
A conflicting key is rejected before attachment. Every other lifecycle/key
pairing is rejected. The engine never guesses whether a prior response was lost:
all keyless starts have the same observable classification and every matching-key
start resolves through durable key state.

Start admission has this disjoint partition; key/relation conflict and current
peer authorization are evaluated before engine continuity or bounded-wait
mapping:

| Candidate relation | Required inputs | Admission result before wait |
|---|---|---|
| Existing same campaign | lookup `found`; non-`none` prior state; `identical_shareable`; key `present_matching` or `absent`; non-`not_requested` attachment; independent authority, durable commit, admission correlation, and admission capacity `not_applicable` | `same_campaign` when authorized; otherwise `rejected_denied` |
| Authorized new alias for existing campaign | lookup `found`; prior state `accepted\|running`; engine continuity `current`; `identical_shareable`; key `present_new`; non-`not_requested` attachment; independent authority `not_applicable`; admission capacity `exact_reserved_all_caps\|reached_no_reservation`; durable key-binding commit as constrained below | `same_campaign` only for authorized attachment, exact alias/key-index capacity reservation, and committed exact request-key-binding batch; a reached cap derives `rejected_bound` with no commit; denial or exact typed commit failure otherwise; terminal/history campaigns never accept a fresh alias |
| Unbound alias reference to lost active campaign | lookup `found`; prior state `accepted\|running`; engine continuity `lost\|replaced`; `identical_shareable`; key `present_new`; authorized attachment; independent authority/admission capacity `not_applicable`; durable commit `not_attempted`; admission/lifecycle correlations `not_applicable` | no alias admission or key mutation; clause 3 returns `engine_lost` while preserving `present_new` and retry `safe` |
| Denied new alias for existing active campaign | lookup `found`; prior state `accepted\|running`; engine continuity any declared non-neutral value; `identical_shareable`; key `present_new`; attachment `unauthorized\|policy_changed\|identity_changed`; independent authority/admission capacity `not_applicable`; durable commit `not_attempted`; admission/lifecycle correlations `not_applicable` | `rejected_denied` before continuity/capacity is disclosed; alias remains unbound |
| New campaign | lookup `not_requested`; prior state `none`; `single`; key `present_new` or `absent`; attachment `not_requested`; denied/unavailable authority requires admission capacity `not_applicable`; allowed admission, keyed or keyless, requires `exact_reserved_all_caps\|reached_no_reservation`; durable campaign-admission commit as constrained below | `new_campaign` only for allowed authority, exact campaign/run-registry reservation (plus key capacity when keyed), and committed exact admission batch; reached cap derives `rejected_bound`; exact typed denial/unavailability/commit failure otherwise |
| Independent campaign | lookup `not_requested`; prior state `none`; any `same_shape_different_*` relation; key `present_new` or `absent`; attachment `not_requested`; denied/unavailable authority requires admission capacity `not_applicable`; allowed admission, keyed or keyless, requires `exact_reserved_all_caps\|reached_no_reservation`; durable campaign-admission commit as constrained below | `independent_campaign` only for allowed authority, exact campaign/run-registry reservation (plus key capacity when keyed), and committed exact admission batch; reached cap derives `rejected_bound`; exact typed denial/unavailability/commit failure otherwise |
| Explicit deleted campaign reference | lookup `not_found`; prior state `none`; key `present_new\|absent`; attachment/continuity and every admission/shareability/capacity/result/trace/bound axis neutral | `not_found_or_retention_elapsed`; no new admission while the deleted id remains in the request |
| Conflict | key `present_conflicting`; lookup `not_found` plus `present_matching\|present_conflicting`; matching key with a relation other than `identical_shareable`; `identical_shareable` with prior `none`; `single` with prior non-`none`; `present_new` with prior non-`none` but a non-identical relation; or any context/attachment-shape mismatch; independent authority/admission capacity `not_applicable`; durable commit `not_attempted`; admission and lifecycle correlations `not_applicable`; private bound input and application correlation `not_applicable` | `rejected_conflict` |

For new/independent candidates, denied or unavailable authority requires commit
`not_attempted` and correlation `not_applicable`, deriving respectively
`rejected_denied` or `rejected_unavailable`. Allowed authority requires commit
`committed\|failed\|unavailable`: `committed` entails correlation `exact` and
succeeds; `failed\|unavailable` entails correlation `not_applicable` and derives
`rejected_commit_failed`. For an alias candidate, authorized attachment uses the
same commit rule; denied/changed attachment requires `not_attempted` and derives
`rejected_denied`. Authority denial with an attempted write, allowed authority
with `not_attempted`, or committed with non-exact correlation is a rejected
structural/integrity constraint and cannot be reclassified as a harmless failed
commit.

Capacity is evaluated only after authorization and before any new/independent
campaign admission or alias write. Every new campaign, keyed or keyless, reserves
campaign/run-registry capacity; keyed admissions and aliases additionally reserve
key capacity. A new/independent or authorized-alias candidate with
`reached_no_reservation` requires commit `not_attempted`, returns the complete
typed `rejected_bound` receipt, leaves key/campaign unchanged, and discloses no
cross-scope counter. `exact_reserved_all_caps` is consumed atomically by the exact
key/admission batch; commit failure releases the reservation in that same failed
transaction. Missing, mismatched, replayed, or a reservation on a denied/conflict/
existing-reuse row rejects. No campaign, alias, or index can exist without its
corresponding capacity reservation.

Start wait compatibility is then closed over the admission result and prior
state. Any `rejected_*` admission has wait outcome `not_requested`, observed wait
origin `not_applicable`, and lifecycle trace `not_applicable`; a requested wait
cannot survive failed admission. An authorized existing campaign already in
`completed\|cancelled\|failed\|expired` likewise requires `not_requested`,
`not_applicable` observed origin, and `not_applicable` lifecycle trace because the
durable terminal mapping performs no wait. Only a successful new/independent
admission or an authorized existing `accepted\|running` campaign may carry
`active_accepted\|active_running\|completed_with_result\|cancelled\|failed\|expired`, and every such
outcome requires the exact active-wait or lifecycle trace/origin projection below. Any other
prior-state/wait/trace tuple rejects before the operation classifier; no prior
terminal state may silently win while a contradictory wait value is ignored.

An active campaign already observed with engine continuity `lost\|replaced` at
request admission is a disjoint no-wait row: wait MUST be `not_requested`, active
receipt, observed origin, retained-result projection, and lifecycle trace MUST all
be `not_applicable`, and clause 3 alone returns `engine_lost`. A non-neutral wait
observation on that row rejects instead of being discarded by precedence. Engine
loss that occurs during a genuinely admitted bounded wait starts with continuity
`current` and is represented by the exact `engine_loss` lifecycle Transition
receipt; it is never retroactively classified as preexisting loss.

A matching request key is valid only with `identical_shareable`, exact equality
of the complete frozen shareability digest, and independent
authority `not_applicable`; any difference is represented as
`present_conflicting` and rejected. Every typed digest mismatch selects its
matching `same_shape_different_*` class. A new alias is bound only after the same
complete digest and current attachment authority are proved. It cannot coexist with an
independent-admission path, and a target/boot/workspace/policy difference can
never reuse the original campaign. A successful `present_new` commit atomically
binds that key to the admitted or authorized existing campaign before spawn or
receipt publication; every later use is `present_matching`. A failed or
unavailable commit proves non-commit: the key remains new, a new campaign remains
`none` (or an existing campaign remains unchanged), no new process is spawned,
and no campaign/admission-success receipt or state is published; only the
complete fixed-shape typed failure envelope is returned. Durable commit is `not_applicable` only for
matching-key or keyless existing-campaign reuse.

Every successful keyed admission/alias binding creates a fixed minimum resolution
lease in the same atomic key/campaign commit. Every authorized matching-key
resolution first requires `exact_current_clock` or an
`exact_reanchor_committed_same_revision` receipt and then reads that existing lease
without extending it. `continuity_unproved` starts no classification: the headless
engine must commit the no-lifecycle-change re-anchor within the frozen operation
bound or return a complete typed store-unavailable envelope with no stale horizon.
Both successful forms return
`idempotency_horizon=exact_owner_deadline_and_serialization_cut`: the persisted
owner boot/clock identity and absolute monotonic deadline, the final response-
serialization cut, and conservative nonnegative remaining duration
`deadline - cut`. The duration is explicitly server-cut-relative; transport delay
can only shorten it and never resets or extends the lease. Remote/target wall time and
capacity forecasts never define it. Retention TTL may lengthen storage but cannot
shorten the lease. Count/byte pressure cannot evict a protected binding: when no
unprotected victim exists, new reservations return `rejected_bound`. `retry=resolved`
is promised only through the persisted lease. Atomic purge deletes the
key indexes; a later key-only start is `present_new`, may create a new campaign
with a new horizon, and MUST NOT imply recovery of the purged run. `retry=safe`
for an unbound key means only that the current admission attempt may be retried
without duplicating a committed campaign; it is not historical resolution evidence.
The serialization cut occurs inside the same serializable read transaction or
after an exact same-revision revalidation while the binding is read-pinned; purge
cannot commit between that validation and receipt construction. A changed revision
restarts the bounded classification instead of publishing a stale horizon.

The outputs are derived, never independent input axes:

- `admission = not_applicable | new_campaign | same_campaign |
  independent_campaign | rejected_conflict | rejected_denied |
  rejected_unavailable | rejected_bound | rejected_commit_failed`;
- `attachment = not_applicable \| authorized \| denied`;
- `retry = not_applicable \| safe \| resolved \| unsafe`;
- `operation = in_progress_receipt | goal_result |
  cancel_acknowledged | already_terminal | terminal_status | result_unavailable |
  engine_lost | not_found_or_retention_elapsed | postcommit_integrity_error |
  rejected`;
- `campaign_state_after = accepted | running | completed | cancelled | failed |
  expired | unchanged`;
- `lifecycle_observation = not_applicable | exact |
  postcommit_integrity_unknown`;
- `integrity_failure_scope = none | lifecycle_receipt |
  operation_bound_receipt | lifecycle_and_operation_bound_receipt`;
- `goal_result_presence = absent \| terminal_result`;
- `request_key_state_after = present_new | present_matching |
  present_conflicting | absent`; and
- `request_key_effect = unchanged | bound_atomically | resolved_existing |
  rejected`; and
- `idempotency_horizon = not_applicable \| exact_owner_deadline_and_serialization_cut`;
- `bound_effect = none | queued_in_progress | bounded_continuation |
  queued_with_bounded_continuation`.

The ordered operation classifier is:

Before any clause, the complete output tuple is initialized to
`admission=not_applicable`, `attachment=not_applicable`,
`retry=not_applicable`, `operation=rejected`,
`campaign_state_after=unchanged`, `goal_result_presence=absent`,
`lifecycle_observation=not_applicable`, `integrity_failure_scope=none`,
`request_key_state_after` equal to the input key state, and
`request_key_effect=unchanged`, `idempotency_horizon=not_applicable`, and
`bound_effect=none`. A clause overrides only
the named fields, so every row has all twelve outputs. Every authorized lifecycle
or retained-history mapping sets `lifecycle_observation=exact`; every pre-admission
conflict/denial/commit failure leaves it `not_applicable`. Only
`postcommit_integrity_error` may set a non-`none` integrity-failure scope.

After the selected clause, one total horizon projection overrides the initialized
field exactly once: a successfully committed keyed admission or alias, and an
authorized `present_matching` resolution (active, terminal, expired, or
postcommit-integrity-preserved), MUST set
`idempotency_horizon=exact_owner_deadline_and_serialization_cut` from the
same-revision persisted binding receipt, exact idempotency-lease clock receipt, and
final serialization cut. A persisted owner-boot mismatch cannot reach this
projection until `retention_lease_reanchor` commits at the same registry revision.
Every keyless, unbound, conflicting, denied,
unavailable, `rejected_bound`, commit-failed, not-found, or non-start row MUST keep
`not_applicable`. A missing, stale, reset, cross-key, or differently revised
horizon rejects; no primary clause may override this projection.

0. Exact `not_found` lookup is classified before attachment, continuity, or
   lifecycle. `resume\|detail\|cancel` returns
   `operation=not_found_or_retention_elapsed`, every state/Goal/key unchanged or
   absent, attachment/retry/lifecycle observation `not_applicable`, and no
   continuity, terminal-origin, tenant/peer, or prior-Goal disclosure. `start`
   with an explicit deleted id and key `present_new\|absent` returns the same result
   and starts nothing; a matching/conflicting key on that missing id is the
   conflict row. The caller may issue a separate id-free start. This result is
   never `engine_lost`.

1. Every request first validates its operation/context/key compatibility. A start
   with a conflicting key or relation terminates with
   `admission=rejected_conflict`, `request_key_effect=rejected`,
   `operation=rejected`, and every state unchanged before campaign or engine
   state is exposed. Its absent key derives `retry=unsafe`, its unbound
   `present_new` key derives `retry=safe`, and an already-conflicting key has no
   retry resolution. Every existing-campaign reference then reauthorizes the
   current peer/identity/policy. A denied start terminates with
   `admission=rejected_denied`, `attachment=denied`, and `operation=rejected`; a
   denied resume/detail/cancel terminates with `attachment=denied` and
   `operation=rejected`. Both leave every state unchanged and disclose no
   lifecycle or continuity result. Only an authorized reference may continue.
   For these terminal start tuples, an absent key still derives `retry=unsafe`
   and an unbound `present_new` still derives `retry=safe`; an unauthorized
   matching/conflicting key does not disclose a retry resolution.
    Every conflict and every pre-admission denied/unavailable/bound/commit-failed tuple
   requires durable commit `not_attempted\|failed\|unavailable` exactly as assigned
   above, no lifecycle trace, and no private bound input or application
   correlation. An attempted write on a conflict/denial tuple, or a continuation
   attached to any pre-admission failure, is a rejected integrity assignment;
   it cannot be normalized into a bounded error response.
   New/independent starts with prior state `none` require
   `Engine continuity = current`; any other continuity is a rejected structural
   assignment because no prior campaign exists to lose.
2. For `start` only, key retry classification is then total: absent -> `retry=unsafe`;
    `present_new` -> `retry=safe`; an authorized `present_matching` ->
    `retry=resolved`, effect `resolved_existing`; and conflicting -> key effect
    `rejected`. No key mutation has occurred yet. `resume\|detail\|cancel` retain
    `retry=not_applicable`; their required absent start key is a neutral shape
    constraint, not an unsafe start-retry verdict.
3. After conflict and authorization gates, an authorized request referencing
    `accepted\|running` on a lost/replaced engine derives
    `operation=engine_lost`, preserves state, and returns no Goal. Its attachment
    result remains `authorized`, and its key retains the retry classification from
    clause 2. A `present_new` alias on this path is continuity-gated before any
    binding attempt: durable commit is `not_attempted`, the key remains
    `present_new` with effect `unchanged`, and no committed receipt is ignored.
    A committed alias receipt on lost/replaced continuity is a rejected integrity
    assignment, not `engine_lost`. Completed/cancelled/failed/expired state is classified from durable
   result/tombstone history regardless of later engine replacement.
4. Every surviving `start` derives exactly one admission partition, then and only
   then maps its wait result. An existing same-campaign relation has already been
   reauthorized and sets `admission=same_campaign` and
   `attachment=authorized`. An authorized alias sets
   `admission=same_campaign` only after its exact key-binding commit. A new/independent
   relation sets its named admission only when independent authority is allowed,
   durable commit is `committed`, and admission-batch correlation is `exact`.
    Denied authority derives `rejected_denied`; unavailable authority derives
    `rejected_unavailable`; durable commit `failed\|unavailable` derives
     `rejected_commit_failed`; any `reached_no_reservation` admission-capacity
     receipt derives
    `rejected_bound` before commit. Failed/bounded admission leaves
   campaign/key unchanged, spawns nothing, and publishes no campaign or
   admission-success receipt; it returns only the complete fixed-shape typed
   failure envelope.

    A successfully committed new key changes state to `present_matching`, effect
    `bound_atomically`, and `retry=resolved` with the exact bounded idempotency
    horizon, while a rejected
   non-commit leaves it `present_new` and unchanged. There is no observable point
    between key binding and new campaign admission, or between authorized alias
    binding and publication of that binding.

     Once either commit is exact, those durable outputs are fixed before lifecycle
     observation. If a subsequently required lifecycle trace or active-wait receipt
     is `missing\|mismatch`, the operation derives `postcommit_integrity_error`,
     sets `lifecycle_observation=postcommit_integrity_unknown` and
     `integrity_failure_scope=lifecycle_receipt` (or the combined scope when the
     bound receipt also fails), returns the committed campaign id and exact bounded
     integrity receipt, keeps `goal_result_presence=absent`, and reports the exact
     independently committed base state in `campaign_state_after`: `accepted` for
     a new/independent or previously accepted campaign and `running` for a
     previously running campaign. These are real Transition states, never
     epistemic sentinel values that can be fed back into the lifecycle model. A committed
    `present_new` key remains `present_matching` with effect `bound_atomically`
    and retry `resolved`; both it and an authorized matching key return the exact
    bounded idempotency/tombstone horizon. Resolution is promised only through
    that horizon. A matching key remains resolved; an absent key is unsafe
    only for a keyless `start`, while every non-start remains
    `retry=not_applicable`; the campaign id is returned for explicit resume. The receipt binds
    the admission/key commits and typed trace failure without claiming an
    unproved terminal state. It can never reset the key to `present_new`, hide the
     admitted campaign, or permit duplicate admission. Structural failure before
     durable commit still uses the precommit rejected tuple instead.

     The same preservation rule covers every Operation-owned private-bound input
     or application receipt that is required after an exact campaign-admission or
     alias-binding commit. `missing\|mismatch\|replayed`, cross-scope, or wrong-effect
     input/application derives `postcommit_integrity_error`, sets
     `integrity_failure_scope=operation_bound_receipt` (or the combined scope),
     proves `bound_effect=none`, forces `goal_result_presence=absent`, and preserves the exact campaign/key outputs and
     retry classification already committed. If lifecycle mapping is independently
     exact, its exact campaign state and `lifecycle_observation=exact` are retained;
     otherwise only the committed base state and
     `postcommit_integrity_unknown` are returned. No unproved queue/continuation
     effect is exposed, no key/campaign is rolled back, and retry cannot duplicate
     admission. A later authorized `resume\|detail` may retrieve an independently
     retained terminal result; the failed delivery path never claims it was present.

    For a successful new/independent admission with private bound input
    `exact_queue_gate\|exact_queue_and_output`, the exact applied-effect receipt short-circuits ordinary
    wait mapping only for the compatible `not_requested\|active_accepted` rows:
    no sensor/helper is spawned, `bound_effect=queued_in_progress`
   (or `queued_with_bounded_continuation` for the combined input),
   operation is `in_progress_receipt`, and campaign state remains `accepted`.
    A requested bounded wait observes the same accepted/queued state through
    `exact_accepted_no_mutation`; the admission batch and queue-effect receipts
    remain separately exact, lifecycle trace is `not_applicable`, and the state is
     not relabelled `running`. For this successful new/independent-admission row,
     any terminal or `active_running` wait outcome with a queue gate is rejected
     rather than ignored; the separately defined existing-running queue-gate row
     remains valid.

   After successful admission, prior terminal state wins before the wait:
   completed+available -> `goal_result` plus `terminal_result`;
   completed+unavailable or expired-from-completed -> `result_unavailable`; and
   cancelled/failed/expired-from-cancelled-or-failed -> `terminal_status`, all
   with state unchanged.
    Otherwise wait `not_requested` yields `in_progress_receipt` with the required
    campaign id, stage, cursor, retry classification, bounded known evidence, and
    liveness receipt; campaign state is `accepted` for a new/independent campaign
    and unchanged for an existing active campaign;
    `active_accepted` requires `exact_accepted_no_mutation`, no lifecycle trace,
    and yields `in_progress_receipt`, state `accepted`;
    `active_running` uses the exact prior-state relation above and yields
    `in_progress_receipt`, state `running`; `completed_with_result` requires exact
    completion trace plus retained `available` and yields `goal_result`,
    state `completed`, and `terminal_result`; cancelled/failed wait requires its
    exact transition receipt and yields matching `terminal_status`, with state
    respectively `cancelled` or `failed`; expired wait
   requires its exact transition receipt plus observed origin, mapping
   `completed -> result_unavailable` with retained result `unavailable` and
   `cancelled\|failed -> terminal_status`; every expired-wait origin sets campaign
   state `expired`. No start row is evaluated again by a later lifecycle clause;
   only the orthogonal bound-effect composition in clause 7 may annotate it.
5. `resume` and `detail` alone use the read-only attachment mapping. Denial sets
   `attachment=denied`; authorization sets `attachment=authorized` and maps
   accepted/running -> `in_progress_receipt`, completed+available ->
   `goal_result` plus `terminal_result`, completed+unavailable or
   expired-from-completed -> `result_unavailable`, and cancelled/failed/
   expired-from-cancelled-or-failed -> `terminal_status`.
   Campaign state remains unchanged and both batch/trace correlations are
   `not_applicable`; the read consumes but never rewrites the durable receipt.
 6. Authorized `cancel` on accepted/running first resolves the durable cancellation
    commit. `committed + exact_complete_ordered_set` requires the trace to contain
    that exact atomic caller-cancellation batch and yields `cancel_acknowledged`,
    state `cancelled`, lifecycle observation `exact`, and attachment authorized.
    `committed + missing\|mismatched` trace cannot claim non-mutation: it yields
    `postcommit_integrity_error`, preserves the commit-proved `cancelled` state,
    sets lifecycle observation `postcommit_integrity_unknown` and integrity scope
    `lifecycle_receipt`, exposes no Goal/bound effect, and returns the bounded commit
    plus projection-failure receipt. `proved_not_committed\|unavailable` requires no
    lifecycle trace, returns the exact typed rejection/unavailability envelope, and
    leaves the active campaign unchanged; an exact trace on that row rejects.
    `not_attempted` on an authorized active cancel is invalid. An authorized terminal
    campaign requires cancellation commit `not_applicable`, sets `attachment=authorized`, and
    yields `already_terminal`; unauthorized,
    policy-changed, or identity-changed callers require commit `not_applicable` and yield attachment denied; both
    leave state unchanged.
7. After a primary mapping for a successfully admitted campaign or an authorized
    existing-campaign attachment, `exact_queue_gate + exact_applied` with its exact
    wake registration overrides only `bound_effect` to `queued_in_progress`; every
    other field of the complete primary output tuple remains unchanged. The durable
    campaign remains `accepted`; `campaign_state_after` retains the primary mapping's
    value (`accepted` for `active_accepted`, `unchanged` for existing-campaign
    no-wait start/resume/detail). An authorized running campaign remains `running`,
    admits no pending sensor,
    and returns one in-progress signal bound to the pending edge and current
    capacity revision. Capacity-release/store-revision or total-time deadline wakes
    it exactly once; duplicate signals coalesce.
    After that queue projection or any other primary mapping for a successfully
    admitted campaign or an authorized
    existing-campaign attachment, `exact_output_continuation + exact_applied` sets
   `bound_effect=bounded_continuation` while
   preserving the primary operation, Goal/fact completeness, campaign state, and
   attachment. The continuation carries the exact response byte/token/detail
   cursor and receipt and never reveals detail suppressed by authorization.
    `exact_queue_and_output + exact_applied` is accepted only when this clause or
    clause 4 has already selected a valid accepted-or-running queue gate; it
    preserves `queued_with_bounded_continuation`. On every other primary mapping, a combined
   pair proved wrong before any admission/key commit uses the rejected default; if
   discovered after exact commit it uses the postcommit integrity tuple, preserves
   campaign/key, forces Goal presence absent, and exposes no bound effect. Conflict and every pre-admission
    denial, unavailability, bound rejection, or commit failure require
    `bound_effect=none`; their
   fixed typed receipt is already complete within the public response cap.
8. Every other assignment retains the complete rejected default tuple. In
   particular, no campaign lifecycle mutation is accepted without the matching
   atomic transition receipt, and no operation result can manufacture a Goal.

Cancellation, expiry, engine loss, result retention loss, attachment denial, and
unsafe retry are operation/lifecycle results. They MUST NOT invent an environment
goal verdict.

## SecurityPropertyModel Domains

| Input axis | Required values |
|---|---|
| Property under evaluation | `sensor_authorization`, `private_environment_resolution`, `executable_spawn`, `fixed_helper_decode`, `channel_fact_admission`, `surface_name_application`, `overlay_transport`, `overlay_persistence`, `diagnostic_egress` |
| Sensor request class | `harness_context`, `passive_metadata`, `targeted_names`, `full_census`, `private_platform_resolver`, `fixed_helper`, `route_sentinel`, `connector_discover`, `connector_dial`, `identity_challenge`, `target_start_or_wake`, `not_applicable` |
| Complete authority-chain record set | one immutable record for every governing origin, forwarder, and target node/hop; each carries node role, exact connector edge, policy profile, every environment-cap bit, sensor-class verdict, connector-action verdict, underlying-action verdict, target verdict, explicit per-run authority, policy revision, and node/target/boot/workspace identity bindings |
| Authority-chain cardinality | `one`, `many_within_admitted_hop_bound`, `zero`, `over_limit`; only the first two can authorize |
| Gated-action audit admission | `exact_committed_per_authorizing_and_executing_node`, `not_attempted`, `failed`, `unavailable`, `mismatch`, `replay`, `not_applicable`; the exact value is action-digest-specific and a sensor admission cannot substitute for a later spawn admission |
| Explicit census request | `explicit_for_this_campaign`, `absent`, `not_applicable` |
| Explicit target start/wake authority | `explicit_for_this_campaign`, `absent`, `not_applicable`; a standing cap is insufficient |
| Receipt correlation | independently for action-audit, sensor, evidence-boundary, private resolver, spawn, decoder, private execution path, surface-name application, private channel, overlay ingress provenance, overlay persistence provenance, and sanitizer receipts: `exact_match`, `action_digest_mismatch`, `sensor_class_mismatch`, `policy_revision_mismatch`, `authority_chain_or_connector_route_mismatch`, `target_boot_workspace_mismatch`, `campaign_branch_mismatch`, `command_job_scope_mismatch`, `pty_scope_mismatch`, `session_scope_mismatch`, `snapshot_operation_scope_mismatch`, `private_payload_binding_mismatch`, `replayed`, `expired`, `unproved`, `not_applicable` |
| Bound upstream sensor-policy verdict | `allowed`, `denied`, `unavailable`, `unproved`, `not_applicable` |
| Bound evidence-boundary verdict | `admitted_complete`, `admitted_evidence_incomplete`, `admitted_presentation_bounded`, `rejected`, `unknown`, `unproved`, `not_applicable` |
| Candidate fact/boundary relation | `exact_unchanged`, `exact_same_typed_incomplete`, `mismatch`, `unproved`, `not_applicable` |
| Candidate staging state | `unstaged`, `exact_pending_transition`, `mismatch`, `unproved`, `not_applicable`; active only for channel fact admission and engine-derived from the evidence graph |
| Fact-admission phase | `final_boundary_receipt`, `soft_bound_precommit`, `not_applicable` |
| Precommit bound-admission proof | `exact_soft_or_informational_scope_limit_counter_relevance_and_effect_digest`, `missing`, `mismatch`, `replayed`, `unproved`, `not_applicable`; it is private, conditionally authorizing only for the bound effect, and cannot claim cleanup or public fact admission |
| Preliminary native-name verdict | `canonical_utf8`, `canonical_posix_bytes`, `canonical_windows_utf16`, `omitted_whole_name_at_bound`, `rejected`, `unknown`, `not_applicable`; emitted privately by EvidenceBoundary name classification before public receipt composition; unproved classification evidence derives `unknown` and is never a verdict value |
| Private surface-name observation receipt | `exact_surface_name_scope_limit_and_counter`, `not_reached`, `missing`, `mismatch`, `replayed`, `unproved`, `not_applicable`; `exact_surface_name_scope_limit_and_counter` is required for whole-name bound omission and binds the non-campaign operation scope |
| Surface-name consumer | `command_request`, `pty_request`, `session_request`, `snapshot_persistence`, `not_applicable` |
| Bound surface-name application verdict | `canonical`, `omitted`, `rejected`, `unknown`, `unproved`, `not_applicable`; when active, only `canonical` plus an exact application-receipt correlation may authorize transport, and that receipt binds the exact consumer operation, canonical-name identity, opaque material handle, action, target/boot/workspace, and policy revision |
| Bound canonical overlay-name verdict | `canonical_utf8`, `canonical_posix_bytes`, `canonical_windows_utf16`, `omitted_whole_name_at_bound`, `rejected`, `unknown`, `unproved`, `not_applicable`; this is the composed public EvidenceBoundary verdict consumed only after surface-name application |
| Bound upstream private-resolution verdict | `typed_fact`, `rejected`, `unknown`, `unproved`, `not_applicable` |
| Bound private-input verdict | independently for `opaque_input_bytes`, `resolver_candidates`, and `decoder_input_bytes`: `within`, `reached_and_stopped`, `exceeded`, `missing`, `mismatch`, `unproved`, `not_applicable` |
| Private-input counter observation | `exact_scope_limit_and_counter`, `missing`, `mismatch`, `replayed`, `unproved`, `not_applicable`; this pre-effect receipt never claims cleanup. Opaque resolver input counts POSIX raw bytes or checked `2 * Windows UTF-16 code-unit count` before transcoding; decoder input counts raw transport bytes before decoding; candidate counts increment before resolution/dispatch |
| Originating sensor execution | `in_process`, `spawned`, `not_applicable` |
| Bound upstream spawn verdict | `allowed`, `denied`, `unknown`, `not_executing`, `unproved`, `not_applicable` |
| Bound upstream decoder verdict | `evidence`, `typed_failure`, `unproved`, `not_applicable` |
| Bound private-channel receipt | `private_same_process`, `private_confidentiality_and_integrity`, `rejected`, `unknown`, `unproved`, `not_applicable` |
| Bound private execution/restoration path | action `command`, `pty`, `session`, `snapshot_store`, or `snapshot_restore` with verdict `allowed`, `denied`, `unavailable`, `unproved`; or `not_applicable` |
| Payload class | `probe_evidence`, `observed_environment_opaque_input`, `operation_name_metadata`, `overlay_private_material`, `diagnostic_private_input`, `not_applicable` |
| Probe evidence producer | `direct_passive`, `private_environment_resolution`, `fixed_helper`, `spawned_typed_sensor`, `connector_native`, `not_applicable` |
| Observed-value source | `target_process_environment`, `target_shell_or_session_environment`, `harness_environment`, `not_applicable` |
| Requested observed-value context | `target_process`, `target_shell_or_session`, `harness_process`, `not_applicable`; frozen in the goal plan and exact sensor admission before any environment value is read |
| Private resolver class | `approved_platform_native`, `approved_fixed_helper`, `caller_defined_or_unknown`, `not_applicable` |
| Private resolver locality | `same_target_private`, `crossed_process_or_connector_boundary`, `not_applicable` |
| Raw-value retention/egress | `none`, `retention_attempted`, `egress_attempted`, `not_applicable` |
| Private resolver derived output | `non_reconstructive_typed_identity_version_presence`, `structurally_sanitized_path`, `raw_or_reconstructive`, `not_applicable` |
| Private resolver fact kind | `presence_only`, `executable_identity`, `executable_version`, `not_applicable` |
| Private resolver binding proof | `exact_platform_api_and_impl_revision`, `exact_helper_executable_and_decoder`, `mismatch`, `unproved`, `not_applicable` |
| Private resolver conversion proof | `exact_closed_non_reconstructive_conversion`, `conversion_failed`, `raw_or_reconstructive`, `mismatch`, `unproved`, `not_applicable` |
| Resolved executable identity binding | `exact_binary`, `exact_interpreter_and_script`, `missing`, `mismatch`, `unproved`, `not_applicable` |
| Resolver revision verdict | `approved_exact`, `denied`, `unproved`, `not_applicable` |
| Private-tainted producer/sink coverage attestation | `exact_current_complete`, `missing_producer_key`, `missing_sink_key`, `stale`, `mismatch`, `unproved`, `not_applicable`; exact is engine-verified from an immutable trusted build manifest under the current catalogue-trust/security revision and binds the current authoritative private-tainted-input producer registry and forbidden-sink inventory revisions, the complete producer x sink x success/failure/bound-stop canary key set, and the passing build/runtime attestation. Registering either a producer or sink changes the required root and invalidates prior attestations |
| Executable origin | `system_install`, `operator_install`, `user_install`, `workspace_local`, `unknown`, `not_applicable` |
| Executable writability | `not_writable_by_subject`, `user_writable`, `workspace_writable`, `unknown`, `not_applicable` |
| Executable trust provenance | `operator_pinned`, `catalogue_identity`, `canonical_identity_only`, `unpinned`, `pin_mismatch`, `unknown`, `not_applicable` |
| Executable form | `native_binary`, `interpreter_script`, `shim`, `unknown`, `not_applicable` |
| Executable binding completeness | `binary_bound`, `interpreter_and_script_bound`, `incomplete`, `not_applicable` |
| Executable dispatch identity | `unchanged`, `changed_before_spawn`, `unprovable`, `not_applicable` |
| Catalogue adapter/invocation-control proof | `exact_approved_revision_side_effect_direct_argv_least_privilege_forbidden_behaviors_disabled_no_cwd_workspace_search`, `mismatch`, `unproved`, `not_applicable`; exact binds immutable product adapter id/revision, declared side-effect class, direct argv shape, least-privilege environment, downloads/updates/hooks/plugins/startup files/module imports/build scripts/lifecycle scripts/project-code execution all disabled, and executable search excluding cwd/workspace |
| Operator pin use | `executable_trust`, `channel_trust`, `not_used`, `not_applicable` |
| Operator pin origin/management proof | `preexisting_operator_config_current`, `exact_audited_operator_add_current`, `exact_audited_operator_rotate_current`, `exact_audited_operator_revoke`, `llm_or_caller_supplied`, `trust_on_first_use`, `mismatch`, `unproved`, `not_applicable`; exact audit proofs bind operator identity, action, pin identity, old/new revision, and current revocation state without exposing credential material |
| Channel boundary | `same_process`, `authenticated_local_process`, `forwarded_or_remote`, `guest`, `not_applicable` |
| Channel security | `not_applicable`, `local_authenticated_peer`, `authenticated_integrity`, `authenticated_confidentiality_and_integrity`, `point_challenge_only`, `unauthenticated`, `replayed`, `expired`, `unavailable` |
| Channel identity/binding proof | `exact_authenticated_local_binding`, `exact_authenticated_forwarded_or_guest_binding`, `missing`, `fresh_nonce_replayed`, `sequence_replay`, `expired`, `unapproved_rotation`, `clone_detected`, `persistent_instance_mismatch`, `engine_boot_mismatch`, `target_boot_mismatch`, `protocol_mismatch`, `endpoint_route_or_connector_context_mismatch`, `trust_root_or_authenticated_identity_mismatch`, `mid_run_replacement`, `unproved`, `not_applicable`. Each exact receipt binds one fresh nonce and channel epoch, the exact applicable run or non-campaign operation scope, origin/final-target/every-forwarder persistent identities, connector instance, engine and target boots, protocol version, endpoint, ordered route/connector context, operator pin or authenticated transport identity, and message sequence/expiry. It is active for every local-process or forwarded/remote/guest boundary; same-process and no-channel rows require `not_applicable` |
| Per-message action/audit binding | `complete`, `missing_action_digest`, `missing_policy_revision`, `missing_workspace_or_boot`, `missing_audit_receipt`, `sequence_replay`, `expired`, `not_applicable` |
| Channel/message freshness record set | a complete bounded ordered record for every authenticated edge/verifier; each binds edge ordinal and identity, verifier persistent identity and engine boot, approved suspend-inclusive monotonic clock identity, verifier-issued nonce/channel epoch, message sequence, local challenge/epoch issue anchor, local receive anchor, frozen maximum age, and verdict `exact_verifier_monotonic_within_frozen_age`, `expired`, `clock_continuity_unproved`, `rollback_or_discontinuity`, or `mismatch`; sender/remote wall time is provenance only and never authorizes freshness |
| Channel/message freshness coverage | `exact_complete_ordered_edge_set`, `missing_edge`, `extra_edge`, `duplicate_edge`, `root_or_count_mismatch`, `unproved`, `not_applicable`; exact binds the authenticated ordered route edge set and record-set count/root. `not_applicable` is valid only for same-process/no-channel rows |
| Confidentiality scope | `same_process_private`, `authenticated_peer_to_peer`, `origin_to_final_end_to_end`, `hop_only`, `unproved`, `not_applicable`; active only for overlay transport/persistence channels |
| Private endpoint binding | `exact_origin_and_final_target_route`, `missing`, `mismatch`, `replayed`, `expired`, `unproved`, `not_applicable`; active for authenticated-local or cross-boundary non-campaign overlay transport/persistence and binds the final target independently of forwarding hops; same-process and no-overlay rows require `not_applicable` |
| Causal effect-source cardinality | `zero`, `one`, `many`; computed before the Security decision from immutable source receipts only for simultaneous receipt-emitting causal negatives or effect requests: gated-action audit `mismatch\|replay` only when its required correlation proves the exact receipt or an accepted same-source projection; private-input `reached_and_stopped` for `opaque_input_bytes\|resolver_candidates\|decoder_input_bytes` only when paired with `Private-input counter observation = exact_scope_limit_and_counter`; receipt-emitting channel-identity and per-message negatives; and reachable receipt-emitting source-total rows. Exact noncausal surface-name receipts, unavailable/unknown/no-receipt results, positive/conditional authority, and structural/compatibility rejection (including private-endpoint probe cells) are excluded and pair `zero + not_applicable` unless another covered source is present |
| Multi-source causal correlation | `exact_same_source_projection_set`, `exact_same_incident_bound_effect_bundle`, `independent_sources`, `source_mismatch`, `incident_mismatch`, `scope_mismatch`, `disposition_conflict`, `missing`, `not_applicable`; each source receipt binds a non-secret incident digest, exact source axis/sub-key, exact run or operation scope, and mapped disposition. A projection also binds the primary source-receipt digest; a bound-effect member instead binds the exact closed Transition bound-effect-bundle identity and member key |
| Overlay crossing | `none`, `same_process_private`, `authenticated_local_private`, `cross_boundary`, `not_applicable` |
| Overlay transport ingress provenance proof | `exact_immutable_caller_ingress`, `exact_persisted_caller_ingress_chain`, `caller_asserted`, `inherited_or_legacy`, `missing`, `mismatch`, `replayed`, `unproved`, `not_applicable`; the direct exact proof is created when the opaque handle enters TC and binds that handle, origin peer/action, requested operation, target/boot/workspace, and immutable caller-supplied classification. The persisted exact chain binds the original ingress receipt, exact successful store receipt, snapshot identity, retained handle identity, and current restore operation/authorization. Neither contains the value or a reconstructive digest |
| Overlay persistence policy | `off`, `on`, `not_applicable` |
| Overlay persistence policy source/default proof | `explicit_operator_on`, `explicit_operator_off`, `config_absent_default_off`, `config_unset_default_off`, `mismatch`, `unproved`, `not_applicable`; LLM/caller assertion is not a policy source |
| Persistence operation | `store`, `restore`, `not_applicable` |
| Overlay-name policy | `operator_allowlisted`, `not_allowlisted`, `unknown`, `not_applicable` |
| Overlay value provenance | `caller_supplied`, `caller_asserted_only`, `unclassified`, `inherited`, `unresolved`, `legacy_redaction_marker`, `not_applicable` |
| Overlay provenance receipt | `exact_immutable_caller_origin`, `exact_persisted_provenance_chain`, `caller_asserted`, `missing`, `mismatch`, `replayed`, `unproved`, `not_applicable`. Each exact or caller-asserted receipt binds opaque handle, origin peer/action, target/boot/workspace, provenance class, and exactly one name projection: `exact_canonical_name_identity`, `exact_whole_name_bound_marker`, or `name_not_consulted`. The whole-name marker contains no name key/content and is valid only after an actual boundary-name reach; `name_not_consulted` contains no name identity or bound claim and is valid only for a pre-name omission. `missing\|mismatch\|replayed\|unproved` carries no accepted name projection and can never authorize. No receipt contains the value or a reconstructive value digest |
| Overlay secret classification | `secret_shaped`, `not_secret_shaped`, `unknown`, `not_applicable`; never an authorizing input |
| Fixed-helper decoder | `closed_valid`, `closed_invalid`, `free_form_stdout`, `free_form_stderr`, `truncated`, `non_utf8`, `not_applicable` |
| Decoder grammar binding | `exact_fixed_helper`, `exact_targeted_names`, `exact_full_census`, `exact_private_resolver`, `mismatch`, `unproved`, `not_applicable` |
| Diagnostic source | `typed_internal`, `overlay_input`, `helper_or_connector_stderr`, `os_or_library_error`, `remote_error`, `rejected_request_field`, `path`, `endpoint`, `malformed_plan`, `not_applicable` |
| Sanitizer result | `safe_typed_code`, `safe_enum`, `bounded_count`, `structurally_sanitized`, `omitted`, `raw_or_unproved`, `not_applicable` |
| Sanitizer proof | `exact_closed_code_or_enum_conversion`, `exact_source_schema_non_content_operational_count`, `exact_structural_sanitizer`, `payload_length_or_byte_derived`, `mismatch`, `unproved`, `not_applicable`. The count proof binds an approved source-specific diagnostic schema/revision and an enumerated operational-count derivation independent of source payload bytes/content; secret length, byte value, character count, hash bucket, or any caller-selected arithmetic is `payload_length_or_byte_derived` and forbidden |
| Diagnostic numeric-bound proof | `exact_frozen_field_bound_and_within`, `exceeded`, `mismatch`, `unproved`, `not_applicable`; active only for `bounded_count`, and the exact form binds field identity, frozen cap, observed nonnegative count, and within result without carrying source content |

Property selection first applies this closed compatibility projection. The
authority-chain set, rather than one ambient profile, is the policy source. The
   executable bundle is origin, writability, trust, form, binding, dispatch, and adapter controls. The
   channel bundle is boundary, security, channel identity/binding proof, per-message binding,
   complete verifier-local channel/message freshness records and coverage, payload class, and
   overlay crossing. Operator pin use/proof is a conditional cross-cutting bundle
   for executable or channel trust. The persistence bundle is persistence policy, its source/default proof, name policy,
value provenance, and secret classification. Every field called neutral is
exactly `not_applicable`; an inconsistent projection is rejected before any
decision function runs. More strongly, every input axis not explicitly named
active or required by the selected property row and, for fact admission, its
producer subrow is exactly `not_applicable`. There are no ambient or don't-care
fields; an arbitrary value in an unselected bundle rejects the assignment.

The two causal-source axes are globally conditional after property projection and
do not make a neutral property axis active. Zero covered receipt-emitting causal
negative/effect values requires `zero + not_applicable`; exactly one requires
`one + not_applicable`. Two or more are accepted only as one of two closed forms:

- `many + exact_same_source_projection_set`: exactly one primary receipt and every
  additional value is its exact projection with matching incident digest, source
  axis/sub-key, scope, primary-receipt digest, and mapped disposition.
- `many + exact_same_incident_bound_effect_bundle`: every value is a distinct
  private-input `reached_and_stopped` source paired with its exact scope/limit/
  counter observation, all bind the same incident, scope,
  `bound_reached` disposition, and exact closed Transition bound-effect bundle,
  and the member-key set is complete with no duplicate or projection.

A covered source paired with `zero`, any other cardinality/correlation pairing,
or any independent, missing, mismatched, incomplete, or conflicting correlation
rejects before a Security output receipt. In particular, a private-input stop and
an upstream replay cannot coexist as one effect: their dispositions conflict.

| Property | Exact sensor/upstream projection | Active property bundle | Neutral bundles |
|---|---|---|---|
| `sensor_authorization` | non-`not_applicable` sensor class plus complete authority-chain records; census/start per-run fields follow their class; gated-action audit admission and its correlation are active over full domains; all upstream verdicts/correlations are neutral | authority-chain policy, class, and action-audit admission | executable, channel, persistence, decoder, diagnostic |
| `private_environment_resolution` | payload `observed_environment_opaque_input`; class exactly `private_platform_resolver`; requested context plus resolver/source/locality/retention/output/fact-kind/binding/conversion/resolved-executable/revision and private-tainted producer/sink coverage-attestation axes active over full domains; `opaque_input_bytes` and `resolver_candidates` verdicts plus private-input counter observation active; bound sensor and applicable spawned-helper spawn/decoder verdict/correlation axes active over full domains; bound upstream private-resolution verdict and its correlation are neutral because this property emits that receipt | private resolver plus upstream policy, private input bounds, and sink-complete runtime/build attestation | unrelated spawn-executable trust, channel, persistence, and diagnostic axes |
| `executable_spawn` | a spawn-capable class; execution `spawned`; bound sensor verdict/correlation, complete authority-chain/executable axes, catalogue adapter/invocation-control proof, conditional operator-pin bundle, and a distinct spawn-action audit admission/correlation active over full domains | authority chain, executable, adapter invocation controls, conditional pin trust, and spawn-action audit | channel, persistence, decoder, diagnostic |
| `fixed_helper_decode` | class `fixed_helper`, spawned `targeted_names`, spawned `full_census`, or spawned `private_platform_resolver`; execution `spawned`; bound sensor/spawn verdict/correlation, decoder grammar, fixed-helper decoder value, `decoder_input_bytes` verdict, private-input counter observation, and private-tainted producer/sink coverage-attestation axes active over full domains; bound upstream decoder verdict and decoder-receipt correlation neutral because this property emits that receipt | closed typed decoder pipeline plus bounded-while-read input and sink-complete runtime/build attestation | unrelated resolved-target identity, channel, persistence, diagnostic |
| `channel_fact_admission` | payload `probe_evidence`; overlay crossing `none`; evidence producer selects the full-domain immediate-upstream subrow below; fact-admission phase selects exactly one final-boundary or soft-bound-precommit subrow; candidate relation, evidence-channel axes, and conditional channel-pin bundle active as that subrow requires | producer receipt plus exactly one boundary/precommit proof path plus candidate-fact relation plus evidence channel/pin trust | executable identity, all persistence, diagnostic |
| `surface_name_application` | payload `operation_name_metadata`; one non-`not_applicable` surface-name consumer; preliminary native-name verdict and private surface-name observation active over full domains; every bound upstream verdict/correlation neutral because this property emits the pre-composition application receipt | name-only non-campaign application before value/path/allowlist access | authority, executable, resolver, decoder, evidence channel, private overlay channel/value, persistence, diagnostic |
| `overlay_transport` | payload `overlay_private_material`; exactly one private-path producer subrow below is selected; its verdict/correlation, transport-ingress provenance proof/correlation, bound surface-name application verdict/correlation, and channel boundary/security/identity-binding/message/freshness/confidentiality/endpoint plus conditional channel-pin axes are active over full domains; bound upstream private-channel verdict/correlation neutral because this property emits that receipt; sensor/evidence/fact inputs neutral | exact caller ingress plus exact operation/name/handle application plus authorized private path and private channel/pin trust; persistence operation is active only for a snapshot producer | environment-sensor authority, executable sensor, evidence decoder, every unselected persistence axis, diagnostic |
| `overlay_persistence` | payload `overlay_private_material`; exact omission/rejection or operation-specific candidate subrow below selects whether canonical-name, private-path, and private-channel axes are neutral or active over full domains | canonical-name-bound store/restore decision | environment-sensor authority, executable sensor, evidence decoder, diagnostic |
| `diagnostic_egress` | payload `diagnostic_private_input`; authority, execution, and sensor/spawn/decoder/private-channel verdicts neutral; source, result, sanitizer proof/correlation, and conditional numeric-bound proof follow the closed diagnostic tuple | diagnostic | executable, channel, persistence, decoder |

Operator-pin provenance is closed and never trust-on-first-use:

| Operator pin use | Origin/management proof | Exact result |
|---|---|---|
| `executable_trust` | `preexisting_operator_config_current\|exact_audited_operator_add_current\|exact_audited_operator_rotate_current` | eligible only when executable trust provenance is `operator_pinned` and the exact pin identity/revision matches |
| `channel_trust` | the same three current proofs | eligible only when the exact channel identity/binding proof declares the matching operator-pin basis |
| `executable_trust\|channel_trust` | `exact_audited_operator_revoke\|llm_or_caller_supplied\|trust_on_first_use\|mismatch` | `denied`; no spawn/channel/transport/authority receipt; preserve the typed pin-management cause in audit. Executable provenance `pin_mismatch` requires `executable_trust + mismatch`; no other pair represents it |
| `executable_trust\|channel_trust` | `unproved` | `unknown`; no spawn/channel/transport/authority receipt; only bounded operator-side proof refresh may continue |
| `not_used` | `not_applicable` | valid for catalogue/canonical executable trust or authenticated-transport channel identity. Executable provenance `unpinned\|unknown` also requires this exact pair but derives typed `unknown`, starts no spawn, and emits no authority receipt. It cannot satisfy or upgrade an operator-pin claim |
| `not_applicable` | `not_applicable` | valid only for a selected property with no executable/channel trust input |
| any required use with proof `not_applicable`, `not_used` with a nonneutral proof, a non-pin executable/channel claim with pin use, or any other pair | any | structural rejection; `receipt_effect = rejected` |

An LLM-facing operation cannot create, accept, rotate, or revoke a pin. Those are
operator-only audited administration actions; this model consumes their immutable
current proof but never performs trust-on-first-use.

`private_environment_resolution` source/context binding is closed:

| Requested context | Observed-value source | Exact result |
|---|---|---|
| `target_process` | `target_process_environment` | context match; continue only when the plan, sensor admission, target persistent identity, boot, workspace, and action are exact |
| `target_shell_or_session` | `target_shell_or_session_environment` | context match; continue only with the same exact bindings plus the requested shell/session identity |
| `harness_process` | `harness_environment` | context match only for an explicit harness-context plan row whose `EnvironmentPrivateResolve` admission binds this harness persistent identity, boot, workspace, policy, and action; a generic `HarnessContext` sensor receipt cannot substitute |
| any cross-pair | any nonmatching source | `rejected`; destroy opaque input, emit no fact or authority receipt, and do not fall back to ambient harness state |
| `not_applicable` on either axis | any | structural `rejected` before resolution; no receipt or fact |

Every successful private-resolution receipt preserves the requested context and
observed source kind plus their exact target/harness identity bindings, but no raw
or reconstructive environment value.

`surface_name_application` uses one closed preliminary-verdict/observation tuple:

| Preliminary native-name verdict | Private surface-name observation | Selected application/effect |
|---|---|---|
| `canonical_utf8\|canonical_posix_bytes\|canonical_windows_utf16` | `not_reached` | `canonical`; issue the exact scope/consumer-bound non-authorizing application receipt |
| `omitted_whole_name_at_bound` | `exact_surface_name_scope_limit_and_counter` | command/PTY/session `rejected`, snapshot persistence `omitted`; issue the exact non-authorizing application receipt binding the whole-name stop and counter |
| `rejected` | `not_reached` | `rejected`; issue the exact non-authorizing application receipt preserving the preliminary rejection |
| `unknown` | `not_reached` | `unknown`; `receipt_effect = no_receipt` |
| any non-`not_applicable` preliminary verdict | `unproved` | `unknown`; no receipt because the surface observation or bound/counter state is not proved |
| any non-`not_applicable` preliminary verdict | `missing\|mismatch\|replayed` | `rejected`; no application receipt, causal count, or downstream use |
| canonical/rejected/unknown with `exact_surface_name_scope_limit_and_counter`, omitted-with-bound with `not_reached`, preliminary `not_applicable`, observation `not_applicable`, or surface consumer `not_applicable` | wrong/extraneous/inapplicable tuple | structural `rejected`; `receipt_effect = rejected`; no receipt |

Only the first three exact rows can emit a surface application receipt. In
particular, private-observation replay is the deliberate no-receipt class and can
never be counted as a generic replay correlation.

`overlay_transport` private-path producer subrows are closed:

| Producer | Exact path/operation projection | Persistence axes |
|---|---|---|
| command/PTY/session execution | exactly one `command\|pty\|session` allowed-path verdict with its matching scope correlation | persistence operation and every other persistence axis `not_applicable` |
| snapshot store transport | path exactly `snapshot_store`; persistence operation exactly `store`; both receipts bind the same snapshot, action, direction, and opaque handle | persistence policy, name policy, provenance, and secret classification `not_applicable` |
| snapshot restore transport | path exactly `snapshot_restore`; persistence operation exactly `restore`; both receipts bind the same snapshot, action, direction, and opaque handle | persistence policy, name policy, provenance, and secret classification `not_applicable` |

Any command/session path with an active persistence operation, snapshot path with a
neutral or opposite operation, or second producer rejects before channel
classification.

`channel_fact_admission` producer subrows are closed:

| Producer | Required immediate upstream inputs | Inputs that MUST be `not_applicable` |
|---|---|---|
| `direct_passive` | bound sensor verdict/correlation full domains; execution `in_process`; spawn `not_executing` | private-resolution and decoder verdict/correlation; private execution path |
| `private_environment_resolution` | bound private-resolution verdict and resolver-correlation full domains; its receipt encapsulates the sensor/helper/decoder chain | raw sensor, spawn, and decoder verdict/correlation; private execution path |
| `fixed_helper` | bound decoder verdict/correlation full domains; its receipt encapsulates sensor/spawn chain | raw sensor, spawn, and private-resolution verdict/correlation; private execution path |
| `spawned_typed_sensor` | class `targeted_names\|full_census`; bound decoder verdict/correlation full domains; its receipt encapsulates the exact semantic sensor/spawn/grammar chain | raw sensor, spawn, and private-resolution verdict/correlation; private execution path |
| `connector_native` | bound sensor verdict/correlation full domains; execution `in_process`; spawn `not_executing` | private-resolution and decoder verdict/correlation; private execution path |

Every producer subrow additionally activates the bound evidence-boundary verdict,
or the precommit proof as selected below, the candidate fact/boundary relation,
and evidence-channel boundary/security/identity-binding/message-binding fields, and neutralizes
all overlay persistence/value fields. The immediate producer
receipt proves how the typed observation was produced; the independently exact
EvidenceBoundary receipt proves its native-name/census/boundary projection. Both
are required. A complete or presentation-bounded receipt admits only
`exact_unchanged`; an evidence-incomplete receipt admits only
`exact_same_typed_incomplete`, which binds the identical truncation/bound cause
and affected evidence set. `mismatch` rejects and `unproved` derives unknown.
Boundary rejection rejects fact admission, and unknown/unproved boundary evidence
derives unknown; those non-positive rows require relation `not_applicable`.

Fact-admission phase is a closed two-row projection:

| Admission phase | Exact boundary/precommit projection |
|---|---|
| `final_boundary_receipt` | candidate staging state `unstaged`; composed EvidenceBoundary verdict and correlation active over their full domains; precommit proof `not_applicable`; complete/presentation-bounded requires `exact_unchanged`, evidence-incomplete requires `exact_same_typed_incomplete` |
| `soft_bound_precommit` | candidate staging state `unstaged`; composed EvidenceBoundary verdict and its correlation `not_applicable`; precommit proof active over its full domain and bound to the exact private per-sensor-time observation, frozen soft/informational relevance closure, typed incomplete candidate, proposed Transition batch digest, and final expected boundary scope; candidate relation must be `exact_same_typed_incomplete` |

The precommit row derives at most `trusted_pending_effect_commit`; it does not
publish a fact, claim cleanup, or substitute for the final boundary receipt. All
properties other than `channel_fact_admission` require phase and precommit proof
`not_applicable`.

`private_environment_resolution` implementation and output subrows are closed:

| Resolver implementation | Exact execution and upstream projection |
|---|---|
| `approved_platform_native` | class `private_platform_resolver`; execution `in_process`; sensor verdict/correlation active over their full domains; binding proof must be evaluated as `exact_platform_api_and_impl_revision\|mismatch\|unproved`; spawn and decoder verdict/correlation neutral |
| `approved_fixed_helper` | class `private_platform_resolver`; execution `spawned`; sensor, spawn, decoder, and their correlations active over their full domains; binding proof must be evaluated as `exact_helper_executable_and_decoder\|mismatch\|unproved`; the universal executable-spawn layer applies, and the decoder receipt encapsulates its separate `decoder_input_bytes` gate |
| `caller_defined_or_unknown` | class `private_platform_resolver`; execution may be either implementation value; policy/binding/conversion axes remain active so the ordered derivation rejects it; no successful receipt is possible |

Fact kind `presence_only` requires resolved-executable binding
`not_applicable`. Fact kind `executable_identity\|executable_version` requires the
resolved-executable binding axis active over its full domain; the exact positive
value is `exact_binary` for a native binary and
`exact_interpreter_and_script` for a script or shim. A fact-kind/output mismatch,
an arbitrary path, or an executable fact without exact binding rejects. These
subrows use the resolver-specific closed conversion proof; generic diagnostic
sanitizer fields remain neutral and cannot launder a value into evidence.

Evidence and overlay channel subrows are independently closed:

| Flow | Exact positive channel tuple |
|---|---|
| same-process typed evidence | producer's exact immediate receipt; boundary `same_process`; channel security, channel identity/binding proof, message binding, freshness records/coverage, confidentiality scope, and endpoint binding `not_applicable`; overlay crossing `none` |
| authenticated local-process typed evidence | producer's exact immediate receipt; boundary `authenticated_local_process`; security `authenticated_integrity`; channel identity/binding proof `exact_authenticated_local_binding`; message binding `complete`; one exact local authenticated-edge freshness record, coverage `exact_complete_ordered_edge_set`, and verdict exact within age; confidentiality scope and endpoint binding `not_applicable`; overlay crossing `none` |
| forwarded/remote or guest typed evidence | producer's exact immediate receipt; boundary `forwarded_or_remote\|guest`; security `authenticated_integrity\|authenticated_confidentiality_and_integrity`; channel identity/binding proof `exact_authenticated_forwarded_or_guest_binding`; message binding `complete`; coverage `exact_complete_ordered_edge_set` over every authenticated origin/forwarder/target edge and every record exact within age; confidentiality scope and endpoint binding `not_applicable`; overlay crossing `none` |
| same-process overlay | crossing `same_process_private`; boundary `same_process`; channel security, channel identity/binding proof, message binding, and freshness records/coverage `not_applicable`; confidentiality scope `same_process_private`; endpoint binding `not_applicable`; exact allowed command/PTY/session or operation-matched snapshot store/restore receipt bound to the opaque material handle |
| authenticated local-process overlay | crossing `authenticated_local_private`; boundary `authenticated_local_process`; security `authenticated_confidentiality_and_integrity`; channel identity/binding proof `exact_authenticated_local_binding`; message binding `complete`; one exact local authenticated-edge freshness record with exact coverage and within-age verdict; confidentiality scope `authenticated_peer_to_peer`; endpoint binding `exact_origin_and_final_target_route`; exact allowed private execution receipt and opaque-handle/AEAD binding |
| connector/guest overlay | crossing `cross_boundary`; boundary `forwarded_or_remote\|guest`; security `authenticated_confidentiality_and_integrity`; channel identity/binding proof `exact_authenticated_forwarded_or_guest_binding`; message binding `complete`; coverage `exact_complete_ordered_edge_set` over every authenticated edge/verifier and every record exact within age; confidentiality scope `origin_to_final_end_to_end`; endpoint binding `exact_origin_and_final_target_route`; exact allowed private execution receipt and opaque-handle/AEAD binding; every forwarder sees ciphertext and routing metadata only |

Gated-action audit admission and its action-audit receipt correlation form one
closed tuple before any authorization decision:

| Audit admission | Action-audit receipt correlation | Causal classification and result |
|---|---|---|
| `exact_committed_per_authorizing_and_executing_node` | `exact_match` | `zero + not_applicable`; positive audit admission candidate |
| `exact_committed_per_authorizing_and_executing_node` | any receipt-emitting mismatch, `replayed`, or `expired` | `one + not_applicable`; correlation is the sole primary causal source and follows its exact source-total disposition |
| `exact_committed_per_authorizing_and_executing_node` | `unproved` | `zero + not_applicable`; no receipt, effect, or action; `sensor_authorization` derives `unavailable`, `executable_spawn` derives `unknown`, and only bounded fresh audit proof may continue |
| `exact_committed_per_authorizing_and_executing_node` | `not_applicable` or a mismatch class inactive for this action scope | structural rejection; a committed admission requires one applicable correlation result, and no receipt/count is fabricated |
| `mismatch` | `exact_match` | `one + not_applicable`; admission is the sole primary source, emits `exact_action_audit_integrity_failure`, and maps only to `action_audit_mismatch` |
| `replay` | `exact_match` | `one + not_applicable`; admission is the sole primary source, emits `exact_action_audit_integrity_failure`, and maps only to `action_audit_replay` |
| `mismatch\|replay` | any receipt-emitting mismatch, `replayed`, or `expired` | `many + exact_same_source_projection_set` is accepted only when one receipt is primary and every other value is its exact same-source, same-scope, same-incident, same-disposition projection; otherwise reject. In particular, admission `mismatch` plus correlation `replayed` is a disposition conflict and rejects |
| `mismatch\|replay` | `unproved` | fail the action closed: `sensor_authorization\|executable_spawn` derives its selected denied decision, `zero + not_applicable`, no receipt or Transition, and only bounded fresh audit/integrity proof may continue; an uncorrelated admission value cannot hard-exclude |
| `mismatch\|replay` | `not_applicable` or a mismatch class inactive for this action scope | structural rejection; no receipt, count, hard exclusion, or Transition |
| `not_attempted\|failed\|unavailable` | `unproved\|not_applicable` | `zero + not_applicable`; no audit causal receipt; preserve the policy denial or unavailable result |
| `not_attempted\|failed\|unavailable` | `exact_match`, any mismatch, `replayed`, or `expired` | structural rejection because no committed admission receipt exists to correlate; no receipt/count/effect |
| `not_applicable` | `not_applicable` | `zero + not_applicable`; valid only where the selected property has no audit input |
| `not_applicable` | any non-`not_applicable` correlation | structural rejection; sensor/spawn properties cannot use audit-neutral admission and no receipt/count/effect is emitted |

Every other audit tuple rejects before a Security result or source receipt. An
audit-admission negative and action-audit correlation negative can therefore
never be counted as one source accidentally or choose between mismatch and
replay.

Channel identity/binding negatives have one closed scope-aware disposition:

| Proof value | Probe-campaign disposition | Non-campaign operation disposition |
|---|---|---|
| `fresh_nonce_replayed\|sequence_replay` | Security emits the exact typed non-authorizing receipt; Transition consumes it as hard exclusion `connector_replay_detected` while the receipt preserves nonce versus sequence detail | abort/reject that exact operation scope with the same typed receipt; no Transition or campaign is invented |
| `expired` | reject the current channel proof/message; consume the exact Security receipt as recoverable cause `proof_expired`, using `proof_failure/proof_expired` for a proof-owning ready root or `recoverable_invalidation/hard_invalidation` otherwise, and perform only bounded fresh re-attestation/re-gating | abort/reject that exact operation scope and permit retry only after fresh authorization; no Transition or campaign is invented |
| `unapproved_rotation` | hard exclusion `connector_unapproved_rotation` | abort/reject exact operation scope; no Transition |
| `clone_detected` | hard exclusion `connector_clone_detected` | abort/reject exact operation scope; no Transition |
| `persistent_instance_mismatch` | hard exclusion `target_persistent_identity_mismatch` | abort/reject exact operation scope; no Transition |
| `engine_boot_mismatch` | campaign-wide `engine_loss/shutdown_or_engine_lost` with cause `engine_boot` and complete cleanup | abort/reject exact operation scope; no Transition |
| `target_boot_mismatch` | hard exclusion `target_boot_mismatch` | abort/reject exact operation scope; no Transition |
| `protocol_mismatch\|endpoint_route_or_connector_context_mismatch\|trust_root_or_authenticated_identity_mismatch` | hard exclusion `connector_binding_mismatch` while the Security receipt preserves the exact mismatch class | abort/reject exact operation scope with the exact mismatch class; no Transition |
| `mid_run_replacement` | hard exclusion `connector_mid_run_replacement` | abort/reject exact operation scope; no Transition |
| `missing\|unproved` | Security result `unknown`; no downstream action or Transition; bounded fallback/re-attestation may continue | typed unknown/unavailable operation result; no transport, Transition, or campaign |

Every receipt-emitting probe-campaign disposition row above (all except
`missing\|unproved`) must carry the mapped Security receipt as its exact
hard-exclusion, engine-loss, or recoverable-expiry causal input. A different
cause, a missing receipt, or applying a probe Transition to a non-campaign scope
rejects. The `missing\|unproved` unknown row instead uses
`zero + not_applicable` unless a different receipt-emitting causal negative is
present.

Per-message action/audit binding negatives are independently total and never
borrow the channel-establishment mapping merely because both domains contain
`sequence_replay\|expired`:

| Per-message value | Probe-campaign disposition | Non-campaign operation disposition |
|---|---|---|
| `missing_action_digest\|missing_policy_revision\|missing_workspace_or_boot\|missing_audit_receipt` | emit an exact non-authorizing Security receipt with source `per_message_action_audit`; hard exclusion `action_audit_mismatch` with `exact_action_audit_integrity_failure` | abort/reject exact operation scope with the typed missing-field receipt; no Transition or campaign |
| `sequence_replay` | emit the source-bound non-authorizing Security receipt; hard exclusion `action_audit_replay` with `exact_action_audit_integrity_failure` | abort/reject exact operation scope with typed replay receipt; no Transition or campaign |
| `expired` | emit the source-bound non-authorizing expiry receipt; use recoverable `proof_expired` exactly as in the channel-expiry row, never an audit-replay exclusion | abort/reject exact operation scope and retry only after fresh authorization; no Transition or campaign |

`complete` message binding plus freshness coverage
`exact_complete_ordered_edge_set` and an exact-within-age record for every
authenticated ordered edge/verifier is required by every positive authenticated
channel tuple; freshness records and coverage `not_applicable` are valid only for
same-process/no-channel rows. Each negative
receipt binds its source axis so a channel-nonce replay cannot be relabelled as an
audit replay, or vice versa.

The remaining correlation/replay/expiry-capable axes are also source-total:

| Causal source/value | Probe-campaign disposition | Non-campaign operation disposition |
|---|---|---|
| action-audit `Receipt correlation = action_digest_mismatch\|sensor_class_mismatch\|policy_revision_mismatch\|authority_chain_or_connector_route_mismatch\|target_boot_workspace_mismatch\|campaign_branch_mismatch\|command_job_scope_mismatch\|pty_scope_mismatch\|session_scope_mismatch\|snapshot_operation_scope_mismatch` | exact non-authorizing receipt preserving the mismatch subclass -> hard exclusion `action_audit_mismatch` with `exact_action_audit_integrity_failure` | abort/reject the exact action scope with the typed mismatch receipt; no Transition or campaign |
| action-audit `Receipt correlation = replayed` | exact non-authorizing source receipt -> hard exclusion `action_audit_replay` with `exact_action_audit_integrity_failure` | abort/reject exact scope; no Transition |
| applicable non-audit `Receipt correlation = action_digest_mismatch\|sensor_class_mismatch\|policy_revision_mismatch\|authority_chain_or_connector_route_mismatch\|target_boot_workspace_mismatch\|campaign_branch_mismatch\|command_job_scope_mismatch\|pty_scope_mismatch\|session_scope_mismatch\|snapshot_operation_scope_mismatch\|private_payload_binding_mismatch`, except diagnostic sanitizer correlation | exact non-authorizing source receipt preserving the mismatch subclass -> reject that receipt/fact, commit the exact terminal sensor `rejected` result when applicable, and follow only bounded fallback; no connector hard exclusion is fabricated | abort/reject the exact owning property/action scope with the typed mismatch receipt; no Transition or campaign |
| channel-bound `Receipt correlation = replayed` | exact non-authorizing source receipt -> hard exclusion `connector_replay_detected` | abort/reject exact scope; no Transition |
| same-process non-audit `Receipt correlation = replayed`, except diagnostic sanitizer correlation | reject that receipt/fact, commit the exact terminal sensor `rejected` result when applicable, and follow only bounded fallback; no hard-exclusion Transition is fabricated | abort/reject exact scope; no Transition |
| any `Receipt correlation = expired`, except diagnostic sanitizer correlation | if it supports a current proof, recover through `proof_failure/proof_expired`; otherwise reject this receipt and require bounded fresh observation without a state transition | abort/reject exact scope and retry only after fresh authorization; no Transition |
| diagnostic sanitizer `Receipt correlation = replayed\|expired` on an otherwise source-compatible non-omitted diagnostic result | `diagnostic_egress = rejected`; emit nothing and start no Transition | reject diagnostic egress; `receipt_effect = no_receipt`; no Transition or campaign |
| `Channel security = replayed` | exact non-authorizing source receipt -> hard exclusion `connector_replay_detected` | abort/reject exact scope; no Transition |
| `Channel security = expired` | recover through `proof_expired` when a current proof is affected; otherwise reject the channel and require fresh attestation | abort/reject exact scope and retry only after fresh authorization; no Transition |
| `Private endpoint binding = mismatch` | structurally unreachable: this axis is active only for non-campaign overlay transport/persistence; reject the assignment before any effect, campaign, or Transition | emit the exact non-authorizing `connector_binding_mismatch` receipt preserving endpoint subcause and abort/reject the exact overlay operation scope; no Transition or campaign |
| `Private endpoint binding = replayed` | structurally unreachable: this axis is active only for non-campaign overlay transport/persistence; reject the assignment before any effect, campaign, or Transition | abort/reject the exact overlay operation scope; no Transition or campaign |
| `Private endpoint binding = expired` | structurally unreachable: this axis is active only for non-campaign overlay transport/persistence; reject the assignment before any effect, campaign, or Transition | abort/reject the exact overlay operation scope and retry only after fresh authorization; no Transition or campaign |

An action-audit mismatch subclass is receipt-emitting only when that field is
active for the exact action scope. Campaign/branch is active only for campaign
work; command/job, PTY, session, and snapshot-operation mismatch classes are each
active only for their matching non-campaign action. `private_payload_binding_mismatch`
and any mismatch class for an inactive audit field are structural rejections,
produce no receipt, and do not enter causal counting.

Receipt-correlation replay/expiry is property-total and is evaluated before the
numbered property derivations. An active row below selects the stated decision;
no later rule may authorize the stale/replayed input:

| Selected property | `Receipt correlation = replayed` | `Receipt correlation = expired` | Receipt/effect |
|---|---|---|---|
| `sensor_authorization` | `sensor_decision = denied`; no action; action-audit replay uses its exact hard-exclusion mapping | `sensor_decision = unavailable`; no action; require fresh audit/authorization | `issued_exact_non_authorizing_effect` bound to the active correlation source |
| `private_environment_resolution` | `private_resolution = rejected`; no resolver fact or action | `private_resolution = unknown`; no resolver fact or action; require fresh authorization/observation | `issued_exact_non_authorizing_effect` |
| `executable_spawn` | `spawn_decision = denied`; no spawn | `spawn_decision = unknown`; no spawn; require fresh authorization | `issued_exact_non_authorizing_effect` |
| `fixed_helper_decode` | `decoder_admission = typed_failure`; no evidence | `decoder_admission = typed_failure`; no evidence; require fresh producer/decoder receipt | `issued_exact_non_authorizing_effect` |
| `channel_fact_admission` | `fact_admission = rejected`; no fact or pending effect | `fact_admission = unknown`; no fact or pending effect; require fresh observation/boundary proof | `issued_exact_non_authorizing_effect` |
| `surface_name_application` | structural rejection because all Receipt-correlation inputs are neutral for this property | structural rejection because all Receipt-correlation inputs are neutral for this property | `receipt_effect = rejected`; no receipt |
| `overlay_transport`, active correlation sub-key `private_execution_path\|surface_name_application\|overlay_ingress_provenance` | `private_channel = rejected`; no transport | `private_channel = unknown`; no transport; require fresh proof for that exact sub-key | `issued_exact_non_authorizing_effect` bound to the existing immutable source receipt and sub-key |
| `overlay_persistence`, active correlation sub-key `canonical_evidence_boundary\|overlay_persistence_provenance\|private_execution_path\|private_channel` | `overlay_persistence = rejected`; no store/restore | `overlay_persistence = unknown`; no store/restore; require fresh proof for that exact sub-key | `issued_exact_non_authorizing_effect` bound to the existing immutable source receipt and sub-key |
| `diagnostic_egress` | `diagnostic_egress = rejected`; emit nothing | `diagnostic_egress = rejected`; emit nothing | `receipt_effect = no_receipt`; causal axes `zero + not_applicable` unless another covered source exists |

For properties with several active correlations, the exact expired/replayed
sub-key is preserved. Covered sub-keys include channel-fact producer and
evidence-boundary receipts; overlay-transport private execution-path,
surface-name-application, and caller-ingress-provenance receipts; and
overlay-persistence canonical EvidenceBoundary, persistence-provenance,
private-path, and private-channel receipts. Each entry means the downstream
`Receipt correlation` axis observed replay/expiry of an existing immutable source
receipt; it never means that a proof-axis enum merely claimed `replayed`.
Precommit proof replay remains the
separate no-receipt rejection defined in channel-fact admission. Simultaneous covered sources still must satisfy the causal
cardinality/correlation rule; a diagnostic sanitizer replay/expiry never becomes
a receipt-emitting projection.

Replay causality is partitioned by source axis before property projection:

| Replay class | Exact causal/receipt treatment |
|---|---|
| gated-action audit admission/correlation replay; channel-bound generic Receipt-correlation replay; same-process non-audit downstream immutable-receipt correlation replay except diagnostic sanitizer; channel-security replay; channel-identity nonce/sequence replay; per-message sequence replay; non-campaign private-endpoint replay | receipt-emitting causal negative; include it in effect-source cardinality, preserve the exact source/sub-key/scope, and apply its closed disposition plus property decision. This includes exact downstream surface-name-application, overlay-ingress-provenance, and overlay-persistence-provenance correlation sub-keys only when their immutable source receipt exists |
| precommit bound-admission proof enum `replayed`; private surface-name observation enum `replayed`; private-input counter-observation enum `replayed`; Overlay transport ingress provenance proof enum `replayed`; Overlay provenance receipt enum `replayed`; diagnostic sanitizer correlation replay/expiry | deliberate no-receipt rejection; select the owning property's `rejected`/typed-failure result, `receipt_effect = no_receipt` (or `rejected` for a structural projection), and `zero + not_applicable` absent another covered source |
| EvidenceBoundary requested-set, full-census-consent, private name-observation, or other bound-proof enum marked `replayed` in its own closed domain | EvidenceBoundary checker rejection with `boundary_receipt_effect = rejected`; emit no public boundary receipt and do not enter Security causal counting |

No replay value may migrate between these classes merely because another axis
uses the same word. In particular, the private-channel correlation input is
neutral while `overlay_transport` emits that receipt; it becomes an active bound
input only in the applicable downstream `overlay_persistence` projection.

The full-domain channel tuple then uses this ordered, total combination function.
Its inputs are the selected boundary row plus channel security, identity/binding,
per-message binding, freshness records/coverage, confidentiality, endpoint,
active receipt correlations, causal cardinality, and multi-source correlation.
The combination result is derived, never caller-supplied:

| First matching combination class | One final decision | One receipt/Transition effect |
|---|---|---|
| any wrong active/neutral shape, impossible boundary/crossing, or required-N/A/extraneous tuple | structural `rejected` | `receipt_effect = rejected`; no receipt, authority, fallback mutation, or Transition |
| two or more receipt-emitting sources with `independent_sources\|source_mismatch\|incident_mismatch\|scope_mismatch\|disposition_conflict\|missing`, or causal count/receipt-set disagreement | `rejected` as an inconsistent security snapshot; no downstream action | existing immutable source receipts remain audit evidence but no new aggregate receipt or Transition is issued; require bounded fresh attestation |
| causal count `many + exact_same_incident_bound_effect_bundle`, with complete distinct member-key coverage, exact shared incident/scope/bundle identity, and any number of compatible no-receipt negatives | preserve every member's exact `bound_reached` property-local disposition and choose the stricter public decision across the complete member set plus the no-receipt lattice `rejected > unknown > positive` | issue/consume exactly the complete member receipt set as transaction-bound inputs to one atomic `bound_effect_bundle_commit`, preserving the exact cleanup union; no member is collapsed into a projection, no hard exclusion is fabricated, and no separate Transition is issued |
| exactly one receipt-emitting causal source, or several exact same-source/same-incident/same-disposition projections, with any number of compatible no-receipt negatives | preserve that source's exact closed disposition and choose the stricter public decision across it and the no-receipt lattice `rejected > unknown > positive` | issue/consume exactly the source-total receipt and its mapped hard exclusion, engine loss, recoverable expiry, audit-integrity effect, campaign-local property rejection (including the exact terminal sensor `rejected` result when applicable, bounded fallback only, and no hard-exclusion Transition), or non-campaign abort; no-receipt axes emit nothing and MUST NOT erase, downgrade, or remap it |
| causal count `zero + not_applicable`, no structural or receipt-emitting source, and at least one no-receipt typed rejection (for example `unauthenticated`, confidentiality failure, freshness mismatch/coverage-integrity failure) | `rejected` | `no_receipt`; no authority or Transition |
| causal count `zero + not_applicable`, no rejection, and at least one no-receipt unknown (for example missing identity, unavailable security, freshness expiry/continuity loss, or unproved endpoint) | `unknown` | `no_receipt`; only the bounded fallback/re-attestation named by the owning row; no Transition |
| no negative class and the exact selected positive tuple holds | the selected positive channel/fact/private-transport decision | its one exact authorizing receipt; no negative effect |
| every remaining assignment | structural `rejected` | `receipt_effect = rejected`; no receipt or Transition |

Thus identity `fresh_nonce_replayed` plus freshness expiry preserves the replay
hard-exclusion effect, while identity `missing` plus security `unauthenticated`
derives the no-receipt rejected row. The function is applied before every
property-specific success rule; no later derivation may select a different result.

All other supported nonpositive channel values have one closed mapping:

| Nonpositive condition | Exact result and bounded continuation | Receipt/Transition effect |
|---|---|---|
| required channel security is `local_authenticated_peer\|point_challenge_only\|unavailable` | typed `unknown`; no fact or private transport; only bounded stronger-attestation or alternate-route fallback | `no_receipt`; no authority or Transition |
| channel security is `unauthenticated` | typed `rejected`; reject the current fact/private transport; only bounded alternate-route fallback | `no_receipt`; no authority or Transition |
| overlay channel security is `authenticated_integrity` without authenticated confidentiality | typed `rejected`; reject the private transport; only bounded alternate-route fallback | `no_receipt`; no authority or Transition |
| overlay confidentiality scope is `hop_only` | typed `rejected`; hop protection cannot authorize end-to-end private transport; only bounded alternate-route fallback | `no_receipt`; no authority or Transition |
| boundary/crossing tuple contradicts its selected evidence/overlay row, including non-`not_applicable` channel fields on same-process evidence or crossing/boundary disagreement | structural `rejected` before classification | `no_receipt`; no authority, fallback effect, or Transition |
| overlay confidentiality is `unproved`, or private endpoint binding is `missing\|unproved` | typed `unknown`; no transport; only bounded fresh proof or alternate-route fallback | `no_receipt`; no authority or Transition |
| freshness coverage `missing_edge\|extra_edge\|duplicate_edge\|root_or_count_mismatch` | typed `rejected`; reject the complete message/channel scope because the authenticated route and verifier set are not equal | `no_receipt`; no authority or Transition |
| freshness coverage `unproved` | typed `unknown`; no fact or transport; require one fresh complete ordered verifier set | `no_receipt`; no authority or Transition |
| exact coverage with any record `mismatch` | typed `rejected`; reject the exact message/channel scope; this has precedence over every other per-edge freshness negative | `no_receipt`; no authority or Transition |
| exact coverage, no mismatch, and any record `clock_continuity_unproved\|rollback_or_discontinuity` | typed `unknown`; fail closed, invalidate every affected verifier-local epoch, and establish new boot/clock-bound challenges before retry; this precedes ordinary expiry | `no_receipt`; sender/remote wall time cannot repair it |
| exact coverage, no mismatch/clock failure, and any record `expired` | typed `unknown`; invalidate every expired edge epoch/message and require bounded fresh challenge/attestation | `no_receipt`; no authority or Transition |
| authenticated boundary with freshness records or coverage `not_applicable`, same-process/no-channel row with either non-`not_applicable`, or exact coverage whose record keys do not equal the authenticated ordered edge set | structural `rejected` | `receipt_effect = rejected`; no authority or Transition |

`diagnostic_egress` uses one disjoint closed source/result tuple and never emits an
authority receipt:

| Diagnostic source | Sanitizer result | Sanitizer proof | Numeric-bound proof | Sanitizer Receipt correlation | Selected decision/effect |
|---|---|---|---|---|---|
| any non-`not_applicable` source | `safe_typed_code\|safe_enum` | `exact_closed_code_or_enum_conversion` | `not_applicable` | `exact_match` | `diagnostic_egress = typed`; `receipt_effect = no_receipt` |
| any non-`not_applicable` source | `bounded_count` | `exact_source_schema_non_content_operational_count` | `exact_frozen_field_bound_and_within` | `exact_match` | `diagnostic_egress = typed`; `receipt_effect = no_receipt` |
| any non-`not_applicable` source | `bounded_count` | `exact_source_schema_non_content_operational_count` | `exceeded` | `exact_match` | `diagnostic_egress = omitted`; `receipt_effect = no_receipt`; emit no count |
| any non-`not_applicable` source | `bounded_count` | `exact_source_schema_non_content_operational_count` | `mismatch` | `exact_match` | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| any non-`not_applicable` source | `bounded_count` | `exact_source_schema_non_content_operational_count` | `unproved` | `exact_match` | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| any non-`not_applicable` source | `safe_typed_code\|safe_enum\|bounded_count` | `payload_length_or_byte_derived` | any | any | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| `rejected_request_field\|path\|endpoint` | `structurally_sanitized` | `exact_structural_sanitizer` | `not_applicable` | `exact_match` | `diagnostic_egress = sanitized`; `receipt_effect = no_receipt` |
| any non-`not_applicable` source | `safe_typed_code\|safe_enum` | `mismatch\|unproved` | `not_applicable` | any non-`not_applicable` value | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| any non-`not_applicable` source | `safe_typed_code\|safe_enum` | `exact_closed_code_or_enum_conversion` | `not_applicable` | any named mismatch, `replayed\|expired\|unproved` | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| any non-`not_applicable` source | `bounded_count` | `mismatch\|unproved` | any active value | any non-`not_applicable` value | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| any non-`not_applicable` source | `bounded_count` | `exact_source_schema_non_content_operational_count` | any active value | any named mismatch, `replayed\|expired\|unproved` | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| `rejected_request_field\|path\|endpoint` | `structurally_sanitized` | `mismatch\|unproved` | `not_applicable` | any non-`not_applicable` value | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| `rejected_request_field\|path\|endpoint` | `structurally_sanitized` | `exact_structural_sanitizer` | `not_applicable` | any named mismatch, `replayed\|expired\|unproved` | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| `typed_internal\|overlay_input\|helper_or_connector_stderr\|os_or_library_error\|remote_error\|malformed_plan` | `structurally_sanitized` | any | any | any | structural `diagnostic_egress = rejected`; `receipt_effect = rejected`; emit nothing |
| any non-`not_applicable` source | `omitted` | `not_applicable` | `not_applicable` | `not_applicable` | `diagnostic_egress = omitted`; `receipt_effect = no_receipt` |
| any non-`not_applicable` source | `raw_or_unproved` | any | any | any | `diagnostic_egress = rejected`; `receipt_effect = no_receipt`; emit nothing |
| any source | `safe_typed_code\|safe_enum\|bounded_count\|structurally_sanitized` in any assignment not matched above, including wrong-exact, required-N/A, or extraneous tuples | any | any | any | structural `diagnostic_egress = rejected`; `receipt_effect = rejected`; emit nothing |
| any source | any result in any remaining assignment not matched above, including source/result `not_applicable` or omitted with an extraneous companion | any | any | any | structural `diagnostic_egress = rejected`; `receipt_effect = rejected`; emit nothing |

The omitted row may discard any declared private diagnostic source without
inspecting its content. Structural sanitization is restricted to contextual
`rejected_request_field\|path\|endpoint` classes; every raw/error/overlay/helper/
remote/malformed-plan class can yield only a closed typed code, safe enum, bounded
count, or omission. No wrong-exact, unbounded-count, or extraneous field can fall
through to a positive row.

Every reachable receipt-emitting disposition row in the closed causal tables above emits
an exact non-authorizing Security receipt binding its causal axis, sub-key when
applicable, scope, and mapped disposition. Unknown/noncausal rows and the three
private-endpoint probe cells emit nothing. For the replay/expiry tables, the
pre-decision causal-source axes enforce exactly one primary source; additional
values are accepted only as its exact projections under
`many + exact_same_source_projection_set`. Private-input stop receipts use the
separate exact bound-effect-bundle form above. A covered source paired with zero,
independent multiple sources, missing/mismatched correlation, or conflicting
mappings reject. Thus no
aggregate can choose replay versus ordinary expiry, and the checker can enumerate
every causal-correlation outcome.

After the exact positive tuples, closed causal tables, and closed nonpositive table,
every unmatched boundary/security/channel-identity/message/confidentiality/
endpoint/crossing tuple is a structural rejection before a Security output; no
positive tier may be inferred from a label such as "local" alone.

`overlay_persistence` selects exactly one structural subrow before authority is
evaluated:

Overlay persistence policy provenance is closed before those subrows:

| Policy value | Source/default proof | Exact result |
|---|---|---|
| `off` | `explicit_operator_off\|config_absent_default_off\|config_unset_default_off` | proved off; value lookup is forbidden and the applicable omission row may continue |
| `on` | `explicit_operator_on` | proved explicit operator opt-in; continue to name/provenance/path/channel gates |
| `off\|on` | `unproved` | `overlay_persistence = unknown`; no value lookup, store, restore, or receipt |
| `off` | `explicit_operator_on` | contradictory policy source; `overlay_persistence = rejected`; no value lookup, store, restore, or receipt |
| `on` | `explicit_operator_off\|config_absent_default_off\|config_unset_default_off` | contradictory policy source; `overlay_persistence = rejected`; no value lookup, store, restore, or receipt |
| `off\|on` | `mismatch` | contradictory policy source; `overlay_persistence = rejected`; no value lookup, store, restore, or receipt |
| `not_applicable` | `not_applicable` | valid only for a structural subrow that explicitly neutralizes persistence policy |
| `not_applicable` | any non-`not_applicable` proof | structural rejection; `receipt_effect = rejected` |
| `off\|on` | `not_applicable` | structural rejection; `receipt_effect = rejected` |

| Persistence subrow | Exact projection |
|---|---|
| forbidden inherited material | operation `store\|restore`; provenance `inherited\|legacy_redaction_marker`; persistence policy, source/default proof, name policy, canonical-name, private path/channel, crossing, and secret classification all `not_applicable`; derives `rejected` before config or value consultation |
| caller-asserted omission | operation `store\|restore`; provenance `caller_asserted_only`; provenance receipt `caller_asserted` with correlation `exact_match` and `name_not_consulted`; persistence policy, source/default proof, name-policy, and secret-classification axes `not_applicable`; canonical-name, private path/channel, and crossing neutral; derives `omitted` before policy or value/name evaluation |
| unproved-provenance omission | operation `store\|restore`; provenance `unclassified\|unresolved`; provenance receipt `missing\|unproved` with correlation `unproved`; persistence policy, source/default proof, name-policy, and secret-classification axes `not_applicable`; there is no accepted name projection; canonical-name, private path/channel, and crossing neutral; derives `omitted` before policy or value/name evaluation |
| persistence-off omission | operation `store\|restore`; provenance `caller_supplied`; operation-matched exact provenance receipt/correlation with `name_not_consulted`; persistence `off` with exact explicit-off or absent/unset-default-off proof, name policy and secret classification `not_applicable`; canonical-name, private path/channel, and crossing neutral; derives `omitted` before value/name evaluation |
| not-allowlisted omission | operation `store\|restore`; provenance `caller_supplied`; operation-matched exact provenance receipt/correlation with `exact_canonical_name_identity`; persistence `on` with `explicit_operator_on`, name policy `not_allowlisted`, secret classification `not_applicable`; bound canonical overlay-name verdict `canonical_utf8\|canonical_posix_bytes\|canonical_windows_utf16` with exact EvidenceBoundary correlation; private path/channel and crossing neutral; derives `omitted` after authenticated canonical-name comparison but before value/path access |
| boundary-name omission | operation `store\|restore`; persistence `on` with `explicit_operator_on`, provenance `caller_supplied`, operation-matched exact provenance receipt/correlation with `exact_whole_name_bound_marker`, canonical-name verdict `omitted_whole_name_at_bound` with exact EvidenceBoundary correlation, name policy `unknown`, secret classification `not_applicable`, crossing/path/channel neutral; derives `omitted` before allowlist or value lookup |
| same-process operation candidate | operation `store\|restore`; persistence `on` with `explicit_operator_on`, name `operator_allowlisted`, provenance `caller_supplied`, operation-matched exact provenance receipt/correlation with `exact_canonical_name_identity`, crossing `same_process_private`; canonical-name verdict/correlation and the matching `snapshot_store\|snapshot_restore` path verdict/correlation active over full domains; private channel neutral; secret classification becomes active only after those gates pass |
| authenticated-local operation candidate | the same operation-specific on/explicit-operator-on/allowlisted/caller-supplied/provenance-receipt tuple, crossing `authenticated_local_private`; canonical-name, matching snapshot path, and private-channel verdict/correlation plus exact local confidential channel tuple, confidentiality scope, and endpoint binding active over full domains; secret classification becomes active only after those gates pass |
| connector/guest operation candidate | the same operation-specific on/explicit-operator-on/allowlisted/caller-supplied/provenance-receipt tuple, crossing `cross_boundary`; canonical-name, matching snapshot path, and private-channel verdict/correlation plus exact cross-boundary confidential channel tuple, end-to-end confidentiality scope, and endpoint binding active over full domains; secret classification becomes active only after those gates pass |

Secret classification is exactly `not_applicable` in every early rejection or
omission subrow and MUST NOT inspect the value there. It becomes active only after
the complete positive candidate's policy, canonical-name, immutable-provenance,
private-path, and applicable private-channel gates pass, and remains
non-authorizing. Persistence operation `not_applicable`, or `none\|not_applicable` crossing
on a candidate row, is a rejected projection. No omission or inherited-material
row may consult or emit a private path/channel receipt. A store receipt cannot
authorize restore, a restore receipt cannot authorize store, and any operation,
action, snapshot identity, or correlation mismatch rejects.

Overlay provenance is receipt-derived, never a trusted enum label. For `store`,
`caller_supplied` requires `exact_immutable_caller_origin` created when the opaque
overlay handle entered TC. For `restore`, it requires
`exact_persisted_provenance_chain` linking that creation receipt and the exact
successful store receipt to the current snapshot/name/handle, plus current
`snapshot_restore` authorization. A caller claim yields only `caller_asserted` and
the caller-asserted omission row with `name_not_consulted`. Missing/unproved proof
selects only the unproved-provenance omission row; policy-off selects only the
persistence-off row with `name_not_consulted` before name evaluation;
`not_allowlisted` selects only its canonical-name-bound row
after exact origin and canonical equality proof. Mismatch or replay always
rejects and can never be normalized to omission. The
final store/restore receipt binds the provenance-receipt identity. Secret-shape
classification remains non-authorizing.

Every bound receipt is engine-derived and immutable, with the fields applicable
to its property: action digest, policy revision, ordered authority-chain/
connector-route digest, and target/boot/workspace. Sensor, decoder, resolver,
fact, and probe EvidenceBoundary receipts are additionally sensor-class and
environment-campaign/branch bound. Decoder and private-resolver receipts also
bind the exact private-tainted producer registry revision, forbidden-sink
inventory revision, required canary-set root/count, and passing attestation
identity; an older or incomplete attestation cannot be substituted. Authenticated
channel receipts bind the exact ordered freshness-record edge-set root/count and
every verifier-local verdict. Command overlay receipts are command/job
bound; PTY receipts are PTY bound; session receipts are session bound; snapshot
store/restore receipts are snapshot-operation bound. Canonical overlay-name
receipts use exactly the same non-campaign operation scope as their consumer.
Every positive local-process, forwarded/remote, or guest channel receipt also
contains its exact channel identity/binding proof; `missing`, any typed mismatch,
nonce/sequence replay, expiry, or `unproved` can never authorize evidence or overlay
transport. Same-process paths require that proof `not_applicable` rather than
inventing a connector.
Every positive sensor or spawn receipt additionally references the exact local
append-only action-audit admission at each authorizing/executing node; a campaign
summary never substitutes for those records. Exactly one scope identity is active and every other scope identity is
`not_applicable`; no property may manufacture a campaign merely to authorize an
existing execution path. The ordered digest covers every
origin/forwarder/target identity, edge identity and direction, and per-node
verdict in canonical hop order. Private overlay receipts additionally bind a
fresh opaque material handle and AEAD context; neither a raw value nor a stable
public hash of low-entropy material is exposed. A caller field
cannot assert or upgrade it. Downstream use requires `exact_match`; every named
mismatch is denied/rejected, while `unproved` follows the exact owning-property
row and may derive `unavailable`, `unknown`, `omitted`, or `rejected`, but never
authorizes. A verdict without its matching correlation receipt never
authorizes. Authority is local to each node: no origin, forwarder, target, or
ambient daemon profile grants another node authority, and any required denial
wins over all allows.

The nine decisions and their immutable receipt effects are independently derived
and never free input axes:

- `sensor_decision = allowed \| denied \| unavailable \| not_applicable`;
- `private_resolution = typed_fact \| rejected \| unknown \| not_applicable`;
- `spawn_decision = allowed \| denied \| unknown \| rejected \| not_applicable`;
- `fact_admission = trusted | trusted_pending_effect_commit | rejected | unknown |
  not_applicable`;
- `surface_name_application = canonical | omitted | rejected | unknown |
  not_applicable`;
- `private_channel = private_same_process |
  private_confidentiality_and_integrity | rejected | unknown | not_applicable`;
- `overlay_persistence = stored_private | restored_private | omitted | rejected | unknown |
  not_applicable`;
- `decoder_admission = evidence | typed_failure | rejected | unknown |
  not_applicable`; and
- `diagnostic_egress = typed | sanitized | omitted | rejected |
  not_applicable`; and
- `receipt_effect = issued_exact_authority | issued_exact_conditional_effect |
  issued_exact_non_authorizing_effect | no_receipt | rejected | not_applicable`.

Exactly the decision selected by `Property under evaluation` is non-
`not_applicable`; the other decisions are neutral. `receipt_effect` is
`issued_exact_authority` exactly for sensor `allowed`, private resolution
`typed_fact`, spawn `allowed`, decoder `evidence`, final fact admission `trusted`,
either positive private-channel tier, and operation-matched overlay
`stored_private\|restored_private`. It is `issued_exact_conditional_effect` only
for `trusted_pending_effect_commit`; that receipt is bound to one proposed
Transition digest and cannot authorize any other effect or public fact.
It is `issued_exact_non_authorizing_effect` for an exact surface-name result, an
 exactly correlated or accepted-projection action-audit `mismatch\|replay`
 integrity failure, private-resolution
`reached_and_stopped` or decoder-input `reached_and_stopped` paired with
`exact_scope_limit_and_counter`, any exact typed
channel identity/binding negative:
`fresh_nonce_replayed|sequence_replay|expired|unapproved_rotation|clone_detected|
persistent_instance_mismatch|engine_boot_mismatch|target_boot_mismatch|
protocol_mismatch|endpoint_route_or_connector_context_mismatch|
trust_root_or_authenticated_identity_mismatch|mid_run_replacement`, or any typed
 per-message action/audit negative
`missing_action_digest|missing_policy_revision|missing_workspace_or_boot|
 missing_audit_receipt|sequence_replay|expired`, or any reachable receipt-emitting
 source-total action-audit or applicable non-audit correlation mismatch,
 `Receipt correlation\|Channel security` `replayed\|expired`, or
 `Private endpoint binding` `mismatch\|replayed\|expired` row listed above. A structural private-endpoint probe
 projection instead derives `rejected` and `no_receipt`. Those receipts bind
the exact typed cause, applicable operation or probe scope, affected owned-resource
set where applicable, the exact source axis, and the exact scope-aware abort, hard-exclusion, engine-loss,
or proof-expiry/re-attestation disposition,
but never claim cleanup or grant action/fact authority. Other terminal non-allow
results derive `no_receipt`; invalid projections derive `rejected`; diagnostic
output has no downstream authority and derives `no_receipt`. The ordered total
derivations are:

1. **Sensor authorization**: require a complete, bounded authority record for
   every origin, forwarder, and target governing the connector chain. Apply each
   node's own profile, cap subset, sensor action, connector action, underlying
   action, target verdict, revision, and identity binding. A denial at any node
    or authority cardinality `over_limit` derives `denied`; cardinality `zero` or
    unavailable/unproved authority derives `unavailable`. Denied/unavailable
     policy requires action-audit `not_attempted`. The closed audit tuple and its
     causal cardinality/correlation pairing are evaluated before this derivation;
     any invalid tuple rejects. An all-positive policy chain
    next requires `exact_committed_per_authorizing_and_executing_node` plus exact
    action-audit correlation before it derives `allowed`; audit failure or
     unavailability derives `unavailable`. Audit `mismatch` or `replay` derives
     `denied` and starts no action. It emits the exact typed non-authorizing
     integrity-failure receipt only under the closed correlated audit rows; with
     correlation `unproved`, it emits nothing and creates no Transition. A proved receipt maps only to the corresponding
    `action_audit_mismatch\|action_audit_replay` hard exclusion; it is never a
    policy-denial receipt. The append-only admission binds this exact action and is
    committed before observation/connector use. `full_census`
   additionally requires `explicit_for_this_campaign`; `target_start_or_wake`
   requires the separate explicit per-run authority at every affected node.
   Standing caps never imply either consent. Unknown connector kind derives
   security denial and no action, while RouteModel independently maps the route
   to `unsupported/no_representable_candidate`; it is never a policy-denied
   branch.
2. **Private environment resolution**: this is the only FR-053 value-consumption
   carve-out. Payload must be `observed_environment_opaque_input`, semantic class
   `private_platform_resolver`, resolver class `approved_platform_native` or
   `approved_fixed_helper`, locality `same_target_private`, raw retention/egress
   `none`, derived output
   `non_reconstructive_typed_identity_version_presence`, resolver revision
    `approved_exact`, binding proof exact for its implementation, and conversion proof
    `exact_closed_non_reconstructive_conversion`. Requested observed-value context
    and actual source MUST satisfy the closed source/context table before any read;
    mismatch or either `not_applicable` rejects and ambient harness state never
    substitutes. The exact
   `EnvironmentPrivateResolve` sensor receipt must be
    allowed; no harness-context, passive, names, or generic-helper receipt can
    substitute. A platform-native resolver is exactly `in_process` with spawn and
   decoder verdict/correlation axes `not_applicable`. A fixed resolver helper is
   exactly `spawned` and additionally requires exact allowed executable, spawn,
   and closed-decoder receipts. Any other implementation/execution pairing, or
   `not_applicable` on a required resolver class, locality, retention, output,
   fact-kind, binding, conversion, revision, requested context, source, sensor,
   execution, or (when a fixed helper is selected) spawn/decoder axis, derives
   `private_resolution = rejected` and `receipt_effect = rejected`. Presence-only output
   requires no resolved-executable binding; identity/version output requires the
    exact form-appropriate resolved-executable binding. Both
    `opaque_input_bytes` and `resolver_candidates` must be `within` under the
    exact private-input counter observation. Opaque input counts POSIX raw bytes
    or checked `2 * Windows UTF-16 code-unit count` monotonically while reading,
    before transcoding, with a cap-plus-sentinel strategy before unbounded allocation;
    candidates are counted before resolution/dispatch. `reached_and_stopped`
    paired with `exact_scope_limit_and_counter`
    stops further consumption, emits only typed `input_bound_reached`, produces
    no fact, derives `unknown`, and emits the exact non-authorizing private-input
    stop receipt binding the affected resource set. The separate shared cleanup
    receipt must then prove destruction of partial input and zero live resources;
    the Security receipt cannot self-assert cleanup. An `unproved` required
    private-input verdict or counter observation derives `unknown`, starts no
    action, and emits no fact or receipt. `exceeded`, missing/mismatched/replayed
    receipt, `not_applicable` on a required private-input bound/counter, or any raw
    crossing rejects. A denied bound sensor, fixed-helper spawn
   `denied\|not_executing`, or a bound
   decoder `typed_failure`, derives `rejected`, no fact, and no receipt. An
   unavailable/unproved sensor, unknown/unproved helper spawn, unproved decoder,
   or other unproved required helper evidence derives `unknown`, no fact, and no
   receipt. Receipt mismatch/replay follows the exact source-total disposition
   table and never authorizes. Before any tainted value read, producer success,
   failure, or bound-stop path, coverage attestation MUST be
   `exact_current_complete`. `missing_producer_key\|missing_sink_key\|stale\|mismatch`
   rejects before the read and emits no fact; `unproved` derives unknown and
   starts no resolver. `not_applicable` is structural rejection. Only that
   complete row
   derives `typed_fact`. Policy denial, crossed boundary, retention/egress
   attempt, path/raw/reconstructive output, caller-defined resolver, conversion
   failure, binding mismatch, correlation mismatch, or replay rejects;
   unavailable/unproved policy, revision, binding, or conversion evidence is
   unknown. Success emits the exact private-resolution receipt; it does not
   consume or self-validate that output receipt. The raw value is destroyed before the typed fact enters evidence and
   never appears in a receipt, path fact, log, audit, cache, transport, or
   response.
3. **Executable spawn**: require bound sensor verdict `allowed` with sensor
    correlation `exact_match`, then re-evaluate the complete authority chain and
    require a distinct exact spawn-action audit admission/correlation. Sensor
    audit never substitutes for command/spawn audit; failure/unavailability or
    mismatch/replay fails closed before spawn. Every spawn also requires
    `Catalogue adapter/invocation-control proof =
    exact_approved_revision_side_effect_direct_argv_least_privilege_forbidden_behaviors_disabled_no_cwd_workspace_search`.
    Proof `mismatch` derives `denied`; `unproved` derives `unknown`; either starts
    no spawn and emits no authority receipt.
   Every spawned implementation, including spawned `targeted_names` and
   `full_census`, additionally requires `allow_environment_exec` and the exact
   underlying `CommandStart` at every executing node. This execution layer is
   mandatory even when the semantic sensor class has a passive implementation.
   Any denial or receipt mismatch derives `denied`; unavailable/unproved evidence
   derives `unknown`; `read_only_observer` always denies spawn. Pin mismatch,
   changed dispatch identity, or missing untrusted-exec authority for
   `user_writable`, `workspace_writable`, or `unknown` writability derives
    `denied`. Unknown origin/trust, `unpinned`, incomplete binding, unknown form,
   or unprovable dispatch identity derives `unknown`. Form/binding is total:
   `native_binary + binary_bound` and `{interpreter_script, shim} +
   interpreter_and_script_bound` are the only complete pairs; native plus
   interpreter binding or script/shim plus binary binding derives `denied`, and
    `not_applicable` on any required executable or adapter-proof axis derives
    `spawn_decision = rejected` and `receipt_effect = rejected`. Only after those
    gates may operator/catalogue pinning allow. With every required axis present,
    `canonical_identity_only` allows solely when origin is `system_install`,
    writability is `not_writable_by_subject`, form/binding is the exact complete
    pair above, and dispatch is `unchanged`; every other canonical-only tuple
    derives `unknown`, starts no spawn, and emits no receipt. An allowed spawn
   emits the exactly correlated spawn receipt.
4. **Fixed-helper decode**: sensor and spawn verdicts must both be `allowed` and
   both correlations `exact_match`. Any denial, unknown, mismatch, replay, or
    unproved input derives `typed_failure`. Sensor `unavailable` and spawn
    `not_executing` likewise derive `typed_failure`, no evidence, and
     `receipt_effect = no_receipt`; `not_applicable` on either required bound
    verdict derives `decoder_admission = rejected` and `receipt_effect = rejected`.
    Grammar binding must be
   `fixed_helper -> exact_fixed_helper`, `targeted_names ->
   exact_targeted_names`, `full_census -> exact_full_census`, or
    `private_platform_resolver -> exact_private_resolver`; mismatch or unproved
     grammar derives `typed_failure`, while required grammar `not_applicable`
     derives `decoder_admission = rejected` and `receipt_effect = rejected`.
     `decoder_input_bytes` must be `within` under
     the exact private-input counter observation; raw transport bytes are counted
    before decoding, transcoding, buffering, or parsing. `reached_and_stopped`
      paired with `exact_scope_limit_and_counter`
      requests termination of the owned helper, derives a non-content
     `typed_failure`, and emits the exact non-authorizing private-input stop
     receipt over the affected helper/buffer set. The separate shared cleanup
     receipt must prove termination, buffer destruction, and zero live resources;
     the Security receipt cannot self-assert cleanup. `unproved` required bound or
      counter evidence derives `decoder_admission = unknown`, no evidence, and no
      receipt. A replayed private-input counter observation derives
      `decoder_admission = rejected`, `receipt_effect = no_receipt`, and causal
      `zero + not_applicable` absent another covered source. `exceeded`, missing,
      mismatched, or `not_applicable` required bound/counter evidence derives
      `decoder_admission = rejected` and `receipt_effect = rejected`. Coverage
      attestation MUST additionally be
     `exact_current_complete` before helper input is read or decoded.
     `missing_producer_key\|missing_sink_key\|stale\|mismatch` derives rejected and
     starts no helper; `unproved` derives unknown and `not_applicable` is a
     structural rejection. Only `closed_valid` plus the within-bound receipt and
     exact current complete coverage attestation then derives
   `evidence` and an exact decoder receipt; every other decoder state is a typed
   non-content failure and raw bytes are discarded. The decoder verdict and
   correlation being emitted are neutral inputs at this stage; only a downstream
   producer subrow consumes them.
5. **Channel fact admission**: payload must be `probe_evidence` and overlay
    crossing must be `none`; `overlay_private_material` is a rejected structural
    projection and can never reach an evidence decoder or any trusted admission.
    `final_boundary_receipt` requires a bound EvidenceBoundary verdict
    `admitted_complete\|admitted_evidence_incomplete\|admitted_presentation_bounded`
    with correlation `exact_match`. Complete or presentation-bounded requires
    candidate relation `exact_unchanged`; evidence-incomplete requires
    `exact_same_typed_incomplete`, binding the identical typed truncation/bound
    cause and affected evidence set. This path may derive `trusted`.
    It additionally requires no pending Transition identity for that source; a
    candidate already staged by `soft_evidence_bound_commit` is structurally
    rejected here and may be published only by the Transition finalizer.
    `soft_bound_precommit` instead requires the exact private precommit proof over
    the per-sensor bound observation, frozen `soft_only\|informational_only`
    relevance closure, typed incomplete candidate, proposed Transition digest,
    and expected final boundary scope, plus relation
    `exact_same_typed_incomplete`. This path derives only
    `trusted_pending_effect_commit` and its conditional receipt. It cannot expose
    a fact, claim cleanup, or be replayed for another batch. Boundary and
    precommit paths cannot coexist. Boundary/precommit rejection or mismatch
    rejects; missing/unproved proof is unknown. A `replayed` precommit proof
    rejects, emits no receipt, leaves the candidate `unstaged`, and creates no
    pending Transition; `not_applicable` in the selected precommit phase is
    structural rejection.
   Each producer must match its closed subrow: private resolution consumes only
   its exact `typed_fact` receipt; fixed helper and spawned typed names/census
   sensors consume only their exact class-and-grammar `evidence` decoder receipt;
   and direct/connector-native evidence consumes only its exact allowed sensor
   receipt with `not_executing`. The immediate receipt encapsulates and binds its
   upstream chain; duplicating or contradicting raw upstream verdicts rejects.
   Denial/mismatch rejects and unavailable/unproved evidence is unknown.
    Same-process evidence uses the local boundary with channel identity/binding
    proof `not_applicable`. Authenticated local evidence requires
    `exact_authenticated_local_binding`; forwarded/remote and guest evidence require
    `exact_authenticated_forwarded_or_guest_binding`, the matching integrity tier,
    and complete per-message binding. Each successful receipt binds the exact
     channel-proof digest. Nonce/sequence replay, unapproved rotation, clone
     detection, persistent identity/boot mismatch, protocol/endpoint/route/
     connector-context mismatch, trust-root mismatch, mid-run replacement, and
     expiry follow the applicable exact source-aware disposition table above.
     `local_authenticated_peer\|point_challenge_only\|unavailable` security derives
     typed unknown, while unauthenticated security derives typed rejection, exactly
     under the closed nonpositive table. Missing/unproved channel identity is
     unknown and starts no downstream action. Every negative per-message
     action/audit value follows its separate exact disposition table above;
     `not_applicable` is valid only for same-process evidence and is a structural
     rejection on an authenticated row.
6. **Surface-name application**: payload is name-only
   `operation_name_metadata`; exactly one command, PTY, session, or snapshot
   consumer is selected. No overlay value, path, allowlist, or execution receipt
   is consulted at this stage. Preliminary verdict, private observation, and
   consumer MUST match the closed tuple above. Only its three exact rows emit a
   scope-bound, non-authorizing application receipt;
   EvidenceBoundaryModel consumes that receipt to compose the public boundary
   receipt. This staged dependency is one-way: private name classification,
   Security application, then public boundary composition.
7. **Overlay transport**: payload is opaque `overlay_private_material`; the
    property has no route into fact admission. Before any path or channel lookup,
    command/PTY/session and snapshot-store transport requires
    `exact_immutable_caller_ingress`; snapshot-restore transport instead requires
    `exact_persisted_caller_ingress_chain` over the original ingress, exact
    successful store, snapshot/handle identity, and current restore authorization.
    The required provenance proof must have exact correlation. Every transport
    also requires a
    bound surface-name application verdict `canonical` with exact correlation.
    Both receipts bind the same requested operation, canonical-name identity,
    opaque material handle, origin action, target/boot/workspace, and policy
    revision. Cross-operation substitution between the direct proof and persisted
    restore chain rejects. Inherited or legacy ingress, mismatch, or replay rejects; caller
    assertion, missing/unproved ingress, or unknown/unproved application is typed
    unknown and starts no transport. An omitted or rejected name application,
    wrong operation/name/handle, required ingress proof or surface-application
    verdict `not_applicable`, or either required correlation
    mismatch/replay/`not_applicable` rejects and can
    never reach command/PTY/session or snapshot injection. Then exactly one bound
    private-path producer must be selected: command/PTY/session execution, or
    `snapshot_store\|snapshot_restore` matching the requested persistence operation.
    Its verdict must be `allowed` with correlation `exact_match`, including the
    opaque material handle. A snapshot transport receipt is snapshot-operation,
    snapshot-identity, action, and direction bound; command/PTY/session scope can
    never substitute for it, and store can never substitute for restore. Denied or any mismatch,
   including `private_payload_binding_mismatch`, rejects; unavailable/unproved
   authorization is unknown. Only the exact `same_process_private` tuple with
   confidentiality scope `same_process_private` derives `private_same_process`.
    `authenticated_local_private` requires its exact boundary,
    `exact_authenticated_local_binding`, peer-to-peer
    confidentiality scope, exact origin/final endpoint binding, authenticated
   confidentiality and integrity, complete action/audit binding, and
   opaque-handle/AEAD correlation. `cross_boundary` requires all of those plus
    `exact_authenticated_forwarded_or_guest_binding` and
    `origin_to_final_end_to_end`, binding the original sender and final target over
   the complete route while forwarders see ciphertext and routing metadata only.
     Endpoint mismatch/replay, nonce/sequence replay, unapproved rotation, clone
     detection, persistent identity/boot mismatch, protocol/route/connector
     mismatch, trust-root mismatch, mid-run replacement, and expiry follow the
     applicable exact source-aware disposition table. Hop-only confidentiality,
     crossing/boundary contradiction, integrity-only or unauthenticated private
     transport, and challenge-only/local-peer/unavailable security instead follow
     the closed nonpositive table. Missing/unproved channel identity,
     confidentiality, or endpoint proof is typed unknown and starts no transport.
     Every negative per-message action/audit value follows its separate exact
     disposition table; `not_applicable` is valid only for same-process overlay
     and is a structural rejection on an authenticated row. Only the exact local or end-to-end tuple derives
   `private_confidentiality_and_integrity`.
    The exact private-channel receipt carries the channel-proof digest, exact
    ingress-provenance and surface-name-application receipt identities, an
    ephemeral opaque handle, and correlation metadata, never an overlay value or
    public value digest.
8. **Overlay persistence**: first select the exact structural subrow above.
   Inherited values and legacy redaction markers reject without consulting a
   canonical name or private path. Off and asserted/unclassified/unresolved
   provenance omit before name evaluation and consult no private path. A
   non-allowlisted name omits only after exact authenticated canonical-name
   equality and allowlist comparison, still before private path/value access. An
   exact whole-name boundary omission with unknown name policy omits before
   allowlist lookup and consults no private path. Only an exact
    `on + operator_allowlisted + caller_supplied` candidate proceeds. Store
    additionally requires `exact_immutable_caller_origin`; restore requires the
    exact persisted creation-plus-store provenance chain, each with
    provenance-receipt correlation `exact_match` and
    `exact_canonical_name_identity`. The boundary-name omission path instead
    requires `exact_whole_name_bound_marker` and can never reach positive
    store/restore. Caller assertion or
    missing/unproved provenance omits; mismatch/replay rejects. It requires
   a canonical overlay-name verdict from EvidenceBoundaryModel with exact receipt
   correlation bound to target/boot, native equality class, policy revision, and
   the same opaque material handle. A rejected/mismatched name rejects;
   unknown/unproved name proof is unknown. Its bound
   snapshot execution/restoration path must be `allowed` with correlation
   `exact_match` to the same opaque handle; denial or mismatch rejects and
   unavailable/unproved authority is unknown. Same-process candidates require no
   channel receipt. Authenticated-local and connector/guest candidates require
   the exact positive private-channel tuple, confidentiality scope, endpoint
   binding, and correlation for their crossing. A connector/guest candidate must
   therefore retain origin-to-final end-to-end protection; a set of hop-only
   channels never composes into it. Rejected/mismatched/weak trust rejects and
   unknown/unproved trust is unknown.
    The requested persistence operation selects its exact action before any
    authority is consumed: `store` requires `snapshot_store`; `restore` requires
    `snapshot_restore`. Only a complete operation-matched candidate derives
    `stored_private` for store or `restored_private` for restore. Cross-operation
     receipt substitution, even for the same snapshot and opaque handle, rejects.
     The final operation receipt binds the exact provenance receipt/chain identity.
     Secret classification never authorizes.
9. **Diagnostic egress**: source, result, proof, and sanitizer correlation MUST
   match the closed tuple above. Every rejected/structural row emits nothing;
   `omitted` emits nothing. Helper/connector stderr,
   OS/library strings, remote errors, rejected fields, paths, endpoints, and
   malformed-plan text remain private inputs: all may yield only a closed typed
   code/safe enum or exactly bounded count through exact typed conversion.
   Only rejected request fields, paths, and endpoints may instead yield
   structurally sanitized non-content fields through the exact shared sanitizer.
   Their raw strings can
   never be returned, logged, or placed in a receipt.

The sensor-class cap function is exact before connector composition:

| Sensor request class | Required policy/cap/action evidence |
|---|---|
| `harness_context` | authenticated delivery, schema validation, and peer validation; no environment capability |
| `passive_metadata` | `allow_environment_probe` plus applicable `FileRead` or OS-metadata authority |
| `targeted_names` | `allow_environment_probe` plus target passive-observe authority |
| `full_census` | `allow_environment_probe` + `allow_environment_census` plus target census authority and `explicit_for_this_campaign`; any spawned implementation separately adds the universal execution layer below |
| `private_platform_resolver` | `allow_environment_probe` plus exact `EnvironmentPrivateResolve`, approved resolver revision/binding, same-target private confinement, and zero raw retention/egress; a spawned fixed implementation separately adds the universal execution and closed-decoder layers below |
| `fixed_helper` | `allow_environment_probe` + `allow_environment_exec` plus target sensor and underlying `CommandStart`; writable execution also requires `allow_environment_untrusted_exec` |
| `route_sentinel` | `allow_environment_probe` + `allow_environment_exec` plus connector and target `CommandStart` |
| `connector_discover`, `connector_dial`, `identity_challenge` | exact connector-cap row plus every originating/forwarding/target authority; executing implementations additionally require `allow_environment_exec` and underlying action authority |
| `target_start_or_wake` | `allow_environment_target_start` plus exact connector cap and explicit per-run authorization at every affected node |

The universal execution layer is independent of semantic class: whenever
`Originating sensor execution = spawned`, every executing node also requires
`allow_environment_exec`, its underlying `CommandStart=allowed`, exact executable
identity/binding, and the spawn receipt pipeline. No class row can waive it.

`read_only_observer` rejects every spawned implementation even if its semantic
class also has a non-executing implementation. `not_applicable` is valid only in
the property projections that require it and cannot authorize a sensor.

The connector-cap function is exact:

| Connector kind | Required environment connector authority |
|---|---|
| `local_direct` | base `allow_environment_probe`; executable work separately requires `allow_environment_exec` and its underlying action decision |
| `embedded` | base `allow_environment_probe` plus approved embed-host connector registration and host policy; any external connector it invokes is checked under that connector's own row |
| `wsl` | `allow_environment_probe` + `allow_environment_connector_wsl` |
| `forwarded_remote` | `allow_environment_probe` + `allow_environment_connector_remote` + existing remote authority |
| `container` | `allow_environment_probe` + `allow_environment_connector_container` |
| `sandbox` | `allow_environment_probe` + `allow_environment_connector_sandbox` |
| `vm_guest` | `allow_environment_probe` + `allow_environment_connector_vm` |
| `firecracker_vsock` | `allow_environment_probe` + `allow_environment_connector_firecracker` |
| `not_applicable` | no connector-specific cap; apply the base sensor, profile, and underlying-authority rules |
| `unknown` | `sensor_decision = denied` and no connector action; RouteModel emits `sensor_exhausted/no_representable_candidate` from `gated` and separately classifies the route `unsupported`, never `policy_denied` |

## EvidenceBoundaryModel Domains

This checker contains no environment-name contents. It proves only finite source,
native-equality, representation, consent, and bound classes. Correlation scope is
exactly one of `environment_request`, `environment_campaign`,
`environment_campaign_branch`, `private_resolver_invocation`,
`environment_retention_partition`, `environment_retention_store`, `command_job`, `pty`, `session`, or
`snapshot_operation`; every other scope identity is `not_applicable`. Pre-admission responses use `environment_request`
bound to peer, action digest, and request-key attempt without inventing a
campaign. Post-admission queue/total-time/campaign-output effects use
`environment_campaign` without inventing a branch. Probe facts use the
campaign/branch scope and the full frozen campaign-bound map. Private resolver
input gates additionally bind the exact invocation. Headless retained-run
ceilings use `environment_retention_partition`. Its canonical partition identity
is exactly `(engine_boot_id, peer_scope_id, tenant_scope_id,
admission_policy_revision)`, where that policy component is frozen at campaign
admission and never follows later re-gating revisions,
with an explicit `not_applicable` sentinel for a genuinely absent peer or tenant
scope; no component may be collapsed or omitted. It additionally binds store
revision, the same-partition eligible-set digest, and either exact victim order or
the exact saturation-no-victim maintenance proof. Daemon-wide
partition/record/serialized-byte ceilings use `environment_retention_store`, bound
to daemon store identity/revision, the complete all-partition eligible-set digest
and order, plus the selected victim's exact owning-partition proof when a victim
exists or the exact saturation-no-victim maintenance proof otherwise. Startup recovery
sweeps historical boot partitions as well as the current one. Both scopes require
the exact frozen bound revision, complete eligible-set digest, and exactly one
victim-selection or saturation-no-victim receipt without an active caller. Existing
non-probe overlay paths may invoke name identity under their own operation scope
with only the request-schema per-name bound applicable; they do not manufacture
an environment campaign. Observations carrying environment names evaluate
`name_identity`; a names result additionally evaluates `names_census`.
Unselected name/census axes and decisions are exactly `not_applicable`. For probe
fact scopes, the applicable decisions compose directly into one immutable
EvidenceBoundary receipt consumed by `SecurityPropertyModel` without borrowing a
Security decision. For non-campaign command/PTY/session/snapshot name scopes,
native-name classification is a private preliminary decision: the exact
non-authorizing `surface_name_application` receipt is the sole Security input
permitted in final boundary composition. The order is preliminary classification,
Security application, then public EvidenceBoundary receipt; the final receipt is
never fed back into that surface-name property.

### Native-name identity domain

| Input axis | Required values |
|---|---|
| Name applicability | `environment_name`, `non_name_fact` |
| Name projection stage | `unavailable_before_acquisition`, `candidate_acquired`, `policy_violation_observed`, `not_applicable` |
| Target OS/name-semantics binding | `exact`, `mismatch`, `unknown`, `not_applicable` |
| Target native name semantics | `linux_posix_bytes_case_sensitive`, `macos_posix_bytes_case_sensitive`, `windows_utf16_ordinal_case_insensitive`, `unknown`, `not_applicable` |
| Name acquisition | `target_native_names_api`, `target_native_environment_block`, `caller_supplied_overlay_name`, `value_pair_fallback`, `unknown`, `not_applicable` |
| Source reduction | `names_only_before_boundary`, `caller_name_only`, `raw_value_crossed_boundary`, `unproved`, `not_applicable` |
| Source representation | `valid_utf8`, `raw_posix_bytes`, `windows_utf16_units`, `malformed`, `not_applicable` |
| Native syntax | `ordinary_valid`, `windows_drive_pseudo_name`, `empty`, `contains_nul`, `contains_illegal_equals`, `malformed`, `not_applicable` |
| Wire representation | `utf8_text`, `base64_posix_bytes`, `base64_windows_utf16_units`, `mismatch`, `missing`, `not_applicable` |
| Equality/deduplication key | `exact_target_native`, `display_text`, `lossy_normalized`, `missing`, `not_applicable` |
| Stable ordering key | `exact_target_native_total`, `display_order`, `missing`, `not_applicable` |
| Encoding/equality proof | `lossless_round_trip`, `collision`, `invalid`, `unproved`, `not_applicable` |
| Per-name truncation | `none`, `whole_name_omitted_at_boundary`, `partial_name`, `unmarked`, `not_applicable` |
| Surface-name application correlation | `exact_preliminary_verdict_scope_and_consumer`, `missing`, `mismatch`, `replayed`, `unproved`, `not_applicable`; active only for `caller_supplied_overlay_name` in command/PTY/session/snapshot scope and binds the exact non-authorizing Security receipt |

The name axes other than surface-name application correlation first derive a
private `preliminary_native_name_verdict` with exactly the same value domain as
the final admission. The derived public `native_name_admission` is exactly one of `canonical_utf8`,
`canonical_posix_bytes`, `canonical_windows_utf16`,
`omitted_whole_name_at_bound`, `rejected`, `unknown`, or `not_applicable`.
`non_name_fact` requires every other name axis and the decision
`not_applicable`. `environment_name` selects exactly one projection subrow.
`unavailable_before_acquisition` permits an already known exact OS/semantics
binding or an unknown binding, requires unavailable/unknown acquisition or
unproved reduction, and requires all representation/syntax/wire/key/order/
truncation axes `not_applicable`; it derives `unknown` without discarding known
target identity or inventing a key. `policy_violation_observed` requires `value_pair_fallback` or
`raw_value_crossed_boundary`; dependent key/order/output axes are neutral and it
derives `rejected`. `candidate_acquired` requires known target-bound semantics,
either target-native acquisition with names-only reduction or
`caller_supplied_overlay_name` with `caller_name_only`, and activates every
downstream axis. The caller-name subrow is public name metadata under its
command/PTY/session/snapshot scope; it never observes or transports the overlay
value. It is classified in this order:

1. `value_pair_fallback`, raw-value boundary crossing, partial/unmarked
   truncation, missing/mismatched wire representation, display/lossy/missing
   equality or ordering, collision/invalid proof, malformed
   representation/syntax, empty/NUL, or an illegal equals form derives
   `rejected`. A Windows drive pseudo-name is valid only under Windows
   semantics and uses its documented leading-equals parse; it is rejected under
   POSIX semantics.
2. Target OS/name-semantics mismatch rejects; unknown target OS/binding derives
   unknown before acquisition. Linux binds only Linux POSIX semantics, macOS only
   macOS POSIX semantics, and Windows only Windows UTF-16 semantics.
3. After those structural checks and exact target binding, an `unproved`
   encoding/equality proof derives `unknown`; it cannot produce a canonical or
   omission result. POSIX semantics otherwise require an admitted target-native or caller-name source and
   its matching reduction,
   `raw_posix_bytes\|valid_utf8`, exact native equality/order, and lossless proof.
   Valid UTF-8 may use `utf8_text`; arbitrary bytes require tagged
   `base64_posix_bytes`. Windows semantics require admitted UTF-16 units,
   exact ordinal-case-insensitive equality plus a deterministic raw-code-unit
   tie-break for stable ordering, and lossless proof; valid text
   may use `utf8_text`, otherwise tagged `base64_windows_utf16_units` preserves
   code units. A wire/source tag mismatch rejects.
4. With truncation `none`, the exact matching form derives its canonical output.
   `whole_name_omitted_at_boundary` derives
   preliminary `omitted_whole_name_at_bound`; the omitted name contributes no
   equality key or content to the public result, only the typed bound marker.

Target-native probe names require surface-name application correlation
`not_applicable`; their final admission equals the preliminary verdict. A
`caller_supplied_overlay_name` never publishes that preliminary verdict directly.
SecurityPropertyModel consumes it with the exact operation scope and consumer.
Final composition requires
`exact_preliminary_verdict_scope_and_consumer` and maps the matching Security
result to the same canonical/omitted/rejected final admission. Missing, mismatched,
or replayed application correlation rejects; unproved correlation derives
unknown. For a whole-name bound omission, this correlation and the independent
`exact_security_surface_name_application` effect correlation name the same
Security receipt and scope; disagreement rejects. Thus canonical within-bound
names and over-bound names both have finite, acyclic application evidence.

Display text is never the equality or deduplication key. No case folding,
Unicode normalization, replacement character, locale transform, or lossy decode
may merge two native names.

### Names-only census domain

| Input axis | Required values |
|---|---|
| Census applicability | `names_result`, `not_applicable` |
| Census mode | `targeted`, `full`, `not_applicable` |
| Targeted-name-set receipt | `exact_goal_or_caller_requested_set`, `absent`, `mismatch`, `replayed`, `not_applicable` |
| Emitted/requested name relation | `equal`, `strict_subset_due_exact_bound`, `contains_unrequested`, `substitution_or_mismatch`, `not_applicable` |
| Full-census campaign consent receipt | `exact`, `absent`, `mismatch`, `replayed`, `not_applicable` |
| Raw-entry scan observation | `within`, `reached`, `exceeded`, `unproved`, `not_applicable` |
| Unique emitted-item observation | `within`, `reached`, `exceeded`, `unproved`, `not_applicable` |
| Per-name-native-byte observation | `within`, `reached`, `exceeded`, `unproved`, `not_applicable` |
| Total-byte observation | `within`, `reached`, `exceeded`, `unproved`, `not_applicable` |
| Census reached-dimension set | `none`, `raw_entries`, `emitted_items`, `per_name_native_bytes`, `total_bytes`, `multiple_exact`, `missing_or_mismatch`, `not_applicable` |
| Completeness marker | `complete`, `truncated_item_count`, `truncated_per_name`, `truncated_total_bytes`, `truncated_multiple`, `missing_or_mismatch`, `not_applicable` |
| Output shape | `whole_canonical_names_only`, `partial_name`, `value_pair`, `unproved`, `not_applicable` |
| Emitted-name set binding | `exact_ordered_deduplicated_set_root_and_count`, `missing`, `mismatch`, `not_applicable` |

The derived `census_admission` is `complete`, `truncated`, `rejected`, `unknown`,
or `not_applicable`. A non-applicable census neutralizes every census axis.
Targeted mode requires an exact campaign/operation-bound goal-relevant or
caller-requested name-set receipt, full-consent `not_applicable`, and emitted/
requested relation `equal` for complete output or
`strict_subset_due_exact_bound` for truncated output; `contains_unrequested` or
substitution rejects. Absent/mismatched/replayed scope rejects before observation.
Full mode requires targeted receipt and emitted/requested relation
`not_applicable` and the exact
campaign-bound consent receipt before observation. Missing, mismatched, or replayed
full consent rejects without action. `value_pair`, `partial_name`, any exceeded
counter, set-binding failure, or a reached-set/marker mismatch rejects. Any
unproved observation/output derives unknown. All four counters within plus reached set `none`, marker
`complete`, whole canonical names, and exact emitted-set binding derives complete. One or more `reached`
counter values plus the exact set and matching truncation marker derives
truncated. The reached set equals the exact set of counters whose status is
`reached`: `none` iff all four are `within`; the named singleton iff exactly that
counter reached; and `multiple_exact` iff two or more reached, with the exact
ordered member set bound into the receipt. Marker mapping is closed:
`raw_entries\|emitted_items -> truncated_item_count`,
`per_name_native_bytes -> truncated_per_name`, `total_bytes ->
truncated_total_bytes`, and `multiple_exact -> truncated_multiple`. Every other
set/status/marker tuple derives `missing_or_mismatch` and rejects. Raw-entry and
emitted-item caps stop before the next whole name; a
per-name cap omits
the whole name. Truncation never returns a prefix of a name.

Counter stages are exact: raw entries are counted before parsing/deduplication;
unique emitted items are counted after target-native equality; per-name native
bytes are POSIX raw bytes or the checked product `2 * UTF-16 code-unit count` on
Windows. Counting code points or bare UTF-16 units is invalid. Total bytes count
the complete serialized wire representation after base64/tag expansion. The receipt binds the exact
ordered/deduplicated emitted-name set root, count, all counter values, and
truncation marker, preventing duplicate floods or channel add/remove/substitution.

### Independent bound domain

| Input axis | Required values |
|---|---|
| Bound correlation scope | exactly one of `environment_request`, `environment_campaign`, `environment_campaign_branch`, `private_resolver_invocation`, `environment_retention_partition`, `environment_retention_store`, `command_job`, `pty`, `session`, `snapshot_operation`; request scope binds peer/action/request-key attempt, campaign scope binds the admitted campaign without a branch, resolver scope additionally binds its exact invocation, partition retention scope binds canonical `(engine_boot_id, peer_scope_id, tenant_scope_id, admission_policy_revision)` frozen at admission, and global retention-store scope binds daemon store identity/revision across every current and historical-boot partition. Both retention scopes bind the complete eligible-set digest and exactly one victim-selection or saturation-no-victim maintenance proof; every selected victim still carries its exact owning-partition receipt, while saturation requires victim selection `not_applicable` |
| Owner-campaign arbitration cut | `exact_single_scope`, `exact_hierarchical_campaign_cut`, `changed`, `missing`, `mismatch`, `replayed`, `not_applicable`; a hierarchical cut binds one campaign-owner scheduler epoch/store revision, campaign total-time status, and every simultaneously observed descendant branch/resolver/sensor receipt while preserving each receipt's own correlation scope. It is required whenever campaign and descendant scopes can race |
| Deadline clock receipt | `exact_current_cut`, `continuity_unproved`, `stale_after_competing_transition`, `missing`, `mismatch`, `replayed`, `not_applicable`; required for `per_sensor_time\|total_time` and binds campaign-owner boot id, approved suspend-inclusive monotonic elapsed-clock identity, start anchor, frozen duration, sampled instant, owner scheduler epoch/store revision, and the competing completion/lifecycle commit order. Target/guest clocks and wall time are informational only |
| Deadline registration/arm health | `exact_durable_armed`, `arm_failed`, `registration_missing`, `health_unproved`, `stale`, `not_applicable`; required for `per_sensor_time\|total_time` and bound to the exact Transition deadline-registration member |
| Boundary phase | `counter_observed`, `effects_committed` |
| Frozen bound-set receipt | `exact_scope_revision`, `changed`, `missing`, `mismatch` |
| Response measurement contract | `exact_frozen_default_caps_tokenizer_suite_versions_fixture_and_command`, `changed`, `missing`, `mismatch`, `not_applicable` |
| Applicability/effect map | `exact_complete`, `missing_dimension`, `duplicate_dimension`, `extra_dimension`, `unproved` |
| Each dimension status | independently `within`, `reached`, `exceeded`, `unproved`, or `not_applicable` |
| Each applicable dimension role | the exact closed role from the scope-aware dimension table below; unselected dimensions use `not_applicable` |
| Affected evidence class | `structural_or_hard_present`, `soft_only`, `informational_only`, `unproved`, `not_applicable`; active only for `per_sensor_time` and derived from the frozen complete sensor-to-requirement relevance closure, with hard winning mixed sets |
| Reached-dimension set | `exact_none`, `exact_nonempty_within_admitted_cardinality`, `missing`, `mismatch`, `over_limit` |
| Private bound-observation receipt | `issued_exact`, `missing`, `mismatch`, `unproved`, `not_applicable` |
| Primary bound-effect record set | `exact_complete`, `missing`, `mismatch`, `extra`, `not_applicable`; contains one record per distinct unsuppressed effect after the fixed compatibility/dominance function, never one caller-chosen record per dimension |
| Affected-member coverage | `exact_zero`, `exact_one`, `exact_many_within_bound`, `missing`, `mismatch`, `over_limit`, `not_applicable` |
| Per-reached-dimension disposition/correlation map | exact finite map keyed one-to-one by every reached dimension, each value `applied`, `coalesced_into(<primary-effect-id>)`, or `safety_subsumed_by(<primary-effect-id>)` plus its exact ordered receipt chain drawn from `transition\|environment_operation\|security_private_input_stop\|security_surface_name_application\|retention_scheduler`; or `missing`, `duplicate`, `extra`, `mismatch`, `replayed`, `not_applicable`. A private-input stop chain contains both the Security non-authorizing stop and the sole sensor Transition/cleanup receipt. Every effect id and receipt is scope/transaction bound |
| Retention victim selection | `exact_partition_deadline_then_terminal_time_then_stable_campaign_id`, `exact_global_deadline_then_terminal_time_then_stable_partition_and_campaign_id`, `mismatch`, `cross_partition`, `unproved`, `not_applicable`; active only for retention state `victim_selected` and bound to its complete maintenance-eligible-set digest. Global selection also binds each victim's owning-partition proof. Saturation and within require `not_applicable` because no victim exists |
| Retention maintenance state | `within`, `victim_selected`, `capacity_saturated_protected`, `not_applicable`; active only for retention scopes. Saturation binds a reached-counter set, exact empty complete maintenance-eligible set covering both terminal expiry and tombstone purge, nonempty protected-or-continuity-unproved root/count, and exactly one `earliest_proved_wake\|bounded_reanchor_backoff` scheduling receipt; it selects no victim and owns no deletion or cleanup |
| Retention maintenance effect receipt | `exact_no_mutation_saturation`, `missing`, `mismatch`, `replayed`, `not_applicable`; exact only for `capacity_saturated_protected` and emitted durably by the headless retention scheduler. It binds correlation scope, frozen bound revision, store revision, every reached retention dimension/counter, the complete empty maintenance-eligible-set digest, protected-or-continuity-unproved root/count, and exactly one bounded wake/backoff registration. It proves no Transition, deletion, victim, Operation, or cleanup is applicable |
| Shared cleanup receipt | `exact_complete_no_live_resources`, `missing`, `mismatch`, `not_applicable` |

The platform support registry MUST name one implementation-tested,
suspend-inclusive monotonic elapsed-clock source per supported verifier OS. The
minimum supported mappings are evidence-backed and closed:

| Verifier OS | Approved clock contract | Unavailable/contract-mismatch result |
|---|---|---|
| Windows | `QueryPerformanceCounter`, whose [platform contract](https://learn.microsoft.com/en-us/windows/win32/sysinfo/acquiring-high-resolution-time-stamps) is monotonic and includes standby, hibernate, and connected-standby time; bind the exact OS/API implementation revision used by the build | `clock_continuity_unproved`; start no timed action and re-establish a supported clock/boot domain |
| Linux, including a WSL verifier only when the guest kernel exposes the same contract | `CLOCK_BOOTTIME`, whose [Linux clock contract](https://man7.org/linux/man-pages/man2/clock_getres.2.html) is monotonic and includes suspended time; bind the guest/kernel clock identity rather than the Windows host clock | `clock_continuity_unproved`; no host-clock substitution |
| macOS | `mach_continuous_time`, whose [platform contract](https://developer.apple.com/documentation/kernel/1646199-mach_continuous_time) is monotonic and advances while the system sleeps; bind the exact OS/API implementation revision used by the build | `clock_continuity_unproved`; start no timed action and re-establish a supported clock/boot domain |
| any other guest/remote verifier | one target-native source registered with an equally strong tested contract; parent, sender, and wall clocks are forbidden substitutes | `unavailable\|clock_continuity_unproved` and explicit unsupported-with-reason coverage |

There is one status record for each finite dimension, with this non-negotiable
owner/role/effect map. A plan fixes numeric limits and applicability but cannot
change the listed role:

| Dimension | Required role | Primary effect when reached |
|---|---|---|
| `topology_nodes`, `topology_edges`, `sensors`, `requirements`, `names`, `total_bytes` | `branch_decisive` in `environment_campaign_branch` scope | exact affected branch/frontier `bound_reached` |
| `per_sensor_time` + affected class `structural_or_hard_present` | `branch_decisive` in `environment_campaign_branch` scope with exact deadline and campaign arbitration receipts | exact affected branch/frontier `bound_reached` |
| `per_sensor_time` + affected class `soft_only` | `soft_evidence_only` in `environment_campaign_branch` scope with exact deadline and campaign arbitration receipts | stop that sensor and commit the exact soft observation as incomplete; hard-safe route/Goal evidence remains eligible for `ready_with_warnings` |
| `per_sensor_time` + affected class `informational_only` | `informational_evidence_only` in `environment_campaign_branch` scope with exact deadline and campaign arbitration receipts | stop that sensor and commit only its informational incomplete observation; Route/Goal are unchanged |
| `per_name_native_bytes` | `branch_decisive` in `environment_campaign_branch` scope; `execution_name_reject` in command/job/PTY/session scope; `snapshot_name_omit` in snapshot scope | branch `bound_reached`; reject the whole over-bound execution name; or omit the whole over-bound snapshot name before allowlist lookup |
| `opaque_input_bytes`, `resolver_candidates` | `private_input_gate` in `private_resolver_invocation` scope | stop the resolver, destroy private input, emit typed `input_bound_reached`, and admit no fact |
| `decoder_input_bytes` | `private_input_gate` in `environment_campaign_branch` or `private_resolver_invocation` scope | stop the owned helper, destroy raw decoder input, and return typed non-content failure |
| `total_time` | `campaign_decisive` in `environment_campaign` scope with exact deadline and hierarchical campaign-cut receipts | exact campaign frontier `bound_reached`, or exact pre-route campaign evidence `bound_before_route` |
| `helper_processes`, `concurrency` | `queue_gate` in `environment_campaign` scope | refuse extra live admission and return bounded queued/in-progress state |
| `queued_campaigns`, `run_registry_records`, `run_registry_bytes` | `admission_gate` in `environment_request` scope | atomically reserve one queued slot and one maximum serialized lifecycle footprint before a new/independent campaign commit; on any reach return fixed `rejected_bound` and mutate nothing |
| `retention_partitions` when the immutable admission partition is absent | `admission_gate` in `environment_request` scope | atomically reserve the possible partition slot in the same new/independent campaign reservation; on reach return fixed `rejected_bound` and mutate nothing |
| `active_campaigns` | `queue_gate` in `environment_campaign` scope | `campaign_start` atomically decrements queued/increments active only while within; on reach no Transition occurs and the bounded campaign remains `accepted` |
| `request_key_aliases`, `key_index_records`, `key_index_bytes` for an initial keyed campaign | `admission_gate` in `environment_request` scope | atomically include the alias and forward/reverse index deltas in `reserve_keyed_admission_all_caps`; on reach return fixed `rejected_bound` with no campaign/key mutation |
| `request_key_aliases`, `key_index_records`, `key_index_bytes`, plus the alias-caused incremental `run_registry_bytes` and reserved terminal/tombstone `retained_bytes`, for a new alias on an authorized active campaign | `admission_gate` in `environment_campaign` scope with exact partition/store subreceipts | atomically include alias/index deltas and the exact maximum current-to-terminal/tombstone key-proof footprint in `reserve_alias_all_caps`; on any reach return fixed `rejected_bound` with no key/campaign/ledger mutation and never borrow unused initial-admission headroom |
| `result_detail` | `output_only` only in an admitted `environment_campaign` scope | bounded continuation for an admitted or authorized campaign response; underlying facts and Goal remain unchanged |
| `response_bytes`, `response_tokens` in `environment_request` scope | `fixed_envelope_gate` | every complete pre-admission conflict/denial/unavailability/bound-rejection/commit-failure envelope must remain `within`; `reached\|exceeded` is an implementation invariant rejection with no effect record, cursor, or continuation |
| `response_bytes`, `response_tokens` in an admitted `environment_campaign` scope | `output_only` | bounded continuation for that campaign response; underlying facts and Goal remain unchanged |
| `retained_terminal_artifacts` | `retention_only` in `environment_retention_partition` scope | exact retention-expiry transition only for the deterministically selected same-partition completed/cancelled/failed artifact whose current-clock terminal-retention eligibility is proved elapsed; protected or continuity-unproved artifacts are not eligible victims and instead contribute to saturated-protected proof when no eligible record exists |
| `retained_tombstones` | `retention_only` in `environment_retention_partition` scope | exact retention-purge transition for the deterministically selected same-partition expired tombstone |
| `retention_partitions`, `retained_records`, `retained_bytes` | `retention_only` in `environment_retention_store` scope | headless deterministic expiry or purge of the global victim under its owning-partition receipt; serialized bytes are counted exactly before commit. An empty historical partition is removed with its per-partition metadata in the same purge/fold batch |
| `key_index_records`, `key_index_bytes` for historical maintenance | `retention_only` in either exact retention scope | deterministic expiry/purge considers the historical key indexes in the eligible-set digest; this role never substitutes for the request/campaign admission gate above |

Time dimensions use one approved campaign-owner elapsed-clock adapter whose
monotonic contract includes system suspend and is bound to the engine boot. The
campaign total-time anchor commits with admission; a per-sensor anchor commits
with its pre-action audit immediately before work. An implementation lacking that
clock contract or `exact_durable_armed` registration cannot admit the time-bounded
action as proved and starts nothing. Only
`exact_current_cut` can derive `within\|reached`: `continuity_unproved` during live
work selects the normal engine-loss/failure cleanup path, and
`stale_after_competing_transition\|missing\|mismatch\|replayed` rejects the bound
effect. A sampled instant equal to the deadline is reached. Store commit order,
not thread scheduling or wall timestamps, decides whether a completion committed
before the cut; the same receipt therefore covers suspend, restart, and concurrent
completion without an unbounded wait.
The deadline scheduler is recoverable state, not a best-effort callback. It keeps
one nearest-deadline wake plus store-revision signals, performs a headless startup
overdue sweep, and runs one frozen maximum-interval reconciliation watchdog to
detect a lost timer notification. Each sweep reads the durable registrations and
emits exact current cuts; duplicate callbacks collapse by registration revision.
The winning terminal Transition atomically marks/deletes every owned registration,
after which external disarm is idempotent. Arm failure, missing registration, or
unproved clock health produces typed failure and complete cleanup before spawn;
no path waits for a signal that was never durably armed.

For `snapshot_operation`/command/PTY/session overlay-name canonicalization,
`per_name_native_bytes` is the only applicable feature-003 dimension; its reached
state rejects/omits the whole over-bound name rather than creating a campaign.
`result_detail\|response_bytes\|response_tokens` are `not_applicable` here because
the established command/PTY/session/snapshot surface contracts own their response
bounds outside Environment OperationModel. Probe scopes carry the complete
campaign map. Missing, extra, or role-inconsistent dimensions reject.

An `environment_request` fixed-envelope gate is valid only at status `within`,
with reached set `exact_none`, no private observation, no primary effect, and
applied-effect correlation `not_applicable`. It proves the complete terminal
request error fits; it is not a truncation mechanism. Any reached/exceeded request
response counter rejects as an implementation invariant before publication and
cannot be converted into `output_truncated_with_continuation`.

An admission-gate reach is total but is not environment evidence. EvidenceBoundaryModel
classifies its exact private capacity observation as
`admission_capacity_rejected`, `bounded_evidence_disposition=rejected`, and
`boundary_receipt_effect=rejected`, emitting no public Evidence receipt.
OperationModel is the sole public authority for the fixed `rejected_bound`
envelope and no campaign/key/state mutation. Transition correlation is exactly
`not_applicable`; there is no evidence-incomplete receipt, cleanup claim, cursor,
or bound-effect bundle. A
missing/mismatched/replayed observation is a checker rejection rather than a
bounded result. This applies equally to initial keyed/keyless admission and active
alias binding.

The derived `bounded_evidence_disposition` is exactly one of `within`,
`effects_pending`, `evidence_incomplete`, `presentation_bounded`, `rejected`, or
`unknown`. The primary effect set contains exactly one record for each distinct
unsuppressed effect selected by the closed compatibility/dominance function:
`branch_bound_reached`, `soft_observation_incomplete`,
`informational_observation_incomplete`, `private_input_stopped`, `campaign_bound_reached`,
`output_truncated_with_continuation`, `queue_gate`, `admission_capacity_rejected`,
`snapshot_name_omit`, `execution_name_reject`, `retention_expiry`,
`retention_purge`, or `retention_saturated_protected`.
Cleanup is one shared receipt over the exact union of stopped live members, never
miscounted as a second per-dimension primary effect.

Multi-reach compatibility and dominance are closed and engine-derived:

| Scope/reached shape | Exact composition |
|---|---|
| environment-request admission | any exact nonempty set of simultaneously reached admission-gate dimensions coalesces into one `admission_capacity_rejected`; response byte/token fixed-envelope counters MUST remain `within` and are never members of that reached set |
| command/job, PTY, session, or snapshot name | exactly one reached feature-003 name dimension and one effect; a second reached dimension rejects as a scope-shape mismatch |
| retention partition/store | all simultaneous retention counters coalesce into one deterministic maintenance selection from the exact eligible union; terminal victim -> `retention_expiry`, expired victim with an accepted `purge_eligibility/authorize_purge` proof -> `retention_purge`. Every reached counter maps to that effect. If the complete expiry-or-purge eligible set is empty, select maintenance state `capacity_saturated_protected` and primary effect `retention_saturated_protected`; every reached counter maps to the same exact `retention_scheduler/exact_no_mutation_saturation` receipt, no Transition/deletion/cleanup occurs, and the sweep stops as specified below |
| private resolver | any simultaneous `opaque_input_bytes\|resolver_candidates\|decoder_input_bytes` reaches coalesce into one `private_input_stopped` effect and one cleanup union; every dimension maps to that effect and no fact is admitted |
| campaign branch with any branch-decisive reach | one atomic branch-terminal bound Transition wins; private/decoder stops still contribute their exact cleanup and terminal sensor disposition, while every soft/informational candidate is `safety_subsumed_by(branch_bound_reached)` and is never staged or published |
| campaign branch without a branch-decisive reach but with a private/decoder stop | the private stop wins for each affected producer; compatible soft/informational observations from unaffected producers coalesce into one pending source set, while an observation from the stopped producer is safety-subsumed and admits no fact |
| campaign branch with only soft/informational reaches | all compatible observations coalesce into one pending transaction and one finalizer; the exact complete source set is staged/published together |
| campaign with `total_time` reached in an exact hierarchical cut | campaign terminalization wins before every descendant branch/producer observation, subsumes queue effects, and consumes the exact descendant receipt/cleanup union; output-only effects may still coalesce into one bound over the terminal response |
| campaign without `total_time` | all queue dimensions coalesce into one queue effect and all output dimensions coalesce into one continuation effect; both may coexist in the exact Operation receipt |
| new/independent admission or active-campaign alias | every scope-valid admission dimension above is checked as one conditional all-caps reservation; any reach yields one `admission_capacity_rejected` Operation receipt and no reservation/write, while all-within yields `exact_reserved_all_caps` consumed atomically by the exact admission/key-binding batch |

Nested live bounds have one deterministic linearization point. The owner scheduler
first captures `exact_hierarchical_campaign_cut` across the campaign and all live
descendants. `total_time=reached` wins at that cut: one campaign-root
`campaign_bound_stop` or bound bundle carries every child-scoped observation as a
locally scoped member, cancels each losing child transition, emits exactly one
terminal result per sensor, and commits one deduplicated cleanup union. A child
receipt from the same or older campaign epoch cannot commit independently after
that winner. Only when total time is `within` are branch-decisive receipts
evaluated, then private/decoder stops, then soft/informational reaches. Completion
committed before the sampled cut wins; at the same cut the deadline wins. A stale
receipt after any competing lifecycle revision rejects. Thus exactly-one local
correlation scopes are preserved inside one hierarchical envelope without
processing-order ambiguity or duplicate result/cleanup.

Terminal sensor results are folded exactly once per sensor identity after that
dominance function. The fixed safety order is campaign-decisive stop >
branch-decisive stop > private/decoder-input stop > soft/informational sensor-time
reach. The winner emits the one terminal result (`cancelled_by_campaign_transition`,
`timed_out\|truncated`, `input_bound_reached`, or the soft/informational bound result
as applicable); every secondary reached cause remains fully represented only in
the per-dimension disposition/receipt map. Different sensors each retain one
result, while their cleanup resources form one deduplicated union. Missing a
sensor, emitting two terminal results for one sensor, or counting one resource
twice rejects the bundle.

The names-census value `multiple_exact` is one census-truncation decision with its
single `truncated_multiple` marker. Its internal census counters do not create
duplicate entries in this independent reached-dimension/effect map.

Every multi-reach campaign/branch/resolver composition uses one
`bound_effect_bundle_commit` Transition
transaction keyed by the complete reached set, disposition/correlation map,
unsuppressed effect set, and cleanup union. Security private-stop or conditional
soft receipts are transaction-bound inputs. Queue/output effects contribute only
a frozen proposed-output descriptor; after the state commit, Operation
deterministically derives and applies its receipt from the exact committed bundle.
That postcommit receipt can never be an input to the transition it presents. The batch
atomically commits all Transition-owned terminal/staged members, all exact sensor
terminal dispositions, and cleanup; compatible soft sources share one inert
pending identity and one later finalizer. EvidenceBoundary composes no public
receipt until the full map, cleanup union, and any postcommit Operation receipt are
exact. A crash therefore leaves
either a replayable durable terminal bundle or one inert pending transaction,
never a partially public fact or partially applied effect.

Retention multi-reach is the closed exception: its counters never enter a running
campaign bundle. When an eligible record exists, one exact retention receipt
selects one terminal or tombstone victim under the fixed global/partition order,
and the corresponding `retention_expiry\|retention_purge` Transition consumes the
full coalesced map. When none exists, the distinct exact no-victim path below
applies instead.
Maintenance repeats headlessly only while each committed store revision exposes an
eligible victim. If all remaining records are protected or continuity-unproved,
the exact `capacity_saturated_protected` result emits one durable
`retention_scheduler/exact_no_mutation_saturation` effect receipt, performs no
Transition or deletion, schedules
one deduplicated wake at the earliest proved protected-until, or first commits the
required lease re-anchor and uses one bounded continuity backoff. It then stops;
new admission/alias reservations return precommit `rejected_bound`. Maintenance
resumes only on that timer or a store-revision signal, never a polling loop, and
the wake/backoff itself is bounded by frozen count/time limits.
Startup recovery enumerates and sweeps every historical engine-boot partition;
current-boot reachability is never assumed.

1. Changed/missing/mismatched bound receipt, incomplete applicability coverage,
   any `exceeded` counter, reached-set inconsistency, or extra/mismatched effect
   record derives rejected. An exceeded counter is an implementation invariant
   failure; it is never normalized to ordinary truncation. Whenever response
   byte/token dimensions are applicable, the response measurement contract must
   be the exact frozen default caps, reference tokenizer suite and versions,
   fixture corpus, and measurement command from FR-100/SC-004; otherwise the row
      rejects. Other rows require that axis `not_applicable`. A partition retention
      scope in `victim_selected` state requires exact same-partition victim selection;
      a saturation state requires selection `not_applicable` and exact
      `exact_no_mutation_saturation`. Global victim selection requires the exact
      all-partition eligible-set digest, global order, serialized record/byte counts,
      and every selected victim's owning-partition receipt; global saturation binds
      those counters/order with no selected victim. Mismatch/cross-partition selection
      rejects; unproved selection or saturation proof derives unknown and deletes nothing.
2. Unproved status/applicability or observation-receipt proof derives unknown and
   starts no further action. Exact receipt/map, every applicable status within,
   exact-none reached set, boundary phase `effects_committed`, and no effect/
   cleanup records derives within.
3. A reached counter first emits an immutable private bound-observation receipt
    binding the exact scope, frozen limit/revision, observed counter, reached set,
    affected ownership set, and requested role. A per-sensor-time receipt also
     binds the complete frozen relevance closure and affected evidence class; a
      retention `victim_selected` receipt binds the complete eligible-set digest and
      victim-selection order. At `counter_observed`, saturation instead binds the
      exact empty complete maintenance-eligible set, protected/unproved root/count,
      and proposed wake-or-backoff with victim selection `not_applicable`; at
      `effects_committed`, the scheduler must have durably registered that wake and
      emitted the exact same-content `exact_no_mutation_saturation` effect receipt. Global
     receipts additionally bind store revision, exact partition/record/serialized-
     byte counters, and each owning-partition proof. At phase `counter_observed`, no
   public boundary receipt exists and disposition is `effects_pending`; this
   receipt cannot itself claim cleanup or a state transition.
4. TransitionModel consumes that receipt directly for a single hard branch stop,
     campaign stop, retention expiry, or retention purge. For non-retention live
     scopes it uses the exact `bound_effect_bundle_commit` composition above
     whenever more than one independent dimension is reached, including the exact
     hierarchical campaign cut. Retention uses only the closed coalesced-maintenance
      exception and never a running-campaign bundle. Saturation is the sole
      no-Transition maintenance effect: the retention scheduler consumes the private
      observation, durably registers the bounded wake/backoff, emits
      `exact_no_mutation_saturation`, and maps every reached retention dimension to
      that receipt; Transition, Operation, victim selection, deletion, and cleanup
      are `not_applicable`. Each per-dimension map entry names its exact effect receipt;
      a scalar receipt may not stand in for a multi-reach.
     For a
     soft/informational observation, SecurityPropertyModel first consumes the
     private observation and proposed Transition digest and emits the exact
     `trusted_pending_effect_commit` conditional receipt; Transition then consumes
     both, stages inert pending members, completes cleanup, and returns the pending
     `exact_transition`. EvidenceBoundary composes the final receipt from that
     completed stage; the distinct Transition finalizer must then consume the
     final receipt plus pending identity and atomically publish the staged members.
     Neither intermediate receipt nor pending member is scheduler/read/public fact
     authority. OperationModel consumes the private observation for post-admission queue and
     authorized campaign presentation effects and returns
     `exact_environment_operation`. SecurityPropertyModel first consumes a
     private-input observation and returns
     `exact_security_private_input_stop`, an exact non-authorizing effect receipt
     over the owned resource set that cannot claim cleanup. TransitionModel then
     consumes that receipt in `sensor_private_input_stopped`, commits the one
     top-level terminal sensor result and sole shared cleanup receipt, and returns
     `exact_sensor_private_input_stop`; EvidenceBoundary consumes this complete
     ordered pair. Security separately consumes non-campaign whole-name
     rejection/omission before any value/path/allowlist lookup and returns
     `exact_security_surface_name_application`.
      Pre-admission conflict/denial/unavailability/bound rejection/commit failure uses a
     fixed complete typed error envelope that is provably within its cap and never
     issues an unredeemable output continuation.
   Every stopped live member additionally requires the one exact shared cleanup
   receipt. Phase `effects_committed` without those exact correlations, member
   coverage, or required cleanup rejects. This staging is acyclic: observe;
   derive any conditional/non-authorizing Security effect; stage/clean; compose
   the final public receipt; then atomically publish staged members. The finalizer
   is downstream of the receipt and is not an input to its composition.
5. Any reached branch/campaign evidence, soft/informational observation,
     private-input, or names-census dimension derives
    `evidence_incomplete`. If all reached dimensions are output, queue, or
    retention only, disposition is `presentation_bounded`; these effects never
    downgrade underlying fact completeness. A snapshot `snapshot_name_omit`
    per-name reach derives `presentation_bounded`, omits the entire name before
    allowlist/path/value lookup, and creates no branch or campaign. A command/job,
    PTY, or session `execution_name_reject` instead derives only `rejected` with a
    non-authorizing typed rejection; transport never starts and no presentation
    receipt is issued. Multiple simultaneous dimensions retain the complete
    disposition map and deterministic primary display reason; only the fixed
    safety dominance table may coalesce or subsume an effect.
6. Monotonic raw/unique/private-input/byte counters and monotonic per-sensor/total deadlines,
   bounded queues, cancellation propagation, and the admitted concurrency ceiling
   prevent infinite sensing or waiting.

### Boundary receipt composition

`boundary_receipt_effect` is `issued_exact_complete`,
`issued_exact_evidence_incomplete`, `issued_exact_presentation_bounded`,
`no_receipt`, or `rejected`. It is
derived in this strict order: any component rejection is `rejected`; otherwise
any unknown or effects-pending row is `no_receipt` with public verdict `unknown`;
otherwise `issued_exact_evidence_incomplete` applies when every applicable
name/census decision is accepted and
at least one names-census item is whole-name omitted/truncated or has a committed
branch/campaign/soft/informational/private-input evidence bound; simultaneous presentation-only effects do not
hide that stronger epistemic result. It is `issued_exact_presentation_bounded`
when every applicable name/census decision is accepted, evidence remains complete,
and the only reached effects are output continuation, queue gate, retention expiry,
retention purge, `retention_saturated_protected`,
or snapshot whole-name omission. It is `issued_exact_complete` only when every
applicable name/census decision is canonical/complete and every bound is within.
Command/job, PTY, or session whole-name rejection can therefore produce only
`rejected`, never presentation-bounded. Every receipt binds
    exactly one correlation-scope identity plus all applicable bound counters/effects
and accepted effect receipts. Campaign/branch/resolver receipts additionally bind
target persistent identity, boot, workspace, sensor, plan, and invocation fields
that are applicable to that scope. Request receipts bind peer/action/request-key
attempt only; partition retention receipts bind canonical `(engine_boot_id,
peer_scope_id, tenant_scope_id, admission_policy_revision)`, store revision, and
the exact maintenance state. A victim-selected receipt binds eligible-set digest
and selected victim; a saturation receipt instead consumes the exact
`retention_scheduler/exact_no_mutation_saturation` effect receipt and binds its
empty complete maintenance-eligible-set digest, protected/unproved root/count, and
durably registered exact wake or bounded-reanchor-backoff. No Transition, cleanup,
victim, or Operation receipt is invented. Global retention receipts additionally bind
daemon store identity and all-partition digest/counters/order; victim-selected
global receipts bind the selected partition+victim and exact owning-partition
receipt, while saturation binds no victim. Both require target,
workspace, branch, and active-caller fields `not_applicable`. When name/census identity
is applicable it additionally binds consent/requested-name scope and exactly one applicable name projection: the
canonical name equality class, the exact ordered/deduplicated emitted-name set
root/count plus truncation marker, or the non-reconstructive whole-name-omitted
marker. A non-name fact binds the explicit `not_applicable` name/census projection
instead. It contains no environment value.

Security consumes the composed public verdict as `admitted_complete`,
`admitted_evidence_incomplete`, or `admitted_presentation_bounded` only with exact
receipt correlation. Only evidence-incomplete changes a fact's epistemic
completeness; presentation-bounded leaves it unchanged. Overlay-name
allowlist matching may consume the canonical name equality class from this
receipt under its command/PTY/session/snapshot scope, bound additionally to
policy revision and opaque material handle; this
does not admit the overlay value as evidence.

Every assignment not selected above, including a non-neutral unselected axis, is
a rejected checker constraint. Thus every row has exactly one
`native_name_admission` (possibly `not_applicable`), one census decision
(possibly `not_applicable`), one non-optional bounded-evidence disposition, and
one receipt effect.

## Structural and Derived Constraints

Constraints classify invalid assignments as `unsupported` in the applicable
model unless a more specific normative denial/unknown outcome is stated.

1. `aap_embed` requires `embedded_engine`; MCP deliveries with that harness are
   unsupported unless a separate MCP topology is explicitly under test. MCP
   harnesses require an MCP delivery; `unknown_embed` requires `embedded_engine`.
2. `compact_environment`, `full_environment_probe`, `legacy_system_discover`,
   `legacy_target_list`, and `legacy_target_probe` require an MCP delivery;
   `embedded_facade` requires `embedded_engine`. Every legacy surface remains MCP
   only until its explicit embed contract exists.
3. Each edge satisfies `target(OS)_i = source(OS)_(i+1)`, `edge_count = hop_count`,
    endpoint indices are contiguous, and its target-workspace reference points to
    exactly `Node(i+1)`. The target node is the single source of workspace mapping
    and identity truth; `contradictory_copy` maps exactly to
    `workspace_mismatch`/Route `blocked`, never structural `unsupported`.
    Unknown OS remains explicit.
4. A `wsl` edge is exactly either Windows host -> its bound Linux
    `wsl1\|wsl2` guest or that Linux guest -> its attested Windows host through
    enabled interop. The edge carries the exact host/guest connector and identity
    binding; arbitrary Linux -> Windows is unsupported. Container, sandbox, VM,
    and Firecracker connectors require the
   corresponding target isolation facet. `firecracker_vsock` additionally
   requires a Firecracker guest and an embedding or configured host connector;
   a CID alone is not identity.
5. `embedded` requires structural same-process equivalence. Every executing
   `local_direct`, WSL, forwarded, container, sandbox, VM, or Firecracker edge
   requires a distinct target process and the roles in the compatibility table.
   MCP delivery requires its declared adapter/engine process split. `identity =
   not_applicable` is valid only for a structural same-process edge; every other
   cross-process, forwarded, host, VM, or guest edge requires an identity
   outcome.
6. A shell state other than `not_required` requires the `shell` or
   `persistent_session` lane. Direct argv with a shell-interpreter substitution
   and no shell authority is `interpreter_denied`, never ready.
7. Workspace identity `verified` requires same-path or translated-identity proof
   at that target. Reachability with an identity mismatch never converges.
8. Repeated persistent/boot equality classes emit `hard_excluded` with
   `route_cycle_detected` before the repeated execution hop. Change disposition
    maps exactly: authoritative policy/catalogue-trust-security/cwd/action
    revision -> its recoverable
   invalidation cause; approved pre-execution connector/persistent-id/boot/
   workspace change -> the matching `_change_approved` recoverable cause;
   unapproved connector change -> `connector_unapproved_rotation`; mid-run
   connector change -> `connector_mid_run_replacement`; unapproved/mid-run target
   persistent identity -> `target_persistent_identity_mismatch`; target boot ->
   `target_boot_mismatch`; workspace -> `workspace_mismatch`. No assignment may
   choose between recoverable and hard paths. Every descendant fact is
   recursively invalidated before another sensor can start.
9. A stopped target remains stopped during a normal probe. Start/wake requires
   a standing cap plus separate explicit per-run authority at every governing
   node. Full names-only census likewise requires an explicit request for this
   campaign; a plan or standing cap cannot silently upgrade targeted sensing.
10. The derived sensor decision is a closed function of the complete bounded
    per-node authority records, sensor class, exact connector-cap mapping,
    underlying action, target decision, and connector chain. Missing required caps deterministically
    deny in every profile, including `full_access`; contradictory
    "cap absent + allowed"
    assignments are unsupported checker failures, not test inputs to bless.
11. `read_only_observer` may allow passive/name observation after opt-in but
    rejects every spawned implementation, including spawned targeted-name or
    census adapters, plus helper, sentinel, target-start, shell, and session work.
12. Executable origin, writability, trust provenance, form, binding completeness,
    and dispatch identity are orthogonal. User/workspace/unknown writability
    requires `allow_environment_untrusted_exec` even when operator-pinned or
    system-origin. Scripts/shims require both interpreter and script identity;
    changed or unprovable identity yields no spawn and a typed denial/unknown
    result.
13. Every authenticated local-process, forwarded/remote, or guest evidence/private
    channel requires the exact scope-appropriate channel identity/binding proof,
    authenticated integrity, and complete per-message action/audit binding. A point
    challenge alone is insufficient. Channel-identity negatives follow the exact
    channel disposition table; per-message negatives follow the distinct
    action/audit disposition table; aggregate receipt-correlation, channel-security,
    and endpoint-binding replay/expiry values follow the source-total disposition
    table; every remaining supported nonpositive value follows the closed
    nonpositive channel table. The causal effect-source cardinality/correlation axes MUST satisfy their
    pre-decision pairing rule before any mapped result is emitted. `fresh_nonce_replayed`, unapproved rotation,
    clone detection, persistent identity/boot mismatch, protocol/endpoint/route/
    connector mismatch, trust-root mismatch, mid-run replacement, or sequence replay
    rejects distinctly. Ordinary `expired` rejects only the current proof/message
    and selects bounded fresh re-attestation/re-gating; `missing\|unproved` yields
    unknown and starts no downstream action. Same-process paths require the proof `not_applicable`.
    Overlay values use a structurally separate opaque private channel; they can
    never become evidence, and cross-boundary transport requires authenticated
    end-to-end confidentiality. Replay rejects the corresponding evidence/private
    receipt; expiry invalidates it and re-gates as above; missing/unproved channel
    identity is unknown, while a missing required per-message field follows the
    action-audit-mismatch row. Neither starts downstream action.
14. Overlay persistence is permitted only when persistence is `on`, the canonical
    name is `operator_allowlisted`, and immutable provenance is
    `caller_supplied`. Secret classification never authorizes it. All other
    states are omitted or rejected; inherited values and legacy redaction markers
    are never stored or restored.
15. Only `closed_valid` helper output may become evidence. Free-form, invalid,
    truncated, or undecodable helper output becomes a typed non-content status,
    never relabelled evidence.
16. `raw_or_unproved` diagnostics MUST NOT leave the private decoder. Every
    diagnostic source must become a closed typed code/safe enum, an exactly
    frozen-bound `bounded_count`, or be omitted. Only contextual paths, endpoints,
    and rejected request fields may pass the shared structural sanitizer under an
    exact correlation receipt. Raw/error/overlay/helper/remote/malformed-plan
    sources can never become structural text. An exceeded count is omitted; an
    unproved or mismatched bound emits nothing.
17. A ready route requires `Beachhead proof state = current`. Every later spawn
    atomically re-runs hard validators against current engine/policy/connector/
    target/workspace/catalogue-trust-security revisions, integrity of the
    admitted frozen plan snapshot/digest, and exact allowed substitutions. A
    different registry activation head alone does not stale the admitted run.
    Every other proof state, including `retired`, yields typed `proof_stale` and
    no spawn. A completed campaign's retained result is readable but never a
    dispatch authority; later work requires a new campaign and proof.
18. Deterministic route selection derives `clean` when Route `ready` is present,
    otherwise `soft_warning` when Route `ready_with_warnings` is present, and
    otherwise `not_applicable`. A safe selection uses alternative-rank proof
    `open`, `complete`, `proved_cannot_outrank`, or `bounded_incomplete` according
    to the exhaustive open-role rules; no safe selection requires
    `not_applicable`. `open` requires `rank_changing_open`. When rank-changing
    work is bounded or truncated, its frontier becomes `none` and proof is
    `bounded_incomplete`. Overlap-incomparable freshness retains every maximal
    interval, then operator preference/stable identity select within that set while
    the receipt preserves the incomparability fact. When every still-open alternative is proved irrelevant,
    frontier is `none` and proof is `proved_cannot_outrank`. A
    `safety_or_executability_open` or `rank_changing_open` frontier is
    nonterminal. Every inconsistent combination is an explicit unsupported
    constraint before goal aggregation, so none can fall through. First decisive
    dimension is `ranking_incomplete` exactly for `open` or
    `bounded_incomplete`; all final ranks name their actual lexicographic field.
19. Every keyless start derives `retry=unsafe` regardless of request history; the
    engine never guesses whether a response was lost. `present_new` derives
    `retry=safe`, and a matching key resolves to the original campaign with
    `retry=resolved`.
20. Shareable identical requests may reuse one campaign only after authorization.
    Every differing peer/policy/target/boot/workspace/plan/freshness/bounds class
    starts an independent campaign or is rejected for a typed admission conflict;
    it does not make the environment goal unsupported.
21. Resume reauthorization changes only the attachment result. It does not
    rewrite an underlying campaign goal result.
22. Every campaign lifecycle mutation is the projection of one accepted atomic
    TransitionBatch. Headless completion, failure, engine loss, and retention
    expiry use scheduler/engine/timer receipts; Operation requests consume those
    receipts and cannot fabricate or rewrite them.

## Expected-Outcome Functions

Classification is deliberately two-stage.

### Route classification

Each `RouteModel` assignment is evaluated in this mutually exclusive order:

1. no representable approved sensor/connector/protocol/platform/plan ->
   `unsupported`;
2. required governing policy denial -> `denied`;
3. required target/connector unavailable after bounded authoritative attempts ->
   `unreachable`;
4. complete authoritative hard mismatch or missing/incompatible hard requirement
   -> `blocked`;
5. any remaining hard ambiguity, conflict, stale/incomplete/failed observation,
   unverified identity, or non-current proof -> `unknown`;
6. all hard gates proved with a soft non-satisfaction or soft uncertainty ->
   `ready_with_warnings`;
7. all hard and applicable soft gates proved -> `ready`.

### Goal aggregation

The complete route-outcome presence vector and frontier derived from exhaustive
open-branch role records are then
evaluated in this mutually exclusive order:

1. if a safe ready route exists and the frontier remains `rank_changing_open` ->
   `in_progress`;
2. if a safe ready route exists, `selected_safe_route = clean`, and
   alternative-rank proof is `complete` or `proved_cannot_outrank` -> `ready`;
3. if a safe ready route exists, the frontier is `none`, and either
   `selected_safe_route = soft_warning` OR alternative-rank proof is
   `bounded_incomplete` -> `ready_with_warnings`;
4. with no safe ready route, any open frontier -> `in_progress`;
5. otherwise terminal campaign evidence `bound_before_route` or any `unknown`
   route -> `unknown`;
6. otherwise any `blocked` route -> `blocked`;
7. otherwise, when at least one route is `denied` and every other route is
   denied, unreachable, or unsupported -> `denied`;
8. otherwise, when at least one route is `unreachable` and every other route is
   unreachable or unsupported -> `unreachable`;
9. no routes with terminal campaign evidence `none`, or only `unsupported` routes
   -> `unsupported`.

A ready route plus a still-open rank-changing alternative is `in_progress`.
After that work is bounded/truncated, the same safe route plus
`frontier_effect = none` and `alternative-rank proof = bounded_incomplete` is
`ready_with_warnings`. `blocked + denied` aggregates to `blocked`; `unknown +
denied` aggregates to `unknown`. No assignment may fall through. Each checker
result records the exact clause and bounded contributing reasons.

## Mandatory Live Conformance Families

These are minimum live gates, not substitutes for the exhaustive model. The
coverage universe is generated from the shipped harness/surface support manifest,
versioned per-surface action/field schema registries,
platform support table, connector capability registry, policy-profile registry,
  sensor catalogue, goal-plan/comparator and comparator-conformance-corpus/oracle
  registries, authoritative intentional
  value-bearing private-store inventory, authoritative private-tainted-input
  producer registry, authoritative forbidden-sink inventory, and the explicit
  critical-topology list below. Every required
tuple has a stable key over those source revisions. CI MUST report the generated
required-key set, witnessed-key set, unsupported-with-reason set, and exact
missing keys, and MUST fail when any required key is absent, duplicated, or
silently removed. A test-owned row list cannot redefine or shrink this universe.
Changing a source registry requires an explicit coverage-manifest delta in the
same change.

- every supported harness delivery and product surface on every operating
  system on which that harness is supported;
- every `add\|store\|search\|validate\|test\|activate\|audit` goal-plan administration
  action crossed with its permitted authority, each forbidden authority, exact
  dedicated-registry proof and every sifter type/storage/identity/lifecycle
  alias, success, malformed input, every individual and multi-member forbidden-
  content set (`command_invocation`, `script_body`, `shell_text`, executable
  locator, credential/secret material, environment-value material, permission
  change, authority grant, and side-effect/runtime behavior), unproved
  classification, read/write unavailability,
  unsupported schema version, missing/version conflict, validation/test failure,
  commit failure, audit failure with rollback, and activation attempted against
  a current admitted run; successful activation MUST prove future-run-only
  visibility and a concurrent admitted run MUST retain its exact frozen plan.
  Every forbidden-content row MUST prove zero plan persistence and authority
  change plus typed-only diagnostics/audit with no rejected content;
- every shipped Python, Rust, Node, Go, .NET, Java, C/C++ build, container, Git,
  and GitHub plan family plus typed custom requirements and every authorized
  `operator_local_composite` selection/composition, crossed with automatic/
  explicit selection, exact/no/multiple/conflicting/stale/unavailable/unproved
  manifest evidence and workspace binding, zero/one/bounded-many/over-limit
  selection, every composition outcome, candidate/selected missing, extra,
  duplicate, family/version/digest mismatch, count/root mismatch, and
  activation/catalogue-head change with an exact frozen selected-set snapshot;
  Rust+Node and another maximum-bound multi-stack fixture MUST prove exact
  candidate-to-selected mapping and frozen-set equality. Every ecosystem
  comparator source/outcome and catalogue/corpus proof MUST be witnessed against
  an independent official reference fixture corpus covering canonical,
  normalization-alias, prerelease/development/postrelease,
  epoch-or-equivalent, local/build, malformed, and ambiguous/unsupported roles,
  including missing/duplicate case keys, non-independent oracle, and observed-
  oracle mismatch gates, with Python metadata-only success plus
  canaries proving package import and package-code execution never occur;
- every invalid goal, plan, field, and action on compact, full, and embedded
  surfaces, generated from the current versioned surface schema and plan head;
  each positive row MUST schema-validate its returned corrective call and prove
  an exact complete or bounded-prefix choice set plus zero pre-admission effects,
  while wrong namespace, stale/missing/mismatched catalogue, empty/over-cap/
  duplicate/unproved choices, wrong-surface/invalid corrective calls, and every
  forbidden allocation, mutation, observation, or authority effect are negative
  gates;
- Windows direct argv, `cmd.exe`, PowerShell Desktop/Core, Windows-to-WSL1/WSL2,
  and WSL1/WSL2-to-the-attested-paired-Windows-host direct lanes, with interop
  disabled, nested-shell denial, persistent/boot identity mismatch, and policy
  denial witnessed independently in both directions;
- Linux native, container, sandbox, CI, and Linux-to-container-to-sandbox;
- macOS launch-service, login-shell, interactive-shell, native toolchain/SDK,
  and a container or VM connector where supported;
- every connector kind across every supported source-OS/target-OS boundary;
- same-host, remote-forwarded, remote-to-container, sandbox-to-approved-remote,
  and configured multi-hop with deliberate persistent- and boot-identity cycles;
- AAP embedded engine to Firecracker guest over vsock, including room/VM/boot/
  agent identity, restart, and CID reuse;
- every policy profile crossed with every sensor class and required capability,
  including absent-cap denial under `full_access`;
- trusted and untrusted executable origin, shim identity, and change-before-spawn
  fixtures on Windows, Linux, and macOS, plus positive, mismatch, and unproved
  adapter-revision, side-effect-declaration, direct-argv, least-privilege-env,
  cwd/workspace-search-exclusion, canonical-identity-only positive plus every
  origin/writability/form/binding/dispatch complement, and each independently disabled download,
  update, hook, plugin, startup, import, build, lifecycle, and project-code control;
- private observed-value context fixtures for target process, exact target
  shell/session, and explicitly authorized harness process, every source/context
  cross-pair rejection, ambient-harness fallback rejection, and rejection of a
  generic HarnessContext receipt substituted for `EnvironmentPrivateResolve`;
- operator-pin administration fixtures for pre-existing current pins and audited
  add/rotate/revoke by the operator-only surface, current/revoked use, and explicit
  denial of LLM/caller pinning and trust-on-first-use for executable and channel
  trust;
- authenticated-integrity, authenticated-confidentiality, challenge-only,
  origin-to-final end-to-end confidentiality, hop-only confidentiality rejection,
  endpoint mismatch, replay, expiry, missing-action-binding, and
  missing-audit-binding channels, plus verifier-monotonic exact/expired,
  suspend-inclusive elapsed, wall-clock rollback/jump, engine restart/boot change,
  and clock-continuity-unproved freshness fixtures. Local, two-hop, maximum-hop,
  and mixed-freshness routes MUST cover exact ordered per-edge record-set
  count/roots, missing/extra/duplicate/swapped edge records, one expired or
  continuity-lost forwarder among otherwise exact edges, multiple expired edges,
  and record/root mismatch; no final-target-only proof may pass. Cross-axis
  fixtures MUST include identity nonce replay plus freshness expiry, identity
  missing plus unauthenticated security, per-message replay plus confidentiality
  unknown, causal same-source projections, independent sources, and disposition
  conflict, proving exactly one derived decision/effect/Transition in every row;
- every representable accepted later hard exclusion from prior
  `denied\|exhausted\|truncated`, crossed with no context/fact change and with
  every authorized exact per-record change effect. The generated symbolic family
  MUST cover every nonempty same-class persisting subset, full persistence,
  full invalidation to `blocked`, class shifts `unsupported -> denied`,
  `unsupported -> unreachable`, and `denied -> unreachable`, conflict-resolution
  recomputation, and prior `evidence_unavailable\|bound_reached` with no current
  higher winner. Each valid row MUST derive its replacement outcome from the
  complete current post-batch closure, install exactly one replacement Route
  with lifecycle state `excluded` and the new exact cause/event, exclude the old
  Route from Goal, and authorize no work. Negative twins MUST reject changed
  retained identity/digest, a changed-context-dependent retained record, an
  independently proved current record invalidated, missing/extra/duplicate member
  effects, unauthorized source `add`, invalid dependency root/count, a
  `duplicate\|missing` Transition transaction identity or cross-snapshot member,
  stale provenance used as current, an asserted
  outcome that differs from the ordinary current winner, altered cause/event,
  safe/unknown output despite the blocking exclusion, zero or two replacement
  Routes, a non-change cause that demotes evidence, and reuse of the superseded
  evidence digest after a subset or class change;
- ordinary trusted-fact atomic admission; every typed non-fact terminal sensor
  result with exact producer cleanup or proved-empty resources; applicable
  non-audit immutable-receipt mismatch and same-process non-audit replay at
  every owning property, proving the exact property-local rejection, exact
  non-authorizing receipt, and zero hard-exclusion Transition; and action-audit
  commit failure, unavailability, mismatch, and replay at origin, forwarder, and
  target nodes, including the non-policy hard-exclusion receipts;
- every finite dimension generated from the frozen scope-aware bound registry,
  not a hand-selected subset. Representative families include resolver input
  bytes/candidates, decoder input,
  per-name native bytes, response bytes/tokens, per-sensor and campaign time,
  topology nodes/edges, sensors, requirements, names, total bytes, queued/active
  campaigns, helper processes, concurrency, run-registry records/bytes,
  retention partitions/bytes, request-key aliases, and key-index records/bytes.
  Each applicable dimension is crossed with below-limit, exactly-reached,
  exceeded-invariant, unproved, missing-receipt, and mismatch states plus its
  exact scope/role/effect. Admission reaches MUST cover fixed `rejected_bound`
  with zero reservation/mutation, queue reaches MUST cover signal-driven
  wait/wake and deduplicated capacity release. Matching-key start, resume, and
  detail attachments to an existing `accepted` campaign MUST reuse the exact
  pending `campaign_start` wait/wake and prove zero new registration,
  reservation, Transition, spawn, campaign mutation, or key mutation.
  Branch/campaign reaches MUST cover
  their exact terminal or incomplete-evidence effects, and private stops MUST
  cover exact non-authorizing receipts followed by independent cleanup;
- per-sensor timeout for hard, soft-only, and informational-only relevance,
  including a soft-only `ready_with_warnings` path and a zero-route
  `bound_before_route` campaign result;
- the last-open-branch/campaign-completion/total-time race: completion committed
  before the sampled cut wins; a deadline reached at the same cut wins; a deadline
  reached after the last branch terminalizes but before completion commits uses
  the Route-bearing all-terminal `campaign_bound_stop` arm, consumes the bound
  receipt, retains the exact terminal Routes and Goal unchanged, snapshots, cleans
  up, and commits exactly one `running -> completed`; and a receipt arriving after
  completion is stale and rejects. The same family MUST cross a cancelled-only
  zero-Route ownership closure and a zero-Route closure containing an
  already-retracted descendant plus its cancelled root: when the reached receipt
  wins before completion, both use only the exclusive `bound_before_route` arm,
  derive Goal `unknown`, and cannot retain the pre-bound zero-Route Goal. Every
  ordering proves one terminal campaign Transition, one bound-receipt disposition,
  no silent bound omission, no Goal fabrication, and no duplicate snapshot,
  cleanup, or terminal-retention construction;
- targeted and full names-only census with all four counters below/exactly at
  bound, each single reached-dimension/marker pair, `multiple_exact`, exact
  ordered/deduplicated set root/count, targeted requested-set equality/subset,
  full-census consent exact/absent/mismatch/replay, unrequested-name rejection,
  and partial-name/value-pair/marker/count substitution rejection;
- retained-run expiry and tombstone purge through both timer and capacity paths:
  deterministic same-partition/global victim selection, protected or
  continuity-unproved no-deletion, cross-tenant/cross-policy/stale eligible-set
  rejection, exact key/index deletion, non-final partition delta zero, final
  partition audit fold-and-delete, and the saturated no-victim path with one
  earliest proved signal/backoff, no mutation, no polling, and no duplicate wake;
- overlay transport direct caller-ingress and restore-only persisted-ingress-chain
  receipts, cross-operation substitution rejection, and surface-application
  receipts across exact, asserted, inherited/legacy, missing, mismatch, replay,
  omitted, rejected, and unknown states; overlay persistence allowlist/omission, explicit on/off plus
  absent/unset-config default-off proof before any name/value/path consultation,
  operation-matched store and restore, and
  cross-operation receipt-substitution rejection;
- every inventoried value-bearing private store at legacy-only purge, verified-
  current opaque preservation, mixed-store atomic preserve-plus-purge, commit
  failure, crash before/after commit, restart recovery, unavailable/unknown state,
  prior quarantine, each finite retry attempt, budget exhaustion, total-deadline
  exhaustion, operator-repair terminal state, successful recovery before
  exhaustion, and already-migrated idempotence. Positive rows prove exact
  pre/post equality of current-private opaque record-identity and retained-index
  sets while no legacy/unproved record or index survives. Failure/crash rows prove
  rollback preservation while restore/list/status stays inaccessible. Retry rows
  prove strict attempts-remaining decrease, crash-safe deduplication, signal-only
  wake, no spin/polling, no restart reset, and zero automatic wakes after terminal
  exhaustion. Operator-repair rows cover exact pre-existing store-admin authority
  and committed audit plus LLM/caller/plan/sensor/migration-authority denial. Each inventory entry contributes stable secrecy, migration,
  preservation, deletion, failure, rollback, retry, exhaustion, and recovery keys,
  and any missing key fails the gate;
- every authoritative private-tainted-input producer, including platform-native
  and fixed-helper private resolvers, fixed decoders, and every later registered
  producer, crossed with every authoritative forbidden sink--normal command output,
  rings, buckets, tails, context, errors, logs, audit, snapshots/persistence,
  caches, embed/transport surfaces, and every later registered sink--and with
  success, each typed failure, input-bound stop, cleanup, and attempted raw-egress
  paths; every row requires the exact-current build/runtime attestation, sink-
  complete canaries, and missing/stale/mismatch/unproved-key gates;
- every diagnostic source/result class, structural-sanitizer source restriction,
  bounded-count below/exact-bound, exceeded omission, unproved/mismatch rejection,
  payload-length/byte-derived count laundering rejection, and no raw-string or
  unbounded-count canary;
- versioned provenance-envelope and per-fact source/time/completeness/freshness/
  evidence-grade wire fixtures across compact, full, legacy, and embed surfaces;
  forwarded terminal summaries with exact executing-node authentication plus bound
  local audit, and unauthenticated/missing-audit/mismatch/unproved summaries that
  contribute only typed unknown and no authority; all carry canaries excluding
  environment names/values, raw output, and secret-shaped request data;
- `legacy_system_discover`, public session/workspace status, and terminal identity
  on every supported delivery/OS, proving the versioned names-only wire, empty
  legacy value-pair fields, zero environment-value identity reads, explicit
  migration guidance, and old/new contract canaries;
- `legacy_target_list` and `legacy_target_probe` on local and forwarded delivery:
  registry-only enumeration performs no implicit liveness dial; every requested
  reachability/health observation requires the exact connector sensor class,
  connector capability, underlying remote decision, and pre-action audit before
  any forwarded-socket contact. Fixtures cover denied-before-dial, authorized
  execution, audit denial/failure, unavailable transport, and successful liveness;
  denied or unobserved is never relabelled unreachable, and a health response
  never upgrades reachability into target identity or a beachhead proof;
- one frozen semantic case key witnessed equivalently through compact, full, and
  embedded delivery; legacy actions receiving the same policy-before-observation
  denial; older-engine pre-dispatch detection with actionable guidance; the narrow
  semver-stable embed facade ABI plus connector conformance kit; and the AAP
  Firecracker row against a pinned compatible TC release; and
- current, expired, policy-, catalogue-trust/security-, engine-boot-,
  target-persistent-identity-, target-boot-, workspace-, cwd-, connector-,
  frozen-plan-integrity-, action-audit-mismatch-, action-audit-replay-,
  action-schema/digest-, and substitution-invalidated beachhead proof states at
  real dispatch, plus `retired` proving dispatch denial without reactivation.

Unsupported live combinations require explicit fixture rows and reasons; they
must not disappear from reports.
