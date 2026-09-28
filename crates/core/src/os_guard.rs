// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! The one failsafe: TC never DELETES OS-critical infrastructure.
//!
//! This applies in EVERY policy profile, including the default `full_access`.
//! Everything else -- install, update, edit, configure, run as root -- is
//! allowed by default; only a DESTRUCTIVE command whose target is a protected
//! OS tree or a raw disk is refused. Writing/creating/overwriting any file
//! (including `/etc/hosts` or `C:\Windows\...\hosts`) is NOT gated here.
//!
//! Pure functions, used by the policy engine for both the argv lane
//! (`CommandStart`, via [`argv_deletion_hit`]) and the shell lane
//! (`CommandShellStart`, via [`shell_line_deletion_hit`]), and by the
//! session lane (`shell_session_exec`) directly.
//!
//! The parser unwraps escalators (`sudo -iu`, `su -c`, `chroot`, ...) and
//! wrappers in any order, reads interpreter payloads (`sh -c`, `pwsh
//! -Command` / `-EncodedCommand`, `cmd /c`, `wsl`), `eval`, `$(...)` and
//! backticks, splits lines with the grammar of the shell that runs them, and
//! resolves relative operands against the request cwd and any `cd` in the
//! line.
//!
//! ponytail: this is a guard rail, not a kernel boundary. It matches command
//! basenames and path operands as strings. INDIRECT deletion is NOT caught:
//! `find / -delete`, `python -c "shutil.rmtree('/usr')"`, an interactive
//! `diskpart` script, a Makefile target, a path held in a variable. The
//! complete control is a hardened profile plus OS permissions; this stops the
//! obvious `rm -rf /` mistakes.

use std::iter::Peekable;
use std::path::{Component, Path, PathBuf};
use std::str::Chars;

use crate::shell_deny::{
    interpreter_script_flag, is_wsl_carrier, launched_argv, posix_option_value_words,
    shell_interpreter_denied, wrapper_option,
};

/// Substring every failsafe deny reason carries, so the IPC layer can map it
/// to the typed `OsCriticalPathProtected` error code without re-deriving it.
pub const FAILSAFE_REASON_TAG: &str = "TC's one failsafe";

/// A refused destructive command: the operation (command basename) and the
/// protected path (or disk) operand that triggered the refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsGuardHit {
    pub op: String,
    pub path: String,
}

impl OsGuardHit {
    /// The single deny sentence (item 4 of the failsafe spec). Identical
    /// wording for both lanes; carries [`FAILSAFE_REASON_TAG`].
    #[must_use]
    pub fn reason(&self) -> String {
        format!(
            "refused: {} would delete OS-critical infrastructure ({}); this is {} and applies in every profile",
            self.op, self.path, FAILSAFE_REASON_TAG
        )
    }
}

/// Privilege escalators (and `chroot`) stripped before classifying the real
/// command, so `sudo rm -rf /` is caught. Each carries the getopt letters and
/// long names of its options that take a value (`sudo -iu root` consumes
/// `root`). Distinct from the shell-interpreter deny.
const ESCALATORS: &[(&str, &str, &[&str])] = &[
    (
        "sudo",
        "aCcDgpRrTtUu",
        &[
            "chdir",
            "chroot",
            "close-from",
            "command-timeout",
            "group",
            "host",
            "other-user",
            "prompt",
            "role",
            "type",
            "user",
        ],
    ),
    ("doas", "Cu", &[]),
    (
        "su",
        "cgGsw",
        &[
            "command",
            "group",
            "session-command",
            "shell",
            "supp-group",
            "whitelist-environment",
        ],
    ),
    ("pkexec", "", &["user"]),
    (
        "run0",
        "Dgu",
        &[
            "background",
            "chdir",
            "description",
            "group",
            "machine",
            "nice",
            "property",
            "setenv",
            "slice",
            "unit",
            "user",
        ],
    ),
    ("chroot", "", &["groups", "userspec"]),
];

/// Commands whose path OPERANDS are deletion targets.
const DELETE_COMMANDS: &[&str] = &[
    // POSIX / cross-platform
    "rm",
    "rmdir",
    "unlink",
    "shred",
    "srm", // cmd.exe builtins / aliases
    "del",
    "erase",
    "rd", // PowerShell cmdlets + aliases (rm/del/rmdir alias to these)
    "remove-item",
    "ri",
];

/// Filesystem-format / disk-wipe commands: any protected/device operand is
/// refused. `mkfs`, `mkfs.ext4`, ... all start with `mkfs`.
const WIPE_COMMANDS: &[&str] = &["wipefs"];

/// POSIX reserved words that can lead a simple command (`then rm ...`).
const POSIX_RESERVED: &[&str] = &[
    "!", "{", "}", "if", "then", "elif", "else", "do", "while", "until",
];

/// `wsl` launch options that take a value.
const WSL_VALUE_OPTS: &[&str] = &[
    "-d",
    "--distribution",
    "--distribution-id",
    "-u",
    "--user",
    "--cd",
    "--shell-type",
];

/// Nested payload depth cap (`sh -c`, `su -c`, `eval`, `$(...)`, `wsl`).
/// Every level is strictly shorter than its parent, so this only bounds the
/// stack against a pathological `eval eval eval ...` line.
///
/// ponytail: deeper nesting is not scanned (fail-open); raise it if a real
/// line ever nests further.
const MAX_NESTING: usize = 16;

/// Classify a normalized command basename into how its operands are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DestructiveKind {
    /// Path operands are deletion targets.
    PathOperands,
    /// `dd`: only `of=<disk-device>` is destructive.
    DdOutfile,
    /// Windows `format`: the target drive/volume.
    Format,
    /// Windows `cipher /w:<path>`: wipes free space of that volume.
    Cipher,
}

fn classify(name: &str) -> Option<DestructiveKind> {
    if DELETE_COMMANDS.contains(&name) || WIPE_COMMANDS.contains(&name) || name.starts_with("mkfs")
    {
        return Some(DestructiveKind::PathOperands);
    }
    match name {
        "dd" => Some(DestructiveKind::DdOutfile),
        "format" => Some(DestructiveKind::Format),
        "cipher" => Some(DestructiveKind::Cipher),
        _ => None,
    }
}

/// The word grammar of the shell that runs a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grammar {
    /// sh, bash, zsh, ...: `\` escapes, `'...'` is literal, `$(...)` and
    /// backticks substitute.
    Posix,
    /// PowerShell: backtick escapes, `\` is a path separator, `{...}` blocks.
    Pwsh,
    /// cmd.exe: `^` escapes, only `"` quotes.
    Cmd,
}

impl Grammar {
    /// The grammar of `shell` (`pwsh.exe`, `C:\...\cmd.exe`, `/bin/bash`);
    /// anything not PowerShell or cmd is read as POSIX.
    fn of_shell(shell: &str) -> Self {
        let base = command_basename(shell);
        if base.starts_with("pwsh") || base.starts_with("powershell") {
            Self::Pwsh
        } else if base == "cmd" {
            Self::Cmd
        } else {
            Self::Posix
        }
    }
}

/// Normalize a command token to a comparable basename: split on `/` and `\`,
/// strip one Windows executable extension, lowercase, drop trailing dots/spaces.
fn command_basename(token: &str) -> String {
    let base = token.rsplit(['/', '\\']).next().unwrap_or(token);
    let base = base.trim_end_matches(['.', ' ']);
    let lower = base.to_ascii_lowercase();
    for ext in [".exe", ".com", ".bat", ".cmd"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_owned();
        }
    }
    lower
}

/// What an argv runs once launchers are peeled off: an argv, or a line for
/// a POSIX shell (`su -c LINE`).
enum Unwrapped {
    Argv(Vec<String>),
    Line(String),
}

/// Strip escalators, wrappers (`env`, `nice`, `timeout`, ...), and a
/// `busybox` applet prefix in any interleaving until the head is stable.
/// Every step drops at least one word, so the loop ends.
fn unwrap_launchers(argv: &[String], cwd: Option<&str>) -> (Unwrapped, Option<String>) {
    let mut real = argv.to_vec();
    let mut cwd = cwd.map(str::to_owned);
    loop {
        if let Some(step) = strip_escalator(&real) {
            match step {
                Unwrapped::Argv(rest) => {
                    cwd = launcher_chdir(&real[..real.len() - rest.len()], cwd.as_deref());
                    real = rest;
                }
                line @ Unwrapped::Line(_) => return (line, cwd),
            }
            continue;
        }
        if real.len() > 1 && command_basename(&real[0]) == "busybox" && !real[1].starts_with('-') {
            real.remove(0);
            continue;
        }
        let launched = launched_argv(&real);
        if launched.is_empty() || launched == real {
            return (Unwrapped::Argv(real), cwd);
        }
        cwd = launcher_chdir(&real[..real.len() - launched.len()], cwd.as_deref());
        real = launched;
    }
}

/// The working directory a peeled launcher prefix moves the command into:
/// `sudo -D DIR` / `--chdir=DIR`, `run0 -D DIR`, `env -C DIR` / `--chdir DIR`,
/// and `chroot NEWROOT` (operands then resolve inside NEWROOT). Anything
/// else leaves `cwd` untouched.
fn launcher_chdir(prefix: &[String], cwd: Option<&str>) -> Option<String> {
    let head = prefix
        .first()
        .map(|t| command_basename(t))
        .unwrap_or_default();
    let short = match head.as_str() {
        "sudo" | "run0" => Some("-D"),
        "env" => Some("-C"),
        "chroot" => {
            return prefix
                .iter()
                .skip(1)
                .find(|t| !t.starts_with('-'))
                .and_then(|dir| resolve_dir(dir, cwd));
        }
        _ => None,
    };
    let mut dir: Option<&str> = None;
    let mut i = 1;
    while i < prefix.len() {
        let tok = prefix[i].as_str();
        if let Some(v) = tok.strip_prefix("--chdir=") {
            dir = Some(v);
        } else if tok == "--chdir" {
            dir = prefix.get(i + 1).map(String::as_str);
            i += 1;
        } else if let Some(s) = short {
            if tok == s {
                dir = prefix.get(i + 1).map(String::as_str);
                i += 1;
            } else if let Some(v) = tok.strip_prefix(s).filter(|v| !v.is_empty()) {
                dir = Some(v);
            }
        }
        i += 1;
    }
    dir.map_or_else(|| cwd.map(str::to_owned), |dir| resolve_dir(dir, cwd))
}

/// One escalator peeled off `argv`, or `None` when the head is not one.
/// `sudo -iu root rm -rf /` -> `rm -rf /`; `su -c LINE` -> the line;
/// `chroot NEWROOT cmd` -> `cmd`.
fn strip_escalator(argv: &[String]) -> Option<Unwrapped> {
    let head = command_basename(argv.first()?);
    let &(name, shorts, longs) = ESCALATORS.iter().find(|(name, ..)| *name == head)?;
    if name == "su"
        && let Some(line) = su_command(argv, shorts, longs)
    {
        return Some(Unwrapped::Line(line));
    }
    let mut i = 1;
    while let Some(tok) = argv.get(i) {
        if tok == "--" {
            i += 1;
            break;
        }
        if !tok.starts_with('-') {
            break;
        }
        i = wrapper_option(argv, i, shorts, longs).1;
    }
    // `chroot NEWROOT cmd`, `su LOGIN ...`: drop the operand that is not the
    // command (a `su` login name that is itself destructive stays).
    let skip_operand = name == "chroot"
        || (name == "su"
            && argv
                .get(i)
                .is_some_and(|tok| classify(&command_basename(tok)).is_none()));
    if skip_operand {
        i += 1;
    }
    Some(Unwrapped::Argv(argv[i.min(argv.len())..].to_vec()))
}

/// `su`'s `-c LINE` / `--command=LINE` wherever it sits: util-linux `su`
/// permutes options past the login name (`su - root -c LINE`).
fn su_command(argv: &[String], shorts: &'static str, longs: &[&'static str]) -> Option<String> {
    let mut i = 1;
    while let Some(tok) = argv.get(i) {
        if tok == "--" {
            return None;
        }
        if !tok.starts_with('-') {
            i += 1;
            continue;
        }
        let (option, next) = wrapper_option(argv, i, shorts, longs);
        if let Some(("c" | "command" | "session-command", Some(line))) = option {
            return Some(line.to_owned());
        }
        i = next;
    }
    None
}

/// Is this token a flag (skipped when scanning path operands)?
/// `-rf`, `--recursive`, and cmd-style switches `/s /q /f /a:h`.
fn is_flag(token: &str) -> bool {
    if token.starts_with('-') {
        return true;
    }
    // cmd.exe switch: `/` + one or two letters, optionally `:value`.
    let rest = token.strip_prefix('/').unwrap_or("");
    if rest.is_empty() {
        return false; // bare `/` is the root path, never a flag
    }
    let head: String = rest.chars().take_while(|c| *c != ':').collect();
    (1..=2).contains(&head.len()) && head.chars().all(|c| c.is_ascii_alphabetic())
}

/// The destructive-deletion hit for a fully-formed argv, or `None`.
///
/// Escalators and command wrappers (`env`, `nohup`, `timeout`, ...) are
/// stripped first, so `sudo env rm -rf /usr` is classified as `rm`; an
/// interpreter payload (`sh -c`, `pwsh -Command`, `cmd /c`, `wsl`) is
/// scanned as a line. Relative operands resolve against `cwd`.
#[must_use]
pub fn argv_deletion_hit(argv: &[impl AsRef<str>], cwd: Option<&Path>) -> Option<OsGuardHit> {
    let owned: Vec<String> = argv.iter().map(|a| a.as_ref().to_owned()).collect();
    let cwd = cwd.map(|p| p.to_string_lossy().into_owned());
    argv_hit(&owned, cwd.as_deref(), 0)
}

/// The destructive-deletion hit for a line `shell` will run.
///
/// The line is split into simple commands with that shell's grammar (see
/// [`Grammar`]), then each is run through the argv scan. `cd` /
/// `Set-Location` in the line move the cwd later commands resolve against.
#[must_use]
pub fn shell_line_deletion_hit(line: &str, shell: &str, cwd: Option<&Path>) -> Option<OsGuardHit> {
    let cwd = cwd.map(|p| p.to_string_lossy().into_owned());
    line_hit(line, Grammar::of_shell(shell), cwd, 0)
}

fn argv_hit(argv: &[String], cwd: Option<&str>, depth: usize) -> Option<OsGuardHit> {
    if depth > MAX_NESTING {
        return None;
    }
    let (unwrapped, cwd) = unwrap_launchers(argv, cwd);
    let cwd = cwd.as_deref();
    let real = match unwrapped {
        Unwrapped::Argv(real) => real,
        Unwrapped::Line(line) => {
            return line_hit(&line, Grammar::Posix, cwd.map(str::to_owned), depth + 1);
        }
    };
    let (name_tok, operands) = real.split_first()?;
    if let Some(shell) = shell_interpreter_denied(name_tok) {
        let (grammar, payload) = interpreter_payload(shell, operands)?;
        return line_hit(&payload, grammar, cwd.map(str::to_owned), depth + 1);
    }
    if is_wsl_carrier(name_tok) {
        return wsl_hit(operands, cwd, depth + 1);
    }
    let name = command_basename(name_tok);
    let kind = classify(&name)?;

    match kind {
        DestructiveKind::PathOperands => {
            for operand in operands {
                if is_flag(operand) {
                    continue;
                }
                if operand_is_protected(operand, cwd) {
                    return Some(OsGuardHit {
                        op: name,
                        path: operand.clone(),
                    });
                }
            }
        }
        DestructiveKind::DdOutfile => {
            for operand in operands {
                if let Some(target) = operand.strip_prefix("of=")
                    && (is_disk_device(target) || is_os_critical_path(target))
                {
                    return Some(OsGuardHit {
                        op: name,
                        path: target.to_owned(),
                    });
                }
            }
        }
        DestructiveKind::Format => {
            for operand in operands {
                if !is_flag(operand) && is_os_critical_path(operand) {
                    return Some(OsGuardHit {
                        op: name,
                        path: operand.clone(),
                    });
                }
            }
        }
        DestructiveKind::Cipher => {
            for operand in operands {
                if let Some(target) = operand
                    .strip_prefix("/w:")
                    .or_else(|| operand.strip_prefix("/W:"))
                    && is_os_critical_path(target)
                {
                    return Some(OsGuardHit {
                        op: name,
                        path: target.to_owned(),
                    });
                }
            }
        }
    }
    None
}

/// The line a listed interpreter runs from its script flag, and the grammar
/// it is written in. `None` when there is no script flag (`bash build.sh`).
fn interpreter_payload(shell: &str, args: &[String]) -> Option<(Grammar, String)> {
    let at = interpreter_script_flag(shell, args)?;
    let flag = args[at].as_str();
    let rest = &args[at + 1..];
    let grammar = Grammar::of_shell(shell);
    let payload = match grammar {
        Grammar::Pwsh => pwsh_payload(flag, rest)?,
        Grammar::Cmd => cmd_payload(flag, rest),
        Grammar::Posix => posix_payload(flag, rest)?,
    };
    Some((grammar, payload))
}

/// `-Command WORDS...` joins the rest; `-CommandWithArgs LINE ARGS` takes
/// one word; `-EncodedCommand B64` (any `-e...` prefix, `-ec`) decodes it.
fn pwsh_payload(flag: &str, rest: &[String]) -> Option<String> {
    let name = flag.trim_start_matches(['-', '/']).to_ascii_lowercase();
    if name.starts_with('e') {
        return decode_utf16le_base64(rest.first()?);
    }
    if name == "cwa" || (name.len() > "command".len() && "commandwithargs".starts_with(&name)) {
        return rest.first().cloned();
    }
    Some(rest.join(" "))
}

/// cmd runs the rest of its command line after `/c`, `/k`, or `/r`,
/// including a command glued to the switch (`/crd` is `/c rd`).
fn cmd_payload(flag: &str, rest: &[String]) -> String {
    let glued = flag
        .char_indices()
        .find(|&(at, ch)| {
            matches!(ch.to_ascii_lowercase(), 'c' | 'k' | 'r') && flag[..at].ends_with('/')
        })
        .map_or("", |(at, _)| &flag[at + 1..]);
    std::iter::once(glued)
        .chain(rest.iter().map(String::as_str))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command string of `sh -c`: the first operand after the options,
/// past `--`; a `--name=VALUE` flag (`fish --command=...`) carries it.
fn posix_payload(flag: &str, rest: &[String]) -> Option<String> {
    if let Some((_, inline)) = flag
        .strip_prefix("--")
        .and_then(|long| long.split_once('='))
    {
        return Some(inline.to_owned());
    }
    let mut words = rest.iter().skip(posix_option_value_words(flag));
    while let Some(word) = words.next() {
        if word == "--" {
            return words.next().cloned();
        }
        if word.len() > 1 && word.starts_with(['-', '+']) {
            for _ in 0..posix_option_value_words(word) {
                words.next();
            }
            continue;
        }
        return Some(word.clone());
    }
    None
}

/// PowerShell `-EncodedCommand` text: base64 of UTF-16LE. `None` when it is
/// not base64.
fn decode_utf16le_base64(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    let mut bits = 0u32;
    let mut pending = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        bits = ((bits << 6) | u32::from(value)) & 0xFFFF;
        pending += 6;
        if pending >= 8 {
            pending -= 8;
            bytes.push(((bits >> pending) & 0xFF).to_le_bytes()[0]);
        }
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// `wsl -e PROG ARGS` runs an argv; `wsl [OPTIONS] [--] WORDS` runs WORDS as
/// a line in the distro's default shell. `--cd DIR` moves the cwd.
fn wsl_hit(args: &[String], cwd: Option<&str>, depth: usize) -> Option<OsGuardHit> {
    let mut cwd = cwd.map(str::to_owned);
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        let lower = arg.to_ascii_lowercase();
        if lower == "-e" || lower == "--exec" {
            return argv_hit(&args[i + 1..], cwd.as_deref(), depth);
        }
        if lower == "--" {
            i += 1;
            break;
        }
        if !arg.starts_with('-') {
            break;
        }
        if WSL_VALUE_OPTS.contains(&lower.as_str()) {
            if lower == "--cd" {
                cwd = args
                    .get(i + 1)
                    .and_then(|dir| resolve_dir(dir, cwd.as_deref()));
            }
            i += 1;
        }
        i += 1;
    }
    // wsl hands the rest of its command line, quotes included, to the shell.
    let line = args[i.min(args.len())..]
        .iter()
        .map(|word| {
            if word.is_empty() || word.contains([' ', '\t', '"']) {
                format!("\"{}\"", word.replace('"', "\\\""))
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    line_hit(&line, Grammar::Posix, cwd, depth)
}

fn line_hit(
    line: &str,
    grammar: Grammar,
    mut cwd: Option<String>,
    depth: usize,
) -> Option<OsGuardHit> {
    if depth > MAX_NESTING {
        return None;
    }
    let split = split_line(line, grammar);
    for nested in &split.nested {
        if let Some(hit) = line_hit(nested, grammar, cwd.clone(), depth + 1) {
            return Some(hit);
        }
    }
    // cwd on entry to each open POSIX `( ... )` subshell, restored on exit.
    let mut saved: Vec<Option<String>> = Vec::new();
    for (level, command) in &split.commands {
        if grammar == Grammar::Posix {
            while saved.len() < *level {
                saved.push(cwd.clone());
            }
            while saved.len() > *level {
                cwd = saved.pop().flatten();
            }
        }
        let command = if grammar == Grammar::Posix {
            let start = command
                .iter()
                .position(|word| !POSIX_RESERVED.contains(&word.as_str()))
                .unwrap_or(command.len());
            &command[start..]
        } else {
            command.as_slice()
        };
        let Some((head, args)) = command.split_first() else {
            continue;
        };
        let head = command_basename(head);
        if grammar == Grammar::Posix && head == "eval" {
            if let Some(hit) = line_hit(&args.join(" "), grammar, cwd.clone(), depth + 1) {
                return Some(hit);
            }
            continue;
        }
        if changes_dir(&head, grammar) {
            cwd = cd_target(args, grammar).and_then(|dir| resolve_dir(dir, cwd.as_deref()));
            continue;
        }
        if let Some(hit) = argv_hit(command, cwd.as_deref(), depth + 1) {
            return Some(hit);
        }
    }
    None
}

/// Does `head` change the directory later commands in the line run in?
fn changes_dir(head: &str, grammar: Grammar) -> bool {
    let names: &[&str] = match grammar {
        Grammar::Posix => &["cd", "pushd"],
        Grammar::Cmd => &["cd", "chdir", "pushd"],
        Grammar::Pwsh => &[
            "cd",
            "chdir",
            "sl",
            "set-location",
            "pushd",
            "push-location",
        ],
    };
    names.contains(&head)
}

/// The directory a [`changes_dir`] command moves to, `None` when it is not
/// known (`cd`, `cd -`, `cd ~`).
fn cd_target(args: &[String], grammar: Grammar) -> Option<&str> {
    let mut args = args.iter().map(String::as_str);
    let mut target = None;
    while let Some(arg) = args.next() {
        if grammar == Grammar::Pwsh && arg.starts_with('-') {
            let name = arg.to_ascii_lowercase();
            if matches!(name.as_str(), "-path" | "-literalpath" | "-lp" | "-pspath") {
                target = args.next();
                break;
            }
            if name == "-stackname" {
                args.next();
            }
            continue;
        }
        if is_flag(arg) {
            continue; // `cd -P`, `cd /d`
        }
        target = Some(arg);
        break;
    }
    target.filter(|dir| !is_unresolvable(dir) && !dir.starts_with('-'))
}

/// `dir` against `cwd`: rooted or drive paths stand alone, a relative one
/// joins `cwd` (`None` when `cwd` is unknown).
fn resolve_dir(dir: &str, cwd: Option<&str>) -> Option<String> {
    if is_unresolvable(dir) {
        return None;
    }
    let plain = dir.replace('\\', "/");
    if plain.starts_with('/') || has_drive(&plain) {
        return Some(dir.to_owned());
    }
    cwd.map(|cwd| format!("{cwd}/{dir}"))
}

/// A word whose value the shell supplies (`$VAR`, `~`, `%VAR%`, a `$(...)`
/// placeholder): never resolved against cwd.
fn is_unresolvable(word: &str) -> bool {
    word.starts_with(['$', '~', '%'])
}

fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// A line split into simple commands, each with its `( ... )` nesting
/// level, plus the bodies of `$(...)` and backtick substitutions, which run
/// as lines of their own.
#[derive(Default)]
struct SplitLine {
    commands: Vec<(usize, Vec<String>)>,
    nested: Vec<String>,
}

/// The simple command being assembled by [`split_line`].
#[derive(Default)]
struct Words {
    current: Vec<String>,
    word: String,
    has_word: bool,
    /// The next word is a redirection target, not an operand.
    skip_target: bool,
    /// Open `(` count.
    level: usize,
}

impl Words {
    fn push(&mut self, ch: char) {
        self.word.push(ch);
        self.has_word = true;
    }

    fn flush(&mut self) {
        if !self.has_word {
            return;
        }
        let word = std::mem::take(&mut self.word);
        self.has_word = false;
        if !std::mem::take(&mut self.skip_target) {
            self.current.push(word);
        }
    }

    fn end_command(&mut self, commands: &mut Vec<(usize, Vec<String>)>) {
        self.flush();
        self.skip_target = false;
        if !self.current.is_empty() {
            commands.push((self.level, std::mem::take(&mut self.current)));
        }
    }

    /// A redirection (`>`, `>>`, `2>&1`, `&>`, `*>`, `<`, `<<`): drop an fd
    /// number in front of it and the target word after it.
    fn redirect(&mut self, chars: &mut Peekable<Chars<'_>>) {
        if self
            .word
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '*' || ch == '&')
        {
            self.word.clear();
            self.has_word = false;
        } else {
            self.flush();
        }
        while chars
            .next_if(|ch| matches!(ch, '>' | '<' | '&' | '|'))
            .is_some()
        {}
        self.skip_target = true;
    }
}

/// Split a line into simple commands, each a `Vec<String>` of words, using
/// the grammar of the shell that runs it. Commands end at `;`, `&&`, `||`,
/// `|`, `&`, newlines, and `(` `)` (and `{` `}` in PowerShell).
/// Redirections and comments are dropped; substitution bodies go to
/// [`SplitLine::nested`] and leave a `$` placeholder in their word.
fn split_line(line: &str, grammar: Grammar) -> SplitLine {
    let escape = match grammar {
        Grammar::Posix => '\\',
        Grammar::Pwsh => '`',
        Grammar::Cmd => '^',
    };
    let substitutes = grammar != Grammar::Cmd;
    let mut out = SplitLine::default();
    let mut words = Words::default();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if quote == Some('\'') {
            if ch != '\'' {
                words.push(ch);
            } else if grammar == Grammar::Pwsh && chars.next_if_eq(&'\'').is_some() {
                words.push('\''); // `''` is a quote inside PowerShell '...'
            } else {
                quote = None;
            }
            continue;
        }
        if quote == Some('"') {
            match ch {
                '"' => quote = None,
                '$' if substitutes && chars.next_if_eq(&'(').is_some() => {
                    out.nested.push(capture_parens(&mut chars));
                    words.push('$');
                }
                '`' if grammar == Grammar::Posix => {
                    out.nested.push(capture_backticks(&mut chars));
                    words.push('$');
                }
                // POSIX `\` escapes only `$ ` " \ newline` inside "...";
                // PowerShell's backtick escapes anything; cmd has none.
                _ if ch == escape && grammar != Grammar::Cmd => match chars.peek() {
                    Some(&next)
                        if grammar == Grammar::Pwsh
                            || matches!(next, '$' | '`' | '"' | '\\' | '\n') =>
                    {
                        chars.next();
                        words.push(next);
                    }
                    _ => words.push(ch),
                },
                _ => words.push(ch),
            }
            continue;
        }
        match ch {
            _ if ch == escape => {
                if let Some(next) = chars.next()
                    && next != '\n'
                {
                    words.push(next);
                }
            }
            '\'' if grammar != Grammar::Cmd => {
                quote = Some('\'');
                words.has_word = true;
            }
            '"' => {
                quote = Some('"');
                words.has_word = true;
            }
            '$' if substitutes && chars.next_if_eq(&'(').is_some() => {
                out.nested.push(capture_parens(&mut chars));
                words.push('$');
            }
            '`' if grammar == Grammar::Posix => {
                out.nested.push(capture_backticks(&mut chars));
                words.push('$');
            }
            '#' if substitutes && !words.has_word => {
                while chars.next_if(|next| *next != '\n').is_some() {}
            }
            ' ' | '\t' | '\r' => words.flush(),
            '>' | '<' => words.redirect(&mut chars),
            '&' if grammar != Grammar::Cmd && chars.peek() == Some(&'>') => {
                words.redirect(&mut chars);
            }
            ';' | '\n' | '|' | '&' => {
                // `&&` / `||` collapse to one boundary.
                if matches!(ch, '&' | '|') {
                    chars.next_if_eq(&ch);
                }
                words.end_command(&mut out.commands);
            }
            '(' => {
                words.end_command(&mut out.commands);
                words.level += 1;
            }
            ')' => {
                words.end_command(&mut out.commands);
                words.level = words.level.saturating_sub(1);
            }
            '{' | '}' if grammar == Grammar::Pwsh => words.end_command(&mut out.commands),
            _ => words.push(ch),
        }
    }
    words.end_command(&mut out.commands);
    out
}

/// The body of a `$(...)` whose `$(` was just read, up to its matching `)`.
fn capture_parens(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut body = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    for ch in chars.by_ref() {
        match (quote, ch) {
            (Some(open), _) if ch == open => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => return body,
            (None, ')') => depth -= 1,
            _ => {}
        }
        body.push(ch);
    }
    body
}

/// The body of a backtick substitution whose opening backtick was just read.
fn capture_backticks(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut body = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '`' => break,
            '\\' => body.extend(chars.next()),
            _ => body.push(ch),
        }
    }
    body
}

/// A raw-disk device target for `dd of=` / `mkfs` (`/dev/sda`, `/dev/nvme0n1`,
/// `/dev/mmcblk0`, `\\.\PhysicalDrive0`).
#[must_use]
pub fn is_disk_device(raw: &str) -> bool {
    let slashed = raw.replace('\\', "/");
    let lower = slashed.to_ascii_lowercase();
    if lower.contains("physicaldrive") {
        return true;
    }
    let Some(dev) = lower.strip_prefix("/dev/") else {
        return false;
    };
    // Whole disks and partitions, plus the device-mapper / md / nbd names a
    // root filesystem commonly lives on (`/dev/mapper/ubuntu--vg-root`,
    // `/dev/dm-0`, `/dev/md0`). `loop` devices are left alone (dev use).
    [
        "sd", "nvme", "hd", "disk", "mmcblk", "vd", "xvd", "dm-", "mapper/", "md", "nbd",
        "disk/by-",
    ]
    .iter()
    .any(|p| dev.starts_with(p))
}

/// A path operand as a deletion command sees it, resolved against `cwd`.
///
/// Verbatim prefixes are resolved first (their `?` is not a glob), then the
/// fixed prefix before any glob metacharacter is checked with
/// [`is_os_critical_path`]: `/usr/*` -> `/usr` -> protected; `/tmp/*` ->
/// `/tmp` -> allowed; a bare `*` names `cwd` itself. A rooted `\Windows` is
/// checked both as-is and on the cwd's drive.
#[must_use]
pub fn operand_is_protected(operand: &str, cwd: Option<&str>) -> bool {
    let Some(path) = plain_path(operand) else {
        return true; // device or volume object
    };
    let fixed = glob_fixed_prefix(&path);
    if fixed.is_empty() {
        // `*`, `.*`, `[ab]*`: the glob expands inside cwd.
        return !path.is_empty() && !is_unresolvable(&path) && cwd.is_some_and(is_os_critical_path);
    }
    if is_unresolvable(&fixed) {
        return false;
    }
    if has_drive(&fixed) {
        return is_os_critical_path(&fixed);
    }
    if fixed.starts_with('/') {
        let drive = cwd.map(str::trim_start).filter(|cwd| has_drive(cwd));
        return is_os_critical_path(&fixed)
            || (!fixed.starts_with("//")
                && drive.is_some_and(|cwd| is_os_critical_path(&format!("{}{fixed}", &cwd[..2]))));
    }
    cwd.map_or_else(
        || is_os_critical_path(&fixed),
        |cwd| is_os_critical_path(&format!("{}/{fixed}", cwd.trim_end_matches('/'))),
    )
}

/// The path prefix before the first glob metacharacter (`* ? [`), trimmed to
/// the last separator so a partial component is not treated as a real path.
fn glob_fixed_prefix(operand: &str) -> String {
    operand.find(['*', '?', '[']).map_or_else(
        || operand.to_owned(),
        |idx| {
            let head = &operand[..idx];
            // Keep the separator so `/*` -> `/` (root), `/usr/*` -> `/usr/`.
            head.rfind(['/', '\\'])
                .map_or_else(String::new, |sep| head[..=sep].to_owned())
        },
    )
}

/// Forward-slash `raw`, resolve a Windows verbatim or device prefix (`\\?\`,
/// `\\.\`, `//?/`), and collapse repeated separators. `None` for a device or
/// volume object (`\\?\Volume{..}`, `\\.\PhysicalDrive0`, `\\?\GLOBALROOT`),
/// which is always protected.
fn plain_path(raw: &str) -> Option<String> {
    let slashed = raw.trim().replace('\\', "/");
    let path = match ["//?/", "//./"]
        .iter()
        .find_map(|prefix| slashed.strip_prefix(prefix))
    {
        None => slashed.clone(),
        Some(rest) if has_drive(rest) => rest.to_owned(),
        Some(rest)
            if rest
                .get(..4)
                .is_some_and(|head| head.eq_ignore_ascii_case("unc/")) =>
        {
            format!("//{}", &rest[4..])
        }
        Some(_) => return None,
    };
    Some(collapse_separators(&path))
}

/// Collapse runs of `/`. A leading `//` survives only as a UNC
/// `//server/share` (single separators) on a Windows host; anywhere else it
/// is the root, so `//usr//lib` is `/usr/lib`.
fn collapse_separators(path: &str) -> String {
    let mut parts = path.strip_prefix("//").unwrap_or_default().split('/');
    let unc = cfg!(windows)
        && path.starts_with("//")
        && parts.next().is_some_and(|server| !server.is_empty())
        && parts.next().is_some_and(|share| !share.is_empty());
    let mut out = String::with_capacity(path.len());
    if unc {
        out.push('/');
    }
    for ch in path.chars() {
        if !(ch == '/' && out.ends_with('/') && out.len() > usize::from(unc)) {
            out.push(ch);
        }
    }
    out
}

/// Unix protected TREES: the directory and everything beneath it is OS
/// infrastructure, so removal of any of it is refused. `/usr/local` is
/// deliberately absent (user-installed software), as is most of `/etc` and
/// `/var/lib` (package configuration and application state are the owner's
/// to manage -- only the package databases and the init/auth trees are here).
const UNIX_PROTECTED_TREES: &[&str] = &[
    "/boot",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/lib32",
    "/usr/lib64",
    "/usr/libexec",
    "/usr/share",
    "/sys",
    "/proc",
    "/etc/systemd",
    "/etc/pam.d",
    "/etc/ssh",
    "/etc/sudoers.d",
    "/etc/ld.so.conf.d",
    "/var/lib/dpkg",
    "/var/lib/rpm",
    "/var/lib/pacman",
    "/var/lib/systemd",
    "/System",
    "/private/etc",
];

/// Unix protected ROOTS: removing the directory itself (or an ancestor of
/// it) is refused, but its descendants are ordinary files -- so
/// `/etc/nginx/sites-enabled/default`, `/var/lib/apt/lists/lock`,
/// `/usr/local/bin/x`, `/dev/shm/x` and `/opt/tool` stay deletable.
const UNIX_PROTECTED_ROOTS: &[&str] = &[
    "/usr",
    "/usr/local",
    "/etc",
    "/var",
    "/var/lib",
    "/dev",
    "/opt",
    "/home",
];

/// Unix CRITICAL FILES: single files whose removal breaks login, boot, name
/// resolution or the dynamic loader. Always refused.
const UNIX_CRITICAL_FILES: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/group",
    "/etc/gshadow",
    "/etc/sudoers",
    "/etc/fstab",
    "/etc/hosts",
    "/etc/hostname",
    "/etc/ld.so.conf",
    "/etc/ld.so.cache",
];

/// Windows system subtrees on the system drive (compared case-insensitively,
/// forward-slashed, drive-prefixed at match time). `hosts` etc. live under
/// these but are writable -- only deletion is gated.
const WINDOWS_PROTECTED_SUBTREES: &[&str] = &[
    "/windows",
    "/boot",
    "/efi",
    "/recovery",
    "/system volume information",
    "/bootmgr",
    "/msdos.sys",
    "/io.sys",
    "/ntldr",
    "/pagefile.sys",
    "/hiberfil.sys",
    "/swapfile.sys",
];

/// Windows roots whose descendants are ordinary (removing the root itself is
/// refused): `Program Files` and user profiles hold installed software and
/// data the owner manages; the drive root is handled separately.
const WINDOWS_PROTECTED_ROOTS: &[&str] = &["/program files", "/program files (x86)", "/users"];

/// The system drive letter (lowercase, e.g. `c:`), read from the daemon's
/// environment so a Windows installed on another letter is still protected.
/// ponytail: read once per call; cache if this ever shows up in a profile.
fn system_drive() -> String {
    std::env::var("SystemDrive")
        .ok()
        .filter(|d| {
            d.len() == 2 && d.as_bytes()[1] == b':' && d.as_bytes()[0].is_ascii_alphabetic()
        })
        .map_or_else(|| "c:".to_owned(), |d| d.to_ascii_lowercase())
}

/// Is `raw` an OS-critical location (protected tree root, a path beneath one,
/// an ancestor of one, a drive/volume root, or a raw disk device)?
///
/// Normalizes verbatim/device prefixes, slash direction, repeated
/// separators, case (Windows only), trailing separators, `.`/`..`, WSL
/// `/mnt/<drive>` bridging, and drive-relative `C:foo`.
#[must_use]
pub fn is_os_critical_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_disk_device(trimmed) {
        return true;
    }
    // `\\?\Volume{...}` / `\\.\...` device or volume objects.
    let Some(path) = plain_path(trimmed) else {
        return true;
    };

    // WSL bridge: /mnt/<drive>/... maps to <drive>:/...
    if let Some(mapped) = wsl_mount_to_windows(&path) {
        return windows_protected(&mapped);
    }

    if looks_windows(&path) {
        windows_protected(&path)
    } else {
        unix_protected(&path)
    }
}

/// `/mnt/c/Windows` -> `c:/Windows`. `None` when not a `/mnt/<letter>` path.
fn wsl_mount_to_windows(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/mnt/")?;
    let mut parts = rest.splitn(2, '/');
    let drive = parts.next()?;
    if drive.len() != 1 || !drive.chars().next().unwrap().is_ascii_alphabetic() {
        return None;
    }
    let tail = parts.next().unwrap_or("");
    Some(format!("{drive}:/{tail}"))
}

fn looks_windows(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with("//") // UNC
}

/// Lexically collapse `.` / `..` without touching the filesystem, keeping a
/// leading `/` (unix) or drive prefix. `..` never rises above the root.
fn collapse(path: &str) -> String {
    let p = Path::new(path);
    let mut out = PathBuf::new();
    let mut prefix = String::new();
    let mut had_root = false;
    for comp in p.components() {
        match comp {
            Component::Prefix(pre) => prefix = pre.as_os_str().to_string_lossy().into_owned(),
            Component::RootDir => had_root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    let tail = out.to_string_lossy().replace('\\', "/");
    let root = if had_root { "/" } else { "" };
    let joined = format!("{prefix}{root}{tail}");
    let trimmed = joined.trim_end_matches('/');
    if trimmed.is_empty() {
        joined
    } else {
        trimmed.to_owned()
    }
}

fn unix_protected(path: &str) -> bool {
    let norm = collapse(path);
    if norm == "/" {
        return true;
    }
    UNIX_PROTECTED_TREES
        .iter()
        .any(|root| tree_hit(&norm, root))
        || UNIX_PROTECTED_ROOTS
            .iter()
            .any(|root| root_hit(&norm, root))
        || UNIX_CRITICAL_FILES.contains(&norm.as_str())
}

/// True when `path` IS `root` or an ancestor of it (removing the ancestor
/// takes the root with it); descendants of `root` are NOT hits.
fn root_hit(path: &str, root: &str) -> bool {
    path == root || root.starts_with(&format!("{path}/"))
}

fn windows_protected(path: &str) -> bool {
    // Lowercase for case-insensitive comparison; drive-relative `c:foo`
    // becomes `c:/foo` so it is checked as drive-absolute (fail-safe).
    let lower = path.to_ascii_lowercase();
    let lower = if lower.len() >= 2
        && lower.as_bytes()[1] == b':'
        && lower.as_bytes().get(2).is_none_or(|c| *c != b'/')
    {
        format!("{}/{}", &lower[..2], &lower[2..])
    } else {
        lower
    };
    // Windows ignores trailing dots and spaces on EVERY path segment
    // (`C:\Windows.\System32` is the real System32), so strip them per
    // segment before matching.
    let lower = lower
        .split('/')
        .map(|seg| seg.trim_end_matches(['.', ' ']))
        .collect::<Vec<_>>()
        .join("/");
    let norm = collapse(&lower);

    // Drive root: `c:` or `c:/`.
    if norm.len() == 2 && norm.as_bytes()[1] == b':' {
        return true;
    }
    // UNC share root `//server/share` -- deleting a share root is destructive.
    if norm.starts_with("//") {
        return norm
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .count()
            <= 2;
    }
    // System-drive subtrees and roots; the drive letter comes from the
    // daemon's `%SystemDrive%` (falls back to `c:`). A trailing dot on a
    // Windows name (`C:\Windows.`) is ignored by the OS, so strip it.
    let drive = system_drive();
    let Some(after_drive) = norm.strip_prefix(drive.as_str()) else {
        return false;
    };
    let after_drive = after_drive.trim_end_matches('.');
    WINDOWS_PROTECTED_SUBTREES
        .iter()
        .any(|sub| tree_hit(after_drive, sub))
        || WINDOWS_PROTECTED_ROOTS
            .iter()
            .any(|root| root_hit(after_drive, root))
}

/// True when `path` equals `root`, sits beneath it, or is an ancestor of it
/// (deleting the ancestor takes the protected root with it).
fn tree_hit(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/")) || root.starts_with(&format!("{path}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn protected_paths_unix() {
        for p in [
            "/",
            "/usr",
            "/usr/lib",
            "/etc",
            "/etc/hosts",
            "/boot",
            "/bin",
            "/lib64",
            "/sys",
            "/proc",
            "/dev",
            "/dev/sda",
            "/var/lib",
            "/var/lib/dpkg/status",
            "/var", // ancestor of /var/lib
            "/usr/local",
            "/etc/passwd",
            "/etc/sudoers.d/dev",
            "/System",
            "/private/etc",
            "/usr/../usr/bin",
            "/etc/./ssh",
            "/home",
            "/opt",
        ] {
            assert!(is_os_critical_path(p), "must protect {p}");
        }
    }

    #[test]
    fn allowed_paths_unix() {
        for p in [
            "/home/dev/project",
            "/tmp",
            "/tmp/x",
            "/var/tmp",
            "/opt/tool",
            "/usr2",
            "/etcx",
            "target",
            "./build",
            "/home/dev/.cache",
            // Owner rule: configure, install and uninstall freely --
            // only OS infrastructure itself is untouchable.
            "/usr/local/bin/tool",
            "/usr/local/src/proj/build",
            "/etc/nginx/sites-enabled/default",
            "/etc/resolv.conf",
            "/var/lib/apt/lists/lock",
            "/var/lib/docker",
            "/dev/shm/x",
        ] {
            assert!(!is_os_critical_path(p), "must allow {p}");
        }
    }

    #[test]
    fn protected_paths_windows() {
        for p in [
            r"C:\",
            r"C:\Windows",
            r"C:\Windows\System32",
            r"c:/windows/system32",
            r"C:\WINDOWS\",
            r"\\?\C:\Windows",
            r"C:\Boot",
            r"C:\EFI",
            r"C:\Recovery",
            r"C:\System Volume Information",
            r"C:\bootmgr",
            r"C:Windows",
            r"/mnt/c/Windows",
            r"/mnt/c/WINDOWS/System32",
            r"\\.\PhysicalDrive0",
            r"\\?\Volume{2c1cd3a1-0000-0000-0000-100000000000}\",
            r"C:\Windows.",
            r"C:\Windows.\System32",
            r"C:\Windows \System32\drivers",
            r"C:\Windows.\guard-nx",
            r"C:\pagefile.sys",
            r"C:\Program Files",
            r"C:\Users",
        ] {
            assert!(is_os_critical_path(p), "must protect {p}");
        }
    }

    #[test]
    fn allowed_paths_windows() {
        for p in [
            r"C:\Users\dev\project",
            r"C:\Users\dev\node_modules",
            r"D:\data",
            r"C:\temp\build",
            r"/mnt/c/Users/dev/app",
            r"C:\Windows2",
            r"C:\Program Files\SomeApp",
            r"C:\Program Files (x86)\Old\bin",
        ] {
            assert!(!is_os_critical_path(p), "must allow {p}");
        }
    }

    #[test]
    fn device_mapper_and_launcher_chdir_are_covered() {
        // C1: root filesystems on LVM / md / device-mapper names.
        for dev in [
            "/dev/mapper/ubuntu--vg-root",
            "/dev/dm-0",
            "/dev/md0",
            "/dev/nbd0",
        ] {
            assert!(is_disk_device(dev), "{dev} is a disk device");
            assert!(argv_deletion_hit(&argv(&["mkfs.ext4", dev]), None).is_some());
            assert!(argv_deletion_hit(&argv(&["wipefs", "-a", dev]), None).is_some());
        }
        assert!(!is_disk_device("/dev/loop0"), "loop devices stay usable");
        // W1: launchers that move the working directory before the command.
        let victim = "usr/lib/tc-guard-nonexistent";
        for case in [
            argv(&["env", "-C", "/", "rm", "-rf", victim]),
            argv(&["env", "--chdir=/", "rm", "-rf", victim]),
            argv(&["sudo", "--chdir=/", "rm", "-rf", victim]),
            argv(&["sudo", "-D", "/", "rm", "-rf", victim]),
            argv(&["run0", "-D/", "rm", "-rf", victim]),
        ] {
            assert!(
                argv_deletion_hit(&case, Some(Path::new("/tmp"))).is_some(),
                "launcher chdir must be tracked: {case:?}"
            );
        }
        assert!(
            argv_deletion_hit(
                &argv(&["env", "-C", "/tmp", "rm", "-rf", "build"]),
                Some(Path::new("/"))
            )
            .is_none(),
            "chdir into an ordinary directory stays allowed"
        );
    }

    #[test]
    fn rm_rf_root_and_system_denied() {
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/"]), None).is_some());
        assert!(
            argv_deletion_hit(&argv(&["rm", "-rf", "--no-preserve-root", "/"]), None).is_some()
        );
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/usr"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["sudo", "rm", "-rf", "/etc"]), None).is_some());
        assert!(
            argv_deletion_hit(&argv(&["sudo", "-u", "root", "rm", "-rf", "/boot"]), None).is_some()
        );
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/usr/*"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["shred", "/dev/sda"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["wipefs", "-a", "/dev/nvme0n1"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["dd", "if=/dev/zero", "of=/dev/sda"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["mkfs.ext4", "/dev/sdb1"]), None).is_some());
    }

    #[test]
    fn windows_deletion_denied() {
        assert!(
            argv_deletion_hit(&argv(&["Remove-Item", "-Recurse", r"C:\Windows"]), None).is_some()
        );
        assert!(
            argv_deletion_hit(&argv(&["del", "/S", "/Q", r"C:\Windows\System32"]), None).is_some()
        );
        assert!(argv_deletion_hit(&argv(&["rd", "/s", r"C:\Boot"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["format", "c:"]), None).is_some());
        assert!(argv_deletion_hit(&argv(&["cipher", "/w:C:\\"]), None).is_some());
    }

    #[test]
    fn ordinary_deletions_allowed() {
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "target"]), None).is_none());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/tmp/x"]), None).is_none());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "node_modules"]), None).is_none());
        assert!(argv_deletion_hit(&argv(&["del", "build\\out.txt"]), None).is_none());
        assert!(
            argv_deletion_hit(&argv(&["Remove-Item", "-Recurse", "node_modules"]), None).is_none()
        );
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/home/dev/project/dist"]), None).is_none());
        assert!(argv_deletion_hit(&argv(&["dd", "if=/dev/zero", "of=./disk.img"]), None).is_none());
    }

    #[test]
    fn non_deletion_commands_always_allowed() {
        for cmd in [
            &["apt", "install", "-y", "cowsay"][..],
            &["winget", "install", "Git.Git"][..],
            &["cargo", "install", "ripgrep"][..],
            &["systemctl", "restart", "nginx"][..],
            &["reg", "add", r"HKLM\Software\X"][..],
            &["dism", "/online", "/cleanup-image"][..],
            &["cp", "-r", "/etc", "/tmp/etc-backup"][..],
            &["cat", "/etc/hosts"][..],
        ] {
            assert!(
                argv_deletion_hit(&argv(cmd), None).is_none(),
                "must allow {cmd:?}"
            );
        }
    }

    #[test]
    fn shell_line_scan() {
        assert!(shell_line_deletion_hit("cd /tmp && rm -rf /usr", "sh", None).is_some());
        assert!(shell_line_deletion_hit("echo hi | sudo rm -rf /etc", "sh", None).is_some());
        assert!(shell_line_deletion_hit("rm -rf target; cargo build", "sh", None).is_none());
        assert!(shell_line_deletion_hit("rm -rf \"/usr/lib\"", "sh", None).is_some());
        assert!(shell_line_deletion_hit("make clean && rm -rf ./dist", "sh", None).is_none());
    }

    // ---- Parser-gap table (G1-G7). TEST SAFETY: pure functions only; every
    // protected operand is `<root>/tc-guard-nonexistent`, never a real path.

    /// Unix and Windows protected roots the refused rows target.
    const UNIX_ROOT: &str = "/usr/lib";
    const WIN_ROOT: &str = r"C:\Windows\System32";
    const UNIX_PROJECT: &str = "/home/dev/proj";
    const WIN_PROJECT: &str = r"C:\Users\dev\proj";

    fn target(root: &str) -> String {
        format!("{root}/tc-guard-nonexistent")
    }

    enum Case {
        Argv(Vec<String>),
        /// `(shell, line)` as `shell_exec` would run it.
        Line(&'static str, String),
    }

    fn a(parts: &[&str]) -> Case {
        Case::Argv(argv(parts))
    }

    fn l(shell: &'static str, line: impl Into<String>) -> Case {
        Case::Line(shell, line.into())
    }

    fn guarded(case: &Case, cwd: Option<&str>) -> bool {
        let cwd = cwd.map(Path::new);
        match case {
            Case::Argv(argv) => argv_deletion_hit(argv, cwd).is_some(),
            Case::Line(shell, line) => shell_line_deletion_hit(line, shell, cwd).is_some(),
        }
    }

    fn describe(case: &Case) -> String {
        match case {
            Case::Argv(argv) => format!("argv {argv:?}"),
            Case::Line(shell, line) => format!("{shell} line {line:?}"),
        }
    }

    /// PowerShell `-EncodedCommand` text: base64 of the UTF-16LE bytes.
    fn b64_utf16le(text: &str) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .zip([16, 8, 0])
                .fold(0u32, |acc, (byte, shift)| acc | (u32::from(*byte) << shift));
            for i in 0..=chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            }
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
        out
    }

    #[allow(clippy::too_many_lines)]
    fn refused_rows() -> Vec<(&'static str, Case, Option<&'static str>)> {
        let u = target(UNIX_ROOT);
        let w = target(WIN_ROOT);
        let wsl_w = "/mnt/c/Windows/System32/tc-guard-nonexistent";
        vec![
            // G1: interpreter payloads, recursively.
            (
                "G1 sh -c line",
                l("sh", format!("bash -c 'rm -rf {u}'")),
                None,
            ),
            (
                "G1 argv sh -c",
                a(&["sh", "-c", &format!("rm -rf {u}")]),
                None,
            ),
            (
                "G1 option run before -ec",
                a(&[
                    "bash",
                    "-x",
                    "-o",
                    "pipefail",
                    "-ec",
                    &format!("rm -rf {u}"),
                ]),
                None,
            ),
            (
                "G1 nested sh in bash",
                l("sh", format!(r#"bash -c "sh -c 'rm -rf {u}'""#)),
                None,
            ),
            (
                "G1 pwsh -Command words",
                a(&[
                    "pwsh",
                    "-NoProfile",
                    "-Command",
                    "Remove-Item",
                    "-Recurse",
                    &w,
                ]),
                None,
            ),
            (
                "G1 powershell -c line",
                a(&["powershell.exe", "-c", &format!("Remove-Item -Recurse {w}")]),
                None,
            ),
            (
                "G1 pwsh -EncodedCommand",
                a(&[
                    "pwsh",
                    "-enc",
                    &b64_utf16le(&format!("Remove-Item -Recurse {w}")),
                ]),
                None,
            ),
            (
                "G1 cmd /c",
                a(&["cmd", "/c", &format!("rd /s /q {w}")]),
                None,
            ),
            (
                "G1 cmd glued /q/crd",
                a(&["cmd.exe", "/q/crd", "/s", &w]),
                None,
            ),
            ("G1 wsl -e argv", a(&["wsl", "-e", "rm", "-rf", &u]), None),
            (
                "G1 wsl bare line",
                a(&["wsl.exe", "-d", "Ubuntu", "rm", "-rf", &u]),
                None,
            ),
            (
                "G1 wsl -- bash -c",
                a(&["wsl", "--", "bash", "-c", &format!("rm -rf {u}")]),
                None,
            ),
            (
                "G1 wsl windows mount",
                a(&["wsl", "-e", "rm", "-rf", wsl_w]),
                None,
            ),
            (
                "G1 cmd line into pwsh",
                l(
                    "cmd.exe",
                    format!(r#"pwsh -NoProfile -Command "Remove-Item -Recurse {w}""#),
                ),
                None,
            ),
            ("G1 busybox applet", a(&["busybox", "rm", "-rf", &u]), None),
            // G2: escalators and wrappers in any interleaving.
            (
                "G2 sudo -iu",
                a(&["sudo", "-iu", "root", "rm", "-rf", &u]),
                None,
            ),
            (
                "G2 wrapper then sudo",
                a(&["nice", "-n", "5", "sudo", "rm", "-rf", &u]),
                None,
            ),
            (
                "G2 interleaved chain",
                a(&[
                    "timeout", "5", "doas", "-u", "root", "env", "A=1", "pkexec", "rm", "-rf", &u,
                ]),
                None,
            ),
            (
                "G2 su -c before login",
                a(&["su", "-c", &format!("rm -rf {u}"), "root"]),
                None,
            ),
            (
                "G2 su login then -c",
                a(&["su", "-", "root", "-c", &format!("rm -rf {u}")]),
                None,
            ),
            (
                "G2 sudo sh -c",
                a(&["sudo", "-u", "root", "sh", "-c", &format!("rm -rf {u}")]),
                None,
            ),
            ("G2 run0", a(&["run0", "-u", "root", "rm", "-rf", &u]), None),
            (
                "G2 chroot",
                a(&["chroot", "/mnt/sysroot", "rm", "-rf", &u]),
                None,
            ),
            (
                "G2 sudo -iu windows mount",
                a(&["sudo", "-iu", "root", "rm", "-rf", wsl_w]),
                None,
            ),
            (
                "G2 line",
                l("bash", format!("sudo -iu root rm -rf {u}")),
                None,
            ),
            // G3: grammar from the shell that runs the line.
            (
                "G3 pwsh keeps backslashes",
                l("pwsh", format!("Remove-Item -Recurse -Force {w}")),
                None,
            ),
            (
                "G3 pwsh by path",
                l(
                    r"C:\Program Files\PowerShell\7\pwsh.exe",
                    format!("Remove-Item -Recurse {w}"),
                ),
                None,
            ),
            (
                "G3 cmd keeps backslashes",
                l("cmd.exe", format!("rd /s /q {w}")),
                None,
            ),
            (
                "G3 posix double quotes keep backslashes",
                l("bash", format!(r#"rm -rf "{w}""#)),
                None,
            ),
            (
                "G3 posix escape",
                l("bash", r"rm -rf /usr/li\b/tc-guard-nonexistent"),
                None,
            ),
            // G4: repeated separators.
            (
                "G4 doubled unix separators",
                a(&["rm", "-rf", "//usr//lib//tc-guard-nonexistent"]),
                None,
            ),
            (
                "G4 doubled windows mount",
                a(&["rm", "-rf", "/mnt//c//Windows//tc-guard-nonexistent"]),
                None,
            ),
            // G5: verbatim and device prefixes.
            (
                "G5 verbatim windows",
                l("pwsh", format!(r"Remove-Item -Recurse \\?\{w}")),
                None,
            ),
            (
                "G5 forward verbatim",
                a(&["rm", "-rf", "//?/C:/Windows/System32/tc-guard-nonexistent"]),
                None,
            ),
            (
                "G5 device namespace",
                a(&["rd", "/s", "/q", &format!(r"\\.\{w}")]),
                None,
            ),
            // G6: operands resolved against cwd, cd tracked within a line.
            (
                "G6 relative in protected cwd",
                a(&["rm", "-rf", "tc-guard-nonexistent"]),
                Some(UNIX_ROOT),
            ),
            (
                "G6 dotdot out of tmp",
                a(&["rm", "-rf", "../../usr/lib/tc-guard-nonexistent"]),
                Some("/tmp/x"),
            ),
            (
                "G6 glob in protected cwd",
                a(&["rm", "-rf", "*"]),
                Some("/usr/lib"),
            ),
            (
                "G6 windows relative",
                a(&["Remove-Item", "-Recurse", "tc-guard-nonexistent"]),
                Some(WIN_ROOT),
            ),
            (
                "G6 rooted on the cwd drive",
                l(
                    "pwsh",
                    r"Remove-Item -Recurse \Windows\System32\tc-guard-nonexistent",
                ),
                Some(WIN_PROJECT),
            ),
            (
                "G6 cd then relative",
                l("bash", "cd /usr/lib && rm -rf tc-guard-nonexistent"),
                Some("/tmp"),
            ),
            (
                "G6 pushd",
                l("bash", "pushd /usr/lib; rm -rf ./tc-guard-nonexistent"),
                None,
            ),
            (
                "G6 Set-Location",
                l(
                    "pwsh",
                    r"Set-Location C:\Windows\System32; Remove-Item -Recurse tc-guard-nonexistent",
                ),
                Some(WIN_PROJECT),
            ),
            (
                "G6 sl -Path",
                l(
                    "pwsh",
                    r"sl -Path C:\Windows; Remove-Item -Recurse System32\tc-guard-nonexistent",
                ),
                None,
            ),
            (
                "G6 cmd cd /d",
                l(
                    "cmd.exe",
                    r"cd /d C:\Windows && rd /s /q System32\tc-guard-nonexistent",
                ),
                None,
            ),
            (
                "G6 cd carried into a subshell",
                l("bash", "cd /usr/lib && (rm -rf tc-guard-nonexistent)"),
                Some(UNIX_PROJECT),
            ),
            (
                "G6 cd inside payload",
                a(&["bash", "-c", "cd /usr/lib; rm -rf tc-guard-nonexistent"]),
                None,
            ),
            // G7: compound grammar, substitutions, eval, redirections.
            ("G7 subshell", l("bash", format!("(rm -rf {u})")), None),
            (
                "G7 brace group",
                l("bash", format!("{{ rm -rf {u}; }}")),
                None,
            ),
            (
                "G7 then",
                l("bash", format!("if true; then rm -rf {u}; fi")),
                None,
            ),
            (
                "G7 do",
                l("bash", format!("for d in a; do rm -rf {u}; done")),
                None,
            ),
            (
                "G7 else",
                l("bash", format!("if false; then :; else rm -rf {u}; fi")),
                None,
            ),
            ("G7 bang", l("bash", format!("! rm -rf {u}")), None),
            ("G7 $()", l("bash", format!("echo $(rm -rf {u})")), None),
            (
                "G7 quoted $()",
                l("bash", format!(r#"echo "$(rm -rf {u})""#)),
                None,
            ),
            (
                "G7 backticks",
                l("bash", format!("echo `rm -rf {u}`")),
                None,
            ),
            ("G7 eval words", l("bash", format!("eval rm -rf {u}")), None),
            (
                "G7 eval quoted",
                l("bash", format!("eval 'rm -rf {u}'")),
                None,
            ),
            (
                "G7 2>&1 before operand",
                l("bash", format!("rm -rf 2>&1 {u}")),
                None,
            ),
            (
                "G7 &> before operand",
                l("bash", format!("rm -rf &>/dev/null {u}")),
                None,
            ),
            (
                "G7 pwsh script block",
                l("pwsh", format!("& {{ Remove-Item -Recurse {w} }}")),
                None,
            ),
            (
                "G7 pwsh subexpression",
                l(
                    "pwsh",
                    format!(r#"Write-Output "$(Remove-Item -Recurse {w})""#),
                ),
                None,
            ),
            (
                "G7 pwsh 2>&1 before operand",
                l("pwsh", format!("Remove-Item -Recurse 2>&1 {w}")),
                None,
            ),
            (
                "G7 cmd parens",
                l("cmd.exe", format!("if exist x (rd /s /q {w})")),
                None,
            ),
        ]
    }

    #[allow(clippy::too_many_lines)]
    fn allowed_rows() -> Vec<(&'static str, Case, Option<&'static str>)> {
        let verbatim_profile = r"\\?\C:\Users\dev\tc-guard-nonexistent";
        vec![
            ("rm target", a(&["rm", "-rf", "target"]), Some(UNIX_PROJECT)),
            ("rm build", a(&["rm", "-rf", "build"]), Some(UNIX_PROJECT)),
            (
                "rm node_modules",
                a(&["rm", "-rf", "node_modules"]),
                Some(WIN_PROJECT),
            ),
            (
                "Remove-Item target",
                a(&["Remove-Item", "-Recurse", "target"]),
                Some(WIN_PROJECT),
            ),
            (
                "bash line",
                l("bash", "rm -rf target build node_modules"),
                Some(UNIX_PROJECT),
            ),
            (
                "pwsh line",
                l(
                    "pwsh",
                    "Remove-Item -Recurse -Force target, build, node_modules",
                ),
                Some(WIN_PROJECT),
            ),
            (
                "cmd line",
                l("cmd.exe", r"rd /s /q build"),
                Some(WIN_PROJECT),
            ),
            (
                "glob in project",
                a(&["rm", "-rf", "*"]),
                Some(UNIX_PROJECT),
            ),
            (
                "tmp child",
                a(&["rm", "-rf", "/tmp/tc-guard-nonexistent"]),
                None,
            ),
            (
                "doubled tmp child",
                a(&["rm", "-rf", "//tmp//tc-guard-nonexistent"]),
                None,
            ),
            (
                "verbatim profile argv",
                a(&["Remove-Item", "-Recurse", verbatim_profile]),
                None,
            ),
            (
                "verbatim profile line",
                l("pwsh", format!("Remove-Item -Recurse {verbatim_profile}")),
                None,
            ),
            (
                "apt-get remove",
                a(&["apt-get", "remove", "-y", "cowsay"]),
                None,
            ),
            (
                "npm uninstall -g",
                a(&["npm", "uninstall", "-g", "typescript"]),
                None,
            ),
            (
                "cargo uninstall",
                a(&["cargo", "uninstall", "ripgrep"]),
                None,
            ),
            (
                "git clean",
                a(&["git", "clean", "-fdx"]),
                Some(UNIX_PROJECT),
            ),
            (
                "package removals line",
                l(
                    "bash",
                    "sudo apt-get remove -y cowsay && npm uninstall -g typescript",
                ),
                None,
            ),
            (
                "redirect to /dev/null",
                l("bash", "rm -f build.log > /dev/null"),
                Some(UNIX_PROJECT),
            ),
            (
                "stderr to /dev/null",
                l("bash", "rm -rf target 2> /dev/null"),
                Some(UNIX_PROJECT),
            ),
            (
                "pwsh redirect",
                l("pwsh", r"Remove-Item build.log 2>&1 > $null"),
                Some(WIN_PROJECT),
            ),
            (
                "comment names a root",
                l("bash", "rm -rf target # never /usr"),
                Some(UNIX_PROJECT),
            ),
            (
                "bash -c target",
                a(&["bash", "-c", "rm -rf target"]),
                Some(UNIX_PROJECT),
            ),
            (
                "pwsh -Command node_modules",
                a(&["pwsh", "-Command", "Remove-Item -Recurse node_modules"]),
                Some(WIN_PROJECT),
            ),
            (
                "cmd /c build",
                a(&["cmd", "/c", "rd /s /q build"]),
                Some(WIN_PROJECT),
            ),
            (
                "sudo -iu tmp child",
                a(&[
                    "sudo",
                    "-iu",
                    "root",
                    "rm",
                    "-rf",
                    "/tmp/tc-guard-nonexistent",
                ]),
                None,
            ),
            (
                "cd away from a root",
                l(
                    "bash",
                    "cd /usr/lib && ls && cd /home/dev/proj && rm -rf target",
                ),
                None,
            ),
            (
                "cd scoped to a subshell",
                l("bash", "(cd /usr/lib && ls); rm -rf build"),
                Some(UNIX_PROJECT),
            ),
            (
                "unknown substitution",
                l("bash", "rm -rf $(mktemp -d)"),
                Some("/"),
            ),
            ("glob in tmp", l("bash", "cd /tmp && rm -rf *"), None),
            (
                "wsl tmp child",
                a(&["wsl", "-e", "rm", "-rf", "/tmp/tc-guard-nonexistent"]),
                None,
            ),
            (
                "wsl relative",
                a(&["wsl", "rm", "-rf", "target"]),
                Some(WIN_PROJECT),
            ),
            ("quoted text", l("bash", "echo 'rm -rf /usr'"), None),
            (
                "commit message",
                l("bash", r#"git commit -m "rm -rf /usr is bad""#),
                None,
            ),
            ("eval agent", l("bash", r#"eval "$(ssh-agent -s)""#), None),
        ]
    }

    #[test]
    fn parser_gaps_are_refused() {
        let missed: Vec<String> = refused_rows()
            .iter()
            .filter(|(_, case, cwd)| !guarded(case, *cwd))
            .map(|(gap, case, cwd)| format!("{gap}: {} cwd={cwd:?}", describe(case)))
            .collect();
        assert!(missed.is_empty(), "not refused:\n{}", missed.join("\n"));
    }

    #[test]
    fn parser_gap_controls_stay_allowed() {
        let refused: Vec<String> = allowed_rows()
            .iter()
            .filter(|(_, case, cwd)| guarded(case, *cwd))
            .map(|(name, case, cwd)| format!("{name}: {} cwd={cwd:?}", describe(case)))
            .collect();
        assert!(
            refused.is_empty(),
            "wrongly refused:\n{}",
            refused.join("\n")
        );
    }
}
