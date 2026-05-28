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
use std::time::Duration;

use crate::ensure::{Endpoint, EnsureDaemonOptions, probe_endpoint};
use crate::pidfile;

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
    /// A force signal (SIGKILL / `taskkill /F`) was sent: the pid was still
    /// alive AND still our daemon.
    Forced,
    /// After the grace window the pid was alive but NO LONGER our daemon --
    /// the OS recycled the number for an unrelated process during the grace.
    /// The force signal was WITHHELD. Closes the kill-leg pid-reuse window
    /// (review finding #2): the pre-SIGTERM identity check alone left the
    /// SIGKILL leg ungated, so a recycled pid could be force-killed.
    IdentitySkipped,
}

/// Hard-kill a pid (SIGTERM then SIGKILL on unix; taskkill /F on
/// windows). Uses OS tools, no libc dependency.
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
        // Windows has no graceful -> grace -> force window: `taskkill /F` is a
        // single forced terminate with no intervening sleep, so the SIGKILL-leg
        // recycle window the unix path closes does not exist here. Re-verify
        // identity once more anyway, for defense in depth and parity with the
        // unix kill-leg gate.
        if !pid_belongs_to_daemon(pid, state_dir) {
            return Ok(HardKillOutcome::IdentitySkipped);
        }
        let out = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()?;
        if out.status.success() {
            Ok(HardKillOutcome::Forced)
        } else {
            Err(std::io::Error::other(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ))
        }
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
        let needle = state_dir.to_string_lossy().to_string();
        let pat = needle.replace('[', "`[").replace(']', "`]");
        let ps = format!(
            "Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\" | \
             Where-Object {{ $_.Name -eq 'terminal-commanderd.exe' -and \
             $_.CommandLine -like '*{pat}*' }} | \
             Select-Object -First 1 -ExpandProperty ProcessId"
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == pid.to_string())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        // `ps -p <pid> -o args=` prints only that pid's command line (empty
        // if the pid is gone). Confirm both the daemon name and state_dir via
        // the shared literal matcher.
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output()
            .ok()
            .map(|o| cmdline_is_our_daemon(&String::from_utf8_lossy(&o.stdout), state_dir))
            .unwrap_or(false)
    }
}

/// Literal (non-regex) test that a process command line `args` identifies our
/// daemon bound to `state_dir`: it must reference both the daemon binary name
/// and the exact state-dir path. Substring match, so a state_dir containing
/// regex metacharacters (`+ ( ) [ ]` ...) is matched verbatim. The shared
/// matcher for both `pid_belongs_to_daemon` and `find_daemon_pid_os`, so the
/// two callers agree by construction (review finding #3).
#[cfg(unix)]
fn cmdline_is_our_daemon(args: &str, state_dir: &Path) -> bool {
    let needle = state_dir.to_string_lossy();
    args.contains("terminal-commanderd") && args.contains(needle.as_ref())
}

/// OS-query fallback: find the pid of a terminal-commanderd process
/// whose command line references `state_dir`, so we only ever target
/// OUR daemon (the daemon is launched with `--data-dir <state_dir>`),
/// never a bare name match.
#[must_use]
pub fn find_daemon_pid_os(state_dir: &Path) -> Option<u32> {
    #[cfg(windows)]
    {
        let needle = state_dir.to_string_lossy().to_string();
        // PowerShell `-like` treats backslash literally, so the path is
        // passed RAW (no escaping). `[`/`]` are wildcard chars in -like;
        // escape them so a bracketed path can't break the pattern.
        let pat = needle.replace('[', "`[").replace(']', "`]");
        let ps = format!(
            "Get-CimInstance Win32_Process -Filter \"Name='terminal-commanderd.exe'\" | \
             Where-Object {{ $_.CommandLine -like '*{pat}*' }} | \
             Select-Object -First 1 -ExpandProperty ProcessId"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(unix)]
    {
        // Enumerate candidate daemons by the FIXED binary name -- a literal
        // with no regex metacharacters -- then confirm each through the SAME
        // literal matcher `pid_belongs_to_daemon` uses. Both callers now agree
        // by construction, and a state_dir containing regex metacharacters can
        // no longer break the search or make pgrep error (review finding #3).
        // The previous `pgrep -f "terminal-commanderd.*{state_dir}"`
        // interpolated the unescaped path straight into a regex.
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
}
