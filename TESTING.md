# Testing Methodology - Terminal Commander

Status: Baseline (TC03 wave 0 deliverable).
Scope: methodology + fixture taxonomy. Test code itself lands in
later goals (TC05 golden fixtures, TC06+ unit tests, TC15+
integration). Written for the 2026-05 goal chain; the "MUST" wording
below is that era's process language, and only what the gate scripts
and CI enforce is checked automatically.

Language: ASCII only.

## 1. Test category map

Each implementation goal MUST place its tests in one of these
categories. Goals MUST state in their final report which categories
they added or modified.

| Category | Where | Runner | Required for |
|---|---|---|---|
| Unit | inside the crate, `#[cfg(test)] mod tests` | `cargo nextest run` | Every behavior-bearing function/struct |
| Doctest | `///` rustdoc examples | `cargo test --doc` | Every public type/trait with non-trivial usage |
| Integration | per-crate `tests/*.rs` | `cargo nextest run` | Crate-boundary contracts |
| Fixture | `tests/fixtures/<category>/...` | exercised by unit + integration | Every parser, sifter, schema, golden output |
| Snapshot | `cargo insta test` | `insta` (decided, NOT adopted: no manifest depends on it as of 2026-10-05) | Output that is easier to compare by snapshot than by hand |
| Load | `crates/core/tests/load.rs` | `cargo nextest run -p terminal-commander-core --test load` | TC11, TC17, TC28 backpressure proof |
| Security | `crates/daemon/tests/security.rs` | `cargo nextest run -p terminal-commanderd --test security` | TC22, TC29 (policy / fuzz-like) |
| End-to-end | `crates/mcp/tests/e2e.rs` | `cargo nextest run -p terminal-commander-mcp --test e2e` | TC27, TC30 demo scenarios |

Locked dev-time dependencies (deferred to TC04 Cargo manifests):

| Concern | Crate | Decision |
|---|---|---|
| ANSI parsing in PTY corpus | `vte` (adopted: `crates/probes`, 0.15) | TC03 (2026-05-21 operator decision) |
| Snapshot framework | `insta` (1.x) with `cargo-insta` CLI (not adopted) | TC03 (2026-05-21 operator decision) |
| JSON-Schema validation | `jsonschema` (0.x latest) (not adopted) | TC03 (2026-05-21 operator decision) |

## 2. What CI runs (and what it does not)

`.github/workflows/npm-binary-build.yml` runs two pre-build gates on every
pull request and push to `main`; the release pipeline needs them green.
Both first run `npm test` in `packages/terminal-commander` (the wrapper
unit tests, from a clean checkout), then:

`pre-build-gates (linux-x64)` first runs `cargo deny check` (advisories,
bans, licenses, sources; policy in `deny.toml`), then `bash
scripts/linux-gate.sh`, which:

1. checks the toolchain: `cargo`, `node`, `cargo-nextest`, `python3`
   present; `rustc` equals `rust-toolchain.toml` channel (1.97.1) and the
   workflow `RUST_TOOLCHAIN` pin;
2. `node scripts/release/verify-optional-dependencies.js` and
   `bash scripts/release/test-synthesize-crates-release-trigger.sh`;
3. `cargo fmt --all --check`;
4. `cargo clippy --workspace --all-targets -- -D warnings`;
5. `cargo nextest run --workspace` (profile `default`, `.config/nextest.toml`);
6. the TC47 load gate: `cargo test -p terminal-commanderd --test
   load_noise_backpressure -- --nocapture`, failing if it self-skips
   (no python3) or runs 0 tests;
7. two grep guards: no process/socket spawn and no direct filesystem
   access in `crates/mcp/src`.

`pre-build-gates (windows-x64)` runs `pwsh scripts/windows-gate.ps1`,
which builds `terminal-commanderd` and runs a fixed list of Windows
regression test binaries (`windows_no_console_spawn`,
`probe_handshake_windows`, `windows_spawn_site_coverage`,
`shell_deny_windows`, `pty_windows` subset, the MCP daemon-loss and
first-request tests), refusing a 0-tests-run pass, plus the live ConPTY
e2e (`TC_CONPTY_E2E=1`, set automatically on GitHub Actions). It does not
run fmt, clippy or the full nextest suite.

Commands for the same checks by hand:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

`install-path-linux` (after `pre-build-gates (linux-x64)`) builds the
three debug binaries and runs `bash scripts/ci/install-path-check.sh
target/debug`. In a throwaway HOME, with no `TC_*`, `XDG_*` or CI
variables and a fake failing `systemctl`, it packs the wrapper and the
linux-x64 platform package, runs `npm install -g` into `~/.npm-global`
(postinstall included), runs `terminal-commander setup daemon-autostart`,
and checks the profile-hook autostart: no daemon from `bash -l -c` or
`bash -c`, exactly one from a fast-closing interactive shell (`script`),
recovery after `kill -9` with the stale socket reported by `doctor daemon`,
the installed MCP adapter (`health` version, `TC_DAEMON_CHILD=1` and no
`TC_SOCKET` / `TC_DATA` in a child's env), and a clean `session reap
--all`. It needs `node` on the system PATH, `npm`, `python3` and
util-linux `script`; run the same command by hand on Linux or WSL.

### Not enforced by CI today

These tools are recommended, not wired into any workflow or gate script.
Run them by hand when relevant.

```bash
# Compile matrix: each individual feature on/off.
cargo hack check --workspace --each-feature --no-dev-deps

# MSRV gate: compile with the declared rust-version (1.92). The MSRV is a
# documented floor, not an automated check.
cargo hack check --workspace --rust-version

# Doctests (the gate runs nextest, which does not run doctests).
cargo test --workspace --doc

# Unused-dependency audit.
cargo machete --with-metadata

# clippy with every feature enabled (the gate omits --all-features).
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The pre-commit subset (see `CONTRIBUTING.md` section 5) is fmt, clippy
and nextest, the same hot subset the linux gate runs.

## 3. Verification command per goal class

Every goal's `verification_command` block MUST include the right set
for its class. A goal that omits an applicable check is out of
scope-compliance.

### 3.1 Docs-only goal (e.g. TC02, TC03)

```bash
git diff --check
# plus the goal-specific test -f / test -d checks
```

No Rust toolchain commands. No fixture validation. Verification
proves files exist and contain the right invariants.

### 3.2 Schemas / golden fixtures goal (TC05)

```bash
git diff --check
test -d tests/fixtures/contracts/
for f in tests/fixtures/contracts/*.json; do python3 -m json.tool "$f" > /dev/null; done
# After TC05 lands: cargo nextest run -p terminal-commander-core --tests
```

`python3 -m json.tool` is the dev-time check that fixtures parse
without a Rust compile. `python3` is therefore a project dev
prerequisite, per `CONTRIBUTING.md` and TC03 contracts.

### 3.3 Rust-code goal (TC04 onward)

The default verification is the pre-commit subset:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --profile default --no-fail-fast
```

Goal mini-specs MAY add narrower commands (e.g. `cargo nextest run
-p terminal-commander-store`) to scope verification to the active
crate, but the workspace-wide pre-commit subset MUST pass before
the goal commits.

### 3.4 MCP-tool-surface goal (TC23, TC24)

Pre-commit subset PLUS:

```bash
cargo nextest run -p terminal-commander-mcp --tests
# Each new MCP tool must have at least one integration test that
# exercises it through the daemon IPC end (not just a unit test
# of the MCP-side handler).
```

### 3.5 Daemon goal (TC15-TC22)

Pre-commit subset PLUS:

```bash
cargo nextest run -p terminal-commanderd --tests
# Probe / job / bucket goals also run their integration fixture
# from tests/fixtures/<category>/ end-to-end.
```

### 3.6 Probe goal (TC15, TC18, TC19, TC20)

Pre-commit subset PLUS:

```bash
cargo nextest run -p terminal-commander-probes --tests
# File-probe goals MUST add at least one PollWatcher test (WSL /mnt/c
# emulation via fixture mountinfo) — see fixtures/probes/wsl-mountinfo/.
```

### 3.7 Policy goal (TC22, TC29)

Pre-commit subset PLUS:

```bash
cargo nextest run -p terminal-commanderd --test security
# Includes: default-deny coverage, sudo-block, regex-safety, audit
# emission ordering, profile-version validation.
```

### 3.8 Integration / E2E goal (TC27, TC30)

Pre-commit subset PLUS:

```bash
cargo nextest run -p terminal-commander-mcp --test e2e
# E2E tests may run longer (up to ~120s); not in the pre-commit
# subset.
```

## 4. Source-status discipline

Every test MUST report one of these source-status labels for the
behavior it exercises (in test name, docstring, or report):

| Label | Meaning |
|---|---|
| `live` | Behavior is production-bound and fully wired. |
| `partial` | Some code paths covered, others not yet. Lists which. |
| `degraded` | Behavior compiles and runs but a downstream dependency is mocked or stubbed; lists the seam. |
| `disabled` | Behavior intentionally turned off (feature flag, `cfg`); test only verifies it stays disabled. |
| `test-only` | Behavior exists only in `#[cfg(test)]` or fixtures; not reachable from production. |
| `mock` | Behavior is a test double; production has no implementation yet. |
| `blocked` | Test exists but cannot run due to an external blocker (kernel, fixture, transport); the blocker is named. |
| `unknown` | Behavior was touched but source-status was not assessed; FORBIDDEN as a commit-time label. Resolve before commit. |

`unknown` is a hard fail in pre-commit. The other labels are
acceptable in the right context but MUST appear in the goal report.

The "did it compile" check is NEVER a verification of behavior.
This rule is mirrored from `CONTRIBUTING.md` section 9.

## 5. No-mock invariant for production paths

Per `CONTRIBUTING.md` section 9 and `SECURITY.md` section 9.5:

- Test-only helpers stay isolated to `tests/`, `fixtures/`, or
  modules gated by `#[cfg(test)]`.
- Production code paths MUST NOT reach into test-only helpers.
- A test that "passes" by calling test-only logic in a production
  configuration is a verification failure, not a success.

## 6. Fixture rules

Fixtures live under `tests/fixtures/<category>/<descriptor>.<ext>`
where the category is one of the directories listed in section 8.

Rules:

1. **Deterministic.** A fixture MUST produce the same bytes every
   time. No timestamps embedded inline; use placeholder tokens
   (e.g. `<TS>`) that tests replace at compare time.
2. **Small and safe to commit.** Hard cap: 256 lines OR 16 KiB
   total, whichever comes first. Larger inputs MUST be trimmed,
   summarized, or generated at test-time by a documented script
   under `scripts/dev/`.
3. **No secrets.** No real credentials, real private paths, real
   hostnames, or real tokens. Use synthetic identifiers
   (`api_key_PLACEHOLDER`, `/home/dev/repo`, `host-a`, etc.).
4. **No raw-stream dumps as success path.** Bucket / event fixtures
   carry STRUCTURED events with summaries and pointers, not raw
   stream copies (`SECURITY.md` section 3 B5).
5. **One concern per fixture.** A fixture that exercises both a
   regex match and a dedupe is two fixtures.
6. **Filename names the case.** `apt-missing-package.stderr`
   beats `case_07.txt`.
7. **License/attribution headers** are not needed for ASCII text
   data; they ARE needed for any third-party fixture material
   (cited in `tests/README.md`).
8. **Mountinfo fixtures** (`tests/fixtures/probes/wsl-mountinfo/`)
   are a special category: they capture `/proc/self/mountinfo`
   excerpts (Linux native, WSL2 9P drvfs, WSL2 ext4) for the
   WSL-aware file-probe detection logic (TC18/TC20/TC25). They
   contain real format but synthetic uids/paths.

## 7. ReDoS and runaway-test budget

Per `POLICY.md` section 2.1 and `RISK_REGISTER.md`:

- Every regex used in a fixture / test MUST compile in under 50 ms
  on a developer machine.
- Every regex execution against a fixture MUST complete in under
  10 ms.
- Tests that exercise large input (load / security tests) run under
  the default profile's timeout (`.config/nextest.toml`: SLOW at 60 s, killed
  at 5 min); no separate load/security/e2e nextest profiles exist.

Regex test cases in `tests/fixtures/sifters/regex/` MUST include
both "safe" and "expected-rejected" examples (TC10/TC29 will
verify rejection of the latter without hanging the runner).

## 8. Fixture taxonomy (directories)

The TC03 baseline lays out:

```text
tests/
  README.md
  fixtures/
    terminal/                 # raw terminal stream excerpts (stdout/stderr/mixed)
      apt-missing-package.stderr
      cargo-compile-error.stderr
      npm-install-failure.stderr
      pytest-collection-error.stderr
      gcc-missing-header.stderr
      generic-error.stderr
      repeated-warning.stderr
    command-output/           # full command runs (small)
      cargo-test-pass.stdout
      cargo-test-fail.stdout
    files/                    # tailable file excerpts
      app-log-rotating.before
      app-log-rotating.after
    buckets/                  # structured bucket event examples (JSON)
      build_42.events.json
    rules/                    # registry rule JSON (input shapes)
      apt-missing-package.json
      cargo-compile-error.json
    context/                  # event_context shape examples
      around_event_2138.json
    policy/                   # policy decision fixtures
      developer_local-allow.json
      read_only_observer-deny.json
    probes/
      wsl-mountinfo/
        native-linux.mountinfo
        wsl2-9p-drvfs.mountinfo
        wsl2-ext4.mountinfo
    contracts/                # added after TC03 (TC05): JSON contract fixtures
```

Goals add to these directories within their `allowed_files_or_area`
set. Adding a new top-level fixture category requires amending this
section in a TC03-class goal.

## 9. Scripts

`scripts/dev/verify-baseline.sh` runs a minimal, no-toolchain check
suitable for early goals (before TC04 lands the workspace). It is a
TC03-era script: its branch guard fails on any branch other than
`feature/terminal-commander-mvp`, so it is not a general pre-push
check (use `scripts/linux-gate.sh` / `scripts/windows-gate.ps1`). It:

- proves the branch is `feature/terminal-commander-mvp`;
- confirms the required doctrine files exist (`README.md`, `LICENSE`,
  `NOTICE`, `SECURITY.md`, `POLICY.md`,
  `docs/security/PRIVILEGE_MODEL.md`, `CONTRIBUTING.md`, `SPEC.md`,
  `ARCHITECTURE.md`, `ROADMAP.md`, `TESTING.md`);
- validates every JSON file under `tests/fixtures/` parses;
- prints a SOURCE-STATUS table for each fixture category.

It is bash. It runs on WSL2 with no systemd, no network egress, and
no Windows-specific tooling. `python3` is the only non-bash
prerequisite (used for `python3 -m json.tool`).

## 10. Evidence rules (every goal)

Every goal's final report MUST include, at minimum:

- `git branch --show-current` output (exactly the target branch).
- Files changed (paths).
- Verification command output summary (PASS/FAIL per command).
- Source-status notes for every behavior touched (section 4).
- Risk register row reference if the goal mitigates a row.

A report that omits source-status notes is incomplete and the goal
is not Completed.

## 11. Out of scope (MVP)

- Mutation testing (`cargo-mutants`).
- Coverage gating (`cargo-llvm-cov`). Coverage may be reported but
  is not a CI gate.
- Property-based testing as a default (`proptest`/`quickcheck`).
  Individual goals MAY add it where it pays off; not mandatory.
- Fuzz harness (`cargo-fuzz`) as a CI step. TC29's "fuzz-like"
  scope means structured fault injection through fixtures, not
  libFuzzer-driven fuzzing.

Each of these MAY be revisited at TC32 (evidence review and backlog
refinement).
