# TC embed parity: implementation and verification

As of 2026-10-09. Local branch `feat/embed-parity`, based on latest pulled
`main` at `7e4e2c08ede01c257601ab99e32f128dffb249c0` / v0.3.14.
Implementation and documentation are prepared for delivery on this branch.
The installed daemon was not upgraded or restarted. The original
[CODEX report](CODEX-REPORT-24-TERMINAL-COMMANDER.md) remains intact.

## Delivered contracts

| Area | Result |
|---|---|
| Full engine embed | `EmbeddedEngine` uses the common daemon dispatcher; named typed methods, exhaustive `IpcRequest`, bounded Serde envelopes, no implicit IPC listener or daemon attachment |
| Capability parity | Commands, shell, PTY, supported sessions/snapshots, files/watches, registry, recipes, sifters/buckets/context, subscriptions, policy, audit, credentials and lifecycle use the existing implementations; platform/policy/host-route qualifications are explicit |
| Identity | Shared health/discovery API/schema version 1, build-input fingerprint and fresh bootstrap identity; non-cryptographic build discriminator is not release attestation |
| Isolated execution | Absolute executable/cwd, validated explicit application env with reserved `TC_DAEMON_CHILD=1`, no ambient PATH/PATHEXT/WSLENV resolution; conflicting marker values rejected; low-level Clear remains exact |
| Raw output | Per-stream raw byte/activity evidence independent of framing, decoding and suppression; reading/complete/failed/incomplete are distinct |
| Process ownership | Runtime validation before spawn, runtime-independent cleanup/reaping; Unix ownership anchor retained through final group signal; verified completion or typed uncertainty |
| CPU | Whole-job Windows Job Object / owned Linux cgroup accounting; bounded Linux process-group fallback treats positive observations as lower bounds and unprovable zero as unknown; unsupported/unavailable states are explicit |
| Compound observation | MCP and embed share run-and-watch; one start, bounded wall-clock observation, preserved IDs/cursor after degradation, no replay after JobLost |
| Receipt recovery | First real terminal outcome wins; persistence failure is observable; abandonment counts actual writes; abrupt owner exit, cancellation and SQLite failure regressions retain truthful uncertainty |
| Output bounds | Tail enforces UTF-8 byte limits, including zero; upstream capture loss and ring eviction flags survive; head-only receipt reports selected-frame capture loss |
| Owner credentials | Host-managed boot/job/prompt-generation challenge by default; local non-Serde authority, generation-bound completion; native prompt is an explicit host option |
| Lifecycle | Exclusive data-directory owner, admission closure, cancellation, bounded drain, store closure and explicit shutdown evidence |

See the complete [operation matrix](../embedding/PARITY.md),
[embedding guide](../EMBEDDING.md), [migration guide](../embedding/MIGRATION-0.1.86.md)
and [AAP handoff](../embedding/AAP-SISTER-PROJECT-HANDOFF.md).

Existing public request shapes and default inherited execution lanes retain
their behavior. New isolated execution and owner challenges are additive.
MCP aliases/formatting and operator-selected remote routing remain adapter/host
responsibilities; remote routes are explicitly `HostTransportRequired`.
Output subscriptions retain boot-local cursors and their existing seek/reset
behavior; they are not relabeled durable communication ACKs.

## Final observed verification

All commands below ended with observed exit 0 through Terminal Commander.
Only one full workspace test suite ran at a time.

| Check | Result | TC receipt |
|---|---|---|
| Windows workspace nextest | 1,329 passed; 7 skipped | `job_01a1226d3344729c8be0beb2e76c4e03` |
| Linux documented gate | 1,653 passed; 8 skipped; strict clippy, fmt, load test and MCP guards passed | `job_01a1226f11617056b5113a9460f9ba2e` |
| Windows documented gate | 25 selected regression tests passed; daemon/MCP restart/stdio checks passed | `job_01a1226f117270418fae56518b9dced4` |
| Windows strict workspace clippy | Passed, warnings denied | `job_01a1226d3357768dbe604de430dfabe5` |
| Workspace formatting | Passed; final Linux gate checks latest source too | `job_01a12269f46571a0998af973859a61c4` |
| Workspace doctest command | Passed; current crates contain zero runnable doctest cases | `job_01a12273786b720fad5050dc3d989bbd` |
| Private Linux daemon + MCP smoke | Discovery, health, execution, events and status passed | `job_01a122737887712a984eaf3e5f967849` |
| Embed examples | Both built and ran against final Windows source | `job_01a1227142b473ab8ca9edc4a209a7d9`, `job_01a1227142c974aea4b9f2e4cd2b1cd6` |
| JavaScript wrapper | 524 passed; 25 skipped; zero failures | `job_01a12269f874742fa7aad1cd09483775` |
| cargo-deny | Advisories, bans, licenses and sources passed; duplicate/unused-allowlist warnings retained | `job_01a1226cc44f72658c5f60d67df3dee2` |
| Diff whitespace | `git diff --check` passed | Direct bounded check |

Untouched initial baselines passed 1,258 Windows and 1,575 Linux tests.
New regressions were demonstrated failing before their fixes, including Unix
cleanup ownership, marker preservation, tail limits/loss and the head-only
CR-padding loss flag. Structural guard failures were updated to verify the new
spawn/ownership layout rather than deleting the invariants.

Windows nextest reported 28 retained-stdio/leaky warnings; its untouched baseline
reported 27. The existing stale-discovery test responsible for the observed
additional tail warning passed a focused rerun without a leak warning
(`job_01a1227342317774beb7e93fdf697bc0`). These warnings are preserved as runner
evidence, not represented as failures or as proof that every helper process has
perfect teardown.

Linux verification used the pinned toolchain and a private TC target directory
with debug information/incremental builds disabled after its cache filled the
WSL disk. Only that verified disposable TC build cache was cleaned; no other
project files/caches were removed.

## Limits and ownership

- macOS/other Unix behavior and CPU support were not driven live. Linux owned
  cgroup sampling has parsing/identity tests; this unprivileged host did not
  demonstrate a live TC-owned cgroup.
- Unix hosts must leave TC child reaping to TC. Descendants escaping the owned
  process group are outside that ownership contract.
- The optional Windows ConPTY live-output gate and native owner-prompt UI were
  not driven here. The documented Windows gate explicitly reported its local
  ConPTY opt-in skip; headless structural/validation checks passed.
- The smoke used private daemon/MCP processes. It did not upgrade the installed
  harness adapter or certify an installed-binary release.
- AAP was inspected read-only. Its guest TC port/adoption/security gates remain
  downstream work. SymForge's separate agent confirms receipt of our handoff;
  its full parity/session/remediation and newly reported splice-range regression
  remain under its own verification.
- The historical lost job's causal daemon termination remains unproven.
  [Forensics](EMBED-RECEIPT-FORENSICS.md) reports available timestamps and missing
  receipt evidence without inventing successful execution or replaying it.

## Communication proposal, separately scoped

The recommended [minimum coordinator](../contracts/communication-buckets-mvp.md)
uses the existing typed engine/store infrastructure and explicit user or standing
consent. The [filesystem protocol](../contracts/communication-filesystem-v1.md)
adds a proposed route for peers that share a configured directory but cannot use
networking: immutable exact-size cards, six-record windows, authenticated pairing,
signed sequence/timestamp receipts, bounded negotiated compression and explicit ACK.

The [broader future design](../contracts/communication-buckets-v1.md) records mesh
and council options. None of these communication features is implemented by this
embed repair. Mount/permission/privacy guarantees, codec/signature dependencies,
tokenizer support and two-peer crash/flood tests must pass before advertising them.
