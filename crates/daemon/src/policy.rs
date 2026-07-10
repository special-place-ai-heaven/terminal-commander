// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Policy engine (TC22). Decides allow / deny / allow_with_audit for
//! the gated actions defined in `SECURITY.md` section 4. The default
//! shipping profile is `developer_local` per `POLICY.md` section 2.1.
//!
//! Source-status: PARTIAL implementation of the `POLICY.md` doctrine.
//! See `docs/specs/2026-05-29-tc22-policy-engine-implementation.md`.
//!
//! ENFORCED today (TC22 Phase 1 + Phase 2):
//! - cross-profile command deny set (sudo/doas/su/pkexec/kexec/polkit),
//!   by basename and absolute path;
//! - default-deny on the sensitive path SUFFIX list (anchored on
//!   README.md:294-297) for FileRead / FileWatch in every profile;
//! - per-profile mutation gates (`read_only_observer` denies command_*
//!   and registry_*; `admin_debug` denies registry mutations;
//!   `registry_activate` is AllowWithAudit for dev_local / repo_only);
//! - `repo_only` `$REPO_ROOT` containment: FileRead / FileWatch /
//!   CommandStart whose path/cwd resolves outside the configured root are
//!   denied (Phase 1);
//! - command allow-list (`[policy.commands] allow_roots`): when an
//!   operator configures a non-empty list, off-list commands are denied
//!   (`no_allow_rule`) for both exec profiles. Default-deny is OPT-IN; an
//!   unconfigured list allows any command surviving the structural deny
//!   set (Phase 2);
//! - `[policy.paths]` allow/deny lists (TC22 A1/A3): `read_allow` (FileRead),
//!   `watch_allow` (FileWatch), `write_allow` (FileWrite), and `deny_extra`
//!   (all three) are compiled to anchored glob-regexes at construction and
//!   enforced deny-first on top of the default-deny suffix list and repo_only
//!   containment. An empty list is "not configured" and allows (zero-config
//!   stays usable); a non-empty list is authoritative (`no_allow_rule` on a
//!   miss).
//! - `[policy.probes]` allow_kinds / deny_kinds (TC22 A2): ENFORCED as a
//!   deny-first secondary filter on the three real probe-creating ops
//!   (`command_start_combed` -> `command`, `pty_command_start` /
//!   `shell_session_start` -> `pty`, `file_watch_start` -> `file_watch`) via
//!   `PolicyAction::ProbeCreate { kind }`. Kinds are CASE-SENSITIVE snake_case
//!   from the closed set {command, file_watch, pty}. Deny beats allow; an
//!   EMPTY allow_kinds is "not configured" == allow (zero-config stays usable),
//!   mirroring the path allow-list posture. The gate is layered ON TOP of each
//!   op's primary gate -- it can only narrow, never widen.
//!
//! NOT YET enforced (later phases): the `[limits]` checks (max jobs,
//! rates, sizes) and the `allow_override` mechanism (POLICY.md sections
//! 4, 5, 6 step 3).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Closed-set policy decision (matches `docs/contracts/enums/policy-decision.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Deny,
    AllowWithAudit,
    Error,
}

/// Profile names (closed set in MVP; matches POLICY.md section 2).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyProfile {
    #[default]
    DeveloperLocal,
    RepoOnly,
    ReadOnlyObserver,
    AdminDebug,
    /// Convenience profile (Hybrid trust model -- reconciliation Decision 1).
    /// Exec-capable like `developer_local`; its loader preset (`resolved_caps`)
    /// flips ALL `[policy.caps]` true. NEVER short-circuits `evaluate()` -- it
    /// only sets the caps inputs, so gated actions stay `AllowWithAudit`.
    FullAccess,
}

/// Action being evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAction<'a> {
    CommandStart {
        argv: &'a [String],
        cwd: &'a Path,
    },
    /// Shell-lane start (TC49). `shell_line` is the dedicated shell string;
    /// argv[0] is NOT a user-chosen interpreter here. Gated by `allow_shell`.
    /// NOTE: `COMMANDS_DENY` is argv[0]-only and deliberately does NOT scan
    /// `shell_line` (accepted residual risk, Decision 1).
    CommandShellStart {
        shell_line: &'a str,
        cwd: &'a Path,
        shell: &'a str,
    },
    /// Persistent shell-session start (P1 / TC50). Mirrors `CommandShellStart`
    /// gating but behind the independent `allow_session` capability: a session
    /// is a longer-lived interactive shell and gets its own operator switch.
    /// `shell` is the resolved interpreter; argv[0] is NOT user-chosen here.
    SessionStart {
        shell: &'a str,
        cwd: &'a Path,
    },
    CommandStdin,
    CommandSignal,
    FileRead {
        path: &'a Path,
    },
    FileWatch {
        path: &'a Path,
    },
    /// File WRITE (TC22 A3). The MUTATING filesystem action that finally
    /// makes `[policy.paths] write_allow` enforceable. Gated deny-first in
    /// CANONICAL form against the default-deny suffix list, `deny_extra`,
    /// and `write_allow`; subject to `repo_only` containment via
    /// [`action_path_subject`]; and DENIED outright under
    /// `read_only_observer`. `path` is the canonical TARGET (the IPC
    /// handler canonicalizes the parent + filename, since the target may
    /// not exist yet).
    FileWrite {
        path: &'a Path,
    },
    /// Probe-kind gate (A2, ENFORCED). The secondary, deny-first filter that
    /// makes `[policy.probes]` `allow_kinds` / `deny_kinds` enforceable at
    /// probe creation. `kind` is one of the canonical snake_case
    /// [`terminal_commander_ipc::ProbeKind`] wire tags: `"command"`,
    /// `"file_watch"`, `"pty"`. There is NO standalone `probe_create` MCP tool
    /// or IPC method; instead the three real probe-creating operations
    /// (`command_start_combed`, `pty_command_start` / `shell_session_start`,
    /// `file_watch_start`) layer this gate ON TOP of their own primary op gate
    /// (`CommandStart` / `SessionStart` / `FileWatch`). `evaluate` has a
    /// dedicated arm (deny-first; empty `allow_kinds` == not configured ==
    /// allow, mirroring the path allow-list posture). This is a TIGHTENING
    /// secondary filter -- it never widens an op the primary gate denied.
    ProbeCreate {
        kind: &'a str,
    },
    RegistryCreate,
    RegistryActivate,
    BucketWait,
    BucketRead,
    EventContext,
}

/// Decision record returned by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyVerdict {
    pub decision: PolicyDecision,
    pub reason: String,
}

/// Resolved capability set fed to the engine (mirror of `[policy.caps]`).
///
/// All-false by default; deny-first preserved. These are INPUTS to
/// `evaluate()`, never a bypass: a cap being on only flips a gated action
/// from `Deny` to `AllowWithAudit` on an exec-capable profile.
// 4 independent opt-in capability flags; a bitfield/enum would hurt the config/serde surface
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyCaps {
    pub allow_shell: bool,
    pub allow_session: bool,
    pub allow_privileged: bool,
    pub allow_remote: bool,
}

/// The seven binaries that are denied across every profile per the
/// PRIVILEGE_MODEL.md headline invariant.
pub const COMMANDS_DENY: &[&str] = &[
    "sudo",
    "doas",
    "su",
    "pkexec",
    "kexec",
    "polkit-agent",
    "polkit-auth-agent-1",
];

/// Default-deny sensitive path SUFFIXES (matched as `ends_with`).
/// Mirrors SECURITY.md section 5 (anchored on README.md:294-297).
pub const DEFAULT_DENY_PATH_SUFFIXES: &[&str] = &[
    ".ssh/id_rsa",
    ".ssh/id_ed25519",
    ".ssh/id_ecdsa",
    "/etc/shadow",
    "/etc/sudoers",
    ".pgpass",
    ".netrc",
    ".aws/credentials",
    ".aws/config",
    ".kube/config",
    ".docker/config.json",
    ".npmrc",
    ".pypirc",
    ".vault-token",
];

/// Policy engine. Thread-safe via `&self`. Holds the active profile and,
/// for `repo_only`, the canonicalized repo-root used for containment.
///
/// NOTE: this type is `Clone` but intentionally NOT `Copy` — it carries
/// an owned `repo_root: Option<PathBuf>`. Callers that previously relied
/// on copy semantics must `.clone()` at each hand-off.
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    pub profile: PolicyProfile,
    /// Canonicalized `$REPO_ROOT` for `repo_only` containment (POLICY.md
    /// section 2.2). `None` for every other profile, and for `repo_only`
    /// engines built without the config key — in which case containment
    /// denies all path/cwd-bearing actions fail-safe.
    repo_root: Option<PathBuf>,
    /// Configured command allow-list (argv[0] basenames) from
    /// `[policy.commands] allow_roots`. `None` (or empty) means "not
    /// configured": both exec profiles then allow any command surviving
    /// the structural deny set (default-deny is opt-in). A non-empty list
    /// is authoritative and enforced for both `developer_local` and
    /// `repo_only`.
    command_allow_roots: Option<Vec<String>>,
    /// Compiled `[policy.paths] read_allow` glob list (POLICY.md section 5).
    /// Empty == "not configured" == allow any `FileRead` surviving the
    /// default-deny + `deny_extra` layers (zero-config stays usable). A
    /// non-empty list is authoritative: a `FileRead` path matching no glob
    /// is denied (`no_allow_rule`). Compiled once at construction.
    read_allow: GlobList,
    /// Compiled `[policy.paths] watch_allow` glob list (POLICY.md section 5).
    /// Same opt-in semantics as `read_allow`, applied to `FileWatch`.
    watch_allow: GlobList,
    /// Compiled `[policy.paths] write_allow` glob list (POLICY.md section 5,
    /// TC22 A3). Same opt-in semantics as `read_allow`, applied to
    /// `FileWrite`: empty == not configured == allow (zero-config writes
    /// stay usable once the operator opts into the write tool); a non-empty
    /// list is authoritative and a write path matching no glob is denied
    /// (`no_allow_rule`). Independent from `read_allow` -- an operator can
    /// permit reads broadly while confining writes to a narrow tree.
    write_allow: GlobList,
    /// Compiled `[policy.paths] deny_extra` glob list (POLICY.md section 5).
    /// Additional hard denies layered onto the default-deny suffix check for
    /// `FileRead` / `FileWatch`. Deny beats allow: a path matching `deny_extra`
    /// is denied even if it would satisfy the allow-list.
    deny_extra: GlobList,
    /// Resolved capability set (Hybrid trust model, Decision 1). Defaults to
    /// all-false; only `with_config_caps` (fed by `DaemonConfig::resolved_caps`)
    /// sets it. Caps are inputs to `evaluate()`, never an `evaluate()` bypass.
    caps: PolicyCaps,
    /// `[policy.probes] allow_kinds` (TC22 A2). The probe-kind allow-list for
    /// `ProbeCreate`. Empty == "not configured" == allow (zero-config stays
    /// usable), mirroring the path allow-lists. A non-empty list is
    /// authoritative: a `kind` not present is denied (`no_allow_rule`). Matched
    /// by EXACT, case-sensitive equality against the snake_case `ProbeKind`
    /// wire tags ({command, file_watch, pty}). Set by `with_probe_kinds`.
    probe_allow_kinds: Vec<String>,
    /// `[policy.probes] deny_kinds` (TC22 A2). The probe-kind deny-list for
    /// `ProbeCreate`. Deny beats allow: a `kind` present here is denied
    /// (`probe_kind_denied`) even if it also appears in `probe_allow_kinds`.
    /// Same exact-match semantics as `probe_allow_kinds`. Set by
    /// `with_probe_kinds`.
    probe_deny_kinds: Vec<String>,
}

impl PolicyEngine {
    /// Construct an engine for a profile with no repo-root configured.
    /// For `repo_only` this yields a fail-safe engine that cannot
    /// confirm any path is inside the repo, so path/cwd actions are
    /// denied. Use [`PolicyEngine::with_repo_root`] for a usable
    /// `repo_only` engine.
    #[must_use]
    pub const fn new(profile: PolicyProfile) -> Self {
        Self {
            profile,
            repo_root: None,
            command_allow_roots: None,
            // `GlobList::default()` is not const; spell the empty (unconfigured)
            // list out so `new` stays `const fn`. Empty == not enforced.
            read_allow: GlobList {
                configured: 0,
                regexes: Vec::new(),
                had_compile_error: false,
            },
            watch_allow: GlobList {
                configured: 0,
                regexes: Vec::new(),
                had_compile_error: false,
            },
            write_allow: GlobList {
                configured: 0,
                regexes: Vec::new(),
                had_compile_error: false,
            },
            deny_extra: GlobList {
                configured: 0,
                regexes: Vec::new(),
                had_compile_error: false,
            },
            // `PolicyCaps::default()` is not const; spell the all-false set out.
            caps: PolicyCaps {
                allow_shell: false,
                allow_session: false,
                allow_privileged: false,
                allow_remote: false,
            },
            // Probe-kind lists default UNCONFIGURED (empty == not enforced ==
            // allow), matching the path/command allow-list posture. The
            // bootstrap path layers them on with `with_probe_kinds`. `Vec::new`
            // is const so `new` stays `const fn`.
            probe_allow_kinds: Vec::new(),
            probe_deny_kinds: Vec::new(),
        }
    }

    /// Construct an engine for a profile with an explicit repo-root.
    /// The root is canonicalized once here; if canonicalization fails
    /// (path missing), the raw path is retained so containment compares
    /// against the operator-supplied value rather than silently
    /// widening to "no root".
    #[must_use]
    pub fn with_repo_root(profile: PolicyProfile, repo_root: PathBuf) -> Self {
        let canonical = std::fs::canonicalize(&repo_root).unwrap_or(repo_root);
        Self {
            profile,
            repo_root: Some(canonical),
            command_allow_roots: None,
            read_allow: GlobList::default(),
            watch_allow: GlobList::default(),
            write_allow: GlobList::default(),
            deny_extra: GlobList::default(),
            caps: PolicyCaps::default(),
            probe_allow_kinds: Vec::new(),
            probe_deny_kinds: Vec::new(),
        }
    }

    /// Construct an engine from the full loaded profile schema (POLICY.md
    /// section 4): the active profile, the optional `$REPO_ROOT`, and the
    /// optional command allow-list. This is the ctor the daemon uses at
    /// bootstrap; the narrower `new` / `with_repo_root` remain for tests
    /// and for non-exec profiles.
    #[must_use]
    pub fn with_config(
        profile: PolicyProfile,
        repo_root: Option<PathBuf>,
        command_allow_roots: Option<Vec<String>>,
    ) -> Self {
        let repo_root = repo_root.map(|r| std::fs::canonicalize(&r).unwrap_or(r));
        // Normalize an empty list to None so fallback logic has one case.
        let command_allow_roots = command_allow_roots.filter(|v| !v.is_empty());
        Self {
            profile,
            repo_root,
            command_allow_roots,
            // Path allow/deny lists default to UNCONFIGURED (empty == not
            // enforced). The bootstrap path layers them on with `with_paths`;
            // tests that exercise only command/containment posture leave them
            // empty so existing behavior is preserved exactly.
            read_allow: GlobList::default(),
            watch_allow: GlobList::default(),
            write_allow: GlobList::default(),
            deny_extra: GlobList::default(),
            caps: PolicyCaps::default(),
            // Probe-kind lists default UNCONFIGURED here too; the bootstrap
            // path layers them on with `with_probe_kinds`.
            probe_allow_kinds: Vec::new(),
            probe_deny_kinds: Vec::new(),
        }
    }

    /// Layer the compiled `[policy.paths]` allow/deny globs onto an engine
    /// (POLICY.md section 5). Consuming builder: chained after
    /// [`Self::with_config`] / [`Self::with_config_caps`] at bootstrap so the
    /// narrower ctors and their existing callers keep their signatures.
    ///
    /// Each glob list is compiled to anchored regexes ONCE here, off the
    /// `evaluate` hot path. Semantics mirror the command allow-list
    /// ([`Self::command_allowed`]): an EMPTY list is "not configured" and is
    /// NOT enforced (zero-config file reads stay usable); a NON-EMPTY list is
    /// authoritative and a path matching none of its globs is denied
    /// (`no_allow_rule`). `deny_extra` is an additional HARD deny layered onto
    /// the default-deny suffix check; deny beats allow.
    ///
    /// A glob that fails to compile is dropped and the list is marked
    /// fail-safe (see [`GlobList`]): a broken allow-list never widens to
    /// allow-all.
    #[must_use]
    pub fn with_paths(
        mut self,
        read_allow: &[String],
        watch_allow: &[String],
        write_allow: &[String],
        deny_extra: &[String],
    ) -> Self {
        self.read_allow = GlobList::compile(read_allow);
        self.watch_allow = GlobList::compile(watch_allow);
        self.write_allow = GlobList::compile(write_allow);
        self.deny_extra = GlobList::compile(deny_extra);
        self
    }

    /// Layer the `[policy.probes]` allow/deny KIND lists onto an engine
    /// (POLICY.md section 6 steps 2c / 2e; TC22 A2). Consuming builder,
    /// chained after [`Self::with_paths`] at bootstrap, mirroring
    /// [`Self::with_paths`]'s style so the narrower ctors keep their
    /// signatures.
    ///
    /// Semantics mirror the path allow-list exactly: `deny_kinds` is a HARD
    /// deny (deny beats allow); an EMPTY `allow_kinds` is "not configured" and
    /// is NOT enforced (zero-config probe creation stays usable); a NON-EMPTY
    /// `allow_kinds` is authoritative and a `kind` not in it is denied
    /// (`no_allow_rule`). Matching is EXACT, case-sensitive equality against
    /// the snake_case [`terminal_commander_ipc::ProbeKind`] wire tags.
    ///
    /// HARDENING: any configured kind outside the closed set
    /// ({command, file_watch, pty}) is logged via `tracing::warn!` (a
    /// silently-ineffective deny_kind is an operator footgun), but NOT
    /// rejected -- forward-compat for kinds a future ProbeKind variant adds.
    #[must_use]
    pub fn with_probe_kinds(mut self, allow: &[String], deny: &[String]) -> Self {
        const KNOWN_KINDS: &[&str] = &["command", "file_watch", "pty"];
        for k in allow.iter().chain(deny.iter()) {
            if !KNOWN_KINDS.contains(&k.as_str()) {
                tracing::warn!(
                    kind = %k,
                    known_kinds = ?KNOWN_KINDS,
                    "[policy.probes] lists an unknown probe kind not in the closed \
                     set; it will never match a real probe (likely an operator typo)"
                );
            }
        }
        self.probe_allow_kinds = allow.to_vec();
        self.probe_deny_kinds = deny.to_vec();
        self
    }

    /// Build an engine carrying a resolved capability set (Hybrid trust model,
    /// Decision 1/5). Caps are inputs to `evaluate()` -- they only flip a gated
    /// action from `Deny` to `AllowWithAudit` on an exec-capable profile; they
    /// NEVER short-circuit the engine.
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

    /// Read-only accessor: is the `allow_shell` capability set on this engine?
    ///
    /// The `caps` field is private (caps are inputs to [`Self::evaluate`], never
    /// a public toggle). This accessor lets bootstrap-wiring tests confirm that
    /// the resolved `[policy.caps]` were threaded into the engine without
    /// exposing the full caps set or a mutation path.
    #[must_use]
    pub const fn caps_allow_shell(&self) -> bool {
        self.caps.allow_shell
    }

    /// Read-only accessor mirroring [`Self::caps_allow_shell`] for the
    /// `allow_session` capability (P1 session lane). Lets bootstrap-wiring
    /// tests confirm the resolved cap was threaded without exposing a mutation
    /// path.
    #[must_use]
    pub const fn caps_allow_session(&self) -> bool {
        self.caps.allow_session
    }

    /// Read-only accessor: the full RESOLVED capability set carried by this
    /// engine.
    ///
    /// These are the caps the engine evaluates against -- the bootstrap path
    /// feeds [`Self::with_config_caps`] with `DaemonConfig::resolved_caps()`
    /// (`base || full_access`), so under `full_access` every cap reads ON even
    /// when `[policy.caps]` lists one as `false`. `policy_status` surfaces this so
    /// the active per-call caps are visible (POLICY.md section 4.1), without
    /// exposing a mutation path -- the field stays private and `PolicyCaps` is
    /// `Copy`, so this returns a snapshot, never an alias.
    #[must_use]
    pub const fn resolved_caps(&self) -> PolicyCaps {
        self.caps
    }

    /// Default-constructed engine uses the `developer_local` profile.
    #[must_use]
    pub fn default_engine() -> Self {
        Self::new(PolicyProfile::default())
    }

    /// Evaluate a gated action.
    ///
    /// CALLER CONTRACT (security): `FileRead` / `FileWatch` paths are matched
    /// in CANONICAL form. The guard normalizes the subject with
    /// `canonicalize_lexical` before running the default-deny suffix check,
    /// `deny_extra`, and the allow-list, so a `..` prefix can never satisfy an
    /// allow glob. Callers that already canonicalize (the IPC file handlers)
    /// see no behavior change because `canonicalize_lexical` is idempotent on a
    /// canonical path; direct callers get the same self-contained protection.
    // Single linear decision tower (structural denies -> per-profile arms);
    // splitting the security-critical gate would scatter the deny-first logic.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn evaluate(&self, action: &PolicyAction<'_>) -> PolicyVerdict {
        // First: structural denies that apply across every profile.
        if let PolicyAction::CommandStart { argv, .. } = action
            && let Some(arg0) = argv.first()
        {
            let basename = std::path::Path::new(arg0.as_str())
                .file_name()
                .and_then(|os| os.to_str())
                .unwrap_or(arg0.as_str());
            if COMMANDS_DENY.contains(&basename) {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "command '{basename}' is in the closed deny set (sudo/doas/su/pkexec/kexec)"
                    ),
                };
            }
        }
        // `[policy.paths]` enforcement (POLICY.md section 5 / section 6 step
        // 2e), preceded by the cross-profile default-deny sensitive-suffix
        // check (SECURITY.md section 5). Layered for FileRead / FileWatch
        // BEFORE the shell/session lanes and per-profile match.
        //
        // CALLER CONTRACT: FileRead / FileWatch paths are matched in CANONICAL
        // form. The IPC handlers (`resolve_and_authorize_file`) already
        // canonicalize before constructing the action, so for them this is
        // idempotent; for any direct `evaluate` caller it is the guard that
        // makes the gate self-contained -- a `..` prefix can never satisfy an
        // allow glob.
        //
        // SECURITY: the subject is normalized ONCE with `canonicalize_lexical`
        // (the same routine `repo_root_contains` uses) so `..` / `.` are
        // collapsed before any glob runs. Matching the RAW path would let
        // `/allowed/../../etc/secret` lexically satisfy an `/allowed/**` allow
        // glob even though it resolves OUTSIDE the allowed tree; matching the
        // canonical form closes that traversal hole. The default-deny suffix
        // check runs on the same canonical form so a `..`-disguised or
        // symlinked sensitive target is caught consistently with the
        // allow/deny lists below.
        //
        // Order within this block is deny-first:
        //   0. default-deny sensitive suffix (`.ssh/id_rsa`, `/etc/shadow`, ...).
        //   1. `deny_extra` is an additional HARD deny (step 2a doctrine).
        //      Deny beats allow: a path matching deny_extra is denied even if
        //      it would satisfy the allow-list.
        //   2. The per-action allow-list (`read_allow` for FileRead,
        //      `watch_allow` for FileWatch). EMPTY == not configured == allow
        //      (zero-config stays usable); a NON-EMPTY list is authoritative
        //      and a path matching no glob is denied (`no_allow_rule`).
        // This runs ahead of repo_only containment so deny_extra and the
        // allow-list apply in EVERY profile, not just repo_only.
        if let PolicyAction::FileRead { path }
        | PolicyAction::FileWatch { path }
        | PolicyAction::FileWrite { path } = action
        {
            // Normalize the subject ONCE; every check below matches this form.
            let canonical = canonicalize_lexical(path);
            if Self::path_default_denied(&canonical) {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "path '{}' matches a default-deny sensitive suffix (SECURITY.md \u{a7}5)",
                        path.display()
                    ),
                };
            }
            if self.deny_extra.matches(&canonical) {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "path '{}' matches a [policy.paths] deny_extra rule (default_deny_match)",
                        path.display()
                    ),
                };
            }
            let allow = match action {
                PolicyAction::FileRead { .. } => &self.read_allow,
                PolicyAction::FileWatch { .. } => &self.watch_allow,
                // The outer `if let` guarantees this arm is FileWrite.
                _ => &self.write_allow,
            };
            if !allow.allows(&canonical) {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "path '{}' is not in the [policy.paths] allow-list (no_allow_rule)",
                        path.display()
                    ),
                };
            }
        }

        // Shell-lane gate (TC49). Evaluated BEFORE the per-profile match so a
        // single deny-first rule covers every profile: shell_exec is allowed
        // (AllowWithAudit) ONLY on an exec-capable profile with `allow_shell`
        // on; otherwise denied. NOTE: COMMANDS_DENY is argv[0]-only and does
        // NOT scan `shell_line` (accepted residual risk, Decision 1).
        if let PolicyAction::CommandShellStart { .. } = action {
            let exec_profile = matches!(
                self.profile,
                PolicyProfile::DeveloperLocal
                    | PolicyProfile::AdminDebug
                    | PolicyProfile::FullAccess
            );
            if exec_profile && self.caps.allow_shell {
                return PolicyVerdict {
                    decision: PolicyDecision::AllowWithAudit,
                    reason: "shell execution allowed by allow_shell capability (audited)"
                        .to_owned(),
                };
            }
            return PolicyVerdict {
                decision: PolicyDecision::Deny,
                reason:
                    "shell execution denied: allow_shell capability is off or profile forbids shell"
                        .to_owned(),
            };
        }

        // Session-lane gate (P1 / TC50). Same deny-first shape as the shell
        // lane, but gated by the independent `allow_session` capability so a
        // persistent session is a separate operator opt-in from one-shot shell.
        if let PolicyAction::SessionStart { .. } = action {
            let exec_profile = matches!(
                self.profile,
                PolicyProfile::DeveloperLocal
                    | PolicyProfile::AdminDebug
                    | PolicyProfile::FullAccess
            );
            if exec_profile && self.caps.allow_session {
                return PolicyVerdict {
                    decision: PolicyDecision::AllowWithAudit,
                    reason: "shell_session_start allowed by allow_session capability (audited)"
                        .to_owned(),
                };
            }
            return PolicyVerdict {
                decision: PolicyDecision::Deny,
                reason:
                    "shell_session_start denied: allow_session capability is off or profile forbids sessions"
                        .to_owned(),
            };
        }

        // Probe-kind gate (TC22 A2; POLICY.md section 6 steps 2c / 2e).
        // SECONDARY, deny-first filter layered on top of each probe-creating
        // op's PRIMARY gate (CommandStart / SessionStart / FileWatch): the
        // caller evaluates its primary action first, and only on an allow does
        // it evaluate ProbeCreate. This arm can only NARROW (deny a kind), never
        // widen -- it returns Allow, not AllowWithAudit, because the caller's
        // primary op gate already owns the allow-audit row.
        //
        // Order is deny-first, mirroring the path allow-list semantics exactly:
        //   1. kind in `probe_deny_kinds`  -> Deny (`probe_kind_denied`).
        //      DENY BEATS ALLOW.
        //   2. `probe_allow_kinds` non-empty AND kind not in it
        //      -> Deny (`no_allow_rule`).
        //   3. else (empty allow == not configured == allow, or kind present)
        //      -> Allow.
        // Matching is EXACT, case-sensitive equality against the snake_case
        // ProbeKind wire tags ({command, file_watch, pty}).
        if let PolicyAction::ProbeCreate { kind } = action {
            if self.probe_deny_kinds.iter().any(|k| k == kind) {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "probe kind '{kind}' is denied by [policy.probes] deny_kinds (probe_kind_denied)"
                    ),
                };
            }
            if !self.probe_allow_kinds.is_empty()
                && !self.probe_allow_kinds.iter().any(|k| k == kind)
            {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "probe kind '{kind}' is not in the [policy.probes] allow_kinds (no_allow_rule)"
                    ),
                };
            }
            return PolicyVerdict {
                decision: PolicyDecision::Allow,
                reason: format!("probe kind '{kind}' permitted"),
            };
        }

        // Per-profile policy.
        match self.profile {
            PolicyProfile::ReadOnlyObserver => {
                if matches!(
                    action,
                    PolicyAction::CommandStart { .. }
                        | PolicyAction::CommandStdin
                        | PolicyAction::CommandSignal
                        | PolicyAction::RegistryCreate
                        | PolicyAction::RegistryActivate
                        // FileWrite is a filesystem MUTATION: the read-only
                        // observer denies it alongside command_* / registry_*.
                        | PolicyAction::FileWrite { .. }
                ) {
                    PolicyVerdict {
                        decision: PolicyDecision::Deny,
                        reason:
                            "read_only_observer denies command_*, registry_*, and file_write mutations"
                                .to_owned(),
                    }
                } else {
                    PolicyVerdict {
                        decision: PolicyDecision::Allow,
                        reason: "read-only operation allowed".to_owned(),
                    }
                }
            }
            PolicyProfile::AdminDebug => {
                // Admin debug is operator-only; the MCP client must
                // never see this profile. We still deny mutations.
                if matches!(
                    action,
                    PolicyAction::RegistryCreate | PolicyAction::RegistryActivate
                ) {
                    PolicyVerdict {
                        decision: PolicyDecision::Deny,
                        reason: "admin_debug is inspect-only; registry mutations denied".to_owned(),
                    }
                } else {
                    PolicyVerdict {
                        decision: PolicyDecision::Allow,
                        reason: "admin_debug allowed".to_owned(),
                    }
                }
            }
            PolicyProfile::RepoOnly => {
                // TC22 Phase 1: repo_only confines path/cwd-bearing
                // actions to $REPO_ROOT (POLICY.md section 2.2). A
                // path/cwd outside the root -> deny. Actions without a
                // path/cwd subject (bucket reads, event_context, etc.)
                // fall through to the shared dev_local/repo_only verdict.
                if let Some(subject) = action_path_subject(action)
                    && !self.repo_root_contains(subject)
                {
                    let reason = self.repo_root.as_ref().map_or_else(
                        || {
                            format!(
                                "repo_only has no configured $REPO_ROOT; '{}' cannot be \
                                 confirmed inside the repo (fail-safe deny)",
                                subject.display()
                            )
                        },
                        |root| {
                            format!(
                                "repo_only confines to $REPO_ROOT '{}'; '{}' is outside it",
                                root.display(),
                                subject.display()
                            )
                        },
                    );
                    return PolicyVerdict {
                        decision: PolicyDecision::Deny,
                        reason,
                    };
                }
                self.dev_local_repo_only_verdict(action)
            }
            // FullAccess is exec-capable like developer_local. Its only added
            // power comes from caps (preset all-true by the loader), which are
            // evaluated above (shell) and via the same shared verdict here --
            // never an `evaluate()` bypass.
            PolicyProfile::DeveloperLocal | PolicyProfile::FullAccess => {
                self.dev_local_repo_only_verdict(action)
            }
        }
    }

    /// Shared allow/audit verdict for the two exec-capable profiles,
    /// applied AFTER repo_only's containment gate. TC22 Phase 2: enforces
    /// the command allow-list (default-deny posture) for `CommandStart`.
    fn dev_local_repo_only_verdict(&self, action: &PolicyAction<'_>) -> PolicyVerdict {
        if let PolicyAction::CommandStart { argv, .. } = action {
            let basename = argv
                .first()
                .map(|a0| {
                    Path::new(a0.as_str())
                        .file_name()
                        .and_then(|os| os.to_str())
                        .unwrap_or(a0.as_str())
                        .to_owned()
                })
                .unwrap_or_default();
            if !self.command_allowed(&basename) {
                return PolicyVerdict {
                    decision: PolicyDecision::Deny,
                    reason: format!(
                        "command '{basename}' is not in the {:?} allow-list (no_allow_rule)",
                        self.profile
                    ),
                };
            }
        }
        if matches!(action, PolicyAction::RegistryActivate) {
            PolicyVerdict {
                decision: PolicyDecision::AllowWithAudit,
                reason: "registry activate requires audit emission".to_owned(),
            }
        } else {
            PolicyVerdict {
                decision: PolicyDecision::Allow,
                reason: format!("{:?} allows the action", self.profile),
            }
        }
    }

    /// Is `basename` permitted to execute under the active exec profile?
    ///
    /// Default-deny is OPT-IN: when the operator configures a non-empty
    /// `command_allow_roots`, it is authoritative and anything off it is
    /// denied (`no_allow_rule`) — for BOTH `developer_local` and
    /// `repo_only`. With NO configured list, both profiles allow any
    /// command that survives the cross-profile structural deny set (and,
    /// for `repo_only`, the path/cwd containment gate). This keeps
    /// zero-config Terminal Commander usable for its core job (running
    /// and combing arbitrary dev commands); tightening to an allow-list
    /// is an explicit operator choice. POLICY.md section 2.2 specifies
    /// `repo_only` uses "the same allow-list as developer_local", so the
    /// two share this command posture; `repo_only`'s distinct safety
    /// property is containment, not command denial.
    fn command_allowed(&self, basename: &str) -> bool {
        self.command_allow_roots
            .as_ref()
            .is_none_or(|roots| roots.iter().any(|r| r == basename))
    }

    /// True if `candidate` is inside the configured repo-root. Returns
    /// `false` when no root is configured (fail-safe: an unrooted
    /// `repo_only` engine cannot prove containment).
    ///
    /// The candidate is canonicalized before comparison so the prefix
    /// form matches `repo_root` (itself canonicalized at construction).
    /// A subject path often does not exist yet (e.g. a file about to be
    /// created), and `canonicalize` requires the path to exist; so we
    /// canonicalize the NEAREST EXISTING ANCESTOR and re-append the
    /// non-existent remainder. This keeps both sides in the same form
    /// (critical on Windows, where `canonicalize` returns a `\\?\`
    /// verbatim prefix that a raw path lacks), while still rejecting
    /// `..` escapes because the existing-ancestor canonical form
    /// collapses them.
    fn repo_root_contains(&self, candidate: &Path) -> bool {
        let Some(root) = self.repo_root.as_ref() else {
            return false;
        };
        canonicalize_lexical(candidate).starts_with(root)
    }

    fn path_default_denied(path: &Path) -> bool {
        let s = path.to_string_lossy();
        DEFAULT_DENY_PATH_SUFFIXES
            .iter()
            .any(|suf| s.ends_with(suf))
    }
}

/// Canonicalize a path for containment comparison, tolerating paths that
/// do not exist yet. Canonicalizes the nearest existing ancestor and
/// re-appends the non-existent tail, so the result shares `canonicalize`'s
/// form (notably the Windows `\\?\` verbatim prefix) with the repo-root.
/// `..` components in the existing portion are collapsed by the real
/// canonicalize, so a `repo/../../etc` style escape still resolves
/// outside the root and is rejected by the caller's `starts_with`.
fn canonicalize_lexical(candidate: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(candidate) {
        return c;
    }
    // Walk up to the nearest ancestor that exists and canonicalizes.
    let mut existing = candidate;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    while let Some(parent) = existing.parent() {
        if let Some(name) = existing.file_name() {
            tail.push(name);
        }
        match std::fs::canonicalize(parent) {
            Ok(base) => {
                let mut out = base;
                for name in tail.iter().rev() {
                    out.push(name);
                }
                return out;
            }
            Err(_) => existing = parent,
        }
    }
    // No ancestor canonicalizes (e.g. a bare relative path with no
    // existing root): fall back to the lexical form.
    candidate.to_path_buf()
}

/// Translate one profile path-glob into an ANCHORED regex source string.
///
/// Glob grammar (POLICY.md section 4 schema examples, e.g.
/// `/home/me/projects/**`, `/home/me/projects/**/target/**`):
/// - `**` matches any run of characters INCLUDING `/` (cross-segment) -> `.*`
/// - `*`  matches any run of characters EXCEPT `/` (single segment) -> `[^/]*`
/// - `?`  matches exactly one non-separator character -> `[^/]`
/// - every other character is a literal; regex metacharacters are escaped.
///
/// The result is anchored (`^...$`) so a glob matches the WHOLE path, never a
/// substring -- `/proj` must not match `/project/secret`. Because `**` -> `.*`
/// is greedy across separators, `/proj/**` matches `/proj/a/b/c`.
///
/// Returns the regex SOURCE; the caller compiles it. A compile failure is the
/// caller's fail-safe signal (a profile with an uncompilable glob denies, never
/// allow-all).
fn glob_to_regex_src(glob: &str) -> String {
    let mut out = String::with_capacity(glob.len() + 2);
    out.push('^');
    let bytes = glob.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'*' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    // `**` -> any chars incl. separators.
                    out.push_str(".*");
                    i += 2;
                } else {
                    // single `*` -> any chars except a separator.
                    out.push_str("[^/]*");
                    i += 1;
                }
            }
            b'?' => {
                out.push_str("[^/]");
                i += 1;
            }
            other => {
                // Escape one byte as a literal. `regex::escape` works on a
                // &str; build a one-char string from the (UTF-8 safe) char at
                // this index. Globs are paths, so iterate by char to stay
                // UTF-8 correct for multibyte literals.
                let ch = glob[i..].chars().next().unwrap_or(other as char);
                out.push_str(&regex::escape(&ch.to_string()));
                i += ch.len_utf8();
            }
        }
    }
    out.push('$');
    out
}

/// A compiled profile path allow/deny list (POLICY.md section 5).
///
/// Holds the configured glob count and the anchored regexes compiled ONCE at
/// engine construction (off the `evaluate` hot path). Empty == "not
/// configured": for an allow-list this means ALLOW (zero-config stays usable);
/// for a deny-list this means no extra denies. A glob that fails to compile is
/// dropped from the matcher AND flips `had_compile_error`, so an allow-list
/// with a broken glob fails SAFE: it can no longer satisfy a path it was
/// supposed to (see [`GlobList::allows`]).
#[derive(Debug, Clone, Default)]
struct GlobList {
    /// Number of globs the operator configured (before compile filtering).
    configured: usize,
    /// Successfully compiled anchored regexes.
    regexes: Vec<regex::Regex>,
    /// True if ANY configured glob failed to compile.
    had_compile_error: bool,
}

impl GlobList {
    /// Compile a raw glob list once. Globs that fail to compile are dropped
    /// and recorded in `had_compile_error` (fail-safe; never panics). A failure
    /// is SURFACED via `tracing::warn!` so an operator sees a bad glob at
    /// bootstrap rather than silently losing it.
    fn compile(globs: &[String]) -> Self {
        let configured = globs.len();
        let mut regexes = Vec::with_capacity(configured);
        let mut had_compile_error = false;
        for g in globs {
            match regex::Regex::new(&glob_to_regex_src(g)) {
                Ok(re) => regexes.push(re),
                Err(err) => {
                    had_compile_error = true;
                    tracing::warn!(
                        glob = %g,
                        error = %err,
                        "policy.paths glob failed to compile; dropped (fail-safe deny)"
                    );
                }
            }
        }
        let list = Self {
            configured,
            regexes,
            had_compile_error,
        };
        if list.had_compile_error {
            tracing::warn!(
                configured,
                compiled = list.regexes.len(),
                "policy.paths list has uncompilable globs; the list now fails safe \
                 (an allow-list with a broken glob can no longer match those paths)"
            );
        }
        list
    }

    /// Is this list configured (non-empty as authored by the operator)?
    /// An unconfigured allow-list is "not enforced" (zero-config allow).
    const fn is_configured(&self) -> bool {
        self.configured > 0
    }

    /// Does `path` match ANY glob in this list?
    fn matches(&self, path: &Path) -> bool {
        let s = path.to_string_lossy();
        self.regexes.iter().any(|re| re.is_match(&s))
    }

    /// Allow-list verdict for `path`:
    /// - unconfigured (empty) -> `true` (not enforced; zero-config allow);
    /// - configured + at least one glob matches -> `true`;
    /// - configured but no glob matches -> `false` (`no_allow_rule`).
    ///
    /// FAIL-SAFE: if the list was configured but a glob failed to compile and
    /// nothing matched, this returns `false` -- a broken allow-list never
    /// widens to allow-all.
    fn allows(&self, path: &Path) -> bool {
        if !self.is_configured() {
            return true;
        }
        self.matches(path)
    }
}

/// Extract the filesystem subject (path or cwd) an action operates on,
/// for containment checks. Returns `None` for actions with no path/cwd
/// subject (bucket/event/registry actions).
const fn action_path_subject<'a>(action: &'a PolicyAction<'a>) -> Option<&'a Path> {
    match action {
        PolicyAction::CommandStart { cwd, .. } => Some(cwd),
        PolicyAction::FileRead { path }
        | PolicyAction::FileWatch { path }
        | PolicyAction::FileWrite { path } => Some(path),
        // Shell-lane and session-lane starts are gated by an early return in
        // `evaluate` (they never reach the repo_only containment check), so they
        // have no path subject here. Arms present only to keep the match
        // exhaustive.
        PolicyAction::CommandShellStart { .. }
        | PolicyAction::SessionStart { .. }
        | PolicyAction::CommandStdin
        | PolicyAction::CommandSignal
        | PolicyAction::ProbeCreate { .. }
        | PolicyAction::RegistryCreate
        | PolicyAction::RegistryActivate
        | PolicyAction::BucketWait
        | PolicyAction::BucketRead
        | PolicyAction::EventContext => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::path::PathBuf;

    #[test]
    fn sudo_denied_in_every_profile() {
        for prof in [
            PolicyProfile::DeveloperLocal,
            PolicyProfile::RepoOnly,
            PolicyProfile::ReadOnlyObserver,
            PolicyProfile::AdminDebug,
        ] {
            let e = PolicyEngine::new(prof);
            let argv = vec!["sudo".to_owned(), "apt".to_owned(), "update".to_owned()];
            let cwd = PathBuf::from("/home/dev");
            let v = e.evaluate(&PolicyAction::CommandStart {
                argv: &argv,
                cwd: &cwd,
            });
            assert_eq!(v.decision, PolicyDecision::Deny, "{prof:?}");
        }
    }

    // Unix absolute-path + glob policy semantics. On Windows, canonicalize_lexical
    // resolves these synthetic non-existent paths against the real FS (drive root +
    // \\?\ verbatim prefix, backslashes) so the Unix glob regexes miss. The daemon's
    // production target is Linux/WSL (Windows-native daemon deferred, ARCHITECTURE.md
    // section 10), so these are Unix-only tests. A future Windows-native daemon would
    // ALSO need the policy glob compilation to account for the \\?\ canonical prefix
    // (deferred capability, not a current prod path).
    #[cfg(unix)]
    #[test]
    fn default_deny_path_denied() {
        let e = PolicyEngine::default_engine();
        for s in [
            "/home/dev/.ssh/id_rsa",
            "/etc/shadow",
            "/home/dev/.aws/credentials",
            "/home/dev/.kube/config",
        ] {
            let p = PathBuf::from(s);
            let v = e.evaluate(&PolicyAction::FileRead { path: &p });
            assert_eq!(v.decision, PolicyDecision::Deny, "{s}");
        }
    }

    #[test]
    fn read_only_observer_denies_command_start() {
        let e = PolicyEngine::new(PolicyProfile::ReadOnlyObserver);
        let argv = vec!["cargo".to_owned(), "build".to_owned()];
        let cwd = PathBuf::from(".");
        let v = e.evaluate(&PolicyAction::CommandStart {
            argv: &argv,
            cwd: &cwd,
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn shell_start_denied_by_default() {
        // developer_local is exec-capable, but caps default all-false, so the
        // shell lane is denied with no explicit opt-in.
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal);
        let v = e.evaluate(&PolicyAction::CommandShellStart {
            shell_line: "echo a | wc -c",
            cwd: Path::new("."),
            shell: "/bin/bash",
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
        assert!(
            v.reason.contains("shell execution denied"),
            "policy reason must be surface-neutral, got: {}",
            v.reason
        );
    }

    #[test]
    fn shell_start_allowed_with_audit_when_cap_on() {
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::DeveloperLocal,
            None,
            None,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::CommandShellStart {
            shell_line: "echo a | wc -c",
            cwd: Path::new("."),
            shell: "/bin/bash",
        });
        assert_eq!(v.decision, PolicyDecision::AllowWithAudit);
    }

    #[test]
    fn shell_start_denied_in_repo_only_even_with_cap() {
        // repo_only is NOT exec-capable for the shell lane: even with the cap on,
        // the early-return shell gate denies because the profile forbids shell.
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::RepoOnly,
            None,
            None,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::CommandShellStart {
            shell_line: "ls",
            cwd: Path::new("."),
            shell: "/bin/bash",
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn shell_start_denied_in_read_only_observer_even_with_cap() {
        // read_only_observer is the strictest profile: the shell lane is denied
        // even with allow_shell explicitly on (profile forbids shell).
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::ReadOnlyObserver,
            None,
            None,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::CommandShellStart {
            shell_line: "echo a | wc -c",
            cwd: Path::new("."),
            shell: "/bin/bash",
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn session_start_denied_by_default() {
        // developer_local is exec-capable, but caps default all-false, so the
        // session lane is denied with no explicit opt-in.
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal);
        let v = e.evaluate(&PolicyAction::SessionStart {
            shell: "/bin/bash",
            cwd: Path::new("."),
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn session_start_allowed_with_audit_when_cap_on() {
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::DeveloperLocal,
            None,
            None,
            PolicyCaps {
                allow_session: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::SessionStart {
            shell: "/bin/bash",
            cwd: Path::new("."),
        });
        assert_eq!(v.decision, PolicyDecision::AllowWithAudit);
    }

    #[test]
    fn session_start_denied_in_repo_only_even_with_cap() {
        // repo_only is NOT exec-capable for the session lane: even with the cap
        // on, the early-return session gate denies (profile forbids sessions).
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::RepoOnly,
            None,
            None,
            PolicyCaps {
                allow_session: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::SessionStart {
            shell: "/bin/bash",
            cwd: Path::new("."),
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn session_start_denied_in_read_only_observer_even_with_cap() {
        // read_only_observer is the strictest profile: the session lane is
        // denied even with allow_session explicitly on.
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::ReadOnlyObserver,
            None,
            None,
            PolicyCaps {
                allow_session: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::SessionStart {
            shell: "/bin/bash",
            cwd: Path::new("."),
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn session_cap_independent_of_shell_cap() {
        // allow_shell on but allow_session off -> session denied (caps are
        // independent opt-ins, not a shared exec switch).
        let e = PolicyEngine::with_config_caps(
            PolicyProfile::DeveloperLocal,
            None,
            None,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let v = e.evaluate(&PolicyAction::SessionStart {
            shell: "/bin/bash",
            cwd: Path::new("."),
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn developer_local_allows_normal_command() {
        let e = PolicyEngine::default_engine();
        let argv = vec!["cargo".to_owned(), "test".to_owned()];
        let cwd = PathBuf::from(".");
        let v = e.evaluate(&PolicyAction::CommandStart {
            argv: &argv,
            cwd: &cwd,
        });
        assert_eq!(v.decision, PolicyDecision::Allow);
    }

    #[test]
    fn developer_local_registry_activate_requires_audit() {
        let e = PolicyEngine::default_engine();
        let v = e.evaluate(&PolicyAction::RegistryActivate);
        assert_eq!(v.decision, PolicyDecision::AllowWithAudit);
    }

    #[test]
    fn admin_debug_denies_registry_create() {
        let e = PolicyEngine::new(PolicyProfile::AdminDebug);
        let v = e.evaluate(&PolicyAction::RegistryCreate);
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn file_read_allowed_when_not_in_default_deny() {
        let e = PolicyEngine::default_engine();
        let p = Path::new("/home/dev/repo/src/main.rs");
        let v = e.evaluate(&PolicyAction::FileRead { path: p });
        assert_eq!(v.decision, PolicyDecision::Allow);
    }

    // --- TC22 Phase 1: repo_only containment (AC1/AC2) ---

    #[test]
    fn repo_only_denies_file_read_outside_root_allows_inside() {
        // Real dirs so canonicalize() resolves on every platform.
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_repo_root(PolicyProfile::RepoOnly, repo.path().to_path_buf());

        let inside = repo.path().join("src/main.rs");
        let v_in = e.evaluate(&PolicyAction::FileRead { path: &inside });
        assert_eq!(
            v_in.decision,
            PolicyDecision::Allow,
            "in-repo read must be allowed: {}",
            v_in.reason
        );

        let out = outside.path().join("secret.txt");
        let v_out = e.evaluate(&PolicyAction::FileRead { path: &out });
        assert_eq!(
            v_out.decision,
            PolicyDecision::Deny,
            "out-of-repo read must be denied"
        );
    }

    #[test]
    fn repo_only_denies_command_with_cwd_outside_root() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_repo_root(PolicyProfile::RepoOnly, repo.path().to_path_buf());
        let argv = vec!["cargo".to_owned(), "build".to_owned()];

        let v_in = e.evaluate(&PolicyAction::CommandStart {
            argv: &argv,
            cwd: repo.path(),
        });
        assert_eq!(v_in.decision, PolicyDecision::Allow, "{}", v_in.reason);

        let v_out = e.evaluate(&PolicyAction::CommandStart {
            argv: &argv,
            cwd: outside.path(),
        });
        assert_eq!(
            v_out.decision,
            PolicyDecision::Deny,
            "command with cwd outside repo_root must be denied"
        );
    }

    #[test]
    fn repo_only_without_root_fails_safe_deny() {
        // An unrooted repo_only engine cannot prove containment, so any
        // path/cwd action is denied (fail-safe).
        let e = PolicyEngine::new(PolicyProfile::RepoOnly);
        let p = Path::new("/anywhere/file.txt");
        let v = e.evaluate(&PolicyAction::FileRead { path: p });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn repo_only_still_denies_cross_profile_sudo() {
        // Containment must not bypass the structural deny set.
        let repo = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_repo_root(PolicyProfile::RepoOnly, repo.path().to_path_buf());
        let argv = vec!["sudo".to_owned()];
        let v = e.evaluate(&PolicyAction::CommandStart {
            argv: &argv,
            cwd: repo.path(),
        });
        assert_eq!(v.decision, PolicyDecision::Deny);
    }

    #[test]
    fn repo_only_allows_non_path_actions() {
        // Bucket/event actions have no path subject; containment must
        // not block them.
        let repo = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_repo_root(PolicyProfile::RepoOnly, repo.path().to_path_buf());
        assert_eq!(
            e.evaluate(&PolicyAction::BucketRead).decision,
            PolicyDecision::Allow
        );
        assert_eq!(
            e.evaluate(&PolicyAction::RegistryActivate).decision,
            PolicyDecision::AllowWithAudit
        );
    }

    // --- TC22 Phase 2: command allow-list (AC3/AC4) ---

    #[test]
    fn developer_local_no_list_allows_any_non_deny_command() {
        // Zero-config developer_local: default-deny is opt-in, so any
        // command surviving the structural deny set is allowed.
        let e = PolicyEngine::default_engine();
        let cwd = PathBuf::from(".");
        for cmd in ["echo", "python", "node", "rm", "cargo", "some-obscure-tool"] {
            let argv = vec![cmd.to_owned()];
            let v = e.evaluate(&PolicyAction::CommandStart {
                argv: &argv,
                cwd: &cwd,
            });
            assert_eq!(v.decision, PolicyDecision::Allow, "{cmd} should be allowed");
        }
    }

    #[test]
    fn developer_local_with_list_denies_off_list_allows_on_list() {
        // Operator opts in to default-deny via allow_roots.
        let e = PolicyEngine::with_config(
            PolicyProfile::DeveloperLocal,
            None,
            Some(vec!["cargo".to_owned(), "git".to_owned()]),
        );
        let cwd = PathBuf::from(".");

        let on = vec!["cargo".to_owned(), "build".to_owned()];
        assert_eq!(
            e.evaluate(&PolicyAction::CommandStart {
                argv: &on,
                cwd: &cwd
            })
            .decision,
            PolicyDecision::Allow
        );

        let off = vec!["rm".to_owned(), "-rf".to_owned()];
        let v = e.evaluate(&PolicyAction::CommandStart {
            argv: &off,
            cwd: &cwd,
        });
        assert_eq!(v.decision, PolicyDecision::Deny, "rm off-list must deny");
        assert!(
            v.reason.contains("no_allow_rule"),
            "deny reason should carry no_allow_rule: {}",
            v.reason
        );
    }

    #[test]
    fn allow_list_matches_by_basename_not_full_path() {
        let e = PolicyEngine::with_config(
            PolicyProfile::DeveloperLocal,
            None,
            Some(vec!["cargo".to_owned()]),
        );
        let cwd = PathBuf::from(".");
        let argv = vec!["/usr/local/bin/cargo".to_owned(), "test".to_owned()];
        assert_eq!(
            e.evaluate(&PolicyAction::CommandStart {
                argv: &argv,
                cwd: &cwd
            })
            .decision,
            PolicyDecision::Allow,
            "absolute-path cargo should match the 'cargo' basename allow entry"
        );
    }

    #[test]
    fn repo_only_with_list_enforces_both_containment_and_allow_list() {
        // AC4-adjacent: repo_only honors the same allow-list as
        // developer_local AND adds containment.
        let repo = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_config(
            PolicyProfile::RepoOnly,
            Some(repo.path().to_path_buf()),
            Some(vec!["cargo".to_owned()]),
        );

        // on-list + in-repo -> allow
        let on = vec!["cargo".to_owned()];
        assert_eq!(
            e.evaluate(&PolicyAction::CommandStart {
                argv: &on,
                cwd: repo.path()
            })
            .decision,
            PolicyDecision::Allow
        );
        // off-list + in-repo -> deny (allow-list)
        let off = vec!["rm".to_owned()];
        assert_eq!(
            e.evaluate(&PolicyAction::CommandStart {
                argv: &off,
                cwd: repo.path()
            })
            .decision,
            PolicyDecision::Deny
        );
        // on-list + outside-repo -> deny (containment wins first)
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(
            e.evaluate(&PolicyAction::CommandStart {
                argv: &on,
                cwd: outside.path()
            })
            .decision,
            PolicyDecision::Deny
        );
    }

    #[test]
    fn repo_only_no_list_allows_in_repo_command() {
        // Confirms the resolved posture: repo_only with no allow_roots
        // behaves like developer_local (allow-any) but contained.
        let repo = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_config(
            PolicyProfile::RepoOnly,
            Some(repo.path().to_path_buf()),
            None,
        );
        let argv = vec!["echo".to_owned()];
        assert_eq!(
            e.evaluate(&PolicyAction::CommandStart {
                argv: &argv,
                cwd: repo.path()
            })
            .decision,
            PolicyDecision::Allow
        );
    }

    // --- TC22 A1: [policy.paths] allow/deny enforcement (POLICY.md s5/s6.2e) ---

    fn to_glob_vec(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()
    }

    /// Build a developer_local engine carrying the given read/watch/deny path
    /// globs (write_allow left empty). Uses developer_local (NOT repo_only) so
    /// the path verdict comes purely from the [policy.paths] layer, with no
    /// repo_root containment in play.
    fn paths_engine(read: &[&str], watch: &[&str], deny: &[&str]) -> PolicyEngine {
        PolicyEngine::new(PolicyProfile::DeveloperLocal).with_paths(
            &to_glob_vec(read),
            &to_glob_vec(watch),
            &[],
            &to_glob_vec(deny),
        )
    }

    /// Build a developer_local engine carrying the given write_allow / deny_extra
    /// globs (read/watch left empty), for the TC22 A3 FileWrite tests. Mirrors
    /// `paths_engine` so the write verdict comes purely from the write layer.
    fn write_paths_engine(write: &[&str], deny: &[&str]) -> PolicyEngine {
        PolicyEngine::new(PolicyProfile::DeveloperLocal).with_paths(
            &[],
            &[],
            &to_glob_vec(write),
            &to_glob_vec(deny),
        )
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn read_allow_nonempty_allows_match_denies_miss() {
        // A1: read_allow non-empty -> path matching a glob Allow; miss Deny
        // with reason no_allow_rule.
        let e = paths_engine(&["/home/me/projects/**"], &[], &[]);

        let hit = PathBuf::from("/home/me/projects/app/src/main.rs");
        let v_hit = e.evaluate(&PolicyAction::FileRead { path: &hit });
        assert_eq!(
            v_hit.decision,
            PolicyDecision::Allow,
            "in-allow-list read must allow: {}",
            v_hit.reason
        );

        let miss = PathBuf::from("/var/log/syslog");
        let v_miss = e.evaluate(&PolicyAction::FileRead { path: &miss });
        assert_eq!(v_miss.decision, PolicyDecision::Deny, "off-allow-list miss");
        assert!(
            v_miss.reason.contains("no_allow_rule"),
            "miss reason must carry no_allow_rule: {}",
            v_miss.reason
        );
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn watch_allow_nonempty_allows_match_denies_miss() {
        // A1: watch_allow governs FileWatch the same way read_allow governs
        // FileRead. read_allow is empty here, proving the two lists are
        // independent: an empty read_allow must not satisfy a FileWatch.
        let e = paths_engine(&[], &["/srv/repos/**"], &[]);

        let hit = PathBuf::from("/srv/repos/team/log.txt");
        let v_hit = e.evaluate(&PolicyAction::FileWatch { path: &hit });
        assert_eq!(
            v_hit.decision,
            PolicyDecision::Allow,
            "in-watch-list watch must allow: {}",
            v_hit.reason
        );

        let miss = PathBuf::from("/home/me/elsewhere/file.txt");
        let v_miss = e.evaluate(&PolicyAction::FileWatch { path: &miss });
        assert_eq!(v_miss.decision, PolicyDecision::Deny, "off-watch-list miss");
        assert!(
            v_miss.reason.contains("no_allow_rule"),
            "miss reason must carry no_allow_rule: {}",
            v_miss.reason
        );
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn deny_extra_beats_allow_list() {
        // A1: deny_extra is a hard deny layered onto default-deny. A path that
        // WOULD satisfy read_allow is still denied if it matches deny_extra.
        let e = paths_engine(
            &["/home/me/projects/**"],
            &[],
            &["/home/me/projects/**/target/**"],
        );

        // matches read_allow but NOT deny_extra -> allow
        let ok = PathBuf::from("/home/me/projects/app/src/main.rs");
        assert_eq!(
            e.evaluate(&PolicyAction::FileRead { path: &ok }).decision,
            PolicyDecision::Allow
        );

        // matches BOTH read_allow and deny_extra -> deny (deny beats allow)
        let denied = PathBuf::from("/home/me/projects/app/target/debug/bin");
        let v = e.evaluate(&PolicyAction::FileRead { path: &denied });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "deny_extra must beat the allow-list"
        );
        assert!(
            v.reason.contains("default_deny_match"),
            "deny_extra reason must carry default_deny_match: {}",
            v.reason
        );
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn deny_extra_applies_to_watch_too() {
        // deny_extra is layered for FileWatch as well as FileRead.
        let e = paths_engine(&[], &["/srv/**"], &["/srv/secret/**"]);
        let denied = PathBuf::from("/srv/secret/key.pem");
        let v = e.evaluate(&PolicyAction::FileWatch { path: &denied });
        assert_eq!(v.decision, PolicyDecision::Deny);
        assert!(v.reason.contains("default_deny_match"), "{}", v.reason);
    }

    #[test]
    fn empty_allow_lists_preserve_zero_config_allow() {
        // Regression guard: unconfigured (empty) read_allow / watch_allow are
        // NOT enforced, so existing zero-config file reads/watches still pass.
        let e = paths_engine(&[], &[], &[]);
        let p = PathBuf::from("/anywhere/at/all/file.rs");
        assert_eq!(
            e.evaluate(&PolicyAction::FileRead { path: &p }).decision,
            PolicyDecision::Allow,
            "empty read_allow must allow"
        );
        assert_eq!(
            e.evaluate(&PolicyAction::FileWatch { path: &p }).decision,
            PolicyDecision::Allow,
            "empty watch_allow must allow"
        );
    }

    #[test]
    fn default_deny_suffix_still_denies_under_path_lists() {
        // The structural default-deny suffix check runs BEFORE the allow-list,
        // so a sensitive path is denied even with a permissive read_allow.
        let e = paths_engine(&["/home/dev/**"], &[], &[]);
        let p = PathBuf::from("/home/dev/.ssh/id_rsa");
        assert_eq!(
            e.evaluate(&PolicyAction::FileRead { path: &p }).decision,
            PolicyDecision::Deny,
            "default-deny suffix must still deny"
        );
    }

    #[test]
    fn allow_list_does_not_widen_repo_only_containment() {
        // repo_only containment runs AFTER the path-list layer. A read_allow
        // glob that matches an OUTSIDE path must NOT let it past containment:
        // the allow-list is a tightening layer, never a widening one.
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        // A maximally-permissive read_allow (`**` matches every path on any
        // platform) so the allow-list definitely SATISFIES the outside path;
        // any deny must then come purely from repo_only containment.
        let e = PolicyEngine::with_repo_root(PolicyProfile::RepoOnly, repo.path().to_path_buf())
            .with_paths(&["**".to_owned()], &[], &[], &[]);
        let out = outside.path().join("secret.txt");
        let v = e.evaluate(&PolicyAction::FileRead { path: &out });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "containment must still deny an outside path the allow-list matched: {}",
            v.reason
        );
    }

    // --- TC22 A1: SELF-CONTAINED guard against `..` traversal -----------
    //
    // These tests call `evaluate` DIRECTLY (NOT via the canonicalizing IPC
    // handler) to prove the allow-list / deny_extra guard normalizes the
    // subject itself. The marker-segment glob `**/<marker>/**` matches any
    // path that passes THROUGH the marked directory, regardless of
    // separator style (`**` spans `/`). A `..`-prefixed raw path lexically
    // contains the marker segment, so it WOULD satisfy the glob if matched
    // raw -- but canonicalization collapses the `..` so the resolved path
    // leaves the marked tree and is correctly denied.
    //
    // RED->GREEN: revert FIX 1 (match the raw `path` instead of
    // `canonicalize_lexical(path)`) and `traversal_*` flip to Allow / miss
    // the deny -- documented in the task evidence.

    /// Build a real on-disk layout under a temp dir and return:
    /// `(tempdir, marker, inside_file_path, traversal_path)` where
    /// `inside_file_path` lives under `<base>/<marker>/` and
    /// `traversal_path` is `<base>/<marker>/../escape.txt` (a real file that
    /// resolves to `<base>/escape.txt`, OUTSIDE the marked dir). The marker
    /// is unique per call so a `**/<marker>/**` glob is unambiguous.
    fn traversal_layout() -> (tempfile::TempDir, String, PathBuf, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let marker = format!("tc_allow_{}", SEQ.fetch_add(1, Ordering::Relaxed));
        let base = tempfile::tempdir().expect("tempdir");
        let allowed_dir = base.path().join(&marker);
        std::fs::create_dir(&allowed_dir).expect("create marked dir");
        let inside = allowed_dir.join("data.txt");
        std::fs::write(&inside, b"in\n").expect("write inside file");
        // A real file OUTSIDE the marked dir that the traversal resolves to.
        let escape = base.path().join("escape.txt");
        std::fs::write(&escape, b"out\n").expect("write escape file");
        // Raw, un-normalized traversal: passes lexically through the marker.
        let traversal = allowed_dir.join("..").join("escape.txt");
        (base, marker, inside, traversal)
    }

    #[test]
    fn traversal_read_allow_denies_dotdot_escape() {
        // developer_local (no repo_root): the only path layer in play is the
        // [policy.paths] allow-list. A `..` escape must be DENIED even though
        // the raw string passes through the allowed marker segment.
        let (_base, marker, _inside, traversal) = traversal_layout();
        let glob = format!("**/{marker}/**");
        let e = paths_engine(&[&glob], &[], &[]);
        let v = e.evaluate(&PolicyAction::FileRead { path: &traversal });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "`..` escape past read_allow must deny (self-contained guard): {}",
            v.reason
        );
        assert!(
            v.reason.contains("no_allow_rule"),
            "traversal miss must carry no_allow_rule: {}",
            v.reason
        );
    }

    #[test]
    fn traversal_watch_allow_denies_dotdot_escape() {
        // Same self-containment property for the FileWatch / watch_allow lane.
        let (_base, marker, _inside, traversal) = traversal_layout();
        let glob = format!("**/{marker}/**");
        let e = paths_engine(&[], &[&glob], &[]);
        let v = e.evaluate(&PolicyAction::FileWatch { path: &traversal });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "`..` escape past watch_allow must deny (self-contained guard): {}",
            v.reason
        );
        assert!(
            v.reason.contains("no_allow_rule"),
            "traversal miss must carry no_allow_rule: {}",
            v.reason
        );
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn traversal_deny_extra_matches_canonical_form() {
        // deny_extra must match the CANONICAL form, not the raw `..` string.
        // The allow-list is wide-open (`**`) so any verdict comes purely from
        // deny_extra. deny_extra = `**/<marker>/**` (the marked dir).
        let (base, marker, inside, traversal) = traversal_layout();
        let deny = format!("**/{marker}/**");
        let e = paths_engine(&["**"], &[], &[&deny]);

        // (a) A path canonically UNDER the marker is denied by deny_extra.
        let v_in = e.evaluate(&PolicyAction::FileRead { path: &inside });
        assert_eq!(
            v_in.decision,
            PolicyDecision::Deny,
            "deny_extra must deny a path canonically under the marker: {}",
            v_in.reason
        );
        assert!(
            v_in.reason.contains("default_deny_match"),
            "deny_extra reason must carry default_deny_match: {}",
            v_in.reason
        );

        // (b) A `..` raw path that LEXICALLY passes through the deny marker
        // but canonically escapes it (-> <base>/escape.txt) must NOT be
        // spuriously denied: deny_extra matches the canonical form, which no
        // longer contains the marker, so the wide-open `**` allow takes over.
        // (If FIX 1 matched raw, deny_extra would wrongly DENY this.)
        let v_escape = e.evaluate(&PolicyAction::FileRead { path: &traversal });
        assert_eq!(
            v_escape.decision,
            PolicyDecision::Allow,
            "deny_extra must match the CANONICAL form, not the raw `..` string: {}",
            v_escape.reason
        );
        drop(base);
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn traversal_in_allow_canonical_path_still_allows() {
        // No over-deny: a canonical path genuinely UNDER the allowed marker
        // (no `..`) is still ALLOWED after the canonicalization change.
        let (_base, marker, inside, _traversal) = traversal_layout();
        let glob = format!("**/{marker}/**");
        let e = paths_engine(&[&glob], &[], &[]);
        let v = e.evaluate(&PolicyAction::FileRead { path: &inside });
        assert_eq!(
            v.decision,
            PolicyDecision::Allow,
            "in-allow canonical path must still allow (no over-deny): {}",
            v.reason
        );
    }

    // --- TC22 A3: [policy.paths] write_allow enforcement (FileWrite) ---

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn write_allow_nonempty_allows_match_denies_miss() {
        // A3: write_allow non-empty -> a path matching a glob Allow; a miss
        // Deny with reason no_allow_rule. Mirrors read_allow exactly.
        let e = write_paths_engine(&["/srv/out/**"], &[]);

        let hit = PathBuf::from("/srv/out/app/result.txt");
        let v_hit = e.evaluate(&PolicyAction::FileWrite { path: &hit });
        assert_eq!(
            v_hit.decision,
            PolicyDecision::Allow,
            "in-write-allow write must allow: {}",
            v_hit.reason
        );

        let miss = PathBuf::from("/home/me/elsewhere/out.txt");
        let v_miss = e.evaluate(&PolicyAction::FileWrite { path: &miss });
        assert_eq!(
            v_miss.decision,
            PolicyDecision::Deny,
            "off-write-allow miss"
        );
        assert!(
            v_miss.reason.contains("no_allow_rule"),
            "write miss reason must carry no_allow_rule: {}",
            v_miss.reason
        );
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn write_allow_independent_from_read_allow() {
        // A3: write_allow is INDEPENDENT from read_allow. An engine with a
        // permissive read_allow but EMPTY write_allow must NOT let a read glob
        // satisfy a FileWrite -- but an empty write_allow is "not configured",
        // so it allows. Conversely a configured read_allow that the write path
        // misses must not deny the write. Prove the two lists do not bleed.
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal).with_paths(
            &["/only/reads/**".to_owned()],  // read_allow (configured, narrow)
            &[],                             // watch_allow
            &["/only/writes/**".to_owned()], // write_allow (configured, narrow)
            &[],                             // deny_extra
        );

        // A write under the WRITE list is allowed even though it misses the
        // read list (the read list governs FileRead only).
        let w = PathBuf::from("/only/writes/x.txt");
        assert_eq!(
            e.evaluate(&PolicyAction::FileWrite { path: &w }).decision,
            PolicyDecision::Allow,
            "write under write_allow must allow regardless of read_allow"
        );
        // A write under the READ list (but not the write list) is DENIED: the
        // read allow-list must not widen writes.
        let r = PathBuf::from("/only/reads/x.txt");
        let v = e.evaluate(&PolicyAction::FileWrite { path: &r });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "read_allow must not satisfy a FileWrite"
        );
        assert!(v.reason.contains("no_allow_rule"), "{}", v.reason);
    }

    // Unix path-glob semantics (see default_deny_path_denied).
    #[cfg(unix)]
    #[test]
    fn write_deny_extra_beats_write_allow() {
        // A3: deny_extra is a hard deny layered onto FileWrite too. A path that
        // WOULD satisfy write_allow is still denied if it matches deny_extra.
        let e = write_paths_engine(&["/srv/out/**"], &["/srv/out/**/secret/**"]);

        // matches write_allow but NOT deny_extra -> allow
        let ok = PathBuf::from("/srv/out/app/result.txt");
        assert_eq!(
            e.evaluate(&PolicyAction::FileWrite { path: &ok }).decision,
            PolicyDecision::Allow
        );

        // matches BOTH write_allow and deny_extra -> deny (deny beats allow)
        let denied = PathBuf::from("/srv/out/app/secret/key.txt");
        let v = e.evaluate(&PolicyAction::FileWrite { path: &denied });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "deny_extra must beat the write allow-list"
        );
        assert!(
            v.reason.contains("default_deny_match"),
            "write deny_extra reason must carry default_deny_match: {}",
            v.reason
        );
    }

    #[test]
    fn write_default_deny_suffix_still_denies() {
        // A3: the structural default-deny suffix check runs BEFORE the
        // write allow-list, so a sensitive path is denied for FileWrite even
        // with a permissive write_allow. Closes the "write a secret" hole.
        let e = write_paths_engine(&["/home/dev/**"], &[]);
        let p = PathBuf::from("/home/dev/.ssh/id_rsa");
        let v = e.evaluate(&PolicyAction::FileWrite { path: &p });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "default-deny suffix must still deny a write"
        );
    }

    #[test]
    fn write_empty_allow_preserves_zero_config_allow() {
        // Regression guard: an unconfigured (empty) write_allow is NOT
        // enforced, so a write to an ordinary path is allowed (opt-in posture
        // identical to read_allow / watch_allow).
        let e = write_paths_engine(&[], &[]);
        let p = PathBuf::from("/anywhere/at/all/out.txt");
        assert_eq!(
            e.evaluate(&PolicyAction::FileWrite { path: &p }).decision,
            PolicyDecision::Allow,
            "empty write_allow must allow"
        );
    }

    #[test]
    fn read_only_observer_denies_file_write() {
        // A3: FileWrite is a MUTATION; read_only_observer must DENY it.
        let e = PolicyEngine::new(PolicyProfile::ReadOnlyObserver);
        let p = Path::new("/home/dev/out.txt");
        let v = e.evaluate(&PolicyAction::FileWrite { path: p });
        assert_eq!(v.decision, PolicyDecision::Deny);
        assert!(
            v.reason.contains("file_write"),
            "deny reason should name file_write: {}",
            v.reason
        );
        // And a FileRead in the same profile is still allowed (we denied the
        // mutation, not the whole file lane).
        assert_eq!(
            e.evaluate(&PolicyAction::FileRead { path: p }).decision,
            PolicyDecision::Allow
        );
    }

    #[test]
    fn repo_only_denies_write_outside_root_allows_inside() {
        // A3: repo_only containment applies to FileWrite via
        // action_path_subject. A write outside $REPO_ROOT is denied; inside
        // is allowed.
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let e = PolicyEngine::with_repo_root(PolicyProfile::RepoOnly, repo.path().to_path_buf());

        let inside = repo.path().join("out/result.txt");
        assert_eq!(
            e.evaluate(&PolicyAction::FileWrite { path: &inside })
                .decision,
            PolicyDecision::Allow,
            "in-repo write must be allowed"
        );

        let out = outside.path().join("escape.txt");
        assert_eq!(
            e.evaluate(&PolicyAction::FileWrite { path: &out }).decision,
            PolicyDecision::Deny,
            "out-of-repo write must be denied"
        );
    }

    #[test]
    fn traversal_write_allow_denies_dotdot_escape() {
        // A3: a `..` escape past write_allow must be DENIED even though the raw
        // string passes through the allowed marker segment (self-contained
        // canonicalization guard, mirrors the read/watch traversal tests).
        let (_base, marker, _inside, traversal) = traversal_layout();
        let glob = format!("**/{marker}/**");
        let e = write_paths_engine(&[&glob], &[]);
        let v = e.evaluate(&PolicyAction::FileWrite { path: &traversal });
        assert_eq!(
            v.decision,
            PolicyDecision::Deny,
            "`..` escape past write_allow must deny (self-contained guard): {}",
            v.reason
        );
        assert!(
            v.reason.contains("no_allow_rule"),
            "traversal write miss must carry no_allow_rule: {}",
            v.reason
        );
    }

    // --- TC22 A1: glob -> regex translator unit tests ---

    #[test]
    fn glob_double_star_matches_across_separators() {
        let re = regex::Regex::new(&glob_to_regex_src("/proj/**")).unwrap();
        assert!(re.is_match("/proj/a/b/c"));
        assert!(re.is_match("/proj/"));
        assert!(!re.is_match("/projx/a"), "must not match a sibling prefix");
        assert!(!re.is_match("/other/proj/a"), "anchored at start");
    }

    #[test]
    fn glob_single_star_stops_at_separator() {
        let re = regex::Regex::new(&glob_to_regex_src("/proj/*.rs")).unwrap();
        assert!(re.is_match("/proj/main.rs"));
        assert!(
            !re.is_match("/proj/sub/main.rs"),
            "single * must not cross a separator"
        );
    }

    #[test]
    fn glob_question_matches_single_non_separator() {
        let re = regex::Regex::new(&glob_to_regex_src("/a/?.txt")).unwrap();
        assert!(re.is_match("/a/x.txt"));
        assert!(!re.is_match("/a/xy.txt"), "? matches exactly one char");
        assert!(!re.is_match("/a//.txt"), "? must not match a separator");
    }

    #[test]
    fn glob_literal_dot_is_not_wildcard() {
        // The '.' in the glob must be a literal dot, not regex "any char".
        let re = regex::Regex::new(&glob_to_regex_src("/a/file.txt")).unwrap();
        assert!(re.is_match("/a/file.txt"));
        assert!(
            !re.is_match("/a/fileXtxt"),
            "literal dot must not match an arbitrary char"
        );
    }

    #[test]
    fn glob_is_anchored_both_ends() {
        let src = glob_to_regex_src("/x");
        assert!(src.starts_with('^'), "anchored at start: {src}");
        assert!(src.ends_with('$'), "anchored at end: {src}");
        let re = regex::Regex::new(&src).unwrap();
        assert!(re.is_match("/x"));
        assert!(!re.is_match("/x/y"), "trailing content must not match");
        assert!(!re.is_match("a/x"), "leading content must not match");
    }

    #[test]
    fn glob_combined_target_pattern_matches_nested() {
        // The schema example: /home/me/projects/**/target/**
        let re = regex::Regex::new(&glob_to_regex_src("/home/me/projects/**/target/**")).unwrap();
        assert!(re.is_match("/home/me/projects/app/target/debug/x"));
        assert!(re.is_match("/home/me/projects/a/b/target/x"));
        assert!(
            !re.is_match("/home/me/projects/app/src/main.rs"),
            "no /target/ segment -> no match"
        );
    }

    #[test]
    fn glob_list_broken_pattern_fails_safe() {
        // A configured allow-list whose globs all fail to compile (so its
        // regex set is empty) must NOT widen to allow-all: an unmatched path
        // is still denied. This is the `allows()` fail-safe invariant:
        // configured > 0 + no matching regex -> deny.
        //
        // NOTE: `GlobList::compile` itself cannot be driven into this state via
        // any glob string, because `glob_to_regex_src` routes every non-wildcard
        // byte through `regex::escape`, so the produced regex source ALWAYS
        // compiles (e.g. "[unterminated" -> "^\[unterminated$", which is valid).
        // That escaping is the first line of fail-safety. To still exercise the
        // downstream `allows()` fail-safe branch (the property that protects an
        // operator if a future grammar change ever drops a glob at compile time),
        // construct the dropped-glob state directly: configured, but no regexes.
        let bad = GlobList {
            configured: 1,
            regexes: Vec::new(),
            had_compile_error: true,
        };
        assert!(bad.had_compile_error, "dropped-glob state must be recorded");
        assert!(bad.is_configured(), "still counts as configured");
        // configured + nothing compiled + no match -> allows() is false.
        assert!(
            !bad.allows(Path::new("/anything")),
            "broken allow-list must fail safe (deny), never allow-all"
        );
        // And the matcher path used by deny_extra never spuriously matches when
        // empty (an empty deny-list adds no denies).
        assert!(
            !bad.matches(Path::new("/anything")),
            "empty regex set must not match"
        );
    }

    // ---- Probe-kind gate (TC22 A2; POLICY.md section 6 steps 2c / 2e) ----
    //
    // RED->GREEN: before the `ProbeCreate` arm existed in `evaluate`, every
    // `ProbeCreate` action fell through to the per-profile verdict and returned
    // Allow regardless of `[policy.probes]`. `probe_kind_in_deny_kinds_denied`,
    // `probe_kind_not_in_nonempty_allow_kinds_denied`, and
    // `probe_kind_deny_beats_allow` would all FAIL (they asserted Deny but got
    // Allow). Reverting the `evaluate` arm reproduces the RED state.

    #[test]
    fn probe_kind_in_deny_kinds_denied() {
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal)
            .with_probe_kinds(&[], &["file_watch".to_owned()]);
        let v = e.evaluate(&PolicyAction::ProbeCreate { kind: "file_watch" });
        assert_eq!(v.decision, PolicyDecision::Deny);
        assert!(
            v.reason.contains("probe_kind_denied"),
            "deny reason must carry the POLICY.md substring; got: {}",
            v.reason
        );
    }

    #[test]
    fn probe_kind_not_in_nonempty_allow_kinds_denied() {
        // allow_kinds is configured (non-empty) and authoritative; a kind not
        // listed is denied with `no_allow_rule`.
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal)
            .with_probe_kinds(&["command".to_owned()], &[]);
        let v = e.evaluate(&PolicyAction::ProbeCreate { kind: "pty" });
        assert_eq!(v.decision, PolicyDecision::Deny);
        assert!(
            v.reason.contains("no_allow_rule"),
            "deny reason must carry the POLICY.md substring; got: {}",
            v.reason
        );
    }

    #[test]
    fn probe_kind_in_allow_kinds_allowed() {
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal)
            .with_probe_kinds(&["command".to_owned(), "pty".to_owned()], &[]);
        let v = e.evaluate(&PolicyAction::ProbeCreate { kind: "pty" });
        assert_eq!(v.decision, PolicyDecision::Allow);
    }

    #[test]
    fn probe_kind_empty_allow_kinds_allows() {
        // Zero-config: empty allow_kinds == "not configured" == allow,
        // mirroring the path allow-list opt-in posture.
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal);
        let v = e.evaluate(&PolicyAction::ProbeCreate { kind: "file_watch" });
        assert_eq!(v.decision, PolicyDecision::Allow);
    }

    #[test]
    fn probe_kind_deny_beats_allow() {
        // A kind present in BOTH lists is denied: deny beats allow.
        let e = PolicyEngine::new(PolicyProfile::DeveloperLocal)
            .with_probe_kinds(&["pty".to_owned()], &["pty".to_owned()]);
        let v = e.evaluate(&PolicyAction::ProbeCreate { kind: "pty" });
        assert_eq!(v.decision, PolicyDecision::Deny);
        assert!(
            v.reason.contains("probe_kind_denied"),
            "deny-beats-allow must surface probe_kind_denied; got: {}",
            v.reason
        );
    }
}
