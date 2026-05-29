// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Policy engine (TC22). Decides allow / deny / allow_with_audit for
//! the gated actions defined in `SECURITY.md` section 4. The default
//! shipping profile is `developer_local` per `POLICY.md` section 2.1.
//!
//! Source-status: PARTIAL implementation of the `POLICY.md` doctrine.
//!
//! ENFORCED today:
//! - cross-profile command deny set (sudo/doas/su/pkexec/kexec/polkit),
//!   by basename and absolute path;
//! - default-deny on the sensitive path SUFFIX list (anchored on
//!   README.md:294-297) for FileRead / FileWatch in every profile;
//! - per-profile mutation gates (`read_only_observer` denies command_*
//!   and registry_*; `admin_debug` denies registry mutations;
//!   `registry_activate` is AllowWithAudit for dev_local / repo_only).
//!
//! NOT YET enforced (see `docs/specs/2026-05-29-tc22-policy-engine-
//! implementation.md`; doctrine in `POLICY.md` sections 2, 4, 5, 6):
//! - command allow-lists / default-deny posture (today it is allow-by-
//!   default within a profile, NOT the documented default-deny);
//! - `$REPO_ROOT` containment. WARNING: `repo_only` shares the
//!   `developer_local` arm below and therefore does NOT yet confine
//!   reads / watches / exec to the repo tree. Do not rely on
//!   `repo_only` as a sandbox until Phase 1 of the spec lands.
//! - the declarative `[paths]` / `[commands]` / `[probes]` / `[limits]`
//!   profile schema, the limits checks, and the `allow_override`
//!   mechanism.

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
}

/// Action being evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAction<'a> {
    CommandStart { argv: &'a [String], cwd: &'a Path },
    CommandStdin,
    CommandSignal,
    FileRead { path: &'a Path },
    FileWatch { path: &'a Path },
    ProbeCreate { kind: &'a str },
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
    /// engines built before TC22 Phase 2 wires the config key — in which
    /// case containment denies all path/cwd-bearing actions fail-safe
    /// (see `repo_only_contained`).
    repo_root: Option<PathBuf>,
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
        }
    }

    /// Default-constructed engine uses the `developer_local` profile.
    #[must_use]
    pub fn default_engine() -> Self {
        Self::new(PolicyProfile::default())
    }

    /// Evaluate a gated action.
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
                Self::dev_local_repo_only_verdict(action, self.profile)
            }
            PolicyProfile::DeveloperLocal => {
                Self::dev_local_repo_only_verdict(action, self.profile)
            }
        }
    }

    /// Shared allow/audit verdict for the two exec-capable profiles,
    /// applied AFTER repo_only's containment gate. TC22 Phase 2 adds the
    /// command allow-list / default-deny posture here.
    fn dev_local_repo_only_verdict(
        action: &PolicyAction<'_>,
        profile: PolicyProfile,
    ) -> PolicyVerdict {
        if matches!(action, PolicyAction::RegistryActivate) {
            PolicyVerdict {
                decision: PolicyDecision::AllowWithAudit,
                reason: "registry activate requires audit emission".to_owned(),
            }
        } else {
            PolicyVerdict {
                decision: PolicyDecision::Allow,
                reason: format!("{profile:?} allows the action"),
            }
        }
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
        PolicyAction::CommandStdin
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
}
