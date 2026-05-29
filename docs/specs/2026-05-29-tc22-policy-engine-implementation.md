# Mini-spec: TC22 policy engine — bring code up to POLICY.md doctrine

Status: DRAFT, awaiting owner approval. No code written.
Date: 2026-05-29.
Author: Claude (dispatched from HANDOVER_TC22_POLICY_GAP.md, direction A).
Supersedes the false "live (TC22)" claim at `crates/daemon/src/policy.rs:8`.

This spec is the gate required by the handover's section 8.5 and by the
global "no production implementation without a spec" rule. It is written
AFTER verifying the gap against source (policy.rs + POLICY.md + config.rs
+ all 7 call sites + the two oracle test files, read in full 2026-05-29).

---

## objective

Make `crates/daemon/src/policy.rs` enforce what `POLICY.md` says TC22 MUST
do, so that the four named profiles behave as documented. Specifically:

1. Default-deny posture (POLICY.md section 6 step 2e): within a profile, an
   action with no matching allow rule is denied (`no_allow_rule`), not
   allowed.
2. `repo_only` actually confines reads/watches/exec to `$REPO_ROOT`
   (POLICY.md section 2.2). This is the CRITICAL fix: today `repo_only`
   is byte-identical to `developer_local` (`policy.rs:181` shared arm).
3. `developer_local` enforces a command allow-list (`commands.allow_roots`)
   instead of allow-any-except-deny-set (POLICY.md section 2.1).
4. The declarative `[profile]`/`[paths]`/`[commands]`/`[probes]`/`[limits]`/
   `[registry]` schema (POLICY.md section 4) loads from config.
5. The `allow_override` mechanism (POLICY.md section 5) exists, with the
   mandatory startup audit record.
6. Limits are checked (POLICY.md section 6 step 3) where the engine owns
   them (`max_jobs`); rate/size limits that live in other subsystems are
   documented as out-of-engine, not silently dropped.

## non_goals

- Kernel enforcement (Landlock, seccomp-bpf). POLICY.md section 1 marks it
  roadmap, not MVP.
- Content-level secret redaction (POLICY.md section 7).
- Outbound network egress policy (POLICY.md section 7; TC has no egress).
- Runtime profile switching (POLICY.md section 3: restart boundary only).
- Adopting the Cedar crate. Handover section 7 verdict: do NOT pull a
  policy-engine dependency to replace a match statement. Hand-rolled stays
  unless/until the policy set must become operator-authored at runtime,
  which is beyond this spec.
- The ReDoS / regex-step limits (POLICY.md section 2.1). Handover section 4
  NOTE: these live in the SIFTER layer (`crates/sifters`), not the policy
  engine. The engine has zero regex. Out of scope here; tracked separately.
- Changing the `PolicyDecision` closed set (Allow/Deny/AllowWithAudit/Error).
  `docs/contracts/enums/policy-decision.md` says amending it requires a
  POLICY.md section 6 change first. We will not touch it.

## allowed_files_or_area

Phase-gated (see "plan"). The full file set across all phases:

- `crates/daemon/src/policy.rs`        — engine logic + types
- `crates/daemon/src/config.rs`        — profile schema structs + load/clamp
- `crates/daemon/src/state.rs`         — thread repo_root into engine ctor
- `crates/daemon/src/command.rs`       — `policy` field type follows engine
- `crates/daemon/src/pty_command.rs`   — same
- `crates/daemon/src/file_watch.rs`    — same
- `crates/daemon/src/runtime.rs`       — same
- `crates/daemon/src/ipc/handlers/common.rs` — same (path action mapping)
- `crates/mcp/src/lib.rs`              — `policy` field type follows engine
- `crates/daemon/tests/security.rs`    — ADD branch tests (do not weaken)
- `crates/daemon/src/policy.rs` `#[cfg(test)]` — ADD branch tests
- `config/terminal-commanderd.example.toml` — document new sections
- `POLICY.md`                          — flip section status to "implemented"
- `tests/fixtures/contracts/policy-decision.v1.json` — drop
  "informative-until-TC22" once conformant (last step only)

## forbidden_files

- Anything under `crates/sifters/**` (ReDoS lives there; out of scope).
- `docs/contracts/enums/policy-decision.md` (closed set; do not edit).
- Any release/CI/packaging file. This is engine work, not a release.

## contracts_or_interfaces

INVARIANT — the public decision seam stays stable:

```rust
PolicyEngine::evaluate(&self, action: &PolicyAction<'_>) -> PolicyVerdict
```

- `PolicyAction` variants MAY gain fields (e.g. CommandStart already has
  `cwd`; we will USE it). Adding a variant requires updating all 7 call
  sites in the same phase.
- `PolicyVerdict { decision, reason }` shape unchanged. `reason` strings
  SHOULD adopt the POLICY.md section 6 reason codes (`no_allow_rule`,
  `default_deny_match`, `command_denied`, `registry_activate_requires_admin`,
  `limit_exceeded`, `policy_invalid`) so audit records are greppable.
- The 7 call sites keep their `if verdict.decision == Deny { ... }` shape.

BREAKING-BY-NECESSITY — `PolicyEngine` loses `#[derive(Copy)]`:
- It will hold loaded profile data (allow-lists, repo_root). It becomes
  `Clone` only. Every `policy: PolicyEngine` field (command.rs:314,
  pty_command.rs:143, file_watch.rs:149, mcp/lib.rs:65, state.rs:84) and
  every `PolicyEngine` passed by value must be audited for moves vs clones.
  This is the dominant blast radius and is why the work is phased. The
  handover undersold this ("confine all new logic inside evaluate()" is
  true for LOGIC but not for the TYPE).

## invariants

I1. sudo/doas/su/pkexec/kexec/polkit denied in EVERY profile, by basename
    AND by absolute path. (Today: `policy.rs:119`, `security.rs:13,34`.)
    MUST still hold. Oracle: `structural_deny_sudo_all_profiles`,
    `fully_qualified_sudo_path_also_denied`.
I2. The 14 default-deny path suffixes denied for FileRead+FileWatch in
    every profile. Oracle: `sensitive_path_default_deny_paths_all_variants`.
I3. `read_only_observer` denies all command_* and registry_* mutations.
    Oracle: `read_only_observer_denies_every_mutation`.
I4. `admin_debug` denies registry mutations. Oracle:
    `admin_debug_denies_registry_mutations`.
I5. NEW: `repo_only` denies FileRead/FileWatch/CommandStart whose path/cwd
    is outside the canonicalized `$REPO_ROOT`. (No oracle yet; this spec
    adds it.) MVP enforcement is `std::fs::canonicalize` + `starts_with(
    repo_root)` IN `policy.rs` (the DECISION). The acting cap-std `Dir`
    sandbox is OUT OF SCOPE and deferred to `crates/probes` / `crates/store`
    (roadmap), since those crates own the actual fs opens (`ProcessProbe::
    spawn` does `Command::new().current_dir(cwd)` at probes/process.rs:189-
    196; policy.rs never opens an fd). Document the residual TOCTOU /
    symlink-escape gap between decide and act in the threat note; closing it
    is the deferred act-site cap-std layer, NOT this spec.
I6. NEW: command allow-list, default-deny is OPT-IN (Q1 re-resolved
    2026-05-29 during Phase 2 implementation — supersedes the earlier
    baseline-fallback plan):
    - When `[policy.commands] allow_roots` is NON-EMPTY it is authoritative
      for BOTH `developer_local` and `repo_only`: a CommandStart whose
      argv[0] basename is not listed is denied (`no_allow_rule`). Matched by
      basename, so `/usr/bin/cargo` matches `cargo`.
    - When `allow_roots` is ABSENT/EMPTY, BOTH exec profiles allow any
      command surviving the cross-profile structural deny set. NO
      compiled-in baseline list. Rationale: TC's core job is running and
      combing arbitrary dev commands; a 6-entry baseline would deny
      echo/python/node/go/cat/bash out of the box and break zero-config use.
      Default-deny is therefore an explicit operator opt-in.
    - `repo_only` shares this command posture (POLICY.md 2.2: "the same
      allow-list as developer_local"); its distinct safety property is
      `$REPO_ROOT` containment (I5), NOT command denial. The earlier
      "repo_only empty -> deny-all" idea is dropped as over-tightening.
    - NOTE retained: the TC14 seed packs (`crates/store/rules/*.json`) are
      regex output-combing rules, not command allow-lists.
I7. NEW: `allow_override` entries each require path + justification +
    `i_understand_risk = true`; loading a profile with overrides emits a
    startup audit record naming each path (POLICY.md section 5).
I8. The default profile remains `developer_local` and the daemon still
    boots with a zero-config / example config (no regression to
    `DaemonConfig::defaults_in`).
I9. The existing 6 policy unit tests + 8 security.rs tests pass UNCHANGED.
    Any divergence is a finding to surface, not a test to edit.

## acceptance_criteria

AC1. New oracle test: `repo_only` denies a FileRead outside repo_root and
     allows one inside it. (covers I5)
AC2. New oracle test: `repo_only` denies CommandStart with cwd outside
     repo_root. (covers I5)
AC3. New oracle tests (covers I6): (a) `developer_local` with NO list
     allows any non-structural-deny command (echo/python/rm/cargo); (b)
     `developer_local` WITH `allow_roots=[cargo,git]` denies `rm`
     (`no_allow_rule`) and allows `cargo`; (c) basename match
     (`/usr/local/bin/cargo` matches `cargo`).
AC4. New oracle tests: (a) a profile loaded from TOML with
     `[policy.commands] allow_roots = [...]` produces an engine that honors
     that list (proves section-4 schema loads end to end); (b) `repo_only`
     with a list enforces both containment AND the allow-list. AC10 below
     covers repo_root validation. (covers objective 4)
AC5. New oracle test: `allow_override` for a default-deny path flips it to
     `AllowWithAudit` (not plain Allow), and the override is rejected if
     `i_understand_risk` is false/absent. (covers I7)
AC6. All 14 pre-existing policy/security tests pass with ZERO edits.
     (covers I9)
AC7. `DaemonConfig::from_toml(example_toml)` still parses; daemon
     `bootstrap` still succeeds with the example config. (covers I8)
AC8. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test -p
     terminal-commanderd` all green.
AC9. POLICY.md section status line no longer says "No policy code ...
     exists yet"; policy.rs:8 no longer says bare "live" — it states what
     IS enforced and cites this spec.
AC10. `DaemonConfig::from_toml` with `profile = repo_only` and no
     `repo_root` returns `ConfigError::Validate` (repo_only cannot boot
     without its confinement root). (covers Q3 verdict)

## evidence_required

- Full `cargo test -p terminal-commanderd` output (pass count, the 5 new
  ACs visible by name).
- `cargo clippy` clean output.
- A before/after of `policy.rs` `evaluate()` showing the `repo_only` arm
  split out from `developer_local`.
- The new TOML in the example config + a parse-roundtrip test result.
- Confirmation the 14 legacy tests are byte-unchanged (git diff of the two
  test files shows only ADDITIONS).

## stop_conditions

- If splitting `repo_only` out requires repo_root that is not available at
  `PolicyEngine::new` (state.rs:160) AND cannot be threaded without
  touching >5 files in one phase: STOP, re-phase, surface.
- If `commands.allow_roots` semantics for the empty/unset case is
  ambiguous (Q1) and the owner has not answered: STOP at Phase 2, do not
  guess the security posture.
- If canonicalization for containment opens a TOCTOU or symlink-escape
  question the spec did not budget for: STOP, write a threat note, surface.
- If removing `Copy` cascades into a move/borrow error storm beyond the
  phase's 5-file budget: STOP, commit the type change alone, re-phase.

## verification_command

```
cargo fmt --check
cargo clippy -p terminal-commanderd -- -D warnings
cargo test -p terminal-commanderd
cargo test -p terminal-commander-mcp   # call-site fields still compile
cargo clean   # per CLAUDE.md, at task boundary
```

---

## plan (phased; each phase <=5 files, verify, then approval)

Ordering rationale: ship the honesty edit FIRST as its own commit (the
false "live" claim is already released in v0.1.36, so it must die
immediately and independently of the unresolved type/containment work);
then the TYPE change and containment (the CRITICAL, highest-blast-radius
item) behind tests; schema loading; override + audit last.

**Phase 0 (SHIP FIRST, standalone commit, ahead of Phase 1) — honesty.**
Files: `policy.rs` (doc-comment only), `POLICY.md` (status note only).
No behavior change. Flip policy.rs:8 to PARTIAL + warn repo_only does not
yet confine. The misleading "live (TC22)" label is in users' hands now
(v0.1.36 released to crates.io/npm); a false "this is enforced" security
claim is strictly worse than an honest "partial" one. Has no dependency on
Q1-Q3. Verify: `cargo check`.

**Phase 1 — repo_only containment + Copy->Clone.**
Files: `policy.rs`, `state.rs`, `command.rs`, `pty_command.rs`,
`file_watch.rs` (exactly 5). Add `repo_root: Option<PathBuf>` to the
engine; split the `DeveloperLocal | RepoOnly` arm; add I5 containment;
drop `Copy`; fix the field-move sites. Add AC1/AC2 oracles.
NOTE: `runtime.rs` + `ipc/handlers/common.rs` + `mcp/lib.rs` are also
field-holders — if the borrow fixes spill into them, that is a Phase 1b.
Verify: full verification_command.

**Phase 2 — section-4 schema load + command allow-list (default-deny).**
Files: `config.rs`, `policy.rs`, `config/terminal-commanderd.example.toml`,
`tests/security.rs` (+ policy.rs tests). Add `[paths]`/`[commands]`/
`[probes]` to `PolicySection` (mind the existing `LimitsSection` name
collision — POLICY.md `[limits]` is a DIFFERENT block; rename or nest).
The baseline command allow-list (Q1 verdict) lands as a NEW const in
`policy.rs` (cargo/npm/pytest/make/ls/git from POLICY.md:167), NOT sourced
from TC14 packs. Implement I6 per-profile asymmetry + default-deny. Add
AC3/AC4. No `crates/probes` / `crates/store` files are touched here
(act-site containment is a separate future goal). Verify: full command.

**Phase 3 — allow_override + startup audit.**
Files: `config.rs`, `policy.rs`, `state.rs` (audit emit), example TOML,
tests. Implement I7. Add AC5. Verify: full command.

**Phase 4 — contract + doc finalization.**
Files: `policy-decision.v1.json` (drop informative-until-TC22), `POLICY.md`
(status -> implemented), `policy.rs:8` (final wording). Verify: full
command + the contract fixture test.

---

## resolved decisions (was: open questions; resolved 2026-05-29 via tech-researcher pass, code-verified)

Q1 — `developer_local` with NO `commands.allow_roots`. RESOLVED: hybrid,
    per-profile split (see I6). `developer_local` falls back to a compiled-in
    BASELINE allow-list (NEW const in `policy.rs` from POLICY.md:167 =
    cargo/npm/pytest/make/ls/git); `repo_only` empty -> DENY-ALL. Pre-1.0
    (v0.1.36, Cargo.toml:30) justifies secure-by-default. CORRECTION: the
    original option (b) "seed allow-list from TC14" was a FALSE premise —
    TC14 packs (`crates/store/rules/*.json`) are regex output-combing rules,
    not command allow-lists. The baseline list is authored fresh in
    `policy.rs`.

Q2 — Containment primitive for I5. RESOLVED: (b) `std::fs::canonicalize` +
    `starts_with` prefix check in `policy.rs`. NO cap-std dependency added.
    Rationale: cap-std `Dir` is a held fd handle for fs *opens*;
    `policy.rs::evaluate` is a pure `&Path -> verdict` that never opens an
    fd, so cap-std is the wrong tool there. The acting cap-std sandbox, if
    ever added, belongs at the act sites in `crates/probes` /
    `crates/store` (`ProcessProbe::spawn`, probes/process.rs:189-196) — a
    deferred roadmap goal, not this spec. Keeps `deny.toml`
    `advisories.ignore = []` (deny.toml:38) RUSTSEC burden flat. Residual
    TOCTOU/symlink gap documented under I5.

Q3 — `repo_root` source at daemon start. RESOLVED: (a) new `[policy]
    repo_root = "..."` TOML key, typed `Option<PathBuf>`, validated as
    required ONLY when `profile = repo_only` (in
    `config::validate_and_clamp`, config.rs:290-341; new AC10). Canonicalize
    once at engine construction. REJECTED (b) git-ancestor walk of
    `data_dir`: `data_dir` is `~/.local/share/...` (example.toml:15) and
    config.rs:301-307 rejects `/mnt/c` — it is outside the repo by design,
    so the walk finds nothing or a coincidental `.git`. The daemon has no
    repo concept today; `find_repo_root` is CLI-only / doctor-only
    (cli/main.rs:587) and CWD-relative, unsafe as a daemon security boundary.

Q4 — Phase 0 sequencing. RESOLVED: ship Phase 0 (honesty) FIRST as a
    standalone commit, ahead of Phase 1 (see plan). The false "live" label
    is already released in v0.1.36; it dies now, independent of Q1-Q3.
