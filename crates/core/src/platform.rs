// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Platform-specific helpers (no I/O beyond what callers perform).

/// Windows only: suppress console allocation for daemon-initiated payload children.
///
/// The GUI-subsystem daemon has no parent console; console-subsystem children would
/// otherwise allocate a visible window (outward-filter leakage). This does **not**
/// apply to the JS bridge (`packages/terminal-commander/lib/wsl/spawn.js`), which
/// must remain visible for WWS04 / EDR legitimacy — see
/// `docs/release/windows-wsl-bridge-contract.md` §4.4.
#[cfg(windows)]
pub fn windows_silent(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}

/// The TC-only `WSLENV` value a Windows->WSL spawn should forward.
///
/// `wsl.exe` forwards every Windows env var NAMED in `WSLENV` into the Linux
/// process it launches. An ambient `WSLENV=SOME_SECRET/u` therefore leaks
/// `SOME_SECRET` across the boundary. Every site that launches a Linux process
/// must REBUILD `WSLENV` to this TC-only allowlist instead of passing the
/// operator's ambient value through:
///
/// - `TC_SESSION` present & non-empty -> `Some("TC_SESSION/u")` (forward only
///   our opaque session token; `/u` = Windows->WSL, no path translation).
/// - otherwise -> `None`, meaning the caller must REMOVE `WSLENV` entirely
///   (we have nothing of our own to forward and must not leak ambient vars).
///
/// This is the Rust mirror of the JS `ensureSessionInWslEnv`
/// (`packages/terminal-commander/lib/wsl/filtered_env.js`). Pure: no I/O, no
/// env reads — the caller supplies the resolved `TC_SESSION` value so the
/// function is trivially unit-testable and platform-agnostic.
#[must_use]
pub fn wslenv_overlay_value(tc_session: Option<&str>) -> Option<String> {
    match tc_session {
        Some(token) if !token.is_empty() => Some("TC_SESSION/u".to_owned()),
        _ => None,
    }
}

/// Rebuild a child command's `WSLENV` to the TC-only allowlist before a
/// `wsl.exe` spawn.
///
/// `wsl.exe` launches a Linux process, so OVERLAY posture applies: the rest of
/// the inherited env (PATH, SystemRoot, ...) is left intact — only `WSLENV` is
/// overridden (to `TC_SESSION/u`) or removed.
///
/// `tc_session` is the value the child will see for `TC_SESSION` (normally the
/// parent's, since the child inherits it). See [`wslenv_overlay_value`] for the
/// rule and the JS `ensureSessionInWslEnv` cross-reference.
#[cfg(windows)]
pub fn sanitize_wslenv<'a>(
    cmd: &'a mut std::process::Command,
    tc_session: Option<&str>,
) -> &'a mut std::process::Command {
    match wslenv_overlay_value(tc_session) {
        Some(value) => cmd.env("WSLENV", value),
        None => cmd.env_remove("WSLENV"),
    }
}

#[cfg(test)]
mod tests {
    use super::wslenv_overlay_value;

    #[test]
    fn wslenv_overlay_forwards_only_session_when_present() {
        // Ambient WSLENV is irrelevant: the rule is computed from TC_SESSION
        // alone, so a SECRET-laden ambient value can never survive.
        assert_eq!(
            wslenv_overlay_value(Some("agent-1")),
            Some("TC_SESSION/u".to_owned()),
            "TC_SESSION present => forward ONLY TC_SESSION/u (ambient dropped)"
        );
    }

    #[test]
    fn wslenv_overlay_is_none_when_session_absent() {
        assert_eq!(
            wslenv_overlay_value(None),
            None,
            "no TC_SESSION => caller must REMOVE WSLENV (drop ambient entirely)"
        );
    }

    #[test]
    fn wslenv_overlay_is_none_when_session_empty() {
        assert_eq!(
            wslenv_overlay_value(Some("")),
            None,
            "empty TC_SESSION counts as absent => remove WSLENV"
        );
    }
}
