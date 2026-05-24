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
    if let Some(p) = std::env::var("TC_DATA").ok().filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(p).join("terminal-commanderd").join("state");
        }
    }
    #[cfg(unix)]
    {
        // Do NOT consult XDG_STATE_HOME — daemon ignores it.
        if let Ok(p) = std::env::var("HOME") {
            return PathBuf::from(p)
                .join(".local")
                .join("share")
                .join("terminal-commanderd");
        }
    }
    std::env::temp_dir()
        .join("terminal-commanderd")
        .join("state")
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
    if let Some(p) = std::env::var("TC_SOCKET").ok().filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        // Match DaemonConfig::pipe_name() exactly:
        //   format!(r"\\.\pipe\terminal-commander-{user}")
        // where user = USERNAME ?? USER ?? "default".
        // NO "-default" suffix beyond the fallback user string.
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "default".to_owned());
        return PathBuf::from(format!(r"\\.\pipe\terminal-commander-{user}"));
    }
    #[cfg(unix)]
    {
        // Match DaemonConfig::socket_path():
        //   self.daemon.data_dir.join("terminal-commanderd.sock")
        return resolve_state_dir().join("terminal-commanderd.sock");
    }
    #[allow(unreachable_code)]
    resolve_state_dir().join("terminal-commanderd.sock")
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
    resolve_state_dir()
        .join("logs")
        .join("terminal-commanderd.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_data_env_overrides_everything() {
        // SAFETY: env-var mutation is inherently racy in multi-threaded
        // test runners. This test is intentionally simple and the var is
        // removed immediately after the assertion. Unsafe required in
        // Rust 1.80+ where set_var/remove_var became unsafe.
        unsafe {
            std::env::set_var("TC_DATA", "/custom/path");
        }
        let result = resolve_state_dir();
        unsafe {
            std::env::remove_var("TC_DATA");
        }
        assert_eq!(result, PathBuf::from("/custom/path"));
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
        // SAFETY: see tc_data_env_overrides_everything comment.
        unsafe { std::env::remove_var("TC_SOCKET") };
        let p = resolve_socket_path();
        let s = p.to_string_lossy();
        assert!(s.starts_with(r"\\.\pipe\terminal-commander-"));
        assert!(
            !s.ends_with("-default"),
            "got {s} — pipe must match daemon DaemonConfig::pipe_name() which has no -default suffix"
        );
    }

    #[test]
    fn empty_tc_data_is_ignored() {
        // SAFETY: see tc_data_env_overrides_everything comment.
        let prev = std::env::var("TC_DATA").ok();
        unsafe { std::env::set_var("TC_DATA", "") };
        let result = resolve_state_dir();
        match prev {
            Some(p) => unsafe { std::env::set_var("TC_DATA", p) },
            None => unsafe { std::env::remove_var("TC_DATA") },
        }
        // Empty must NOT produce an empty PathBuf — it must fall through
        // to the platform default.
        assert!(
            !result.as_os_str().is_empty(),
            "empty TC_DATA must fall through to platform default, got {result:?}"
        );
    }

    #[test]
    fn empty_tc_socket_is_ignored() {
        // SAFETY: see tc_data_env_overrides_everything comment.
        let prev = std::env::var("TC_SOCKET").ok();
        unsafe { std::env::set_var("TC_SOCKET", "") };
        let result = resolve_socket_path();
        match prev {
            Some(p) => unsafe { std::env::set_var("TC_SOCKET", p) },
            None => unsafe { std::env::remove_var("TC_SOCKET") },
        }
        assert!(
            !result.as_os_str().is_empty(),
            "empty TC_SOCKET must fall through to platform default, got {result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn xdg_state_home_is_ignored_on_unix() {
        // Matches daemon's platform_default_data_dir which does NOT consult XDG.
        // SAFETY: see tc_data_env_overrides_everything comment.
        let prev_xdg = std::env::var("XDG_STATE_HOME").ok();
        let prev_tc = std::env::var("TC_DATA").ok();
        let prev_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", "/should-be-ignored");
            std::env::remove_var("TC_DATA");
            std::env::set_var("HOME", "/test-home");
        }
        let result = resolve_state_dir();
        unsafe {
            match prev_xdg {
                Some(p) => std::env::set_var("XDG_STATE_HOME", p),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
            match prev_tc {
                Some(p) => std::env::set_var("TC_DATA", p),
                None => std::env::remove_var("TC_DATA"),
            }
            match prev_home {
                Some(p) => std::env::set_var("HOME", p),
                None => std::env::remove_var("HOME"),
            }
        }
        assert_eq!(
            result,
            std::path::PathBuf::from("/test-home/.local/share/terminal-commanderd"),
            "XDG_STATE_HOME must NOT be consulted (daemon ignores it)"
        );
    }
}
