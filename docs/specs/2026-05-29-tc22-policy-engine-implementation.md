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
    adds it.)
I6. NEW: within `developer_local` and `repo_only`, CommandStart whose
    argv[0] basename is not in `commands.allow_roots` is denied
    (`no_allow_rule`), UNLESS allow_roots is empty/unset, in which case
    behavior degrades to the documented default (see open question Q1).
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
AC3. New oracle test: `developer_local` denies an off-allow-list command
     (e.g. `rm`) with reason `no_allow_rule`, and allows `cargo`. (covers I6)
AC4. New oracle test: a profile loaded from TOML with `[commands]
     allow_roots = [...]` produces an engine that honors that list (proves
     section-4 schema loads end to end). (covers objective 4)
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

Ordering rationale: do the TYPE change and containment (the CRITICAL,
highest-blast-radius item) behind tests first; schema loading second;
override + audit last. Honesty edit to docs lands with Phase 1 so the
false-safety claim dies on the first commit even if later phases slip.

**Phase 0 (optional, separate commit) — dead-weight + honesty.**
Files: `policy.rs` (doc-comment only), `POLICY.md` (status note only).
No behavior change. Flip policy.rs:8 to PARTIAL + warn repo_only does not
yet confine. This is handover direction B folded in as a safety floor, so
even if A stalls the lie is gone. Verify: `cargo check`.

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
Implement I6 + default-deny. Add AC3/AC4. Verify: full command.

**Phase 3 — allow_override + startup audit.**
Files: `config.rs`, `policy.rs`, `state.rs` (audit emit), example TOML,
tests. Implement I7. Add AC5. Verify: full command.

**Phase 4 — contract + doc finalization.**
Files: `policy-decision.v1.json` (drop informative-until-TC22), `POLICY.md`
(status -> implemented), `policy.rs:8` (final wording). Verify: full
command + the contract fixture test.

---

## open questions for owner (answer before Phase 2)

Q1. `developer_local` with NO `commands.allow_roots` configured (e.g. the
    current minimal config that has no `[commands]` block at all): should
    the engine
    (a) deny all commands (strict default-deny; safest but breaks every
        existing zero-config user instantly), or
    (b) fall back to the seed allow-list from TC14 (cargo/npm/pytest/make/
        ls/git per POLICY.md section 4 example), or
    (c) keep today's allow-any-except-deny-set until an allow_roots is
        explicitly set (most backward-compatible; weakest)?
    This is a security-posture call, not an implementation detail.

Q2. Containment primitive for I5: POLICY.md section 1 names cap-std `Dir`,
    but cap-std is NOT a current dependency (verified: zero matches). Use
    (a) add the `cap-std` dep now (matches doctrine, +deps), or
    (b) `Path::canonicalize` + prefix check for MVP (no new dep, but
        TOCTOU/symlink caveats to document)?

Q3. Where does `repo_root` come from at daemon start? `state.rs:160` only
    has the profile enum. Options: a new `[policy] repo_root = "..."` TOML
    key, or derive from `daemon.data_dir`'s git ancestor, or require it
    only when `profile = repo_only`. Owner preference?

Q4. Scope confirm: is the 4-phase sequence acceptable, or should Phase 0
    (honesty) ship NOW as its own PR while the rest is specced/reviewed?
