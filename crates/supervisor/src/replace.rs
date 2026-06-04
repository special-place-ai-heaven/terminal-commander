// Version-aware daemon replacement. Reads the running daemon's version
// from the pidfile, compares to the installed binary version, and on a
// stale daemon: finds the pid (pidfile, else OS query), hard-kills it,
// waits for the endpoint to clear. Hard-kill only; works on a daemon
// too old to have any Shutdown IPC.
//
// A reachable daemon with NO pidfile predates the pidfile feature and
// is therefore stale by construction: we OS-query its pid and replace.
// No system_discover IPC client is used (supervisor's probe is
// connect-only by design).

use std::path::Path;
use std::time::{Duration, Instant};

use crate::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon, probe_endpoint,
    spawn_under_lock,
};
use crate::pidfile;
use crate::proc_lock::{self, TryLockResult};

/// Outcome of a replace attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceOutcome {
    /// No daemon reachable; caller should spawn normally.
    NoDaemonRunning,
    /// Running daemon is current; left untouched.
    UpToDate { version: String },
    /// Stale daemon killed; caller should now spawn the new one.
    Replaced { old: String, new: String },
    /// Reachable but could not be safely replaced; left untouched.
    Skipped { reason: String },
}

/// True if `running` is strictly older than `installed`. Unparseable
/// `running` => stale (an unidentifiable daemon is not trustworthy).
#[must_use]
pub fn is_stale(running: &str, installed: &str) -> bool {
    match (parse3(running), parse3(installed)) {
        (Some(r), Some(i)) => r < i,
        _ => true,
    }
}

/// Replace the running daemon when it is stale OR the caller forces it.
/// `force` is an explicit operator override (e.g. `update --force`); it
/// does NOT lie about staleness, it just authorizes a same-version
/// replacement (the `terminal-commander restart` story).
#[must_use]
pub fn should_replace(stale: bool, force: bool) -> bool {
    stale || force
}

fn parse3(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v');
    let mut it = core.split('.').map(|s| s.split('-').next().unwrap_or(s));
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

/// String form of an endpoint for the pidfile cross-check.
fn endpoint_string(ep: &Endpoint) -> String {
    match ep {
        Endpoint::UnixSocket { path } => path.display().to_string(),
        Endpoint::WindowsPipe { name } => name.clone(),
    }
}

/// Outcome of a [`hard_kill`], surfaced so callers and tests can confirm the
/// kill-leg identity gate fired instead of a blind force-kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardKillOutcome {
    /// The process was gone after the graceful signal; no force was needed.
    Reaped,
    /// A force signal (SIGKILL on unix / native `TerminateProcess` on
    /// windows) was sent: the pid was still alive AND still our daemon.
    Forced,
    /// After the grace window the pid was alive but NO LONGER our daemon --
    /// the OS recycled the number for an unrelated process during the grace.
    /// The force signal was WITHHELD. Closes the kill-leg pid-reuse window
    /// (review finding #2): the pre-SIGTERM identity check alone left the
    /// SIGKILL leg ungated, so a recycled pid could be force-killed.
    IdentitySkipped,
}

/// Hard-kill a pid (SIGTERM then SIGKILL on unix; native Win32
/// `OpenProcess` + `TerminateProcess` on windows). The unix path uses the
/// `kill(1)` tool; the windows path spawns NO external process (corporate-EDR
/// hardening -- no powershell/taskkill).
///
/// Both the graceful and the forced leg are identity-gated: the forced leg
/// re-verifies, after the grace window, that the pid is still our daemon
/// bound to `state_dir`, mirroring the caller's pre-kill check so a pid
/// recycled mid-grace is never force-killed.
pub fn hard_kill(pid: u32, state_dir: &Path) -> std::io::Result<HardKillOutcome> {
    #[cfg(unix)]
    {
        Ok(hard_kill_unix(
            pid,
            Duration::from_millis(800),
            pidfile::pid_alive,
            |p| pid_belongs_to_daemon(p, state_dir),
            |p| {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", &p.to_string()])
                    .status();
            },
            |p| {
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", &p.to_string()])
                    .status();
            },
        ))
    }
    #[cfg(windows)]
    {
        // Windows has no graceful -> grace -> force window: the native
        // `TerminateProcess` is a single forced terminate with no intervening
        // sleep, so the SIGKILL-leg recycle window the unix path closes does
        // not exist here. Re-verify identity once more anyway, for defense in
        // depth and parity with the unix kill-leg gate. Native Win32 only:
        // NO powershell / taskkill / cmd is spawned (corporate-EDR hardening).
        if !pid_belongs_to_daemon(pid, state_dir) {
            return Ok(HardKillOutcome::IdentitySkipped);
        }
        windows_native::terminate_process(pid)?;
        Ok(HardKillOutcome::Forced)
    }
}

/// Graceful-then-forced kill core with the liveness, identity, and signal
/// effects injected so the kill-leg identity gate is unit-testable without
/// real processes or pid recycling. Sends `term`, waits `grace`, and sends
/// `kill` only if the pid is still alive AND still our daemon.
#[cfg(unix)]
fn hard_kill_unix(
    pid: u32,
    grace: Duration,
    is_alive: impl Fn(u32) -> bool,
    still_ours: impl Fn(u32) -> bool,
    term: impl Fn(u32),
    kill: impl Fn(u32),
) -> HardKillOutcome {
    term(pid);
    std::thread::sleep(grace);
    if !is_alive(pid) {
        return HardKillOutcome::Reaped;
    }
    // Kill-leg identity gate (review finding #2): the daemon may have exited
    // during the grace window and the OS recycled the pid for an unrelated
    // process. Mirror the pre-SIGTERM check before escalating to SIGKILL so a
    // recycled pid is never force-killed.
    if !still_ours(pid) {
        return HardKillOutcome::IdentitySkipped;
    }
    kill(pid);
    HardKillOutcome::Forced
}

/// Re-verify, immediately before a kill, that `pid` is still OUR daemon:
/// a `terminal-commanderd` process whose command line references
/// `state_dir`.
///
/// Closes the pid-reuse TOCTOU: between reading a pid (from the pidfile or
/// an OS query) and killing it, the daemon can exit and the OS can recycle
/// that pid for an unrelated process. Gating `hard_kill` on this check means
/// a recycled pid is never force-killed. A pidfile-sourced pid is otherwise
/// trusted blindly; an OS-found pid was matched by cmdline but may still
/// have changed by kill time.
#[must_use]
pub fn pid_belongs_to_daemon(pid: u32, state_dir: &Path) -> bool {
    #[cfg(windows)]
    {
        // Native Win32 identity check -- NO powershell / WMI is spawned
        // (corporate-EDR hardening). We OpenProcess(QUERY_LIMITED_INFORMATION)
        // and QueryFullProcessImageNameW to read the pid's image path, then
        // confirm its file name is the daemon executable.
        //
        // IDENTITY-NARROWING (vs the prior WMI cmdline match): the old code
        // matched the `--data-dir` argument from the WMI command line to scope
        // multi-session. We no longer read the command line: the pidfile is
        // already per-state_dir (`state_dir/terminal-commanderd.pid` gives the
        // pid for THIS data-dir), so the only residual risk this check defends
        // against is PID RECYCLE -- the OS reusing the pidfile's number for an
        // unrelated process (e.g. notepad) after our daemon exited. Confirming
        // "pid X's image is terminal-commanderd.exe" closes exactly that
        // window. Two daemons of DIFFERENT sessions are still both
        // `terminal-commanderd.exe`, but a recycled pid that happens to be a
        // *sibling daemon* is (a) astronomically unlikely and (b) harmless to
        // never-kill-on-uncertainty here: this gate only ever AUTHORIZES a
        // kill that the per-state_dir pidfile already selected, it does not
        // widen the target set. `state_dir` is therefore unused on Windows.
        let _ = state_dir;
        windows_native::pid_image_is_daemon(pid)
    }
    #[cfg(unix)]
    {
        // Identity is decided from the kernel-authoritative argv in
        // `/proc/<pid>/cmdline` (NUL-separated), NOT from a substring test
        // over a flattened command line. A flattened-string match
        // over-matched: any process whose command line merely CONTAINED the
        // string "terminal-commanderd" and our state_dir as some argument --
        // e.g. `vim /.../terminal-commanderd/notes.txt --data-dir /.../state`
        // or a `grep`/`tail` over the daemon's own log dir -- passed the old
        // `cmdline_is_our_daemon` check and would have been force-killed.
        //
        // The strict predicate (`unix_argv_is_our_daemon`) instead requires
        // argv[0] to BE the daemon executable (basename == terminal-commanderd)
        // and the `--data-dir` argument VALUE to EQUAL our state_dir. A pid
        // whose /proc entry is unreadable (race: it exited, or permissions)
        // is treated as "not ours" -- we never kill on uncertainty.
        match read_proc_cmdline(pid) {
            Some(argv) => unix_argv_is_our_daemon(&argv, state_dir),
            None => false,
        }
    }
}

/// Whole-argument test that a process command line `args` identifies our
/// daemon bound to `state_dir`: it must reference the daemon binary name and
/// carry `state_dir` as a COMPLETE `--data-dir` path argument (see
/// [`contains_path_arg`]), so a sibling session whose dir merely PREFIXES
/// ours is never mistaken for it.
///
/// HISTORICAL / TEST-ONLY: this whole-line matcher backed the prior Windows
/// WMI-cmdline identity check. The Windows path now reads the pid's image
/// path natively (`QueryFullProcessImageNameW`) and matches the daemon file
/// name, so no command line is parsed in production on either OS (unix uses
/// the kernel argv via [`unix_argv_is_our_daemon`]). The function is retained
/// because its cross-platform tests pin the whole-argument matching logic
/// (prefix rejection, apostrophe/`=` forms); it has no production caller and
/// is therefore allowed to be dead outside `cfg(test)`.
#[cfg_attr(not(test), allow(dead_code))]
fn cmdline_is_our_daemon(args: &str, state_dir: &Path) -> bool {
    if !args.contains("terminal-commanderd") {
        return false;
    }
    let needle = state_dir.to_string_lossy();
    contains_path_arg(args, needle.as_ref())
}

/// True when `needle` (a state-dir path) appears in `haystack` (a process
/// command line) as a COMPLETE path argument rather than a path PREFIX of
/// a longer one.
///
/// Sessions co-locate under one base: the default session's dir `<base>`
/// is a string prefix of a seeded session's dir `<base>/agent-1`. A bare
/// substring test would therefore confirm a seeded session's daemon as the
/// base session's and authorize a cross-session force-kill. Requiring a
/// token boundary on both sides of the match -- where a path separator is
/// explicitly NOT a boundary -- rejects the prefix while still matching a
/// real `--data-dir <state_dir>` argument, including paths that contain
/// spaces, brackets, or apostrophes (matched verbatim, never interpolated
/// into a shell pattern).
///
/// Test-only helper for [`cmdline_is_our_daemon`]; no production caller
/// remains (see that function), so it is allowed to be dead outside
/// `cfg(test)`.
#[cfg_attr(not(test), allow(dead_code))]
fn contains_path_arg(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let nlen = needle.len();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(needle) {
        let i = from + rel;
        let end = i + nlen;
        let before_ok = i == 0 || is_arg_boundary(bytes[i - 1]);
        let after_ok = end == bytes.len() || is_arg_boundary(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = i + 1;
    }
    false
}

/// A byte that legitimately bounds a path argument on a command line:
/// whitespace separates arguments, `=` precedes a `--flag=value` path, and
/// a quote wraps a path containing spaces. A path separator (`/` or `\`)
/// is deliberately NOT a boundary -- that is exactly the prefix case the
/// whole-argument match must reject.
///
/// Test-only helper for [`cmdline_is_our_daemon`]; no production caller
/// remains, so it is allowed to be dead outside `cfg(test)`.
#[cfg_attr(not(test), allow(dead_code))]
fn is_arg_boundary(b: u8) -> bool {
    b.is_ascii_whitespace() || b == b'=' || b == b'"' || b == b'\''
}

/// Read `/proc/<pid>/cmdline` and split it into the process's argv.
///
/// The kernel stores the command line as NUL-separated, NUL-terminated
/// arguments -- the authoritative argv the process was exec'd with, immune
/// to the word-splitting and quoting ambiguity of a flattened `ps args=`
/// string. Returns `None` if the entry cannot be read (the pid exited, or
/// is not ours to inspect) or is empty (a kernel thread has an empty
/// cmdline); callers MUST treat `None` as "not our daemon" so an uncertain
/// or racing pid is never killed.
#[cfg(unix)]
fn read_proc_cmdline(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    // Trim a single trailing NUL terminator so it does not yield a spurious
    // empty final argv element, then split on the NUL separators.
    let raw = raw.strip_suffix(b"\0").unwrap_or(&raw);
    let argv: Vec<String> = raw
        .split(|&b| b == 0)
        .map(|seg| String::from_utf8_lossy(seg).into_owned())
        .collect();
    if argv.is_empty() {
        return None;
    }
    Some(argv)
}

/// Strict identity predicate over a parsed argv: true ONLY when this argv
/// is our daemon bound to `state_dir`. This is the kill-authorization gate;
/// it is deliberately exact, not a substring/whole-line heuristic.
///
/// Two independent requirements must BOTH hold:
///
/// 1. `argv[0]`'s file-name component is exactly `terminal-commanderd`. A
///    process is only a candidate for kill if it actually IS the daemon
///    executable -- not merely a process (vim, grep, tail, a shell) whose
///    arguments happen to mention the string. The basename is compared so a
///    PATH-resolved bare name, a relative `./terminal-commanderd`, and an
///    absolute `/opt/.../terminal-commanderd` all match, while
///    `terminal-commanderd.log` or `my-terminal-commanderd` (a different
///    binary) do not.
/// 2. The daemon is launched as `--data-dir <state_dir>` (see
///    `ensure_daemon`). We scan argv for a `--data-dir` flag and require its
///    VALUE to EQUAL `state_dir` -- both the space-separated
///    (`--data-dir`, `<value>`) and the joined (`--data-dir=<value>`) forms
///    are accepted. Equality (not prefix / containment) means a sibling
///    session under a longer or shorter dir is never matched.
///
/// Factored to take a borrowed argv so it is unit-testable from a plain
/// `Vec<String>` without a live `/proc`.
#[cfg(unix)]
fn unix_argv_is_our_daemon<S: AsRef<str>>(argv: &[S], state_dir: &Path) -> bool {
    const DAEMON_BIN: &str = "terminal-commanderd";
    const DATA_DIR_FLAG: &str = "--data-dir";
    const DATA_DIR_EQ: &str = "--data-dir=";

    let exe = match argv.first() {
        Some(first) => first.as_ref(),
        None => return false,
    };
    // argv[0] must be the daemon executable itself, compared by file name so
    // a path-qualified or PATH-resolved invocation still matches while a
    // mere substring (e.g. `.../terminal-commanderd.log`) does not.
    if std::path::Path::new(exe)
        .file_name()
        .and_then(|n| n.to_str())
        != Some(DAEMON_BIN)
    {
        return false;
    }

    // Find the `--data-dir` value (space-separated or `=`-joined) and require
    // it to equal our state_dir exactly. Path equality (not string equality)
    // tolerates a benign trailing separator while still rejecting a sibling
    // session's distinct directory.
    let mut iter = argv.iter().skip(1);
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        // Space-separated form: the value is the next argv element.
        // Joined form: the value follows the `--data-dir=` prefix.
        let value = if arg == DATA_DIR_FLAG {
            iter.next().map(AsRef::as_ref)
        } else {
            arg.strip_prefix(DATA_DIR_EQ)
        };
        if let Some(value) = value {
            return std::path::Path::new(value) == state_dir;
        }
    }
    // No `--data-dir` argument: this daemon is not bound to our state_dir by
    // an explicit flag, so it is not ours to kill.
    false
}

/// OS-query fallback: find the pid of a terminal-commanderd process
/// whose command line references `state_dir`, so we only ever target
/// OUR daemon (the daemon is launched with `--data-dir <state_dir>`),
/// never a bare name match.
#[must_use]
pub fn find_daemon_pid_os(state_dir: &Path) -> Option<u32> {
    #[cfg(windows)]
    {
        // Native ToolHelp enumeration -- NO powershell / WMI is spawned
        // (corporate-EDR hardening). Snapshot every process, match
        // `szExeFile == terminal-commanderd.exe`, then confirm via the same
        // image-path check `pid_belongs_to_daemon` uses, excluding our own
        // pid. Returns the first matching daemon pid.
        //
        // IDENTITY-NARROWING (vs the prior WMI cmdline match): this no-pidfile
        // fallback can no longer scope to `state_dir` via the command line (we
        // do not read it). It is reached ONLY when the per-state_dir pidfile is
        // absent -- a daemon predating the pidfile feature, which
        // `replace_if_stale` treats as stale-by-construction and replaces. In
        // the single-session install this is exact; in a (rare) multi-session
        // box with a pidfile-less legacy daemon it returns the first running
        // daemon image, which is acceptable for the legacy-replace path.
        // `state_dir` is therefore unused on Windows here.
        let _ = state_dir;
        windows_native::find_first_daemon_pid()
    }
    #[cfg(unix)]
    {
        // Enumerate candidate daemons by the FIXED binary name -- a literal
        // with no regex metacharacters -- then confirm each through the SAME
        // whole-argument matcher `pid_belongs_to_daemon` uses. Both callers
        // agree by construction, and a state_dir containing metacharacters can
        // no longer break the search or make pgrep error (review finding #3).
        let out = std::process::Command::new("pgrep")
            .args(["-f", "terminal-commanderd"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .find(|&pid| pid_belongs_to_daemon(pid, state_dir))
    }
}

/// Native Win32 daemon-recovery primitives, replacing the prior
/// powershell/WMI/taskkill shell-outs (corporate-EDR hardening: an unsigned
/// exe spawning powershell+taskkill is a high-signal EDR event). Each FFI
/// block is minimal, checks every handle/BOOL return, and closes every
/// handle on all paths via the [`OwnedHandle`] RAII guard. The unix
/// production path uses NO unsafe.
///
/// Lint note: this crate does not opt into the workspace lint table
/// (`unsafe_code = "forbid"` is applied only to crates that set
/// `[lints] workspace = true`), so these `unsafe` FFI blocks compile without
/// a crate-level allow. The SAFETY discipline below is upheld regardless.
///
/// Mirrors the proven pattern in `crates/cli/src/update_locks.rs` and the H2
/// FFI in `crates/probes/src/file.rs`.
#[cfg(windows)]
mod windows_native {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
    };

    /// The daemon image file name we will authorize a kill against. Compared
    /// case-insensitively against the candidate pid's image file name.
    const DAEMON_IMAGE: &str = "terminal-commanderd.exe";

    /// RAII guard so a process handle is always closed on every return path
    /// (success, early return, or `?`). `CloseHandle` failure on drop is
    /// ignored: the handle is being discarded regardless.
    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a handle we opened (OpenProcess) or received
            // from CreateToolhelp32Snapshot and have not closed yet. Closing
            // it exactly once here is the paired release for that open.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Read `pid`'s full image path via the OS and return its file name
    /// lowercased, or `None` if the process cannot be opened/queried (it
    /// exited, the pid is invalid, or access was denied). A `None` result is
    /// treated by callers as "not our daemon" -- we never kill on uncertainty.
    fn pid_image_file_name(pid: u32) -> Option<String> {
        // SAFETY: OpenProcess takes a desired-access mask, an inherit BOOL,
        // and a pid; it returns a valid handle on success or an Err we map to
        // None. PROCESS_QUERY_LIMITED_INFORMATION is the least privilege that
        // permits QueryFullProcessImageNameW.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
        let proc = OwnedHandle(handle);

        let mut buf = [0u16; 1024];
        let mut len = u32::try_from(buf.len()).expect("image-path buffer length fits in u32");
        // SAFETY: `proc.0` is a live handle (just opened) valid for this call.
        // PROCESS_NAME_FORMAT(0) requests the win32 path form. `buf`/`len`
        // describe a properly sized, owned u16 buffer; the call writes at most
        // `len` code units and updates `len` to the count written. We check
        // the BOOL result and a non-zero length before reading the buffer.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                proc.0,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(buf.as_mut_ptr()),
                &raw mut len,
            )
            .is_ok()
        };
        if !ok || len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_ascii_lowercase)
    }

    /// True when `pid`'s image file name is the daemon executable. Defends
    /// against a recycled pid (the OS reusing the pidfile's number for an
    /// unrelated process such as `notepad.exe`) now answering the kill.
    pub(super) fn pid_image_is_daemon(pid: u32) -> bool {
        pid_image_file_name(pid).is_some_and(|name| name == DAEMON_IMAGE)
    }

    /// Force-terminate `pid` natively. Caller has already identity-gated the
    /// pid (see `hard_kill`). Returns an IO error if the process cannot be
    /// opened for termination or the terminate call fails.
    pub(super) fn terminate_process(pid: u32) -> std::io::Result<()> {
        let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
        // SAFETY: OpenProcess as above; PROCESS_TERMINATE is the access right
        // TerminateProcess requires. The Err arm maps the Win32 error into an
        // io::Error without dereferencing anything.
        let handle = unsafe { OpenProcess(access, false, pid) }
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let proc = OwnedHandle(handle);
        // SAFETY: `proc.0` is a live handle opened with PROCESS_TERMINATE.
        // TerminateProcess posts the exit and returns a BOOL we propagate as
        // an io::Error on failure. Exit code 1 mirrors the prior `taskkill /F`.
        unsafe { TerminateProcess(proc.0, 1) }.map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    /// Enumerate all processes via a ToolHelp snapshot and return the first
    /// whose image is the daemon executable (confirmed by image path), other
    /// than our own pid. Used only as the no-pidfile fallback.
    pub(super) fn find_first_daemon_pid() -> Option<u32> {
        // SAFETY: CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) returns a
        // valid snapshot handle or an Err we map to None. The handle is owned
        // by `OwnedHandle` and released on every return path.
        let snapshot =
            OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?);
        // SAFETY: GetCurrentProcessId has no preconditions.
        let current_pid = unsafe { GetCurrentProcessId() };

        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
                .expect("PROCESSENTRY32W size fits in u32"),
            ..Default::default()
        };

        // SAFETY: `snapshot.0` is a valid snapshot handle and `entry` is a
        // properly initialized PROCESSENTRY32W with `dwSize` set, as the API
        // requires. Process32FirstW fills `entry` and returns Ok/Err.
        let mut has_entry = unsafe { Process32FirstW(snapshot.0, &raw mut entry).is_ok() };
        while has_entry {
            let pid = entry.th32ProcessID;
            if pid != current_pid && exe_name_from_entry(&entry).eq_ignore_ascii_case(DAEMON_IMAGE)
            {
                // Confirm via the authoritative image path (the snapshot's
                // szExeFile is the base name only) before returning the pid.
                if pid_image_is_daemon(pid) {
                    return Some(pid);
                }
            }
            // SAFETY: same invariants as Process32FirstW; advances `entry` to
            // the next process or returns Err at the end of the snapshot.
            has_entry = unsafe { Process32NextW(snapshot.0, &raw mut entry).is_ok() };
        }
        None
    }

    /// Decode the NUL-terminated UTF-16 `szExeFile` base name from a snapshot
    /// entry into a Rust string.
    fn exe_name_from_entry(entry: &PROCESSENTRY32W) -> String {
        let end = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        String::from_utf16_lossy(&entry.szExeFile[..end])
    }
}

/// If a reachable daemon is older than `installed_version`, kill it and
/// wait for the endpoint to clear. The CALLER then spawns the new
/// daemon (via `ensure_daemon`). Never spawns here.
/// When `force` is true, a reachable same-version daemon is replaced
/// anyway (the `restart` path); the endpoint cross-check still applies,
/// so a forced replace never kills a process bound to a different
/// endpoint.
pub async fn replace_if_stale(
    opts: &EnsureDaemonOptions,
    installed_version: &str,
    force: bool,
) -> ReplaceOutcome {
    if !probe_endpoint(&opts.endpoint).await {
        return ReplaceOutcome::NoDaemonRunning;
    }

    let ep_str = endpoint_string(&opts.endpoint);

    let (old_version, pid) = match pidfile::read_pidfile(&opts.state_dir) {
        Some(rec) => {
            if !should_replace(is_stale(&rec.version, installed_version), force) {
                return ReplaceOutcome::UpToDate {
                    version: rec.version,
                };
            }
            if rec.endpoint != ep_str {
                return ReplaceOutcome::Skipped {
                    reason: format!(
                        "pidfile endpoint {:?} != target {:?}; refusing to kill",
                        rec.endpoint, ep_str
                    ),
                };
            }
            (rec.version, rec.pid)
        }
        None => {
            // Reachable but no pidfile => predates the feature => stale.
            match find_daemon_pid_os(&opts.state_dir) {
                Some(pid) => ("pre-pidfile".to_owned(), pid),
                None => {
                    return ReplaceOutcome::Skipped {
                        reason: "reachable daemon, no pidfile, no killable pid found".to_owned(),
                    };
                }
            }
        }
    };

    // Re-verify at kill time that `pid` is still OUR daemon. Closes the
    // pid-reuse TOCTOU (F3): the daemon may have exited and the OS recycled
    // the pid for an unrelated process between the read above and now. Also
    // covers the find_daemon_pid_os -> kill window (M4).
    if !pid_belongs_to_daemon(pid, &opts.state_dir) {
        return ReplaceOutcome::Skipped {
            reason: format!(
                "pid {pid} no longer a terminal-commanderd bound to {:?}; \
                 refusing to kill (pid may have been recycled)",
                opts.state_dir
            ),
        };
    }

    if let Err(e) = hard_kill(pid, &opts.state_dir) {
        return ReplaceOutcome::Skipped {
            reason: format!("hard-kill pid {pid} failed: {e}"),
        };
    }

    // Wait for the endpoint to become unreachable (bounded ~3s).
    for _ in 0..30 {
        if !probe_endpoint(&opts.endpoint).await {
            pidfile::remove_pidfile(&opts.state_dir);
            return ReplaceOutcome::Replaced {
                old: old_version,
                new: installed_version.to_owned(),
            };
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    ReplaceOutcome::Skipped {
        reason: format!("killed pid {pid} but endpoint still reachable after 3s"),
    }
}

/// Single-flight `replace`-then-`ensure` under ONE cross-process lock
/// (H6). This is the entry point the daemon `update` run-mode uses
/// instead of calling `replace_if_stale` + `ensure_daemon` separately:
/// holding the same guard across the kill AND the spawn closes the
/// kill -> spawn gap (a concurrent adapter cannot bind a fresh daemon in
/// the window between this process killing the stale one and binding the
/// replacement), and the lock is the same one `ensure_daemon` and the
/// daemon itself take, so all bring-up paths rendezvous on it.
///
/// Returns an `EnsureDaemonStatus` describing the end state:
/// `AlreadyRunning` when a current daemon was left in place (or a peer
/// brought one up under contention), `Started` when this call spawned
/// one, or `Unavailable` on failure. The detailed `old -> new` replace
/// narrative is dropped in favor of the unified status; callers that
/// need it can still call `replace_if_stale` directly.
pub async fn ensure_or_replace(
    opts: &EnsureDaemonOptions,
    version: &str,
    force: bool,
) -> EnsureDaemonStatus {
    let start = Instant::now();

    // Fast path: a current, non-stale daemon is already up and we are
    // not forcing a restart — no lock, no kill, no spawn.
    if !force
        && probe_endpoint(&opts.endpoint).await
        && let Some(rec) = pidfile::read_pidfile(&opts.state_dir)
        && !is_stale(&rec.version, version)
    {
        return EnsureDaemonStatus::AlreadyRunning {
            endpoint: opts.endpoint.clone(),
            pid: Some(rec.pid),
        };
    }

    let _ = std::fs::create_dir_all(&opts.state_dir);
    let lock_path = pidfile::lock_path(&opts.state_dir);
    match proc_lock::try_acquire(&lock_path) {
        Ok(TryLockResult::Acquired(guard)) => {
            // Under the lock: replace a stale/forced daemon (best effort;
            // a Skipped/UpToDate outcome simply leaves the endpoint bound),
            // then decide whether a spawn is still needed.
            let outcome = replace_if_stale(opts, version, force).await;
            tracing::debug!("ensure_or_replace: replace outcome = {outcome:?}");

            // If anything is still bound to the endpoint we deliberately
            // did NOT clear it (up-to-date, refused, or a peer rebound it):
            // do not spawn a competing daemon.
            if probe_endpoint(&opts.endpoint).await {
                let pid = pidfile::read_pidfile(&opts.state_dir).map(|r| r.pid);
                return EnsureDaemonStatus::AlreadyRunning {
                    endpoint: opts.endpoint.clone(),
                    pid,
                };
            }

            // Endpoint is free; spawn the replacement WHILE still holding
            // the guard. The guard releases only when this fn returns.
            spawn_under_lock(opts.clone(), start, &guard).await
        }
        Ok(TryLockResult::Contended) => {
            // A peer owns the bring-up (replace+spawn). Wait for an
            // endpoint to bind, bounded by startup_timeout.
            let deadline = start + opts.startup_timeout;
            while Instant::now() < deadline {
                if probe_endpoint(&opts.endpoint).await {
                    let pid = pidfile::read_pidfile(&opts.state_dir).map(|r| r.pid);
                    return EnsureDaemonStatus::AlreadyRunning {
                        endpoint: opts.endpoint.clone(),
                        pid,
                    };
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            // Fall back to the structured timeout via ensure_daemon, which
            // produces the canonical Unavailable diagnostics.
            ensure_daemon(opts.clone()).await
        }
        Err(e) => {
            // Lock unavailable: degrade gracefully (liveness over
            // single-flight). Best-effort replace, then defer to
            // ensure_daemon, which re-probes and spawns if still down.
            tracing::warn!("bring-up lock unavailable ({e}); replace+ensure without single-flight");
            let _ = replace_if_stale(opts, version, force).await;
            ensure_daemon(opts.clone()).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_compare() {
        assert!(is_stale("0.1.13", "0.1.14"));
        assert!(is_stale("0.1.13", "0.2.0"));
        assert!(!is_stale("0.1.14", "0.1.14"));
        assert!(!is_stale("0.2.0", "0.1.14"));
        assert!(
            is_stale("garbage", "0.1.14"),
            "unparseable running => stale"
        );
        assert!(!is_stale("v0.1.14", "0.1.14"), "v-prefix tolerated");
    }

    #[test]
    fn force_replaces_even_when_versions_match() {
        // is_stale stays version-accurate; force is a separate flag, not
        // a staleness lie. This documents the replace decision contract.
        assert!(!is_stale("0.1.18", "0.1.18"));
        assert!(should_replace(
            /* stale */ false, /* force */ true
        ));
        assert!(should_replace(true, false));
        assert!(should_replace(true, true));
        assert!(!should_replace(false, false));
    }

    #[test]
    fn recycled_or_unrelated_pid_is_refused_at_kill() {
        // M4 / F3 regression: pid_belongs_to_daemon is the guard that closes
        // the probe->kill TOCTOU. A pid that is alive but is NOT our daemon
        // bound to our state_dir must be refused, so a recycled pid (the OS
        // reusing the number for an unrelated process) is never force-killed.
        //
        // This test process itself is a live, real pid that is definitely not
        // a terminal-commanderd bound to /tmp/tc-m4-not-a-daemon.
        let unrelated_live_pid = std::process::id();
        assert!(
            !pid_belongs_to_daemon(
                unrelated_live_pid,
                std::path::Path::new("/tmp/tc-m4-not-a-daemon")
            ),
            "a live pid that is not our daemon must be refused (no force-kill of a recycled pid)"
        );

        // A pid that is almost certainly dead must also be refused (empty ps
        // output => not our daemon).
        assert!(
            !pid_belongs_to_daemon(0xFFFF_FFF0, std::path::Path::new("/tmp/tc-m4-not-a-daemon")),
            "a dead/absent pid must be refused"
        );
    }

    // --- Finding #2: the SIGKILL leg must be identity-gated too ---
    //
    // Before the fix, `hard_kill` re-checked identity before SIGTERM but the
    // SIGKILL leg fired on liveness alone. If the daemon exited during the
    // 800ms grace and the OS recycled the pid, SIGKILL hit an unrelated
    // process. These drive the kill core with injected liveness/identity so
    // the recycle race is deterministic -- no real processes, no flaky pid
    // reuse. The assertion is that the force signal is WITHHELD.
    #[cfg(unix)]
    #[test]
    fn sigkill_withheld_when_pid_recycled_during_grace() {
        use std::cell::Cell;
        let killed = Cell::new(false);
        let outcome = hard_kill_unix(
            4242,
            Duration::from_millis(0),
            |_| true,  // still alive after the grace window...
            |_| false, // ...but NO LONGER our daemon (pid recycled)
            |_| {},    // term: no-op
            |_| killed.set(true),
        );
        assert_eq!(outcome, HardKillOutcome::IdentitySkipped);
        assert!(
            !killed.get(),
            "SIGKILL must NOT be sent to a recycled pid (kill-leg identity gate)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sigkill_sent_when_still_our_daemon_after_grace() {
        use std::cell::Cell;
        let killed = Cell::new(false);
        let outcome = hard_kill_unix(
            4242,
            Duration::from_millis(0),
            |_| true, // still alive
            |_| true, // and still our daemon
            |_| {},
            |_| killed.set(true),
        );
        assert_eq!(outcome, HardKillOutcome::Forced);
        assert!(
            killed.get(),
            "a live, still-ours daemon must be force-killed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn no_force_signal_when_graceful_reaped_it() {
        use std::cell::Cell;
        let killed = Cell::new(false);
        let outcome = hard_kill_unix(
            4242,
            Duration::from_millis(0),
            |_| false, // gone after SIGTERM
            |_| panic!("identity must not be probed once the pid is already gone"),
            |_| {},
            |_| killed.set(true),
        );
        assert_eq!(outcome, HardKillOutcome::Reaped);
        assert!(!killed.get());
    }

    // --- Finding #3: cmdline match is literal, not a regex ---
    //
    // `find_daemon_pid_os` used to interpolate the raw state_dir into a
    // `pgrep -f` regex; a path with regex metacharacters mis-matched or made
    // pgrep error. The shared literal matcher must match such a path verbatim.
    #[cfg(unix)]
    #[test]
    fn cmdline_match_is_literal_not_regex() {
        let dir = std::path::Path::new("/tmp/tc (run)+[v1]/state.d");
        let cmd = format!("terminal-commanderd --data-dir {}", dir.display());
        assert!(
            cmdline_is_our_daemon(&cmd, dir),
            "a state_dir with regex metacharacters must match the cmdline verbatim"
        );
        // A different state_dir must not match.
        assert!(!cmdline_is_our_daemon(
            &cmd,
            std::path::Path::new("/tmp/other")
        ));
        // Must require the daemon binary, not just the path.
        assert!(
            !cmdline_is_our_daemon(&format!("cat {}", dir.display()), dir),
            "the daemon binary name is required, not just the path"
        );
        // Must require the path, not just the binary name.
        assert!(
            !cmdline_is_our_daemon("terminal-commanderd --data-dir /tmp/elsewhere", dir),
            "the exact state_dir is required, not just the binary name"
        );
    }

    #[test]
    fn cmdline_match_rejects_path_prefix_of_another_session() {
        // The default session's daemon lives at <base>; a seeded session at
        // <base>/agent-1. A bare substring match would confirm the seeded
        // daemon as the base session's and authorize a cross-session
        // force-kill. The whole-argument matcher must reject the prefix.
        let base = std::path::Path::new("/home/u/.local/share/terminal-commanderd/state");
        let seeded_cmd = "terminal-commanderd --data-dir /home/u/.local/share/terminal-commanderd/state/agent-1 start";
        assert!(
            !cmdline_is_our_daemon(seeded_cmd, base),
            "a seeded session's daemon (state/agent-1) must not match the base session (state)"
        );
        let base_cmd =
            "terminal-commanderd --data-dir /home/u/.local/share/terminal-commanderd/state start";
        assert!(cmdline_is_our_daemon(base_cmd, base));
        // ...and the base daemon must not be confused for the seeded session.
        let seeded = base.join("agent-1");
        assert!(!cmdline_is_our_daemon(base_cmd, &seeded));
    }

    #[test]
    fn cmdline_match_handles_apostrophe_and_equals_forms() {
        // A path with an apostrophe is matched verbatim: the Windows path
        // no longer interpolates it into a PowerShell -like literal (where
        // a ' used to terminate the string early). The --data-dir=<path>
        // form is also accepted.
        let dir = std::path::Path::new("/home/OBrien'X/state");
        assert!(cmdline_is_our_daemon(
            "terminal-commanderd --data-dir /home/OBrien'X/state start",
            dir
        ));
        assert!(cmdline_is_our_daemon(
            "terminal-commanderd --data-dir=/home/OBrien'X/state",
            dir
        ));
        // A sibling whose name merely extends the path must not match.
        assert!(!cmdline_is_our_daemon(
            "terminal-commanderd --data-dir /home/OBrien'X/state-2 start",
            dir
        ));
    }

    // --- Cross-session-kill / non-daemon-kill: the UNIX identity predicate ---
    //
    // Before the fix, the unix kill gate flattened the candidate's command line
    // and required only that the string "terminal-commanderd" appear SOMEWHERE
    // and that our state_dir appear as some whole argument. Any process whose
    // argv merely mentioned both -- a text editor opening a file under the
    // daemon's dir, a `grep`/`tail` over the log dir, an unrelated binary passed
    // our --data-dir -- passed the gate and would have been SIGTERM/SIGKILLed.
    //
    // `unix_argv_is_our_daemon` decides identity from the parsed argv:
    //   1. argv[0]'s basename must BE the daemon executable, and
    //   2. the --data-dir VALUE must EQUAL our state_dir.
    // These drive it from plain Vec<String> argvs (no live /proc needed).
    #[cfg(unix)]
    #[test]
    fn unix_argv_matches_the_real_daemon() {
        let dir = std::path::Path::new("/home/u/.local/share/terminal-commanderd/state");
        // The real daemon, exactly as `ensure_daemon` spawns it.
        let argv = vec![
            "/opt/tc/terminal-commanderd",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state",
            "start",
            "--mode",
            "ipc-server",
        ];
        assert!(
            unix_argv_is_our_daemon(&argv, dir),
            "the real daemon (argv[0]=terminal-commanderd, --data-dir=<our dir>) must match"
        );

        // The `--data-dir=<value>` joined form is equally accepted.
        let joined = vec![
            "terminal-commanderd",
            "--data-dir=/home/u/.local/share/terminal-commanderd/state",
            "start",
        ];
        assert!(
            unix_argv_is_our_daemon(&joined, dir),
            "the --data-dir=<value> form of the real daemon must match"
        );

        // A bare, PATH-resolved argv[0] still matches (basename comparison).
        let bare = vec![
            "terminal-commanderd",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state",
        ];
        assert!(unix_argv_is_our_daemon(&bare, dir));
    }

    #[cfg(unix)]
    #[test]
    fn unix_argv_rejects_non_daemon_impostors() {
        let dir = std::path::Path::new("/home/u/.local/share/terminal-commanderd/state");

        // (a) A text editor opening a file UNDER the daemon's dir, whose argv
        // even carries a --data-dir to our state_dir. The OLD flattened-string
        // gate matched this; argv[0]=vim must reject it.
        let vim = vec![
            "vim",
            "/home/u/.local/share/terminal-commanderd/notes.txt",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state",
        ];
        assert!(
            !unix_argv_is_our_daemon(&vim, dir),
            "a non-daemon process (vim) must NEVER be matched, even when its argv \
             mentions the daemon string and our state_dir"
        );

        // (b) A different binary whose argv merely CONTAINS both substrings.
        let grep = vec![
            "grep",
            "-rn",
            "terminal-commanderd",
            "/home/u/.local/share/terminal-commanderd/state",
        ];
        assert!(
            !unix_argv_is_our_daemon(&grep, dir),
            "grep over the daemon dir must NEVER be matched"
        );

        // (c) argv[0] is a file whose NAME merely contains the daemon string
        // (e.g. tailing the log, or a differently-named binary). Basename
        // equality -- not substring -- must reject it.
        let tail_log = vec![
            "tail",
            "-f",
            "/home/u/.local/share/terminal-commanderd/logs/terminal-commanderd.log",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state",
        ];
        assert!(!unix_argv_is_our_daemon(&tail_log, dir));
        let wrong_bin = vec![
            "/opt/tc/my-terminal-commanderd",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state",
        ];
        assert!(
            !unix_argv_is_our_daemon(&wrong_bin, dir),
            "a different binary whose name merely contains the daemon string must reject"
        );
        let log_as_argv0 = vec![
            "/home/u/.local/share/terminal-commanderd/logs/terminal-commanderd.log",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state",
        ];
        assert!(
            !unix_argv_is_our_daemon(&log_as_argv0, dir),
            "argv[0]=...terminal-commanderd.log must reject (basename != terminal-commanderd)"
        );

        // (d) The REAL daemon binary, but bound to a DIFFERENT --data-dir
        // (a sibling session). Value equality -- not prefix/containment --
        // must reject it: this is the cross-session-kill guard.
        let other_session = vec![
            "terminal-commanderd",
            "--data-dir",
            "/home/u/.local/share/terminal-commanderd/state/agent-1",
            "start",
        ];
        assert!(
            !unix_argv_is_our_daemon(&other_session, dir),
            "the daemon of a sibling session (different --data-dir) must NEVER be matched"
        );
        // And a shorter/prefix dir must not be confused either.
        let parent_dir = std::path::Path::new("/home/u/.local/share/terminal-commanderd");
        assert!(
            !unix_argv_is_our_daemon(
                &[
                    "terminal-commanderd",
                    "--data-dir",
                    "/home/u/.local/share/terminal-commanderd/state",
                ],
                parent_dir
            ),
            "a daemon under <dir>/state must not match a request for <dir>"
        );

        // (e) The daemon binary with NO --data-dir at all is not ours to kill.
        let no_flag = vec!["terminal-commanderd", "start"];
        assert!(
            !unix_argv_is_our_daemon(&no_flag, dir),
            "a daemon without an explicit --data-dir is not bound to our dir"
        );

        // (f) Empty argv is rejected (mirrors an unreadable /proc entry).
        let empty: Vec<&str> = vec![];
        assert!(!unix_argv_is_our_daemon(&empty, dir));
    }
}

// --- FIX #1: native Win32 daemon-recovery path (no powershell/taskkill) ---
//
// These exercise the real FFI against live processes. To drive the
// image-name identity gate without a real daemon build, a benign long-lived
// system exe (`ping.exe`) is COPIED to a temp file named
// `terminal-commanderd.exe` and spawned: its image FILE NAME then matches the
// daemon, so the identity gate fires exactly as it would for the real daemon,
// while the process itself is harmless and self-terminating.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// Copy a benign long-lived system exe to `<tmp>/terminal-commanderd.exe`
    /// and spawn it so the process's image FILE NAME is the daemon name. The
    /// returned `TempDir` keeps the copied exe alive for the test's duration.
    fn spawn_fake_daemon() -> (tempfile::TempDir, std::process::Child) {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = dir.path().join("terminal-commanderd.exe");
        let ping = std::path::Path::new(r"C:\Windows\System32\PING.EXE");
        std::fs::copy(ping, &fake).expect("copy ping.exe to terminal-commanderd.exe");
        // `ping -n 30 127.0.0.1` stays alive ~30s; far longer than the test.
        let child = std::process::Command::new(&fake)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn fake daemon");
        (dir, child)
    }

    #[test]
    fn pid_belongs_to_daemon_rejects_non_daemon_and_absent() {
        // The test runner's own image is the test binary, NOT
        // terminal-commanderd.exe -> must be refused (defends a recycled pid).
        let self_pid = std::process::id();
        assert!(
            !pid_belongs_to_daemon(self_pid, std::path::Path::new(r"C:\tc-not-a-daemon")),
            "the test runner's own pid (image != terminal-commanderd.exe) must be refused"
        );
        // A pid that is almost certainly absent must also be refused.
        assert!(
            !pid_belongs_to_daemon(0xFFFF_FFF0, std::path::Path::new(r"C:\tc-not-a-daemon")),
            "an absent pid must be refused"
        );
    }

    #[test]
    fn pid_belongs_to_daemon_true_only_when_image_matches() {
        let (_dir, mut child) = spawn_fake_daemon();
        let pid = child.id();
        // Image file name IS terminal-commanderd.exe -> accepted. state_dir is
        // not consulted on Windows (per-state_dir pidfile already scopes).
        let belongs = pid_belongs_to_daemon(pid, std::path::Path::new(r"C:\any\state"));
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            belongs,
            "a process whose image file name is terminal-commanderd.exe must be accepted"
        );
    }

    #[test]
    fn hard_kill_terminates_a_spawned_daemon_image() {
        let (_dir, mut child) = spawn_fake_daemon();
        let pid = child.id();
        let outcome = hard_kill(pid, std::path::Path::new(r"C:\any\state")).expect("hard_kill");
        assert_eq!(
            outcome,
            HardKillOutcome::Forced,
            "an identity-matching live process must be force-terminated"
        );
        // Reap and confirm it actually exited (TerminateProcess took effect).
        let status = child.wait().expect("wait on terminated child");
        assert!(
            !status.success(),
            "TerminateProcess(exit=1) must make the child exit non-zero"
        );
        // A second hard_kill of the now-dead pid must not force-kill anything:
        // the pid no longer resolves to our daemon image.
        let after = hard_kill(pid, std::path::Path::new(r"C:\any\state")).expect("hard_kill again");
        assert_eq!(
            after,
            HardKillOutcome::IdentitySkipped,
            "a dead/absent pid must NOT be force-killed (identity gate)"
        );
    }

    #[test]
    fn hard_kill_withholds_force_for_non_daemon_pid() {
        // The test runner itself is alive but is not the daemon image; hard_kill
        // must refuse to terminate it (this would otherwise kill the test host).
        let self_pid = std::process::id();
        let outcome =
            hard_kill(self_pid, std::path::Path::new(r"C:\any\state")).expect("hard_kill self");
        assert_eq!(
            outcome,
            HardKillOutcome::IdentitySkipped,
            "the test runner (non-daemon image) must NEVER be force-killed"
        );
    }

    #[test]
    fn find_daemon_pid_os_enumerates_running_daemon_image() {
        let (_dir, mut child) = spawn_fake_daemon();
        let pid = child.id();
        // Native ToolHelp enumeration must discover the running daemon-image
        // process. state_dir is unused on Windows (no-pidfile fallback).
        let found = find_daemon_pid_os(std::path::Path::new(r"C:\any\state"));
        let _ = child.kill();
        let _ = child.wait();
        // The snapshot may observe ANY terminal-commanderd.exe on the box, but
        // it must find at least one (ours), and ours must be a valid candidate.
        assert!(
            found.is_some(),
            "find_daemon_pid_os must enumerate the running daemon-image process (spawned pid {pid})"
        );
    }
}
