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

/// Hard-kill a pid (SIGTERM then SIGKILL on unix; taskkill /F on
/// windows). Uses OS tools, no libc dependency.
pub fn hard_kill(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
        std::thread::sleep(Duration::from_millis(800));
        if pidfile::pid_alive(pid) {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ))
        }
    }
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
    let needle = state_dir.to_string_lossy().to_string();
    #[cfg(windows)]
    {
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
        // if the pid is gone). Require both the daemon name and state_dir.
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output()
            .ok()
            .map(|o| {
                let args = String::from_utf8_lossy(&o.stdout);
                args.contains("terminal-commanderd") && args.contains(&needle)
            })
            .unwrap_or(false)
    }
}

/// OS-query fallback: find the pid of a terminal-commanderd process
/// whose command line references `state_dir`, so we only ever target
/// OUR daemon (the daemon is launched with `--data-dir <state_dir>`),
/// never a bare name match.
#[must_use]
pub fn find_daemon_pid_os(state_dir: &Path) -> Option<u32> {
    let needle = state_dir.to_string_lossy().to_string();
    #[cfg(windows)]
    {
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
        let out = std::process::Command::new("pgrep")
            .args(["-f", &format!("terminal-commanderd.*{needle}")])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .trim()
            .parse()
            .ok()
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

    if let Err(e) = hard_kill(pid) {
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
}
