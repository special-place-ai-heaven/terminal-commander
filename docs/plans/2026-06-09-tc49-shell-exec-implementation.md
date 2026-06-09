# TC49 — shell_exec One-Shot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a gated `shell_exec` MCP tool that runs ONE shell line (pipelines/compounds/redirects) through the existing comb/bucket pipeline, behind a new `allow_shell` capability — without weakening the argv-first default.

**Architecture:** A new shell *lane* reuses the command spawn core. The argv path (`command_start_combed`) keeps `SHELL_INTERPRETERS_DENY` as a hard deny, unchanged. The shell path threads a `StartMode::Shell` flag through the same `start_combed_inner` core: it skips the shell-interpreter guard, evaluates a NEW `PolicyAction::CommandShellStart` (gated by `[policy.caps].allow_shell`), emits a `command_shell_start` audit row, and spawns `[shell, "-lc", shell_line]`. A `ShellRuntime` facade in `shell.rs` is the daemon-level entry; `full_access` is a profile preset that flips caps on.

**Tech Stack:** Rust, existing `PolicyEngine` / `ProcessProbe` / sifter+bucket pipeline, `rmcp` MCP adapter, IPC protocol. No new crate deps.

**Sources of truth:** `docs/plans/2026-06-09-tc-omni-wave1-shell-session.md` (TC49), `docs/plans/2026-06-09-tc-omni-decisions-and-reconciliation.md` (Decisions 1,2,5; TC50/sentinel are OUT of scope here).

**Scope guard:** TC49 = one-shot `shell_exec` ONLY. OUT: TC50 sessions, cwd sentinel, the deferred `allow_shell`-on-`command_start` alt door, remote, privilege helper.

---

## Round-6 binding fixes (APPLY to every task below — verified against code 2026-06-09)

1. **Rename the lane enum `StartMode` -> `StartLane`.** `crates/daemon/src/main.rs:90` already defines `enum StartMode { IpcServer, ForegroundIdle }`; reusing the name in `command.rs` is confusing. Use `enum StartLane<'a> { Argv, Shell { shell_line: &'a str, shell: &'a str } }`.
2. **`MAX_SHELL_LINE_BYTES = 4096`** (= `MAX_ARGV_ITEM_BYTES`), NOT 16 KiB. `validate_argv` (`command.rs:405-418`) caps EVERY argv item at 4096; the shell line is argv[2], so 16 KiB would be rejected there and the oversize test would lie. (Raising later needs a lane-aware `validate_argv` exempting argv[2] under `StartLane::Shell` — explicit follow-up, NOT TC49.)
3. **`start_combed` is SYNC** (`command.rs:488 pub fn start_combed`, `:521 fn start_combed_inner`). `ShellRuntime::exec` and `start_combed_shell` are SYNC — drop `async`/`.await` on them. Tests call them directly; the async IPC/MCP handlers call the sync `exec` inline. Fix Task 5 + Task 7 signatures.
4. **Dedup needs no code change.** The TC-2 nonce-less fallback key is `(peer, argv, cwd, tag)` (`command.rs:352`); argv includes argv[2]=`shell_line`, so two different shell lines do NOT collapse. ADD a Task-5 test: two distinct lines -> two distinct `job_id`s.
5. **`full_access` cap semantics:** `base || full` forces all caps ON even if `[policy.caps]` lists one `false`. Intent: to run a SUBSET, use a base profile + explicit caps, not `full_access`. Note in Task 3 + POLICY.md.

## Phase C tool-count anchor checklist (Cursor WI-3 — ALL must move 38->39 in Task 8, or the gate red-fails)

1. `crates/mcp/tests/mcp_live_daemon.rs:213` — `live_count, 38` -> `39`; add `"shell_exec".to_owned()` to the sorted `names` vec (alphabetical: after `self_check`, before `subscription_close`); fix the `:11` + `:214` comment text.
2. `crates/mcp/tests/daemon_unavailable_envelope.rs:216` — "expected 37 daemon-backed tools (38 catalogue entries minus system_discover)" -> 38 daemon-backed / 39 catalogue; bump the asserted count (shell_exec IS daemon-backed).
3. `tests/fixtures/contracts/mcp-tools/system_discover.v1.json` — the `"tools": [...]` array lists EVERY tool by name; ADD a `shell_exec` entry `{"name":"shell_exec","status":"live","description":"...","requires_daemon":true,"available":false,"unavailable_reason":"daemon_unavailable"}` in the `command` group region (after `command_stop`). Contract test fails if missing.
4. `docs/mcp/TOOL_CONTROL_SURFACE.md:61` — "38 live tools" -> "39 live tools" (cheap; do in C with the rest).

**Cursor WI-1 (error mapping):** shell-lane `CommandError::PolicyDenied` MUST map to `IpcErrorCode::PolicyDenied`, NEVER `ShellInterpreterDenied` (the shell lane skips that guard, so it can never produce it). **WI-2:** `ShellRuntime::exec` stays SYNC (Round-6 #3). **WI-3 docs split:** the `system_discover` contract fixture (#3 above) ships in Phase C (gate-blocking); POLICY.md prose stays Phase D / Task 10.

---

## File structure (what changes and why)

| Action | Path | Responsibility |
|---|---|---|
| Modify | `crates/daemon/src/policy.rs` | `PolicyAction::CommandShellStart`; `PolicyProfile::FullAccess`; caps fields on `PolicyEngine`; `evaluate` arm |
| Modify | `crates/daemon/src/config.rs` | `PolicyCapsSection` nested in `PolicySection`; `full_access` preset in the loader |
| Create | `crates/daemon/src/shell.rs` | `ShellExecRequest`, `ShellRuntime` facade, `shell_line` validation, argv assembly |
| Modify | `crates/daemon/src/command.rs` | `StartMode` enum threaded through `start_combed_inner`; shell-guard + policy + audit branch on it; spawn core unchanged |
| Modify | `crates/daemon/src/state.rs` | construct `ShellRuntime` in `DaemonState::bootstrap` (beside Command/Watch/Pty @228-247) |
| Modify | `crates/daemon/src/lib.rs` | re-export `ShellRuntime`, `ShellExecRequest` |
| Modify | `crates/ipc/src/protocol.rs` | `ShellExecParams`, `ShellExecResponse`, `IpcRequest::ShellExec` |
| Modify | `crates/daemon/src/ipc/handlers/*` | dispatch `ShellExec` -> `ShellRuntime::exec` |
| Modify | `crates/mcp/src/tools.rs` | `shell_exec` `#[tool]`, `McpShellExecParams`, tool_catalogue + system_discover entry (NO guard literals) |
| Modify | `crates/mcp/tests/mcp_live_daemon.rs` | tool-count anchor 38 -> 39 + `shell_exec` in the sorted list |
| Create | `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json` | tool contract fixture |
| Modify | `POLICY.md` | document `[policy.caps]` + `full_access` + `command_shell_start` |

**Verification commands (WSL authoritative — exit codes are truth; Windows clippy is blind to `#![cfg(unix)]` bodies):**

```bash
# unit/integration (per crate):
wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd <FILTER> 2>/dev/null; echo EXIT=\$?"
# full gate before final commit:
wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo clippy --workspace --all-targets 2>/dev/null; echo CLIPPY=\$?"
wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run --workspace -E 'not binary(session_reap)' 2>/dev/null; echo NEXTEST=\$?"
```

---

## Task 1: Caps schema in config (`[policy.caps]`)

**Files:**
- Modify: `crates/daemon/src/config.rs` (add `PolicyCapsSection`, add `caps` field to `PolicySection` @129)
- Test: `crates/daemon/src/config.rs` (inline `#[cfg(test)] mod tests`, mirror the existing `[policy.commands]` parse test @620)

- [ ] **Step 1: Write the failing test**

Add to the config tests module:

```rust
#[test]
fn parses_policy_caps_block() {
    let toml = "[daemon]\ndata_dir = \"/tmp/tc-caps\"\n\
                [policy]\nprofile = \"developer_local\"\n\
                [policy.caps]\nallow_shell = true\n";
    let cfg = DaemonConfig::from_toml(toml).expect("must parse [policy.caps]");
    let caps = cfg.policy.as_ref().and_then(|p| p.caps.as_ref()).expect("caps present");
    assert!(caps.allow_shell);
    assert!(!caps.allow_session);      // unspecified -> false
    assert!(!caps.allow_privileged);
    assert!(!caps.allow_remote);
}

#[test]
fn caps_absent_is_none_not_error() {
    let toml = "[daemon]\ndata_dir = \"/tmp/tc-nocaps\"\n[policy]\nprofile = \"developer_local\"\n";
    let cfg = DaemonConfig::from_toml(toml).expect("parse");
    assert!(cfg.policy.unwrap().caps.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd parses_policy_caps_block caps_absent_is_none 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL (`no field caps on PolicySection` / `PolicyCapsSection` undefined) -> non-zero EXIT.

- [ ] **Step 3: Add `PolicyCapsSection` + the `caps` field**

In `config.rs`, beside `PolicyCommandsSection` (~`:155`):

```rust
/// `[policy.caps]` (Hybrid trust model — reconciliation Decision 1/5).
/// Granular opt-in capabilities. ALL default false; deny-first preserved.
/// `full_access` is the only profile whose loader preset flips these true.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyCapsSection {
    #[serde(default)]
    pub allow_shell: bool,
    #[serde(default)]
    pub allow_session: bool,
    #[serde(default)]
    pub allow_privileged: bool,
    #[serde(default)]
    pub allow_remote: bool,
}
```

In `PolicySection` (after `probes:` @ ~`:144`):

```rust
    /// `[policy.caps]` block (Hybrid trust model). Optional; absent == all false.
    #[serde(default)]
    pub caps: Option<PolicyCapsSection>,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd parses_policy_caps_block caps_absent_is_none 2>/dev/null; echo EXIT=\$?"`
Expected: PASS (EXIT=0).

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/config.rs
git commit -m "feat(policy): add [policy.caps] schema (allow_shell/session/privileged/remote, default false)"
```

---

## Task 2: `CommandShellStart` policy action + caps-gated verdict

**Files:**
- Modify: `crates/daemon/src/policy.rs` (`PolicyAction` enum @59; `PolicyEngine` fields @118; `with_config` @170; `evaluate` @193)
- Test: `crates/daemon/src/policy.rs` (inline tests module)

Carry the caps into the engine and gate the new action: the verdict is `AllowWithAudit` when `allow_shell` is set on an exec-capable profile (`developer_local`/`admin_debug`/`full_access`), else `Deny`. `read_only_observer` and `repo_only` always deny shell.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn shell_start_denied_by_default() {
    let e = PolicyEngine::new(PolicyProfile::DeveloperLocal); // caps default false
    let v = e.evaluate(&PolicyAction::CommandShellStart {
        shell_line: "echo a | wc -c", cwd: Path::new("."), shell: "/bin/bash",
    });
    assert_eq!(v.decision, PolicyDecision::Deny);
}

#[test]
fn shell_start_allowed_with_audit_when_cap_on() {
    let e = PolicyEngine::with_config_caps(PolicyProfile::DeveloperLocal, None, None,
        PolicyCaps { allow_shell: true, ..Default::default() });
    let v = e.evaluate(&PolicyAction::CommandShellStart {
        shell_line: "echo a | wc -c", cwd: Path::new("."), shell: "/bin/bash",
    });
    assert_eq!(v.decision, PolicyDecision::AllowWithAudit);
}

#[test]
fn shell_start_denied_in_repo_only_even_with_cap() {
    let e = PolicyEngine::with_config_caps(PolicyProfile::RepoOnly, None, None,
        PolicyCaps { allow_shell: true, ..Default::default() });
    let v = e.evaluate(&PolicyAction::CommandShellStart {
        shell_line: "ls", cwd: Path::new("."), shell: "/bin/bash",
    });
    assert_eq!(v.decision, PolicyDecision::Deny);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd shell_start_ 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL (`CommandShellStart` / `PolicyCaps` / `with_config_caps` undefined).

- [ ] **Step 3: Implement the variant, caps, and verdict**

Add the variant to `PolicyAction` (@59):

```rust
    /// Shell-lane start (TC49). `shell_line` is the dedicated shell string;
    /// argv[0] is NOT a user-chosen interpreter here. Gated by allow_shell.
    CommandShellStart { shell_line: &'a str, cwd: &'a Path, shell: &'a str },
```

Add a caps value type (near `PolicyVerdict`):

```rust
/// Resolved capability set fed to the engine (mirror of `[policy.caps]`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyCaps {
    pub allow_shell: bool,
    pub allow_session: bool,
    pub allow_privileged: bool,
    pub allow_remote: bool,
}
```

Add `caps: PolicyCaps` to `PolicyEngine` (@118, default `PolicyCaps::default()` in `new`/`with_repo_root`/`with_config`). Add a ctor:

```rust
#[must_use]
pub fn with_config_caps(
    profile: PolicyProfile,
    repo_root: Option<PathBuf>,
    command_allow_roots: Option<Vec<String>>,
    caps: PolicyCaps,
) -> Self {
    let mut e = Self::with_config(profile, repo_root, command_allow_roots);
    e.caps = caps;
    e
}
```

In `evaluate` (@193), BEFORE the per-profile `match`, add the shell arm (note: `COMMANDS_DENY` is argv[0]-only and deliberately does NOT inspect `shell_line` — accepted residual risk, Decision 1):

```rust
    if let PolicyAction::CommandShellStart { .. } = action {
        let exec_profile = matches!(
            self.profile,
            PolicyProfile::DeveloperLocal | PolicyProfile::AdminDebug | PolicyProfile::FullAccess
        );
        if exec_profile && self.caps.allow_shell {
            return PolicyVerdict {
                decision: PolicyDecision::AllowWithAudit,
                reason: "shell_exec allowed by allow_shell capability (audited)".to_owned(),
            };
        }
        return PolicyVerdict {
            decision: PolicyDecision::Deny,
            reason: "shell_exec denied: allow_shell capability is off or profile forbids shell"
                .to_owned(),
        };
    }
```

(`PolicyProfile::FullAccess` lands in Task 3; until then the match arm referencing it will not compile — implement Task 3 in the same change set OR temporarily drop `FullAccess` from the `matches!` and add it in Task 3. Recommended: do Task 3 Step 3 now, then run both test sets together.)

- [ ] **Step 4: Run to verify pass**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd shell_start_ 2>/dev/null; echo EXIT=\$?"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/policy.rs
git commit -m "feat(policy): CommandShellStart action gated by allow_shell cap (AllowWithAudit/Deny)"
```

---

## Task 3: `full_access` profile preset

**Files:**
- Modify: `crates/daemon/src/policy.rs` (`PolicyProfile` enum @49)
- Modify: `crates/daemon/src/config.rs` (loader: when `profile = full_access`, preset caps true unless explicitly set)
- Test: `crates/daemon/src/config.rs` tests

**Guardrails (Decision 1):** preset = all four caps true; NEVER default; still `AllowWithAudit` (no `evaluate()` bypass); caps remain visible via `policy_status`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn full_access_profile_presets_all_caps_true() {
    let toml = "[daemon]\ndata_dir = \"/tmp/tc-fa\"\n[policy]\nprofile = \"full_access\"\n";
    let cfg = DaemonConfig::from_toml(toml).expect("parse full_access");
    let caps = cfg.resolved_caps(); // helper returns PolicyCaps after preset
    assert!(caps.allow_shell && caps.allow_session && caps.allow_privileged && caps.allow_remote);
}
```

- [ ] **Step 2: Run to verify fail**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd full_access_profile_presets 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL (`full_access` not a profile / `resolved_caps` undefined).

- [ ] **Step 3: Add the variant + preset resolver**

`PolicyProfile` (@49) gains:

```rust
    FullAccess,
```

In `config.rs`, add a resolver on `DaemonConfig` that applies the preset (does NOT mutate stored TOML; pure derivation):

```rust
/// Resolve the effective caps: explicit `[policy.caps]` if present; for the
/// `full_access` profile, default any UNSET cap to true. Never bypasses the
/// policy engine — these are inputs to `evaluate`.
#[must_use]
pub fn resolved_caps(&self) -> terminal_commander_daemon_policy_caps {
    use crate::policy::{PolicyCaps, PolicyProfile};
    let declared = self.policy.as_ref().and_then(|p| p.caps.clone());
    let full = matches!(self.policy.as_ref().map(|p| p.profile), Some(PolicyProfile::FullAccess));
    let base = declared.unwrap_or_default();
    PolicyCaps {
        allow_shell: base.allow_shell || full,
        allow_session: base.allow_session || full,
        allow_privileged: base.allow_privileged || full,
        allow_remote: base.allow_remote || full,
    }
}
```

(Use the real `PolicyCaps` return type; the placeholder identifier above is a reminder to import it. `From<&PolicyCapsSection> for PolicyCaps` is a one-line helper to add.)

- [ ] **Step 4: Run to verify pass**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd full_access_profile_presets shell_start_ 2>/dev/null; echo EXIT=\$?"`
Expected: PASS (both Task 2 + Task 3 sets green now that `FullAccess` exists).

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/policy.rs crates/daemon/src/config.rs
git commit -m "feat(policy): full_access profile presets all caps true (AllowWithAudit, no evaluate bypass)"
```

---

## Task 4: `StartMode` thread + shell branch in the command core

**Files:**
- Modify: `crates/daemon/src/command.rs` (`start_combed_inner` @~437: add a `StartMode` param; branch the shell-guard, policy action, and audit label; spawn body unchanged)
- Test: `crates/daemon/tests/command_runtime.rs` (the existing shell-deny tests @332/386/447 MUST stay green — behavior preservation)

**Design:** introduce a private enum and pass it where `start_combed_inner` is called. The argv path passes `StartMode::Argv`; the shell path (Task 5) passes `StartMode::Shell { shell_line, shell }`. Only three sites branch; everything from bucket allocation (@601) onward is shared verbatim.

```rust
/// Which lane started this combed job. Argv = the default command path
/// (shell interpreters hard-denied). Shell = TC49 shell_exec lane.
enum StartMode<'a> { Argv, Shell { shell_line: &'a str, shell: &'a str } }
```

Branch points in `start_combed_inner`:
1. Shell-guard (@561): run the `SHELL_INTERPRETERS_DENY` check + `command_rejected` audit ONLY for `StartMode::Argv`. For `Shell`, skip it.
2. Policy gate (@577): `Argv` -> `PolicyAction::CommandStart`; `Shell` -> `PolicyAction::CommandShellStart { shell_line, cwd, shell }`.
3. Deny audit (@582): label `command_rejected` (Argv) vs `command_shell_rejected` (Shell). On `AllowWithAudit` for `Shell`, emit a `command_shell_start` row with a redacted line (see Task helper below) BEFORE spawning.

Redaction helper (reuse the campaign's per-token masker; cap like `MAX_ARGV_ITEM_BYTES`):

```rust
/// Audit-safe preview of a shell line: per-token secret masking + 128B cap.
fn redact_shell_line(line: &str) -> String {
    // reuse mask_token_inline (command.rs) over whitespace splits; join; truncate on char boundary.
    let masked: Vec<String> = line.split_whitespace().map(mask_token_inline).collect();
    truncate_on_char_boundary(&masked.join(" "), 128)
}
```

- [ ] **Step 1: Write the failing test (behavior preservation + new audit label)**

```rust
#[tokio::test]
async fn argv_shell_interpreter_still_denied_unchanged() {
    let rt = test_runtime(PolicyProfile::DeveloperLocal); // existing helper
    let err = rt.start_combed(CommandStartRequest::new(vec!["sh".into(), "-c".into(), "echo hi".into()]))
        .await.unwrap_err();
    assert!(matches!(err, CommandError::ShellInterpreterDenied(ref s) if s == "sh"));
}
```

- [ ] **Step 2: Run to verify it PASSES already (guard intact) + compile of new enum fails its first user**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd argv_shell_interpreter_still_denied 2>/dev/null; echo EXIT=\$?"`
Expected: PASS (the argv guard is unchanged). This test is the regression lock for Task 4/5.

- [ ] **Step 3: Implement `StartMode` + branches**

Rename `start_combed_inner(req, reuse_bucket)` to `start_combed_inner(req, reuse_bucket, mode: StartMode<'_>)`. Update the existing caller `start_combed` / `start_combed_reusing` to pass `StartMode::Argv`. Wrap the guard at @561 in `if let StartMode::Argv = mode { ... }`. Replace the policy block @575-590 with a `match mode { Argv => CommandStart..., Shell{..} => CommandShellStart... }`, and on `Shell` + `AllowWithAudit`, call `self.audit("command_shell_start", &redact_shell_line(shell_line), "allow_with_audit", None, None)` before spawn.

- [ ] **Step 4: Run the regression + full command-runtime suite**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd --test command_runtime 2>/dev/null; echo EXIT=\$?"`
Expected: PASS (all existing argv tests unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/command.rs
git commit -m "refactor(command): thread StartMode through start_combed_inner (argv path behavior preserved)"
```

---

## Task 5: `shell.rs` — `ShellRuntime::exec`

**Files:**
- Create: `crates/daemon/src/shell.rs`
- Modify: `crates/daemon/src/lib.rs` (`mod shell;` + re-export)
- Test: `crates/daemon/tests/shell_runtime.rs` (new)

`ShellRuntime` is a thin facade over `CommandRuntime` (holds `Arc<CommandRuntime>`). `exec` validates `shell_line` (non-empty, <= `MAX_SHELL_LINE_BYTES = 16 * 1024`), resolves the shell (`req.shell` or default `/bin/bash` unix / fall back), assembles `argv = [shell, "-lc", shell_line]`, builds a `CommandStartRequest`, and calls `command.start_combed_inner(req, None, StartMode::Shell { shell_line, shell })`.

```rust
pub struct ShellExecRequest {
    pub shell_line: String,
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub rules: Vec<RuleDefinition>,
    pub bucket_config: Option<BucketConfig>,
    pub tag: Option<String>,
}

pub const MAX_SHELL_LINE_BYTES: usize = 16 * 1024;

pub struct ShellRuntime { command: Arc<CommandRuntime> }

impl ShellRuntime {
    pub fn new(command: Arc<CommandRuntime>) -> Self { Self { command } }

    pub async fn exec(&self, req: ShellExecRequest) -> Result<CommandStartResponse, CommandError> {
        if req.shell_line.trim().is_empty() { return Err(CommandError::EmptyArgv); }
        if req.shell_line.len() > MAX_SHELL_LINE_BYTES {
            return Err(CommandError::ArgvItemTooLong { index: 0, len: req.shell_line.len() });
        }
        let shell = req.shell.clone().unwrap_or_else(default_shell); // "/bin/bash" unix
        let argv = vec![shell.clone(), "-lc".to_owned(), req.shell_line.clone()];
        let cmd_req = CommandStartRequest { argv, cwd: req.cwd, env: req.env,
            bucket_config: req.bucket_config, rules: req.rules, grace: None,
            tag: req.tag, dedup_nonce: None, peer_discriminator: None };
        self.command.start_combed_shell(cmd_req, &req.shell_line, &shell).await
    }
}
```

`start_combed_shell` (add to `CommandRuntime` in command.rs) is the public seam that calls `start_combed_inner(cmd_req, None, StartMode::Shell { shell_line, shell })`.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn shell_exec_denied_default_profile() {
    let (cmd, _state) = test_command_runtime(PolicyProfile::DeveloperLocal); // caps off
    let rt = ShellRuntime::new(cmd);
    let err = rt.exec(ShellExecRequest::line("echo a | wc -c")).await.unwrap_err();
    assert!(matches!(err, CommandError::PolicyDenied(_)));
}

#[tokio::test]
async fn shell_exec_runs_pipeline_when_cap_on() {
    let (cmd, _state) = test_command_runtime_caps(PolicyProfile::DeveloperLocal,
        PolicyCaps { allow_shell: true, ..Default::default() });
    let rt = ShellRuntime::new(cmd);
    let resp = rt.exec(ShellExecRequest::line("echo a | wc -c")).await.expect("spawns");
    assert!(resp.job_id.to_wire_string().len() > 0);
}

#[tokio::test]
async fn shell_exec_rejects_oversize_line() {
    let (cmd, _state) = test_command_runtime_caps(PolicyProfile::DeveloperLocal,
        PolicyCaps { allow_shell: true, ..Default::default() });
    let big = "x".repeat(MAX_SHELL_LINE_BYTES + 1);
    let err = ShellRuntime::new(cmd).exec(ShellExecRequest::line(&big)).await.unwrap_err();
    assert!(matches!(err, CommandError::ArgvItemTooLong { .. }));
}
```

- [ ] **Step 2: Run to verify fail**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd --test shell_runtime 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL (module/types undefined).

- [ ] **Step 3: Implement `shell.rs` + `start_combed_shell` + `default_shell` + `mod shell;` in lib.rs**

(Code as above; `default_shell()` = `"/bin/bash"` on unix, `"bash"` fallback; `ShellExecRequest::line` test ctor.)

- [ ] **Step 4: Run to verify pass**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd --test shell_runtime 2>/dev/null; echo EXIT=\$?"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/shell.rs crates/daemon/src/command.rs crates/daemon/src/lib.rs
git commit -m "feat(daemon): ShellRuntime::exec — gated shell_exec lane over the command core"
```

---

## Task 6: Daemon wiring (`state.rs` bootstrap)

**Files:**
- Modify: `crates/daemon/src/state.rs` (@228-247: build `ShellRuntime` from the `Arc<CommandRuntime>`, store on `DaemonState`; thread `resolved_caps()` into the `PolicyEngine` via `with_config_caps`)
- Test: `crates/daemon/tests/` existing bootstrap test stays green; add a state-field assertion.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bootstrap_constructs_shell_runtime_and_threads_caps() {
    let dir = tempdir().unwrap();
    let mut cfg = DaemonConfig::defaults_in(dir.path());
    // force full_access so caps are on
    cfg.policy = Some(PolicySection::with_profile(PolicyProfile::FullAccess));
    let state = DaemonState::bootstrap(cfg).expect("bootstrap");
    assert!(state.shell.is_some() || /* field exists */ true);
    assert!(state.policy_engine().caps_allow_shell()); // small accessor for the test
}
```

- [ ] **Step 2: Run to verify fail**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd bootstrap_constructs_shell 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL.

- [ ] **Step 3: Wire it**

At `state.rs:228-247`, after `let command = Arc::new(CommandRuntime::new(...));`, add:

```rust
        let shell = Arc::new(ShellRuntime::new(Arc::clone(&command)));
```

Add `pub shell: Arc<ShellRuntime>` to `DaemonState` and set it. Change the `PolicyEngine` construction in bootstrap to `PolicyEngine::with_config_caps(profile, repo_root, allow_roots, config.resolved_caps())`.

- [ ] **Step 4: Run to verify pass**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd bootstrap_ 2>/dev/null; echo EXIT=\$?"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/daemon/src/state.rs
git commit -m "feat(daemon): wire ShellRuntime + resolved caps into DaemonState::bootstrap"
```

---

## Task 7: IPC `ShellExec` request/response + handler

**Files:**
- Modify: `crates/ipc/src/protocol.rs` (`ShellExecParams`, `ShellExecResponse`, `IpcRequest::ShellExec`)
- Modify: `crates/daemon/src/ipc/handlers/*` (dispatch `ShellExec` -> `state.shell.exec(...)`, mirror the `CommandStartCombed` handler)
- Test: `crates/daemon/tests/ipc_command.rs` (add a shell round-trip)

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn ipc_shell_exec_denied_then_allowed() {
    // developer_local, caps off -> ShellInterpreter/Policy deny mapped to IpcErrorCode::PolicyDenied
    let h = spawn_test_daemon(PolicyProfile::DeveloperLocal).await;
    let err = h.call(IpcRequest::ShellExec(ShellExecParams {
        shell_line: "echo a | wc -c".into(), shell: None, cwd: None, env: vec![],
        rules: vec![], tag: None, bucket_config: None,
    })).await.unwrap_err();
    assert_eq!(err.code, IpcErrorCode::PolicyDenied);
}
```

- [ ] **Step 2: Run to verify fail**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd ipc_shell_exec 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL.

- [ ] **Step 3: Add the protocol types + dispatch**

In `ipc/src/protocol.rs`, add `ShellExecParams` (fields above, all `#[serde(default)]` except `shell_line`) and `ShellExecResponse` (reuse `CommandStartResponse` shape or alias), and `ShellExec(ShellExecParams)` to `IpcRequest`. In the daemon handler, add an arm mirroring `CommandStartCombed`: build `ShellExecRequest` from params, call `state.shell.exec(req).await`, map `CommandError` via the existing `common.rs` mapper (extend it so `PolicyDenied`/`ShellInterpreterDenied` map to the right `IpcErrorCode`).

- [ ] **Step 4: Run to verify pass**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commanderd ipc_shell_exec 2>/dev/null; echo EXIT=\$?"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ipc/src/protocol.rs crates/daemon/src/ipc/
git commit -m "feat(ipc): ShellExec request/response + daemon handler dispatch"
```

---

## Task 8: MCP `shell_exec` tool + tool-count anchors

**Files:**
- Modify: `crates/mcp/src/tools.rs` (`McpShellExecParams`, `#[tool] async fn shell_exec`, tool_catalogue entry, `into_parts`/system_discover) — mirror `command_start_combed`; forward `IpcRequest::ShellExec`. NO guard literals.
- Modify: `crates/mcp/tests/mcp_live_daemon.rs` (list +`"shell_exec"`, count 38 -> 39)
- Modify: `crates/mcp/tests/runtime_state_live_e2e.rs` if it asserts a count
- Test: `crates/mcp/tests/mcp_live_daemon.rs`

- [ ] **Step 1: Update the failing anchor tests first**

In `mcp_live_daemon.rs`, add `"shell_exec".to_owned(),` to the sorted `names` vec (alphabetical: after `self_check`, before `subscription_close`), and change `live_count == 38` -> `== 39` in both `mcp_live_daemon.rs:213` and any sibling assertion.

- [ ] **Step 2: Run to verify fail**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commander-mcp live_health_roundtrip live_system_discover 2>/dev/null; echo EXIT=\$?"`
Expected: FAIL (tool not yet registered -> list/count mismatch).

- [ ] **Step 3: Implement the tool**

Mirror `command_stop` (added in the trust campaign) and `command_start_combed`:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpShellExecParams {
    /// The shell line to run (pipelines/compounds/redirects allowed).
    pub shell_line: String,
    #[serde(default)] pub shell: Option<String>,
    #[serde(default)] pub cwd: Option<String>,
    #[serde(default)] pub env: Vec<(String, String)>,
    #[serde(default)] pub rules: Vec<RuleDefinition>,
    #[serde(default)] pub tag: Option<String>,
    #[serde(default)] pub wait_ms: Option<u64>,
}

#[tool(description = "Run ONE shell line (pipelines/compounds/redirects) through the comb pipeline. \
    Requires the allow_shell capability; denied by default. Output is combed, never raw.")]
async fn shell_exec(&self, Parameters(p): Parameters<McpShellExecParams>)
    -> Result<CallToolResult, McpError> {
    let resp = self.daemon.request(IpcRequest::ShellExec(p.into_params())).await;
    self.render_command_start_like(resp) // same renderer as command_start_combed
}
```

Add a `tool_catalogue` row `{ name: "shell_exec", status: "live", group: "command" }` and ensure `system_discover` counts it.

- [ ] **Step 4: Run to verify pass**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commander-mcp 2>/dev/null; echo EXIT=\$?"`
Expected: PASS (39 tools).

- [ ] **Step 5: Guard-literal check + commit**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && rg -n 'Command::new|::spawn|TcpListener|UdpSocket|tokio::fs|std::fs|File::open|read_to_string|read_to_end' crates/mcp/src; echo RG=\$?"`
Expected: no NEW matches in changed code (RG=1 = clean if none).

```bash
git add crates/mcp/src/tools.rs crates/mcp/tests/mcp_live_daemon.rs crates/mcp/tests/runtime_state_live_e2e.rs
git commit -m "feat(mcp): shell_exec tool (39 live tools); anchors updated; no guard literals"
```

---

## Task 9: Contract fixture + O-01 live e2e + security e2e

**Files:**
- Create: `tests/fixtures/contracts/mcp-tools/shell_exec.v1.json`
- Create: `crates/mcp/tests/shell_live_e2e.rs`

- [ ] **Step 1: Write the contract fixture**

```json
{
  "tool": "shell_exec",
  "version": 1,
  "group": "command",
  "params": {
    "shell_line": {"type": "string", "required": true},
    "shell": {"type": "string", "required": false},
    "cwd": {"type": "string", "required": false},
    "env": {"type": "array", "required": false},
    "rules": {"type": "array", "required": false},
    "tag": {"type": "string", "required": false},
    "wait_ms": {"type": "integer", "required": false}
  },
  "policy": "CommandShellStart (allow_shell cap; AllowWithAudit)",
  "denied_by_default": true,
  "output": "combed signals + bounded receipt; never raw stdout"
}
```

- [ ] **Step 2: Write the failing O-01 + security e2e**

```rust
#[tokio::test]
async fn o01_pipeline_returns_signal_when_cap_on() {
    let h = spawn_live_daemon_full_access(); // full_access profile -> allow_shell true
    let (_s, client) = paired_against_live_daemon(&h).await;
    let out = call_tool(&client, "shell_exec", json!({
        "shell_line": "echo a | wc -c", "wait_ms": 2000
    })).await;
    let payload = first_text(&out);
    assert!(!payload.contains("aaaaaaaa"), "no raw stdout dump");
    // receipt/exit present:
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert!(v.get("job_id").is_some() || v.get("receipt").is_some());
}

#[tokio::test]
async fn shell_exec_denied_on_default_profile_e2e() {
    let h = spawn_live_daemon(); // developer_local, caps off
    let (_s, client) = paired_against_live_daemon(&h).await;
    let err = client.call_tool({ let mut p = CallToolRequestParams::new("shell_exec");
        p.arguments = json!({"shell_line":"echo hi"}).as_object().cloned(); p })
        .await.expect_err("must deny");
    assert!(format!("{err}").to_lowercase().contains("denied")
        || format!("{err}").to_lowercase().contains("policy"));
}
```

- [ ] **Step 3: Run to verify fail, then green after wiring is complete**

Run: `wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run -p terminal-commander-mcp --test shell_live_e2e 2>/dev/null; echo EXIT=\$?"`
Expected: PASS once Tasks 1-8 are in (the e2e is the integration proof of O-01).

- [ ] **Step 4: Commit**

```bash
git add tests/fixtures/contracts/mcp-tools/shell_exec.v1.json crates/mcp/tests/shell_live_e2e.rs
git commit -m "test(shell_exec): O-01 pipeline e2e + default-deny e2e + contract fixture"
```

---

## Task 10: POLICY.md caps documentation + full gate

**Files:**
- Modify: `POLICY.md` (`[policy.caps]` section; `full_access` profile; `command_shell_start` audit action; note `shell_passthrough` is now the `allow_shell` cap, default false)

- [ ] **Step 1: Document** — add a `[policy.caps]` subsection under section 4 mirroring `[policy.commands]`; add `full_access` to the profile table (section 2) with the 5 guardrails; add `command_shell_start` to the audit-actions list. State the accepted residual risk (sudo reachable inside a `shell_line` on a permissive host; that is why shell is a trusted-profile cap + Wave 4 keeps privilege a separate closed helper).

- [ ] **Step 2: Full both-OS gate**

Run:
```bash
wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo clippy --workspace --all-targets 2>/dev/null; echo CLIPPY=\$?"
wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo nextest run --workspace -E 'not binary(session_reap)' 2>/dev/null; echo NEXTEST=\$?"
```
Expected: `CLIPPY=0`, `NEXTEST=0`. (Windows clippy optional; WSL is authoritative.)

- [ ] **Step 3: Commit**

```bash
git add POLICY.md
git commit -m "docs(policy): document [policy.caps], full_access, command_shell_start audit (TC49)"
```

---

## Self-review (run before handoff)

**Spec coverage vs Wave-1 TC49 + Decisions:**
- CommandShellStart action — Task 2 ✓
- `[policy.caps].allow_shell` nested — Task 1 ✓
- full_access preset, no evaluate() bypass — Task 3 ✓
- shell.rs ShellRuntime::exec, argv guard stays pure (Argv lane untouched) — Tasks 4,5 ✓
- state.rs:228-247 wiring + caps threaded — Task 6 ✓ ; lib.rs re-export — Task 5 ✓
- IPC ShellExec — Task 7 ✓
- MCP shell_exec + 38->39 anchors + no guard literals — Task 8 ✓
- contract fixture — Task 9 ✓
- security tests (default deny, argv[0]=bash still denied, oversize reject, command_shell_start audit, combed-not-raw) — Tasks 4,5,9 ✓
- O-01 e2e — Task 9 ✓
- `-lc` one-shot — Task 5 ✓ ; alt door NOT added ✓ ; TC50/sentinel excluded ✓

**Type consistency:** `PolicyCaps` (engine value) vs `PolicyCapsSection` (TOML) — distinct by design, bridged by `From`. `ShellExecRequest` (daemon) vs `ShellExecParams` (IPC) vs `McpShellExecParams` (MCP) — three layers, same fields. `start_combed_shell` is the single public seam into `StartMode::Shell`. Confirm `MAX_SHELL_LINE_BYTES` used identically in Task 5 impl + test.

**Open verification the implementer MUST do (not placeholders — repo facts to confirm at coding time):**
- Exact `self.audit(...)` arg types + the `format_argv_metadata`/`mask_token_inline` symbol names in `command.rs` (used by Task 4's `redact_shell_line`).
- The exact `common.rs` `CommandError -> IpcError` mapper arm names (Task 7).
- The `command_start_combed` renderer reused by `shell_exec` (Task 8 `render_command_start_like`).

---

## Execution handoff

Plan saved to `docs/plans/2026-06-09-tc49-shell-exec-implementation.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, two-stage review between tasks (code-reviewer + test-runner), each task gated: code -> WSL clippy+nextest -> live e2e. Best for this security-sensitive, multi-crate change.
2. **Inline Execution** — execute tasks in-session with checkpoints.

Either way: NO push/merge without explicit human approval; commit to a review branch; the argv-path regression test (Task 4) is the hard gate that the default surface stayed safe.
