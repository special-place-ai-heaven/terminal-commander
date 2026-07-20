# Root Disposition: Environment Probe Adversarial Review

**Status**: Specification amended; independent follow-up review pending

**Source review**:
`docs/reviews/2026-07-17-environment-probe-fable-review.md`

## Intent

Make the goal-directed environment probe planning-ready without weakening the
existing engine, policy, audit, secrecy, or signal boundaries. Probes remain
disposable observers; the engine planner owns branching, convergence,
invalidation, retraction, and synthesis.

## Fable Finding Dispositions

| # | Disposition | Specification result |
|---|---|---|
| 1 | Accept | FR-053 now defines observed values separately from intentional caller overlays. FR-057/058 explicitly migrate value-derived terminal fields, public session values, and unsafe snapshot restoration. FR-077 authorizes only those versioned 0.x security corrections. |
| 2 | Accept | FR-052 separates semantic verdict from observation status. FR-076 and the exhaustive route/goal entailment table define `ready` and every competing state. |
| 3 | Accept | FR-090 requires a concrete goal-plan request envelope to pass the real request schema and policy validator before a beachhead is ready. SC-016 gates template/real-request divergence. |
| 4 | Accept | The sensor policy taxonomy names typed actions, default-false caps, underlying action gates, and every existing profile's resolution, including `read_only_observer`. |
| 5 | Accept | FR-010 removes any legacy-sentinel exemption. FR-077 requires legacy and new discovery to converge on one policy-before-observation sensing engine and permits truthful evidence downgrades. |
| 6 | Accept | FR-060 assigns crossing, forwarding, and target-native authority per node. FR-065 assigns per-node action audit plus a bounded owning-engine campaign summary. |
| 7 | Accept and strengthen | FR-082/083/084/096 and `specs/003-environment-probe/scenario-matrix.md` define a finite exhaustive domain. The logical gate is full Cartesian coverage, not pairwise sampling. |
| 8 | Accept | FR-062/063/094 define channel-bound challenge evidence, operator-only pins, rotation/revocation, boot changes, old peers, and AAP room/VM/agent/vsock identity. |
| 9 | Accept | FR-064 normalizes provenance through a run envelope and per-fact references. FR-100 freezes the response cap and tokenizer measurement before implementation. |
| 10 | Accept | FR-088/089 define conflict precedence, transitive invalidation, descendant cancellation/re-gating, re-observation, and target replacement. |
| 11 | Accept as decision; reject deferral | The persistent typed goal-plan registry stays. The user's approved current goal supersedes the older CAP01 deferral. FR-043 records the rationale and enforces separate types, tables, migrations, actions, and authority from sifter rules. FR-092 freezes run-level plan versions. |
| 12 | Accept as deliberate product commitment | FR-081 introduces one narrow semver-stable probe/connector facade, not stability for all daemon internals. TC owns a conformance kit; AAP owns its pinned Firecracker integration gate. |
| 13 | Accept now | FR-039 defines tagged UTF-8, Unix-byte base64, and Windows-UTF-16 base64 name forms with native equality keys. |
| 14 | Accept | FR-004/067/080/098 explicitly place planning, lifecycle, evidence, and run ownership in the daemon-library engine, never the adapter. |
| 15 | Accept | FR-095 freezes real manual transcripts and measurement rules before implementation; SC-003 binds to them. |

## Additional Code-Audit Corrections

- Constitution 2.1.0 now permits only a private bounded typed decoder for fixed
  product sensors; caller command output still uses sifters.
- One trigger now means one campaign, not guaranteed synchronous completion.
  `in_progress`, request keys, idempotent resume, and peer/policy reauthorization
  are explicit.
- Sensor helpers declare side-effect classes, use direct execution and
  least-privilege environments, and disable downloads, hooks, plugins, startup
  files, module imports, build scripts, and project-code execution.
- Workspace reachability no longer implies identity. FR-025 defines target-side
  workspace equivalence and `workspace_mismatch`.
- Isolation is a composable stack/facet model rather than mutually exclusive
  labels; multi-hop traversal is bounded, per-hop attested, and cycle-safe.
- Environment campaigns are a dedicated engine run type, not an ordinary
  command/file-watch/PTY probe.
- Starting a stopped WSL distro, container, VM, or guest is never a normal
  observation; it requires explicit per-run authority and audit.
- The compact surface decision is explicit: sixth `environment` facade plus the
  full `environment_probe`, both calling the same engine operation.

## Lead Judgment

The original `CONTESTED / NOT READY FOR PLANNING` verdict was correct. All
critical and high findings are accepted and amended. Suggested pairwise matrix
reduction is rejected because it violates the approved full cross-matrix goal;
the revised design uses exhaustive logical coverage plus live boundary gates.
Goal-plan persistence is retained because it now has explicit user-approved use
and a non-overlapping trust boundary.

The specification remains blocked from planning until an independent follow-up
review confirms that no critical or high contradiction remains.
