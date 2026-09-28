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
//! (`CommandShellStart`, via [`shell_line_deletion_hit`]).
//!
//! ponytail: this is a guard rail, not a kernel boundary. It matches command
//! basenames and path operands as strings. INDIRECT deletion is NOT caught:
//! `find / -delete`, `python -c "shutil.rmtree('/usr')"`, an interactive
//! `diskpart` script, a Makefile target. The complete control is a hardened
//! profile plus OS permissions; this stops the obvious `rm -rf /` mistakes.

use std::path::{Component, Path, PathBuf};

use crate::shell_deny::launched_argv;

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

/// Privilege escalators stripped before classifying the real command, so
/// `sudo rm -rf /` is caught. Distinct from the shell-interpreter deny.
const ESCALATORS: &[&str] = &["sudo", "doas", "su", "pkexec", "run0"];

/// Escalator options that consume a following value (so it is not mistaken
/// for the wrapped command).
const ESCALATOR_VALUE_OPTS: &[&str] = &[
    "-u", "--user", "-g", "--group", "-C", "-p", "--prompt", "-h", "--host", "-R", "--chroot",
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

/// Strip leading privilege escalators (and their value options) so the real
/// command is classified. `sudo -u root rm -rf /` -> `rm -rf /`.
fn strip_escalators(argv: &[String]) -> Vec<String> {
    let mut i = 0;
    while let Some(first) = argv.get(i) {
        if !ESCALATORS.contains(&command_basename(first).as_str()) {
            break;
        }
        i += 1;
        while let Some(tok) = argv.get(i) {
            if !tok.starts_with('-') {
                break;
            }
            let consumes_value = ESCALATOR_VALUE_OPTS.contains(&tok.as_str());
            i += 1;
            if consumes_value && argv.get(i).is_some_and(|v| !v.starts_with('-')) {
                i += 1;
            }
        }
        // `su LOGIN -c ...` / `su LOGIN cmd`: drop a bare login name that is
        // not itself the destructive command.
        if command_basename(first) == "su"
            && let Some(tok) = argv.get(i)
            && !tok.starts_with('-')
            && classify(&command_basename(tok)).is_none()
        {
            i += 1;
        }
    }
    argv[i.min(argv.len())..].to_vec()
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
/// stripped first, so `sudo env rm -rf /usr` is classified as `rm`.
#[must_use]
pub fn argv_deletion_hit(argv: &[impl AsRef<str>]) -> Option<OsGuardHit> {
    let owned: Vec<String> = argv.iter().map(|a| a.as_ref().to_owned()).collect();
    let unescalated = strip_escalators(&owned);
    let launched = launched_argv(&unescalated);
    let real = if launched.is_empty() {
        &unescalated
    } else {
        &launched
    };
    let (name_tok, operands) = real.split_first()?;
    let name = command_basename(name_tok);
    let kind = classify(&name)?;

    match kind {
        DestructiveKind::PathOperands => {
            for operand in operands {
                if is_flag(operand) {
                    continue;
                }
                if operand_is_protected(operand) {
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

/// The destructive-deletion hit for a shell line: split into simple commands
/// on `;`, `&&`, `||`, `|`, `&`, and newlines (honoring quotes and `\`), then
/// run [`argv_deletion_hit`] on each.
#[must_use]
pub fn shell_line_deletion_hit(line: &str) -> Option<OsGuardHit> {
    for command in split_simple_commands(line) {
        if command.is_empty() {
            continue;
        }
        if let Some(hit) = argv_deletion_hit(&command) {
            return Some(hit);
        }
    }
    None
}

/// Split a shell line into simple commands, each a `Vec<String>` of words.
/// Quotes and backslash escapes are respected; `NAME=value` prefixes are
/// kept (escalator/wrapper stripping handles them downstream).
fn split_simple_commands(line: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut has_word = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = line.chars().peekable();

    let flush_word = |word: &mut String, has_word: &mut bool, current: &mut Vec<String>| {
        if *has_word {
            current.push(std::mem::take(word));
            *has_word = false;
        }
    };

    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                word.push(ch);
                has_word = true;
            }
            continue;
        }
        if in_double {
            match ch {
                '"' => in_double = false,
                '\\' => {
                    if let Some(next) = chars.next() {
                        word.push(next);
                        has_word = true;
                    }
                }
                _ => {
                    word.push(ch);
                    has_word = true;
                }
            }
            continue;
        }
        match ch {
            '\'' => {
                in_single = true;
                has_word = true;
            }
            '"' => {
                in_double = true;
                has_word = true;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    word.push(next);
                    has_word = true;
                }
            }
            ' ' | '\t' | '\r' => flush_word(&mut word, &mut has_word, &mut current),
            ';' | '\n' | '|' | '&' => {
                // `&&` / `||` collapse to one boundary.
                if (ch == '&' || ch == '|') && chars.peek() == Some(&ch) {
                    chars.next();
                }
                flush_word(&mut word, &mut has_word, &mut current);
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                }
            }
            _ => {
                word.push(ch);
                has_word = true;
            }
        }
    }
    flush_word(&mut word, &mut has_word, &mut current);
    if !current.is_empty() {
        commands.push(current);
    }
    commands
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
    ["sd", "nvme", "hd", "disk", "mmcblk", "vd", "xvd"]
        .iter()
        .any(|p| dev.starts_with(p))
}

/// A path operand as a deletion command sees it.
///
/// Takes the fixed prefix before any glob metacharacter, then checks
/// [`is_os_critical_path`]. `/usr/*` -> `/usr` -> protected; `/tmp/*` ->
/// `/tmp` -> allowed.
#[must_use]
pub fn operand_is_protected(operand: &str) -> bool {
    let fixed = glob_fixed_prefix(operand);
    if fixed.is_empty() {
        return false;
    }
    is_os_critical_path(&fixed)
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

/// Unix protected trees (exact root and everything beneath). `/` itself is
/// handled separately (only the bare-root operand, never its descendants, so
/// `/home`, `/tmp`, `/opt` stay deletable).
const UNIX_PROTECTED: &[&str] = &[
    "/boot",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/usr",
    "/etc",
    "/sys",
    "/proc",
    "/dev",
    "/var/lib",
    "/System",
    "/private/etc",
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
];

/// Is `raw` an OS-critical location (protected tree root, a path beneath one,
/// an ancestor of one, a drive/volume root, or a raw disk device)?
///
/// Normalizes verbatim `\\?\` prefixes, slash direction, case (Windows only),
/// trailing separators, `.`/`..`, WSL `/mnt/<drive>` bridging, and
/// drive-relative `C:foo`.
#[must_use]
pub fn is_os_critical_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_disk_device(trimmed) {
        return true;
    }
    let slashed = trimmed.replace('\\', "/");
    // `\\?\Volume{...}` / `\\.\...` device or volume roots.
    let lower_dev = slashed.to_ascii_lowercase();
    if lower_dev.starts_with("//?/volume{") || lower_dev.starts_with("//./") {
        return true;
    }
    // Strip a `\\?\` verbatim prefix.
    let no_verbatim = slashed.strip_prefix("//?/").unwrap_or(&slashed);

    // WSL bridge: /mnt/<drive>/... maps to <drive>:/...
    if let Some(mapped) = wsl_mount_to_windows(no_verbatim) {
        return windows_protected(&mapped);
    }

    if looks_windows(no_verbatim) {
        windows_protected(no_verbatim)
    } else {
        unix_protected(no_verbatim)
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
    UNIX_PROTECTED.iter().any(|root| tree_hit(&norm, root))
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
    // System-drive subtrees. `%SystemDrive%` is assumed `c:` (documented).
    let Some(after_drive) = norm.strip_prefix("c:") else {
        return false;
    };
    WINDOWS_PROTECTED_SUBTREES
        .iter()
        .any(|sub| tree_hit(after_drive, sub))
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
            "/var/lib/docker",
            "/var", // ancestor of /var/lib
            "/System",
            "/private/etc",
            "/usr/../usr/bin",
            "/etc/./ssh",
        ] {
            assert!(is_os_critical_path(p), "must protect {p}");
        }
    }

    #[test]
    fn allowed_paths_unix() {
        for p in [
            "/home",
            "/home/dev/project",
            "/tmp",
            "/tmp/x",
            "/var/tmp",
            "/opt",
            "/opt/tool",
            "/usr2",
            "/etcx",
            "target",
            "./build",
            "/home/dev/.cache",
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
        ] {
            assert!(!is_os_critical_path(p), "must allow {p}");
        }
    }

    #[test]
    fn rm_rf_root_and_system_denied() {
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/"])).is_some());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "--no-preserve-root", "/"])).is_some());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/usr"])).is_some());
        assert!(argv_deletion_hit(&argv(&["sudo", "rm", "-rf", "/etc"])).is_some());
        assert!(argv_deletion_hit(&argv(&["sudo", "-u", "root", "rm", "-rf", "/boot"])).is_some());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/usr/*"])).is_some());
        assert!(argv_deletion_hit(&argv(&["shred", "/dev/sda"])).is_some());
        assert!(argv_deletion_hit(&argv(&["wipefs", "-a", "/dev/nvme0n1"])).is_some());
        assert!(argv_deletion_hit(&argv(&["dd", "if=/dev/zero", "of=/dev/sda"])).is_some());
        assert!(argv_deletion_hit(&argv(&["mkfs.ext4", "/dev/sdb1"])).is_some());
    }

    #[test]
    fn windows_deletion_denied() {
        assert!(argv_deletion_hit(&argv(&["Remove-Item", "-Recurse", r"C:\Windows"])).is_some());
        assert!(argv_deletion_hit(&argv(&["del", "/S", "/Q", r"C:\Windows\System32"])).is_some());
        assert!(argv_deletion_hit(&argv(&["rd", "/s", r"C:\Boot"])).is_some());
        assert!(argv_deletion_hit(&argv(&["format", "c:"])).is_some());
        assert!(argv_deletion_hit(&argv(&["cipher", "/w:C:\\"])).is_some());
    }

    #[test]
    fn ordinary_deletions_allowed() {
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "target"])).is_none());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/tmp/x"])).is_none());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "node_modules"])).is_none());
        assert!(argv_deletion_hit(&argv(&["del", "build\\out.txt"])).is_none());
        assert!(argv_deletion_hit(&argv(&["Remove-Item", "-Recurse", "node_modules"])).is_none());
        assert!(argv_deletion_hit(&argv(&["rm", "-rf", "/home/dev/project/dist"])).is_none());
        assert!(argv_deletion_hit(&argv(&["dd", "if=/dev/zero", "of=./disk.img"])).is_none());
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
                argv_deletion_hit(&argv(cmd)).is_none(),
                "must allow {cmd:?}"
            );
        }
    }

    #[test]
    fn shell_line_scan() {
        assert!(shell_line_deletion_hit("cd /tmp && rm -rf /usr").is_some());
        assert!(shell_line_deletion_hit("echo hi | sudo rm -rf /etc").is_some());
        assert!(shell_line_deletion_hit("rm -rf target; cargo build").is_none());
        assert!(shell_line_deletion_hit("rm -rf \"/usr/lib\"").is_some());
        assert!(shell_line_deletion_hit("make clean && rm -rf ./dist").is_none());
    }
}
