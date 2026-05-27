// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Per-harness session identity. Resolves an opaque session token from the
// environment with precedence TC_SOCKET > TC_SESSION > per-user default, and
// sanitizes TC_SESSION against pipe-squat / path-traversal. Both the daemon
// (at bind) and clients (mcp/cli at connect) resolve through here so they
// compute identical endpoints with no coordination.
//
// See docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md

use crate::paths::EnvSource;

/// Maximum length of a sanitized `TC_SESSION` token.
const MAX_SESSION_TOKEN_LEN: usize = 64;

/// Resolved session intent, in precedence order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEndpoint {
    /// `TC_SOCKET` set: use this verbatim as the full endpoint.
    FullOverride(String),
    /// `TC_SESSION` set and well-formed: per-harness token.
    Session(String),
    /// Nothing set (or malformed): per-user default, byte-identical to pre-F1.
    Default,
}

/// True iff `token` is a safe session id.
///
/// Allows `[A-Za-z0-9._-]`, length 1..=64, not `..`. Rejects path separators,
/// pipe prefixes, and traversal so a hostile token cannot squat a pipe or
/// escape the state dir.
#[must_use]
pub fn is_valid_session_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_SESSION_TOKEN_LEN
        && token != ".."
        && token.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Resolve session intent from the environment.
///
/// Precedence: `TC_SOCKET` (full override) > `TC_SESSION` (token) > per-user
/// default. A malformed `TC_SESSION` soft-fails to [`SessionEndpoint::Default`].
#[must_use]
pub fn resolve_session(env: &impl EnvSource) -> SessionEndpoint {
    if let Some(sock) = env.get("TC_SOCKET").filter(|s| !s.is_empty()) {
        return SessionEndpoint::FullOverride(sock);
    }
    if let Some(tok) = env.get("TC_SESSION").filter(|s| !s.is_empty()) {
        if is_valid_session_token(&tok) {
            return SessionEndpoint::Session(tok);
        }
        eprintln!(
            "terminal-commander: ignoring malformed TC_SESSION (must be \
             [A-Za-z0-9._-], 1..=64 chars); using per-user default"
        );
    }
    SessionEndpoint::Default
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeEnv(HashMap<String, String>);
    impl FakeEnv {
        fn new() -> Self { Self(HashMap::new()) }
        fn with(mut self, k: &str, v: &str) -> Self {
            self.0.insert(k.to_owned(), v.to_owned());
            self
        }
    }
    impl EnvSource for FakeEnv {
        fn get(&self, key: &str) -> Option<String> { self.0.get(key).cloned() }
    }

    #[test]
    fn tc_socket_wins_as_full_override() {
        let env = FakeEnv::new().with("TC_SOCKET", "/custom/x.sock").with("TC_SESSION", "abc");
        assert_eq!(resolve_session(&env), SessionEndpoint::FullOverride("/custom/x.sock".into()));
    }

    #[test]
    fn tc_session_selects_token_when_no_socket() {
        let env = FakeEnv::new().with("TC_SESSION", "agent-1");
        assert_eq!(resolve_session(&env), SessionEndpoint::Session("agent-1".to_owned()));
    }

    #[test]
    fn unseeded_is_per_user_default() {
        let env = FakeEnv::new();
        assert_eq!(resolve_session(&env), SessionEndpoint::Default);
    }

    #[test]
    fn empty_values_are_treated_as_unset() {
        let env = FakeEnv::new().with("TC_SOCKET", "").with("TC_SESSION", "");
        assert_eq!(resolve_session(&env), SessionEndpoint::Default);
    }

    #[test]
    fn malformed_session_falls_back_to_default() {
        for bad in ["../evil", r"a\b", "a/b", r"\\.\pipe\x", "has space", &"x".repeat(65)] {
            let env = FakeEnv::new().with("TC_SESSION", bad);
            assert_eq!(resolve_session(&env), SessionEndpoint::Default,
                "malformed token {bad:?} must fall back to Default");
        }
    }

    #[test]
    fn well_formed_session_is_accepted() {
        for ok in ["agent-1", "abc.def", "A_B-9", &"x".repeat(64)] {
            let env = FakeEnv::new().with("TC_SESSION", ok);
            assert_eq!(resolve_session(&env), SessionEndpoint::Session(ok.to_owned()),
                "well-formed token {ok:?} must be accepted");
        }
    }
}
