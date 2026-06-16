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
//!   set (Phase 2). The `[policy.paths]` / `[policy.probes]` blocks load
//!   but their allow/deny lists are not yet enforced beyond the above.
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
    /// Resolved capability set (Hybrid trust model, Decision 1). Defaults to
    /// all-false; only `with_config_caps` (fed by `DaemonConfig::resolved_caps`)
    /// sets it. Caps are inputs to `evaluate()`, never an `evaluate()` bypass.
    caps: PolicyCaps,
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
            // `PolicyCaps::default()` is not const; spell the all-false set out.
            caps: PolicyCaps {
                allow_shell: false,
                allow_session: false,
                allow_privileged: false,
                allow_remote: false,
            },
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
            caps: PolicyCaps::default(),
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
            caps: PolicyCaps::default(),
        }
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
        if let PolicyAction::FileRead { path } | PolicyAction::FileWatch { path } = action
            && Self::path_default_denied(path)
        {
            return PolicyVerdict {
                decision: PolicyDecision::Deny,
                reason: format!(
                    "path '{}' matches a default-deny sensitive suffix (SECURITY.md \u{a7}5)",
                    path.display()
                ),
            };
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
                    reason: "shell_exec allowed by allow_shell capability (audited)".to_owned(),
                };
            }
            return PolicyVerdict {
                decision: PolicyDecision::Deny,
                reason: "shell_exec denied: allow_shell capability is off or profile forbids shell"
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
                ) {
                    PolicyVerdict {
                        decision: PolicyDecision::Deny,
                        reason: "read_only_observer denies command_* and registry_* mutations"
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

/// Extract the filesystem subject (path or cwd) an action operates on,
/// for containment checks. Returns `None` for actions with no path/cwd
/// subject (bucket/event/registry actions).
const fn action_path_subject<'a>(action: &'a PolicyAction<'a>) -> Option<&'a Path> {
    match action {
        PolicyAction::CommandStart { cwd, .. } => Some(cwd),
        PolicyAction::FileRead { path } | PolicyAction::FileWatch { path } => Some(path),
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
}
