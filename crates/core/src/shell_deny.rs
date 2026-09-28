// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shared shell-interpreter deny.
//!
//! Recipe validate, the recipe-run re-check, and the non-WSL argv lanes
//! (`command_start_combed`, `pty_command_start`) call [`shell_argv_denied`].
//! A `wsl`/`wsl.exe` carrier ([`is_wsl_carrier`]) stays with the daemon WSL
//! gate so `allow_shell` still applies. Distinct from
//! privilege-escalator denials (`sudo`, `doas`, ...).
//!
//! Matching case-folds every interpreter name and strips one Windows
//! executable extension (`.exe` / `.com` / `.bat` / `.cmd`) so `bash.exe`,
//! `CMD`, and `PowerShell` hit the same list as `bash` and `cmd`.
//! Trailing ASCII dots and spaces, an unnamed `::$DATA` stream, and the
//! powershell 8.3 shape `powers~<digit>` fold into that same match.
//!
//! ponytail: wrapper and interpreter options come from small tables, and
//! `env -S` is split the way GNU env splits it (quotes, `#` comments, the
//! literal escapes); `$VAR`, `${VAR}`, and other `\` escapes are denied
//! rather than modeled. An unlisted launcher is covered only when a script
//! flag follows the interpreter. Basename matching also strips trailing
//! ASCII dots/spaces and an unnamed `::$DATA` stream, and recognizes the
//! powershell 8.3 shape `powers~<digit>`. That is deny-list string matching
//! on every host, not a Win32 security boundary: no filesystem short-name
//! lookup, no other 8.3 names, no arbitrary ADS (`file:stream:$DATA`).
//! A complete control is an explicit command allowlist.

/// Closed-set deny list for shell-interpreter basenames.
///
/// `.exe` aliases stay so an exact `cmd.exe` hit still reports `cmd.exe`.
/// Names that are not listed (`bash.exe`, `sh.exe`, `cmd.com`) match by
/// stem after the extension strip.
pub const SHELL_INTERPRETERS_DENY: &[&str] = &[
    "sh",
    "bash",
    "rbash",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "mksh",
    "lksh",
    "pdksh",
    "oksh",
    "loksh",
    "yash",
    "posh",
    "csh",
    "tcsh",
    "ash",
    "busybox",
    "nu",
    "elvish",
    "xonsh",
    "osh",
    "ysh",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "cmd",
    "cmd.exe",
];

/// Launchers whose first operand is the program they run, each with the
/// short letters and long names of its options that take a value (`env -u
/// NAME`, `timeout --signal=KILL`), read by [`wrapper_option`]. After a
/// wrapper, operands that start with a digit are skipped too, and
/// [`VALUE_OPERAND_WRAPPERS`] take their first operand as a value. Not
/// `xargs` or `sudo`: their operands are not just a program, and `sudo` has
/// its own privilege deny.
const SHELL_ARGV_WRAPPERS: &[(&str, &str, &[&str])] = &[
    ("command", "", &[]),
    ("exec", "a", &[]),
    ("env", "uCSPa", &["unset", "chdir", "split-string", "argv0"]),
    ("nohup", "", &[]),
    ("time", "fo", &["format", "output"]),
    ("nice", "n", &["adjustment"]),
    ("timeout", "sk", &["signal", "kill-after"]),
    ("stdbuf", "ioe", &["input", "output", "error"]),
    ("ionice", "cn", &["class", "classdata"]),
    ("chrt", "", &[]),
    ("taskset", "", &[]),
    ("setsid", "", &[]),
    ("unbuffer", "", &[]),
];

/// Wrappers whose first operand is a value, not the program: `timeout
/// DURATION`, `taskset MASK`, `chrt PRIORITY` (`inf`, `ff`). `nice` and
/// `ionice` take their values through options only.
const VALUE_OPERAND_WRAPPERS: &[&str] = &["timeout", "taskset", "chrt"];

/// Deny label for an `env -S` string this split does not model.
pub const ENV_SPLIT_STRING_DENY: &str = "env -S";

/// Programs that run their operands on another host or in a container.
/// The interpreter they name is not this host's shell, so the argv is not
/// scanned past them. TC does not gate the remote side (`allow_remote` is
/// `target_id` federation only). `podman unshare` is exempt too, on purpose.
/// `wsl` is not here: it runs on this host and has its own gate.
const REMOTE_CARRIERS: &[&str] = &["ssh", "docker", "podman", "nerdctl", "kubectl"];

/// Flags that hand the following argument to an interpreter.
///
/// Compared case-insensitively, matching the previous recipe pair check
/// (`/C`, `-Command`). POSIX `-C` (noclobber) shares that spelling; the
/// check only runs next to a denied interpreter, so `git -C` is allowed.
/// Single-dash letter clusters holding `c` (`-ec`, `-Cc`, `-eluxc`) are
/// recognized in addition for non-PowerShell interpreters, `--name=value`
/// matches by its name (`fish --command=id`), and cmd also takes combined
/// or glued switches ([`is_cmd_script_switch`]).
const SHELL_SCRIPT_FLAGS: &[&str] = &[
    "-c",
    "-lc",
    "-ec",
    "-ic",
    "-command",
    "--command",
    "--commands",
    "/c",
    "/k",
    "/r",
    "/command",
    "-encodedcommand",
    "/encodedcommand",
];

/// Interpreter options that take a separate value, so the value is not
/// read as the first operand (`bash -o pipefail -c`). Case-insensitive.
const POSIX_VALUE_FLAGS: &[&str] = &["-o", "+o", "--rcfile", "--init-file"];
const PWSH_VALUE_FLAGS: &[&str] = &[
    "-executionpolicy",
    "-ex",
    "-ep",
    "-windowstyle",
    "-w",
    "-workingdirectory",
    "-wd",
    "-configurationname",
    "-config",
    "-configurationfile",
    "-custompipename",
    "-settingsfile",
    "-settings",
    "-inputformat",
    "-inp",
    "-if",
    "-outputformat",
    "-o",
    "-of",
];

const WIN_EXE_EXTS: &[&str] = &["exe", "com", "bat", "cmd"];

/// Basename of `argv0`, splitting on both `/` and `\` so a Windows path
/// is classified the same on a Linux daemon.
#[must_use]
pub fn shell_interpreter_denied(argv0: &str) -> Option<&'static str> {
    let base = normalized_win_basename(argv0);
    if base.is_empty() {
        return None;
    }
    if let Some(shell) = listed_shell(base) {
        return Some(shell);
    }
    // `powershell.exe` is the deny-list name that does not fit 8.3, so
    // Windows can surface it as `POWERS~1.EXE` (or `~2`..`~9` on collision).
    is_powershell_short_name(base).then_some("powershell")
}

/// `wsl` / `wsl.exe` under the same basename normalize as the interpreter
/// deny, so `wsl.exe.`, `WSL.EXE ` and `wsl.exe::$DATA` are carriers too.
#[must_use]
pub fn is_wsl_carrier(argv0: &str) -> bool {
    let base = normalized_win_basename(argv0);
    base.eq_ignore_ascii_case("wsl") || base.eq_ignore_ascii_case("wsl.exe")
}

fn listed_shell(base: &str) -> Option<&'static str> {
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
/// Hits the launched program when it is an interpreter, after skipping
/// `NAME=value` words and wrappers (`env`, `nice`, `timeout`, ...) with
/// their options; `env -S STR` is split and re-checked. Otherwise hits a
/// denied interpreter whose option run holds a script flag (`-c`, `-x -c`,
/// `/k`, `-NoProfile -Command`, ...), unless the program is a remote or
/// container carrier (`ssh`, `docker`, ...).
#[must_use]
pub fn shell_argv_denied(argv: &[impl AsRef<str>]) -> Option<&'static str> {
    let Some(rest) = launched(argv) else {
        return Some(ENV_SPLIT_STRING_DENY);
    };
    let head = rest.first()?;
    if let Some(shell) = shell_interpreter_denied(head) {
        return Some(shell);
    }
    if is_remote_carrier(head) {
        return None;
    }
    rest.iter().enumerate().find_map(|(index, arg)| {
        let shell = shell_interpreter_denied(arg)?;
        interpreter_script_flag(shell, &rest[index + 1..]).then_some(shell)
    })
}

/// The argv from the program it launches onward.
///
/// Leading `NAME=value` words and wrappers (with their options and value
/// operands) are dropped, and `env -S STR` is split. Empty when only wrappers
/// remain or the `-S` string is one [`shell_argv_denied`] refuses.
/// The daemon WSL gate uses this so `env wsl.exe ...` is still a carrier.
#[must_use]
pub fn launched_argv(argv: &[impl AsRef<str>]) -> Vec<String> {
    launched(argv).unwrap_or_default()
}

/// [`launched_argv`], or `None` for an `env -S` string [`split_env_words`] refuses.
fn launched(argv: &[impl AsRef<str>]) -> Option<Vec<String>> {
    let mut argv: Vec<String> = argv.iter().map(|arg| arg.as_ref().to_owned()).collect();
    loop {
        match launched_program(&argv) {
            Launched::At(index) => {
                argv.drain(..index);
                return Some(argv);
            }
            // Each expansion drops the `-S` flag, so the loop ends.
            Launched::SplitString(expanded) => argv = expanded,
            Launched::Nothing => return Some(Vec::new()),
            Launched::Unparseable => return None,
        }
    }
}

enum Launched {
    At(usize),
    SplitString(Vec<String>),
    Nothing,
    Unparseable,
}

/// The program this argv launches, past leading `NAME=value` words and
/// wrappers with their options and value operands.
fn launched_program(argv: &[String]) -> Launched {
    let mut wrapper: Option<(&str, &str, &[&str])> = None;
    let mut value_operand = false;
    let mut index = 0;
    while let Some(token) = argv.get(index).map(String::as_str) {
        if is_env_assignment(token) {
            index += 1;
            continue;
        }
        if let Some(found) = shell_wrapper(token) {
            wrapper = Some(found);
            value_operand = VALUE_OPERAND_WRAPPERS.contains(&found.0);
            index += 1;
            continue;
        }
        let Some((_, shorts, longs)) = wrapper else {
            return Launched::At(index);
        };
        if token.starts_with('-') {
            let (option, next) = wrapper_option(argv, index, shorts, longs);
            // `env -S STR` / `--split-string=STR`; no other wrapper has them.
            if let Some(("S" | "split-string", Some(split))) = option {
                let Some(words) = split_env_words(split) else {
                    return Launched::Unparseable;
                };
                let mut expanded = argv[..index].to_vec();
                expanded.extend(words);
                expanded.extend_from_slice(&argv[next..]);
                return Launched::SplitString(expanded);
            }
            index = next;
            continue;
        }
        if value_operand || token.starts_with(|ch: char| ch.is_ascii_digit()) {
            value_operand = false;
            index += 1;
            continue;
        }
        return Launched::At(index);
    }
    Launched::Nothing
}

/// Words env makes of a `-S` string (GNU `env --split-string`): whitespace
/// separates words, a quoted run keeps its whitespace and joins the word
/// around it (`FOO='x ssh'` is one word), `#` at the start of a word ends
/// the string, and `\\ \' \" \# \$` stand for the character. `None` where
/// env would rewrite or reject the string and this does not model it:
/// `$VAR` / `${VAR}`, any other `\` escape, or an unterminated quote.
fn split_env_words(split: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quote = None;
    let mut chars = split.chars().peekable();
    while let Some(ch) = chars.next() {
        let literal = match (quote, ch) {
            (Some(open), _) if ch == open => {
                quote = None;
                continue;
            }
            // Inside single quotes only `\\` and `\'` are escapes.
            (Some('\''), '\\') => chars
                .next_if(|next| matches!(next, '\\' | '\''))
                .unwrap_or('\\'),
            (Some('\''), _) => ch,
            (_, '$') => return None,
            (_, '\\') => chars
                .next()
                .filter(|next| matches!(next, '\\' | '\'' | '"' | '#' | '$'))?,
            (None, '\'' | '"') => {
                quote = Some(ch);
                word.get_or_insert_default();
                continue;
            }
            (None, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r') => {
                words.extend(word.take());
                continue;
            }
            (None, '#') if word.is_none() => break,
            _ => ch,
        };
        word.get_or_insert_default().push(literal);
    }
    if quote.is_some() {
        return None;
    }
    words.extend(word);
    Some(words)
}

/// A wrapper option at `argv[index]`, read the way getopt reads it: in a
/// short cluster the first letter that takes a value ends the cluster and
/// takes the rest of it, or else the next word (`-uX`, `-iu X`, `-tc idle`);
/// `--name[=value]` takes an exact or unique-prefix long name (`--un X`).
/// Returns that value option (its letter or long name, and its value), if
/// any, and the index after the option.
fn wrapper_option<'a>(
    argv: &'a [String],
    index: usize,
    shorts: &'static str,
    longs: &[&'static str],
) -> (Option<(&'static str, Option<&'a str>)>, usize) {
    let token = argv[index].as_str();
    let short = || {
        let body = token.strip_prefix('-').unwrap_or(token);
        body.char_indices().find_map(|(at, letter)| {
            let key = shorts.find(letter)?;
            let rest = &body[at + letter.len_utf8()..];
            Some((&shorts[key..=key], (!rest.is_empty()).then_some(rest)))
        })
    };
    let long = |long: &'a str| {
        let (name, inline) = long
            .split_once('=')
            .map_or((long, None), |(name, value)| (name, Some(value)));
        let mut prefixed = longs
            .iter()
            .copied()
            .filter(|full| !name.is_empty() && full.starts_with(name));
        let unique = match (prefixed.next(), prefixed.next()) {
            (Some(full), None) => Some(full),
            _ => None,
        };
        longs
            .iter()
            .copied()
            .find(|full| *full == name)
            .or(unique)
            .map(|full| (full, inline))
    };
    let hit = token.strip_prefix("--").map_or_else(short, long);
    match hit {
        Some((key, None)) => (
            Some((key, argv.get(index + 1).map(String::as_str))),
            index + 2,
        ),
        Some((key, inline)) => (Some((key, inline)), index + 1),
        None => (None, index + 1),
    }
}

/// A script flag in the option run right after a denied interpreter
/// (`bash -x -c`, `pwsh -NoProfile -Command`), up to its first operand.
fn interpreter_script_flag(shell: &str, args: &[impl AsRef<str>]) -> bool {
    let pwsh = matches!(strip_win_ext(shell), "powershell" | "pwsh");
    let cmd = strip_win_ext(shell) == "cmd";
    let value_flags = if pwsh {
        PWSH_VALUE_FLAGS
    } else {
        POSIX_VALUE_FLAGS
    };
    let mut args = args.iter().map(AsRef::as_ref);
    while let Some(arg) = args.next() {
        // `-NonInteractive` is a PowerShell word, not a POSIX `c` cluster.
        let script_flag = if pwsh {
            is_listed_script_flag(arg) || is_pwsh_command_flag(arg)
        } else {
            is_script_flag(arg) || (cmd && is_cmd_script_switch(arg))
        };
        if script_flag {
            return true;
        }
        if !is_interpreter_option(arg) {
            return false;
        }
        if value_flags
            .iter()
            .any(|flag| arg.eq_ignore_ascii_case(flag))
        {
            args.next();
        }
    }
    false
}

/// `-x`, `--norc`, `+o`, or a short cmd switch (`/d`, `/e:on`).
fn is_interpreter_option(arg: &str) -> bool {
    match arg.as_bytes() {
        [b'-' | b'+', _, ..] => true,
        [b'/', rest @ ..] => {
            (1..=5).contains(&rest.len()) && !rest.contains(&b'/') && !rest.contains(&b'\\')
        }
        _ => false,
    }
}

/// PowerShell takes any prefix of `-Command` / `-EncodedCommand` /
/// `-CommandWithArgs` (`-c`, `-com`, `-e`, `-enc`) and the `-cwa` alias,
/// spelled with `-`, `--`, or `/`.
fn is_pwsh_command_flag(arg: &str) -> bool {
    let Some(name) = arg
        .strip_prefix("--")
        .or_else(|| arg.strip_prefix(['-', '/']))
    else {
        return false;
    };
    name.eq_ignore_ascii_case("cwa")
        || (!name.is_empty()
            && ["command", "encodedcommand", "commandwithargs"]
                .iter()
                .any(|full| {
                    full.get(..name.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(name))
                }))
}

/// A cmd switch run holding a script switch: combined (`/q/c`, `/D/K`) or
/// glued to its command (`/cecho`, which cmd runs as `/c echo`). A
/// one-letter `/c` with more path after it (`/c/Users`) is a Git Bash drive
/// path, not a switch run.
///
/// ponytail: a `cmd` operand followed by a `/c...`, `/k...` or `/r...` path
/// (`cp cmd /root`) is denied too; argv alone cannot tell the two apart.
fn is_cmd_script_switch(arg: &str) -> bool {
    let Some(body) = arg.strip_prefix('/') else {
        return false;
    };
    let mut switches = body.split('/').peekable();
    while let Some(switch) = switches.next() {
        let mut chars = switch.chars();
        let Some(letter) = chars.next().map(|ch| ch.to_ascii_lowercase()) else {
            return false;
        };
        let tail = chars.as_str();
        if matches!(letter, 'c' | 'k' | 'r') {
            return !tail.is_empty() || switches.peek().is_none();
        }
        // `/q`, `/d`, `/e:on`, `/t:0a`, ...
        if !"adefqstuv".contains(letter) || !(tail.is_empty() || tail.starts_with(':')) {
            return false;
        }
    }
    false
}

fn argv_basename(token: &str) -> &str {
    token.rsplit(['/', '\\']).next().unwrap_or(token)
}

/// Trailing-dot/space and unnamed `::$DATA` forms of the basename.
///
/// Applied on every host so a Windows-shaped argv is classified the same
/// on a Linux daemon. Two stream strips cover `cmd.exe.::$DATA` and
/// `cmd.exe::$DATA::$DATA`.
fn normalized_win_basename(token: &str) -> &str {
    let base = argv_basename(token);
    let once = strip_unnamed_data_stream(base.trim_end_matches([' ', '.']));
    strip_unnamed_data_stream(once.trim_end_matches([' ', '.'])).trim_end_matches([' ', '.'])
}

fn strip_unnamed_data_stream(name: &str) -> &str {
    const MARK: &[u8] = b"::$DATA";
    let bytes = name.as_bytes();
    if bytes.len() >= MARK.len() && bytes[bytes.len() - MARK.len()..].eq_ignore_ascii_case(MARK) {
        // MARK is ASCII, so this cut is on a char boundary.
        return &name[..bytes.len() - MARK.len()];
    }
    name
}

/// `POWERS~1` .. `POWERS~9`, the 8.3 shape of `powershell.exe`.
fn is_powershell_short_name(base: &str) -> bool {
    let stem = strip_win_ext(base);
    let bytes = stem.as_bytes();
    bytes.len() == 8
        && bytes[..7].eq_ignore_ascii_case(b"powers~")
        && (b'1'..=b'9').contains(&bytes[7])
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

fn shell_wrapper(token: &str) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    let stem = strip_win_ext(normalized_win_basename(token));
    SHELL_ARGV_WRAPPERS
        .iter()
        .copied()
        .find(|(wrapper, _, _)| stem.eq_ignore_ascii_case(wrapper))
}

fn is_remote_carrier(token: &str) -> bool {
    let stem = strip_win_ext(normalized_win_basename(token));
    REMOTE_CARRIERS
        .iter()
        .any(|carrier| stem.eq_ignore_ascii_case(carrier))
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
    is_listed_script_flag(token) || is_posix_command_cluster(token)
}

fn is_listed_script_flag(token: &str) -> bool {
    let name = if token.starts_with("--") {
        token.split_once('=').map_or(token, |(name, _)| name)
    } else {
        token
    };
    SHELL_SCRIPT_FLAGS
        .iter()
        .any(|flag| name.eq_ignore_ascii_case(flag))
}

/// Single-dash letter cluster of any length that holds `c` in either case
/// (`-ec`, `-exc`, `-Cc`, `-eluxc`). A lone `-C` (noclobber) matches too,
/// as it already does through the table.
fn is_posix_command_cluster(token: &str) -> bool {
    let Some(body) = token.strip_prefix('-') else {
        return false;
    };
    !body.is_empty()
        && body.bytes().all(|byte| byte.is_ascii_alphabetic())
        && body.bytes().any(|byte| byte.eq_ignore_ascii_case(&b'c'))
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

    #[test]
    fn denies_win32_trailing_dot_short_name_and_ads() {
        assert_eq!(shell_interpreter_denied("cmd.exe."), Some("cmd.exe"));
        assert_eq!(shell_interpreter_denied("cmd.exe "), Some("cmd.exe"));
        assert_eq!(shell_interpreter_denied("cmd.exe. "), Some("cmd.exe"));
        assert_eq!(shell_interpreter_denied("bash."), Some("bash"));
        assert_eq!(
            shell_interpreter_denied(r"C:\Windows\System32\cmd.exe."),
            Some("cmd.exe")
        );
        assert_eq!(shell_interpreter_denied("POWERS~1.EXE"), Some("powershell"));
        assert_eq!(shell_interpreter_denied("powers~2.exe"), Some("powershell"));
        assert_eq!(shell_interpreter_denied("Powers~1"), Some("powershell"));
        assert_eq!(
            shell_interpreter_denied(r"C:\Windows\System32\WindowsPowerShell\v1.0\POWERS~1.EXE"),
            Some("powershell")
        );
        assert_eq!(shell_interpreter_denied("cmd.exe::$DATA"), Some("cmd.exe"));
        assert_eq!(shell_interpreter_denied("cmd.exe::$data"), Some("cmd.exe"));
        assert_eq!(shell_interpreter_denied("bash.exe.::$DATA"), Some("bash"));
        assert_eq!(
            shell_interpreter_denied(r"C:\Windows\System32\cmd.exe::$DATA"),
            Some("cmd.exe")
        );
        assert_eq!(
            shell_interpreter_denied("POWERS~1.EXE::$DATA"),
            Some("powershell")
        );

        assert_eq!(
            shell_argv_denied(&["cmd.exe.", "/c", "dir"]),
            Some("cmd.exe")
        );
        assert_eq!(
            shell_argv_denied(&["cmd.exe ", "/c", "dir"]),
            Some("cmd.exe")
        );
        assert_eq!(
            shell_argv_denied(&["POWERS~1.EXE", "-Command", "Get-Date"]),
            Some("powershell")
        );
        assert_eq!(
            shell_argv_denied(&["cmd.exe::$DATA", "/c", "dir"]),
            Some("cmd.exe")
        );
        assert_eq!(
            shell_argv_denied(&["env", "cmd.exe.", "/k", "dir"]),
            Some("cmd.exe")
        );
        // Same basename normalize on wrappers, so a dotted wrapper still
        // hands the next token to the interpreter check.
        assert_eq!(shell_argv_denied(&["env.exe.", "bash"]), Some("bash"));
        assert_eq!(
            shell_argv_denied(&["env.exe::$DATA", "bash", "-c", "id"]),
            Some("bash")
        );

        assert_eq!(shell_interpreter_denied("git.exe."), None);
        assert_eq!(shell_interpreter_denied("git.exe "), None);
        assert_eq!(shell_interpreter_denied("git.exe::$DATA"), None);
        assert_eq!(shell_interpreter_denied("GIT~1.EXE"), None);
        assert_eq!(shell_interpreter_denied("POWERS~1.DLL"), None);
        assert_eq!(shell_interpreter_denied("POWERS~0.EXE"), None);
        assert_eq!(shell_interpreter_denied("POWERS~10.EXE"), None);
        assert_eq!(shell_interpreter_denied("npm.cmd."), None);
        assert_eq!(shell_argv_denied(&["git.exe.", "status"]), None);
        assert_eq!(shell_argv_denied(&["git.exe::$DATA", "status"]), None);
        // Non-ASCII tail must not panic on the ASCII `::$DATA` cut.
        assert_eq!(shell_interpreter_denied("ü::$DATA"), None);
    }

    #[test]
    fn argv_denies_fcr2_wrapper_split_string_and_flag_run() {
        let denied: &[(&[&str], &str)] = &[
            // Launchers outside the old five-item set, with their value flags.
            (&["setsid", "-f", "zsh", "run.zsh"], "zsh"),
            (&["timeout", "-s", "KILL", "5", "sh", "build.sh"], "sh"),
            (&["stdbuf", "-o", "L", "bash", "build.sh"], "bash"),
            (&["ionice", "-c", "3", "bash", "build.sh"], "bash"),
            (&["chrt", "-f", "50", "dash", "build.sh"], "dash"),
            (&["taskset", "-c", "0-3", "bash", "build.sh"], "bash"),
            (&["taskset", "0x3", "bash", "build.sh"], "bash"),
            (&["unbuffer", "sh", "build.sh"], "sh"),
            (&["env", "-u", "HOME", "bash", "build.sh"], "bash"),
            (&["env", "-C", "/tmp", "bash", "build.sh"], "bash"),
            (&["time", "-o", "t.txt", "bash", "build.sh"], "bash"),
            (
                &["nohup", "nice", "-n", "10", "env", "bash", "build.sh"],
                "bash",
            ),
            // env split-string.
            (&["env", "-S", "bash -c id"], "bash"),
            (&["env", "-Sbash -c id"], "bash"),
            (&["env", "--split-string", "sh -c id"], "sh"),
            (&["env", "--split-string=sh -c id"], "sh"),
            (&["env", "-iS", "bash -c id"], "bash"),
            (&["/usr/bin/env", "-S", "FOO=1 nice bash build.sh"], "bash"),
            (&["env", "-S", "-i bash", "-c", "id"], "bash"),
            // Script flag after other interpreter flags, behind an unknown launcher.
            (&["strace", "-f", "bash", "-x", "-c", "id"], "bash"),
            (
                &[
                    "flock",
                    "/tmp/l",
                    "bash",
                    "--noprofile",
                    "--norc",
                    "-c",
                    "id",
                ],
                "bash",
            ),
            (&["xargs", "bash", "-o", "pipefail", "-c", "id"], "bash"),
            (
                &[
                    "strace",
                    "pwsh",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "x",
                ],
                "pwsh",
            ),
            (&["strace", "PWSH", "-noprofile", "-COMMAND", "x"], "pwsh"),
            (&["strace", "pwsh", "-e", "QQ=="], "pwsh"),
            (
                &[
                    "strace",
                    "powershell",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    "x",
                ],
                "powershell",
            ),
            (&["strace", "cmd", "/d", "/q", "/c", "dir"], "cmd"),
            (&["strace", "cmd", "/r", "dir"], "cmd"),
            // Shells that were not listed.
            (&["mksh", "-c", "id"], "mksh"),
            (&["yash"], "yash"),
            (&["env", "rbash", "x.sh"], "rbash"),
            (&["strace", "nu", "-c", "ls"], "nu"),
        ];
        for (argv, expected) in denied {
            assert_eq!(shell_argv_denied(argv), Some(*expected), "argv={argv:?}");
        }
    }

    #[test]
    fn argv_denies_quoted_split_string_value_operands_and_mixed_clusters() {
        let denied: &[(&[&str], &str)] = &[
            // env -S quote removal: `'bash'` and `b""ash` are `bash` to env.
            (&["env", "-S", "'bash' -c 'echo X'"], "bash"),
            (&["env", "-S", "b\"\"ash -c 'echo X'"], "bash"),
            (&["env", "-S", "\"sh\" build.sh"], "sh"),
            // `\` escapes and `${VAR}` are not modeled: fail closed.
            (&["env", "-S", "b\\ash -c id"], ENV_SPLIT_STRING_DENY),
            (&["env", "-S", "${SHELL} -c id"], ENV_SPLIT_STRING_DENY),
            // The first operand of timeout / taskset / chrt is a value.
            (&["timeout", "inf", "bash", "run.sh"], "bash"),
            (&["timeout", ".5", "bash", "run.sh"], "bash"),
            (&["taskset", "ff", "bash", "run.sh"], "bash"),
            (&["chrt", "-f", "50", "bash", "run.sh"], "bash"),
            (&["env", "-a", "x", "bash", "run.sh"], "bash"),
            // Script-flag clusters are case-insensitive and any length.
            (&["flock", "/tmp/x", "bash", "-Cc", "id"], "bash"),
            (&["flock", "/tmp/x", "bash", "-Bc", "id"], "bash"),
            (&["flock", "/tmp/x", "bash", "-eluxc", "id"], "bash"),
        ];
        for (argv, expected) in denied {
            assert_eq!(shell_argv_denied(argv), Some(*expected), "argv={argv:?}");
        }
        for argv in [
            &["env", "-S", "cargo build"][..],
            &["env", "-S", "'cargo' build --release"],
            &["timeout", "10", "cargo", "test"],
            &["timeout", "5m", "npm", "test"],
            &["taskset", "ff", "cargo", "bench"],
            &["ionice", "-c3", "cargo", "build"],
            &["flock", "/tmp/x", "bash", "-x", "script.sh"],
            &["flock", "/tmp/x", "bash", "run.sh"],
            // PowerShell words are not POSIX clusters.
            &[
                "strace",
                "pwsh",
                "-NoProfile",
                "-NonInteractive",
                "-File",
                "x.ps1",
            ],
        ] {
            assert_eq!(shell_argv_denied(argv), None, "false deny for {argv:?}");
        }
    }

    #[test]
    fn argv_allows_ordinary_dev_commands() {
        for argv in [
            &["git", "status"][..],
            &["cargo", "build"],
            &["cargo", "test", "--workspace"],
            &["npm", "test"],
            &["npm.cmd", "run", "build"],
            &["node", "script.js"],
            &["node", "-e", "console.log(1)"],
            &["python", "script.py"],
            &["python", "-c", "print(1)"],
            &["perl", "-e", "print 1"],
            &["rg", "-n", "foo", "src"],
            &["docker", "ps"],
            &["ssh", "host", "uptime"],
            &["wsl", "-e", "cargo", "build"],
            &["timeout", "10", "cargo", "test"],
            &["nice", "-n", "5", "cargo", "build"],
            &["nohup", "node", "server.js"],
            &["time", "-f", "%e", "npm", "test"],
            &["stdbuf", "-oL", "python", "script.py"],
            &["taskset", "-c", "0", "cargo", "bench"],
            &["env", "RUST_LOG=debug", "cargo", "run"],
            &["env", "-S", "cargo build --release"],
            // A shell name as an operand, with no script flag after it.
            &["rg", "bash", "src"],
            &["git", "log", "--grep", "sh"],
            &["which", "bash"],
        ] {
            assert_eq!(shell_argv_denied(argv), None, "false deny for {argv:?}");
        }
    }

    #[test]
    fn remote_and_container_carriers_are_not_scanned() {
        for argv in [
            &["docker", "exec", "c", "sh", "-c", "ls"][..],
            &["docker", "run", "--rm", "alpine", "sh", "-c", "echo hi"],
            &["docker.exe.", "exec", "c", "bash", "-c", "ls"],
            &["ssh", "host", "bash", "-c", "uptime"],
            &["ssh.exe", "host", "bash", "-lc", "uptime"],
            &[
                r"C:\Windows\System32\OpenSSH\ssh.exe",
                "host",
                "sh",
                "-c",
                "x",
            ],
            &["kubectl", "exec", "pod", "--", "sh", "-c", "ls"],
            &["podman", "exec", "c", "bash", "-c", "ls"],
            &["nerdctl", "exec", "c", "sh", "-c", "ls"],
            &["env", "ssh", "host", "bash", "-c", "uptime"],
        ] {
            assert_eq!(shell_argv_denied(argv), None, "false deny for {argv:?}");
        }
        // Only the launched program is a carrier; an operand named `docker`
        // does not stop the scan, and a leading interpreter still denies.
        assert_eq!(
            shell_argv_denied(&["flock", "/tmp/docker", "bash", "-c", "id"]),
            Some("bash")
        );
        assert_eq!(
            shell_argv_denied(&["bash", "-c", "ssh host uptime"]),
            Some("bash")
        );
    }

    #[test]
    fn wsl_carrier_uses_win32_basename_normalize() {
        for name in [
            "wsl",
            "WSL.EXE",
            "wsl.exe.",
            "wsl.exe ",
            "WSL.EXE. ",
            "wsl.",
            "wsl.exe::$DATA",
            r"C:\Windows\System32\wsl.exe.",
            "/mnt/c/Windows/System32/wsl.exe",
        ] {
            assert!(is_wsl_carrier(name), "{name:?} must be a wsl carrier");
        }
        for name in ["wslconfig.exe", "wsl2", "git.exe.", "wsl.cmd", ""] {
            assert!(!is_wsl_carrier(name), "{name:?} is not a wsl carrier");
        }
    }

    #[test]
    fn split_env_words_follows_gnu_split_string() {
        let words = |s: &str| split_env_words(s);
        assert_eq!(
            words("FOO='x ssh' bash -c 'echo 1'").unwrap(),
            ["FOO=x ssh", "bash", "-c", "echo 1"]
        );
        assert_eq!(
            words("'bash' -c 'echo X'").unwrap(),
            ["bash", "-c", "echo X"]
        );
        assert_eq!(words("b\"\"ash").unwrap(), ["bash"]);
        assert_eq!(words("\"a b\"c").unwrap(), ["a bc"]);
        assert_eq!(words("bash\x0b-c\tid").unwrap(), ["bash", "-c", "id"]);
        assert_eq!(words("''").unwrap(), [""]);
        // `#` starts a comment only at the start of a word.
        assert_eq!(
            words("cargo build # bash -c id").unwrap(),
            ["cargo", "build"]
        );
        assert_eq!(words("FOO=a#b bash").unwrap(), ["FOO=a#b", "bash"]);
        // Escapes env keeps as the literal character, in and out of quotes.
        assert_eq!(
            words(r#"a\"b "c\$d" 'e\'f' 'g\h'"#).unwrap(),
            ["a\"b", "c$d", "e'f", r"g\h"]
        );
        for unparseable in [
            "'bash -c id",
            "\"bash",
            "$X -c id",
            "b${X}ash",
            "\"$X\"",
            r"b\ash",
            r"bash\_-c\_id",
            r"bash\c",
            "bash\\",
        ] {
            assert_eq!(words(unparseable), None, "{unparseable:?}");
        }
        assert_eq!(words("'$X'").unwrap(), ["$X"]);
    }

    #[test]
    fn argv_denies_quoted_split_string_assignments_and_getopt_clusters() {
        let denied: &[(&[&str], &str)] = &[
            // A quoted `NAME=value` word keeps its space, so `ssh` is not the program.
            (&["env", "-S", "'FOO=x ssh' bash -c id"], "bash"),
            (&["env", "-S", "FOO='x ssh' bash -c id"], "bash"),
            (&["env", "-S", "\"FOO=x docker\" bash -c id"], "bash"),
            (&["env", "-S", "FOO='a b' bash run.sh"], "bash"),
            (&["env", "-S", "FOO='x ssh' bash -c 'echo 1'"], "bash"),
            (&["env", "-S", "'bash -c id"], ENV_SPLIT_STRING_DENY),
            (&["env", "-S", "$X -c id"], ENV_SPLIT_STRING_DENY),
            // getopt: a value letter ends the cluster, long names take a prefix.
            (&["env", "--un", "X", "bash", "s"], "bash"),
            (&["env", "-iu", "X", "bash", "s"], "bash"),
            (&["env", "-iC", "/tmp", "bash", "s"], "bash"),
            (&["env", "--ch", "/tmp", "bash", "s"], "bash"),
            (&["ionice", "-tc", "idle", "bash", "s"], "bash"),
            (&["time", "-po", "out", "bash", "s"], "bash"),
            (&["env", "-uX", "bash", "s"], "bash"),
            (&["env", "--unset=X", "bash", "s"], "bash"),
            (&["env", "-vS", "bash -c id"], "bash"),
            (&["env", "--sp", "bash -c id"], "bash"),
            // Script flags behind an unlisted launcher.
            (&["flock", "x", "fish", "--command=id"], "fish"),
            (&["flock", "x", "nu", "--commands", "id"], "nu"),
            (
                &["flock", "x", "pwsh", "-CommandWithArgs", "$args", "x"],
                "pwsh",
            ),
            (&["flock", "x", "pwsh", "-cwa", "$args", "x"], "pwsh"),
            (&["flock", "x", "cmd", "/cecho", "hi"], "cmd"),
            (&["flock", "x", "cmd", "/q/c", "echo", "hi"], "cmd"),
            (&["flock", "x", "cmd", "/Q/D/K", "echo", "hi"], "cmd"),
        ];
        for (argv, expected) in denied {
            assert_eq!(shell_argv_denied(argv), Some(*expected), "argv={argv:?}");
        }
        for argv in [
            &["env", "-S", "git --version"][..],
            &["env", "-S", "FOO='x y' git status"],
            &["env", "-S", "cargo build # bash -c id"],
            &["env", "-i", "cargo", "build"],
            &["timeout", "-s", "KILL", "10", "cargo", "test"],
            &["nice", "-n", "10", "cargo", "build"],
            &["env", "-u", "X", "cargo", "build"],
            &["time", "-p", "cargo", "test"],
            // A drive path after a `cmd` operand is not a switch run.
            &["grep", "-rn", "cmd", "/c/Users/x"],
        ] {
            assert_eq!(shell_argv_denied(argv), None, "false deny for {argv:?}");
        }
        assert_eq!(
            launched_argv(&["env", "-S", "'FOO=x ssh' wsl.exe bash -c id"]),
            ["wsl.exe", "bash", "-c", "id"]
        );
    }
}
