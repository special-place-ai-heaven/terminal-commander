# TC embed parity: implementation and verification

As of 2026-10-10. Local branch `feat/embed-parity`, based on latest pulled
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

The [SymForge output-tail incident audit](OUTPUT-TAIL-RECEIPT-TRUTH-AUDIT.md)
records the original job's incorrect installed-daemon flags, root cause,
committed repair and a focused live regression rerun.

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

## Integration CI follow-up

PR #266's initial Linux run on `176797733198e2566850a7b650633bea1b683960`
failed `fixtures_match_live_tool_responses`: the live command had valid cleanup
state `uncertain` with a `detail` object, while the success fixture illustrates
`complete` without `detail`. The recursive key-set comparison incorrectly
required the same tagged-enum variant. Windows's remote pre-build gate passed;
the dependent build/pack jobs were skipped after the Linux failure.

The follow-up checks `process_cleanup` against the actual `ProcessCleanup`
wire type and its canonical serialized shape. Every defined variant is
accepted; a missing required variant detail or an unmodeled field is rejected.
The reported cleanup uncertainty is preserved. No production behavior or
fixture payload was changed to manufacture a complete observation.

Three deterministic regression cases failed before the correction
(`job_01a122962c56746ea4063fba0acfb47b`). Afterward, all four native fixture
tests, including the original live check, passed
(`job_01a1229836e17067890325d31e3f7c59`); targeted strict clippy also passed
(`job_01a12299a8137511a22d6249db1378b0`). The complete Linux gate then passed
1,656 tests / 8 skipped, strict clippy/fmt, load and MCP guards
(`job_01a1229a557073788b4556dd14dd04cb`). The Windows gate also passed its
25 selected tests and daemon/MCP checks
(`job_01a1229dd299764082d3210388e4b246`); its local ConPTY opt-in check was
skipped. Fresh remote checks are required before treating the follow-up as
ready to release.

### Cancellation proof follow-up

The next Linux CI run (`37995493981`, head `67b0bd8`) passed the original live
fixture check, then failed `stopped_command_eventually_reports_completed_cleanup`
after 3.329 seconds. Windows's remote pre-build gate passed. The failed assertion
did not include the observed cleanup value, so the log alone cannot establish
which cleanup state was returned.

Investigation found a dropped verification retry: Linux's bounded `/proc` scan
can return `TimedOut` after 20 ms, but `wait_stopped` propagated the first error
immediately despite its existing 100 ms proof window. The correction retries
only timed-out observations within that same window and its existing 5 ms
interval. Exhausted timeouts and other errors remain uncertain; only an observed
successful proof can publish complete cleanup. No command or process-group
signal is replayed, and the live test still requires complete cleanup within
its original deadline.

The transient-timeout regression failed before the production correction
(`job_01a122b44ee472d0919b4e5d1fdb7406`, exit 100). Three final proof-loop
tests pass with Tokio's paused clock, including persistent timeout exhaustion
and immediate non-timeout error propagation
(`job_01a122b781e97079baf54d1f0b0efc2b`). The probes crate enables the existing
Tokio package's `test-util` feature only for development; no crate/version or
lockfile change is introduced. The unchanged live cancelled-command test also
passes (`job_01a122b7c2c87195b451a8d45186f128`). Its pre-fix ten local runs
all passed; the deterministic proof-loop regression, rather than those live
passes, establishes the dropped-retry defect.

The full Linux gate passed 1,659 tests / 8 skipped, strict clippy/fmt, load and
MCP guards (`job_01a122bae2dc731a83777331d10ccfa1`). Dependency policy also
passed (`job_01a122b7eca77700b02084b9d682716f`); the existing duplicate-crate
warnings remain. The Windows gate passed its 25 selected tests and daemon/MCP
checks (`job_01a122bdb2c37631a6ff4e5b26468a8c`); the local ConPTY opt-in check
was skipped. The subsequent remote run is recorded below.

### Cgroup CPU comparison follow-up

Remote run `37998515445` on `782589f` passed both OS gates, all five platform
builds, install smoke and npm packaging. Linux passed all 1,659 tests / 8 skipped,
including the original fixture and cancellation regressions. Windows also drove
the opt-in live ConPTY cases successfully. The separate delegated-cgroup job
failed `disabled_governor_is_byte_identical_to_ungoverned`: its normalized
responses differed only in the typed CPU observation. The disabled job used
`linux_process_group` with `unknown: job_exited`; the governed job used its owned
`linux_cgroup` with `unavailable: query_failed` after cgroup teardown.

The test now checks that each CPU sample and process identity belong to the
reported job. Only when Linux actually selects cgroup governance does it assert
the two distinct accounting sources and align CPU observations for the remaining
byte comparison. Other response fields retain strict equality; other platforms
and governance modes retain the original CPU comparison. Terminal CPU states are
not hardcoded: the contract permits typed known, unknown and unavailable evidence.
Production CPU accounting, teardown and cleanup behavior are unchanged.

The focused local regression passed (`job_01a122ce1e6570e3b70d90ed6f42071d`).
The final Linux gate passed 1,659 tests / 8 skipped, strict clippy/fmt, load and
MCP guards (`job_01a122d3623172f7b20555c04d880ea4`, observed exit 0). The
sequential Windows gate passed its 25 selected tests and daemon/MCP checks
(`job_01a122d60edb7039bcc36f8dd18ead1f`, observed exit 0); local ConPTY was
explicitly skipped. This WSL host selects rlimit rather than an owned cgroup,
so delegated-cgroup CI is still required at publication to establish the
correction in its affected environment.

### Release verification recurrence

Source integration run `38000887605` on `9fadd0c` passed the entire workflow:
Linux 1,659 tests / 8 skipped, Windows including live ConPTY, delegated-cgroup
28 tests / 1 skipped plus its accounting marker, all five platform builds,
install smoke and npm packaging. PR #266 merged normally at `c09fbb1`.
Release Please fired, attributed the Rust changes through its normal package
sentinel and created release PR #267 for v0.3.15. Its sync workflow brought all
npm and Cargo versions into agreement and enabled automatic merge after checks.

Release CI `38002203274` on synchronized head `aba2ad1` then reproduced
`stopped_command_eventually_reports_completed_cleanup` after 3.111 seconds:
1,419 passed, one failed, 8 skipped and 239 tests not run. The earlier bounded
scan retry repaired a demonstrated defect but did not fully resolve this live
recurrence. The assertion still lacked the observed cleanup enum, so its log
cannot distinguish failed proof from delayed or missing publication. Release
auto-merge is blocked and v0.3.15 has not been published. Further investigation
must establish that distinction before changing production or retrying CI.

The recurrence was reproduced locally before further production changes:
`job_01a122ed7e507410be99381f0b125278` failed under eight concurrent focused
runners, reporting Cancelled / cleanup Running / streams Reading after the
original three-second assertion window. A subsequent bounded trace
(`job_01a122eec72377b589f2b75207220489`) observed Linux ESRCH during group
inspection, followed by the existing ten-second grace-error wait; another case
reported terminal uncertain cleanup with the same raw error. The original scan
ignored only ENOENT when an enumerated process disappeared. A deterministic
non-anchor ESRCH regression failed (`job_01a122f0968375fd9b5deecb51c048bc`)
and passed after a narrow guard (`job_01a122f11bb773b4bfcb0b02b376ac0f`).
Temporary PID tracing confirmed a vanished member differed from the retained
ownership anchor. The next focused load run still produced uncertain cleanup
without a raw OS error (`job_01a122f16caa76d48d80ed7d187c234c`), so neither
that guard nor a partial passing test is sufficient evidence of completion.

The second failure was isolated to parsing Linux `/proc/<pid>/stat` field 5
as unsigned. A four-runner trace (`job_01a122f4451c76478318e2705708ff7a`)
observed a distinct, departing PID with process group `-1`; the parse error again
entered the ten-second grace-error wait. Linux man-pages documents that field
as signed `%d` ([proc_pid_stat(5)](https://www.man7.org/linux/man-pages/man5/proc_pid_stat.5.html));
the runtime trace establishes the negative value for this reproduction.

The repair skips ESRCH only for an enumerated non-anchor PID and parses the
process group as `i32`. Permission errors, malformed data and incomplete scans
still fail closed. It also checks that the retained anchor's group matches the
owned group before marking the anchor seen; a mismatched or absent anchor cannot
prove complete cleanup. The 20-ms scan bound, 100-ms final proof budget,
ten-second graceful-stop budget and original three-second test deadline are
unchanged. The permanent test assertion now records the observed state, cleanup,
process observation and identity when it fails. Temporary tracing and repetition
harness changes were removed.

With both fixes and the anchor group guard, the previously failing four-runner,
2,000-immediate-stop workload passed (`job_01a122f5aebc76129e6bf70dcb618dcd`).
Six focused probe regressions, the embedded cleanup test and formatting passed
on the final source diff. This local result does not establish release CI success;
full OS gates and fresh remote verification are still required.

Final focused receipts: six probe tests passed in
`job_01a122f6a6797163aeec26a6acc0ba2e`; embedded cleanup passed in
`job_01a122f6cfe17777996dd89a28908e8a`. The signed-pgrp unit was added
following the failing live trace, so it has no pre-fix unit RED receipt.
Independent read-only review found no defect in the final proof guards and
identified helper-level race tests versus the separate live reproduction.
The first Linux gate stopped at strict Clippy's `manual_let_else` check
(`job_01a122f80a4c72a0b2a5a0a2c7ce20d8`); its mechanical correction passed
formatting (`job_01a122f908b570d0aefa737edac082a4`).

The final Linux gate passed (`job_01a122f9493c703e884ae23f69b911bd`, exit 0):
1,661 tests / 8 skipped, strict Clippy and formatting, eight live load checks
and both MCP boundaries. Windows verification runs sequentially afterward.
The installed daemon remains v0.3.14; these receipts cover the current source,
not an installed-daemon replacement.

The sequential Windows gate passed
(`job_01a122fc0168747d929beb9779a9c0d1`, exit 0): 25 selected regressions
and daemon/MCP checks. Local ConPTY was explicitly skipped; remote CI must
exercise it. The full-suite lane was released to SymForge through the supported
queue. Its appended connectivity checkpoint is preserved in this commit;
SymForge's parity work and draft integration contract remain separately owned
and are not certified by these TC checks.
