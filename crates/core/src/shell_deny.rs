// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shared shell-interpreter deny.
//!
//! Recipe validate, the recipe-run re-check, and the non-WSL argv lanes
//! (`command_start_combed`, `pty_command_start`) call [`shell_argv_denied`].
//! A `wsl`/`wsl.exe` carrier stays with the daemon WSL gate so `allow_shell`
//! still applies. Distinct from
//! privilege-escalator denials (`sudo`, `doas`, ...).
//!
//! Matching case-folds every interpreter name and strips one Windows
//! executable extension (`.exe` / `.com` / `.bat` / `.cmd`) so `bash.exe`,
//! `CMD`, and `PowerShell` hit the same list as `bash` and `cmd`.
//!
//! ponytail: a dash-flag after a wrapper is skipped, not that flag's argument
//! (`env -u NAME bash` with no script flag). An adjacent script flag still
//! denies `env -u NAME bash -ec`. Win32 trailing-dot, 8.3, and `::$DATA`
//! (RISK-002) are not normalized.

/// Closed-set deny list for shell-interpreter basenames.
///
/// `.exe` aliases stay so an exact `cmd.exe` hit still reports `cmd.exe`.
/// Names that are not listed (`bash.exe`, `sh.exe`, `cmd.com`) match by
/// stem after the extension strip.
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

/// Leading argv words that launch the next program. Same set the shell-line
/// privilege scan skips (`SHELL_COMMAND_PREFIX_WORDS` in the daemon policy).
const SHELL_ARGV_WRAPPERS: &[&str] = &["command", "exec", "env", "nohup", "time"];

/// Flags that hand the following argument to an interpreter.
///
/// Compared case-insensitively, matching the previous recipe pair check
/// (`/C`, `-Command`). POSIX `-C` (noclobber) shares that spelling; the
/// check only runs next to a denied interpreter, so `git -C` is allowed.
/// Short lowercase clusters (`-ec`, `-exc`) are recognized in addition.
const SHELL_SCRIPT_FLAGS: &[&str] = &[
    "-c",
    "-lc",
    "-ec",
    "-ic",
    "-command",
    "--command",
    "/c",
    "/k",
    "/command",
    "-encodedcommand",
    "/encodedcommand",
];

const WIN_EXE_EXTS: &[&str] = &["exe", "com", "bat", "cmd"];

/// Basename of `argv0`, splitting on both `/` and `\` so a Windows path
/// is classified the same on a Linux daemon.
#[must_use]
pub fn shell_interpreter_denied(argv0: &str) -> Option<&'static str> {
    let base = argv_basename(argv0);
    if base.is_empty() {
        return None;
    }
    if let Some(shell) = SHELL_INTERPRETERS_DENY
        .iter()
        .copied()
        .find(|shell| base.eq_ignore_ascii_case(shell))
    {
        return Some(shell);
    }
    let stem = strip_win_ext(base);
    if stem.len() == base.len() {
        return None;
    }
    SHELL_INTERPRETERS_DENY
        .iter()
        .copied()
        .find(|shell| stem.eq_ignore_ascii_case(strip_win_ext(shell)))
}

/// Deny a shell interpreter anywhere this argv would launch one.
///
/// Hits a leading interpreter, an interpreter after a wrapper (`env`,
/// `command`, `exec`, `nohup`, `time`, plus dash-flags and `NAME=value`
/// assignments), or a denied interpreter
/// immediately followed by a script flag (`-c`, `-ec`, `/k`, `-EncodedCommand`, ...).
#[must_use]
pub fn shell_argv_denied(argv: &[impl AsRef<str>]) -> Option<&'static str> {
    if let Some(shell) = leading_interpreter(argv) {
        return Some(shell);
    }
    argv.windows(2).find_map(|pair| {
        let shell = shell_interpreter_denied(pair[0].as_ref())?;
        is_script_flag(pair[1].as_ref()).then_some(shell)
    })
}

fn leading_interpreter(argv: &[impl AsRef<str>]) -> Option<&'static str> {
    let mut wrapper = false;
    for arg in argv {
        let token = arg.as_ref();
        if is_env_assignment(token) {
            continue;
        }
        if is_shell_wrapper(token) {
            wrapper = true;
            continue;
        }
        if wrapper && token.starts_with('-') {
            continue;
        }
        return shell_interpreter_denied(token);
    }
    None
}

fn argv_basename(token: &str) -> &str {
    token.rsplit(['/', '\\']).next().unwrap_or(token)
}

fn strip_win_ext(basename: &str) -> &str {
    let bytes = basename.as_bytes();
    for ext in WIN_EXE_EXTS {
        let ext_len = ext.len();
        if bytes.len() > ext_len + 1 && bytes[bytes.len() - ext_len - 1] == b'.' {
            let suffix = &basename[basename.len() - ext_len..];
            if suffix.eq_ignore_ascii_case(ext) {
                return &basename[..basename.len() - ext_len - 1];
            }
        }
    }
    basename
}

fn is_shell_wrapper(token: &str) -> bool {
    let stem = strip_win_ext(argv_basename(token));
    SHELL_ARGV_WRAPPERS
        .iter()
        .any(|wrapper| stem.eq_ignore_ascii_case(wrapper))
}

fn is_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_script_flag(token: &str) -> bool {
    SHELL_SCRIPT_FLAGS
        .iter()
        .any(|flag| token.eq_ignore_ascii_case(flag))
        || is_posix_command_cluster(token)
}

/// Single-dash letter cluster that includes lowercase `c` (`-ec`, `-exc`).
/// `-C` (noclobber) does not match. Longer names (`-Command`) use the table.
fn is_posix_command_cluster(token: &str) -> bool {
    let Some(body) = token.strip_prefix('-') else {
        return false;
    };
    if body.is_empty() || body.starts_with('-') || body.len() > 4 {
        return false;
    }
    body.bytes().all(|byte| byte.is_ascii_lowercase()) && body.as_bytes().contains(&b'c')
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

    #[test]
    fn denies_case_and_windows_extensions() {
        assert_eq!(shell_interpreter_denied("bash.exe"), Some("bash"));
        assert_eq!(shell_interpreter_denied("BASH.EXE"), Some("bash"));
        assert_eq!(shell_interpreter_denied("sh.exe"), Some("sh"));
        assert_eq!(shell_interpreter_denied("Sh.EXE"), Some("sh"));
        assert_eq!(shell_interpreter_denied("CMD"), Some("cmd"));
        assert_eq!(shell_interpreter_denied("Cmd"), Some("cmd"));
        assert_eq!(shell_interpreter_denied("PowerShell"), Some("powershell"));
        assert_eq!(shell_interpreter_denied("PoWeRsHeLl"), Some("powershell"));
        assert_eq!(shell_interpreter_denied("POWERSHELL"), Some("powershell"));
        assert_eq!(shell_interpreter_denied("cmd.com"), Some("cmd"));
        assert_eq!(shell_interpreter_denied("bash.bat"), Some("bash"));
        assert_eq!(shell_interpreter_denied("pwsh.cmd"), Some("pwsh"));
        assert_eq!(
            shell_interpreter_denied(r"C:\Program Files\Git\bin\bash.exe"),
            Some("bash")
        );
        assert_eq!(shell_interpreter_denied("git.exe"), None);
        assert_eq!(shell_interpreter_denied("npm.cmd"), None);
    }

    #[test]
    fn argv_denies_wrappers_and_script_flags() {
        let denied: &[&[&str]] = &[
            &["bash.exe", "-c", "whoami"],
            &["sh.exe", "-c", "id"],
            &["CMD", "/c", "dir"],
            &["Cmd", "/k", "dir"],
            &["PowerShell", "-Command", "Get-Date"],
            &["PoWeRsHeLl", "-EncodedCommand", "QQ=="],
            &["env", "bash", "-ec", "id"],
            &["env", "cmd.exe", "/k", "dir"],
            &["/usr/bin/env", "bash", "-ic", "id"],
            &["env", "-i", "bash", "-ec", "id"],
            &["env", "FOO=bar", "bash", "-c", "id"],
            &["command", "-p", "bash", "-lc", "id"],
            &["nohup", "sh", "-c", "id"],
            &["time", "pwsh", "-Command", "id"],
            &["exec", "zsh", "-c", "id"],
            &["nice", "bash", "-exc", "id"],
            &["xargs", "bash", "-ec", "id"],
            // The argv lane does not apply this to a `wsl` carrier; the WSL
            // gate owns that argv so `allow_shell` can still allow it.
            &["wsl.exe", "-e", "bash", "-lc", "id"],
        ];
        for argv in denied {
            assert!(
                shell_argv_denied(argv).is_some(),
                "expected deny for {argv:?}"
            );
        }
        assert_eq!(
            shell_argv_denied(&["env", "bash", "-ec", "id"]),
            Some("bash")
        );
        assert_eq!(
            shell_argv_denied(&["env", "cmd.exe", "/k", "dir"]),
            Some("cmd.exe")
        );
        assert_eq!(
            shell_argv_denied(&["PowerShell", "-EncodedCommand", "QQ=="]),
            Some("powershell")
        );
        assert_eq!(shell_argv_denied(&["CMD", "/C", "dir"]), Some("cmd"));
    }

    #[test]
    fn argv_allows_non_shells() {
        for argv in [
            &["git", "status"][..],
            &["git", "-c", "color.ui=auto", "status"],
            &["env", "git", "status"],
            &["env", "git", "-c", "color.ui=auto", "status"],
            &["command", "-v", "git"],
            &["time", "cargo", "test"],
            &["npm.cmd", "test"],
        ] {
            assert_eq!(shell_argv_denied(argv), None, "false deny for {argv:?}");
        }
        // `bash -C` is still a leading interpreter. `git -C` is not.
        assert_eq!(shell_argv_denied(&["bash", "-C"]), Some("bash"));
        assert_eq!(shell_argv_denied(&["git", "-C", "repo", "status"]), None);
        assert!(is_script_flag("-ec"));
        assert!(is_script_flag("-exc"));
        assert!(is_script_flag("/k"));
        assert!(is_script_flag("-EncodedCommand"));
        assert!(is_script_flag("-C"));
        assert!(!is_script_flag("--noprofile"));
    }
}
