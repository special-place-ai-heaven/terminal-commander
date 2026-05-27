// SPDX-License-Identifier: Apache-2.0
// Canonical path resolution for the daemon state dir, IPC endpoint,
// and log dir. Shared by terminal-commander-mcp,
// terminal-commander (cli), and any other consumer that needs to
// match the daemon's defaults.
//
// These functions MUST match the defaults in
// `crates/daemon/src/config.rs::DaemonConfig` exactly. If a
// consumer reports `daemon: unavailable` because it probed the wrong
// path, the bug is here.

use std::path::PathBuf;

use crate::ensure::Endpoint;

/// Read-only view of the process environment, injected into path
/// resolution so the logic is testable without mutating the
/// process-global env table (which races across the parallel test
/// runner). Production code uses [`ProcessEnv`]; tests use a fixed map.
pub trait EnvSource {
    /// Return the value of `key`, or `None` if unset.
    fn get(&self, key: &str) -> Option<String>;
}

/// Production [`EnvSource`] backed by `std::env::var`.
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// The directory under which the daemon stores its sqlite DB, the
/// log file, the IPC endpoint (Unix), and any other per-user state.
///
/// Resolution order:
/// 1. `TC_DATA` env var (overrides everything; empty string is treated as unset).
/// 2. Windows: `%LOCALAPPDATA%\terminal-commanderd\state`.
/// 3. Unix: `$HOME/.local/share/terminal-commanderd`.
///    NOTE: `XDG_STATE_HOME` is intentionally NOT consulted here. The daemon's
///    `platform_default_data_dir()` ignores `XDG_STATE_HOME` and goes straight to
///    `$HOME/.local/share/terminal-commanderd`. Consumers must probe the same path
///    the daemon binds or they will always report `daemon: unavailable`.
/// 4. Fallback: `$TMPDIR/terminal-commanderd/state`.
///
/// The subdirectory name is `terminal-commanderd` (with the `d` suffix),
/// matching `DaemonConfig`'s startup default exactly.
#[must_use]
pub fn resolve_state_dir() -> PathBuf {
    resolve_state_dir_with(&ProcessEnv)
}

/// [`resolve_state_dir`] with an injected env source. Production calls the
/// zero-arg wrapper; tests pass a fixed env to avoid process-global mutation.
#[must_use]
pub fn resolve_state_dir_with(env: &impl EnvSource) -> PathBuf {
    let base = state_dir_base(env);
    match crate::session::resolve_session(env) {
        // A full TC_SOCKET override does not affect the state dir base.
        crate::session::SessionEndpoint::Session(token) => base.join(token),
        _ => base,
    }
}

/// The state-dir base before any per-session subdir.
///
/// `TC_DATA`, else the platform default. Byte-identical to the pre-F1
/// `resolve_state_dir_with`.
fn state_dir_base(env: &impl EnvSource) -> PathBuf {
    if let Some(p) = env.get("TC_DATA").filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        if let Some(p) = env.get("LOCALAPPDATA") {
            return PathBuf::from(p).join("terminal-commanderd").join("state");
        }
    }
    #[cfg(unix)]
    {
        // Do NOT consult XDG_STATE_HOME — daemon ignores it.
        if let Some(p) = env.get("HOME") {
            return PathBuf::from(p).join(".local").join("share").join("terminal-commanderd");
        }
    }
    std::env::temp_dir().join("terminal-commanderd").join("state")
}

/// The IPC endpoint path:
/// - Windows: `\\.\pipe\terminal-commander-{USERNAME}` (NO `-default` suffix).
///   Falls back to `terminal-commander-default` when USERNAME/USER are unset.
///   Matches `DaemonConfig::pipe_name()` exactly.
/// - Unix: `<state_dir>/terminal-commanderd.sock`.
///   Matches `DaemonConfig::socket_path()` exactly.
///
/// May be overridden by `TC_SOCKET` env var (empty string is treated as unset,
/// matching `apply_socket_env_override` in the daemon).
#[must_use]
pub fn resolve_socket_path() -> PathBuf {
    resolve_socket_path_with(&ProcessEnv)
}

/// [`resolve_socket_path`] with an injected env source.
#[must_use]
pub fn resolve_socket_path_with(env: &impl EnvSource) -> PathBuf {
    if let Some(p) = env.get("TC_SOCKET").filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        // Match DaemonConfig::pipe_name() exactly:
        //   format!(r"\\.\pipe\terminal-commander-{user}")
        // where user = USERNAME ?? USER ?? "default".
        // NO "-default" suffix beyond the fallback user string.
        let user = env
            .get("USERNAME")
            .or_else(|| env.get("USER"))
            .unwrap_or_else(|| "default".to_owned());
        return PathBuf::from(format!(r"\\.\pipe\terminal-commander-{user}"));
    }
    #[cfg(unix)]
    {
        // Match DaemonConfig::socket_path():
        //   self.daemon.data_dir.join("terminal-commanderd.sock")
        return resolve_state_dir_with(env).join("terminal-commanderd.sock");
    }
    #[allow(unreachable_code)]
    resolve_state_dir_with(env).join("terminal-commanderd.sock")
}

/// Wrap a socket-path-shaped PathBuf into an Endpoint enum, choosing
/// the platform variant by inspecting the path prefix.
#[must_use]
pub fn endpoint_from_socket_path(p: &std::path::Path) -> Endpoint {
    let s = p.to_string_lossy();
    if s.starts_with(r"\\.\pipe\") {
        Endpoint::WindowsPipe {
            name: s.into_owned(),
        }
    } else {
        Endpoint::UnixSocket {
            path: p.to_path_buf(),
        }
    }
}

/// Where the daemon writes its log file. Always
/// `<state_dir>/logs/terminal-commanderd.log`.
#[must_use]
pub fn resolve_log_path() -> PathBuf {
    resolve_log_path_with(&ProcessEnv)
}

/// [`resolve_log_path`] with an injected env source.
#[must_use]
pub fn resolve_log_path_with(env: &impl EnvSource) -> PathBuf {
    resolve_state_dir_with(env)
        .join("logs")
        .join("terminal-commanderd.log")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// In-memory [`EnvSource`] for tests. No process-global state, so tests
    /// run race-free under any `--test-threads` value.
    struct FakeEnv(HashMap<String, String>);

    impl FakeEnv {
        fn new() -> Self {
            Self(HashMap::new())
        }
        fn with(mut self, key: &str, val: &str) -> Self {
            self.0.insert(key.to_owned(), val.to_owned());
            self
        }
    }

    impl EnvSource for FakeEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn tc_data_env_overrides_everything() {
        let env = FakeEnv::new().with("TC_DATA", "/custom/path");
        assert_eq!(resolve_state_dir_with(&env), PathBuf::from("/custom/path"));
    }

    #[test]
    fn endpoint_from_windows_pipe_path() {
        let p = PathBuf::from(r"\\.\pipe\terminal-commander-poslj");
        match endpoint_from_socket_path(&p) {
            Endpoint::WindowsPipe { name } => {
                assert_eq!(name, r"\\.\pipe\terminal-commander-poslj");
            }
            other => panic!("expected WindowsPipe, got {other:?}"),
        }
    }

    #[test]
    fn endpoint_from_unix_socket_path() {
        let p = PathBuf::from("/tmp/foo/terminal-commanderd.sock");
        match endpoint_from_socket_path(&p) {
            Endpoint::UnixSocket { path } => assert_eq!(path, p),
            other => panic!("expected UnixSocket, got {other:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_socket_path_has_no_default_suffix() {
        // Bug: prior cli/mcp helpers appended "-default" while daemon
        // did not, so probes missed the daemon's actual pipe.
        // No TC_SOCKET in the fake env → falls through to the pipe name.
        let env = FakeEnv::new().with("USERNAME", "poslj");
        let p = resolve_socket_path_with(&env);
        let s = p.to_string_lossy();
        assert!(s.starts_with(r"\\.\pipe\terminal-commander-"));
        assert!(
            !s.ends_with("-default"),
            "got {s} — pipe must match daemon DaemonConfig::pipe_name() which has no -default suffix"
        );
    }

    #[test]
    fn empty_tc_data_is_ignored() {
        // Empty must NOT produce an empty PathBuf — it must fall through
        // to the platform default.
        let env = FakeEnv::new().with("TC_DATA", "");
        let result = resolve_state_dir_with(&env);
        assert!(
            !result.as_os_str().is_empty(),
            "empty TC_DATA must fall through to platform default, got {result:?}"
        );
    }

    #[test]
    fn empty_tc_socket_is_ignored() {
        let env = FakeEnv::new().with("TC_SOCKET", "");
        let result = resolve_socket_path_with(&env);
        assert!(
            !result.as_os_str().is_empty(),
            "empty TC_SOCKET must fall through to platform default, got {result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn xdg_state_home_is_ignored_on_unix() {
        // Matches daemon's platform_default_data_dir which does NOT consult XDG.
        // No TC_DATA in the fake env; HOME set; XDG_STATE_HOME must be ignored.
        let env = FakeEnv::new()
            .with("XDG_STATE_HOME", "/should-be-ignored")
            .with("HOME", "/test-home");
        let result = resolve_state_dir_with(&env);
        assert_eq!(
            result,
            std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd"),
            "XDG_STATE_HOME must NOT be consulted (daemon ignores it)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn explicit_session_gets_subdir_under_base() {
        let env = FakeEnv::new().with("HOME", "/test-home").with("TC_SESSION", "agent-1");
        assert_eq!(
            resolve_state_dir_with(&env),
            std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd/agent-1"),
            "explicit session appends /{{token}} under the default base"
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_subdir_hangs_under_tc_data_base() {
        let env = FakeEnv::new().with("TC_DATA", "/custom/root").with("TC_SESSION", "agent-1");
        assert_eq!(
            resolve_state_dir_with(&env),
            std::path::PathBuf::from("/custom/root/agent-1"),
            "TC_DATA relocates the base; session subdir hangs under it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unseeded_state_dir_is_byte_identical_to_pre_f1() {
        let env = FakeEnv::new().with("HOME", "/test-home");
        assert_eq!(
            resolve_state_dir_with(&env),
            std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd"),
            "default (no TC_SESSION) must NOT add any subdir"
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_state_pidfile_log_socket_all_co_locate() {
        let env = FakeEnv::new().with("HOME", "/test-home").with("TC_SESSION", "agent-1");
        let state = resolve_state_dir_with(&env);
        let expected = std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd/agent-1");
        assert_eq!(state, expected, "state dir is the session subdir");
        assert_eq!(crate::pidfile::pidfile_path(&state), expected.join("terminal-commanderd.pid"));
        assert_eq!(resolve_log_path_with(&env), expected.join("logs").join("terminal-commanderd.log"));
        assert_eq!(resolve_socket_path_with(&env), expected.join("terminal-commanderd.sock"));
    }
}
