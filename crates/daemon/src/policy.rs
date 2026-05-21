// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Policy engine (TC22). Decides allow / deny / allow_with_audit for
//! the gated actions defined in `SECURITY.md` section 4. The default
//! shipping profile is `developer_local` per `POLICY.md` section 2.1.
//!
//! Source-status: live (TC22) for the four locked profiles and the
//! default-deny path list anchored on README.md:294-297.

use serde::{Deserialize, Serialize};
use std::path::Path;

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

/// Policy engine. Stateless; thread-safe via `&self`.
#[derive(Debug, Clone, Copy)]
pub struct PolicyEngine {
    pub profile: PolicyProfile,
}

impl PolicyEngine {
    /// Construct an engine for a profile.
    #[must_use]
    pub const fn new(profile: PolicyProfile) -> Self {
        Self { profile }
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
            PolicyProfile::DeveloperLocal | PolicyProfile::RepoOnly => {
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
        }
    }

    fn path_default_denied(path: &Path) -> bool {
        let s = path.to_string_lossy();
        DEFAULT_DENY_PATH_SUFFIXES
            .iter()
            .any(|suf| s.ends_with(suf))
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
}
