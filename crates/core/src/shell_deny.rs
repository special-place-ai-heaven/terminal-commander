// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shared `argv[0]` shell-interpreter deny list.
//!
//! `command_start_combed` rejects these basenames before policy. Recipe
//! validation uses the same list so a stored argv cannot smuggle a shell.
//! Distinct from privilege-escalator denials (`sudo`, `doas`, …).

/// Closed-set deny list for shell-interpreter basenames.
///
/// Adding a name here denies it on both the command gate and recipe
/// validate. `.exe` entries match case-insensitively; the others match
/// the basename exactly.
pub const SHELL_INTERPRETERS_DENY: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "busybox",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "cmd",
    "cmd.exe",
];

/// Basename of `argv0`, splitting on both `/` and `\` so a Windows path
/// is classified the same on a Linux daemon.
#[must_use]
pub fn shell_interpreter_denied(argv0: &str) -> Option<&'static str> {
    let basename = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    SHELL_INTERPRETERS_DENY.iter().copied().find(|&shell| {
        basename == shell
            || (std::path::Path::new(shell)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                && basename.eq_ignore_ascii_case(shell))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_bare_and_path_forms() {
        assert_eq!(shell_interpreter_denied("bash"), Some("bash"));
        assert_eq!(shell_interpreter_denied("/bin/sh"), Some("sh"));
        assert_eq!(shell_interpreter_denied("pwsh"), Some("pwsh"));
        assert_eq!(
            shell_interpreter_denied(r"C:\Windows\System32\cmd.exe"),
            Some("cmd.exe")
        );
        assert_eq!(
            shell_interpreter_denied(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.EXE"),
            Some("powershell.exe")
        );
        assert_eq!(shell_interpreter_denied("git"), None);
        assert_eq!(shell_interpreter_denied("/usr/bin/git"), None);
    }
}
