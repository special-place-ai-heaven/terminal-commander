// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Bounded, evidence-backed host discovery for terminal delegation.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::ipc::protocol::{AccessRoute, HostEnvironment, ProgramProbe, TerminalProbe, WslProbe};
#[cfg(windows)]
use terminal_commander_core::windows_silent;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Every probe process, the WSL ones included, must finish within this long
/// of discovery starting. A per-probe timeout alone did not bound discovery:
/// it started only once the process existed, and process creation is
/// serialized and takes seconds on a busy Windows host.
const DISCOVERY_DEADLINE: Duration = Duration::from_secs(3);
/// How long past the deadline discovery waits for probes to be killed and
/// report. A probe still waiting to create its process is reported as timed
/// out; its thread kills the process as soon as it exists.
const DEADLINE_GRACE: Duration = Duration::from_millis(250);
/// Past this age a call still gets the last result at once, and a refresh
/// starts in the background. Shells and tools rarely change while the daemon
/// runs, but WSL can: a distro starts, or one is installed, and a probe that
/// timed out on a busy host can succeed later.
const DISCOVERY_TTL: Duration = Duration::from_secs(30);
/// Past this age the last result is not served: the call waits for a fresh
/// discovery, bounded like the first one. Ten minutes covers an agent that
/// works in bursts a few minutes apart, so it never waits, while a daemon
/// left idle for hours does not report hours-old WSL state.
const DISCOVERY_MAX_AGE: Duration = Duration::from_mins(10);

/// Host discovery results, shared by every caller in the daemon.
static DISCOVERY: DiscoveryCache = DiscoveryCache::new();
const MAX_VERSION_CHARS: usize = 160;
const MAX_WSL_DISTROS: usize = 16;
const SHELL_SENTINEL: &str = "terminal-commander-shell-probe";
const WSL_SENTINEL: &str = "terminal-commander-wsl-probe";
const WSL_PROGRAM_PLACEHOLDER: &str = "{program}";

#[derive(Clone, Copy)]
struct ProbeSpec {
    name: &'static str,
    argv0: &'static str,
    version_args: &'static [&'static str],
}

struct ProbeOutput {
    success: bool,
    /// The output as one bounded line, for version strings and sentinels.
    text: String,
    /// The output's non-empty lines, each bounded on its own, for output
    /// that is a list (`wsl --list`).
    lines: Vec<String>,
}

enum ProbeRun {
    Complete(ProbeOutput),
    TimedOut,
    Failed,
}

enum Probed {
    Shell(usize, ProgramProbe),
    Tool(usize, ProgramProbe),
    /// The WSL list and execution runs; `None` off Windows or without WSL.
    Wsl(Option<(ProbeRun, ProbeRun)>),
}

/// The host environment from the last discovery, with `discovery_age_ms` set.
///
/// A result older than `DISCOVERY_TTL` is still returned at once and refreshed
/// in the background. A call waits only when there is no result yet or the
/// last one is older than `DISCOVERY_MAX_AGE`, and then at most
/// `DISCOVERY_DEADLINE` + `DEADLINE_GRACE`.
#[must_use]
pub fn cached_host_environment() -> HostEnvironment {
    #[cfg(not(test))]
    let discover = discover_host_environment;
    #[cfg(test)]
    let discover = discover_for_tests;
    DISCOVERY.get(discover)
}

/// Move the shared discovery cache's clock forward, as if `by` had passed.
#[cfg(test)]
pub(super) fn advance_discovery_clock(by: Duration) {
    DISCOVERY
        .clock_offset_ms
        .fetch_add(millis(by), std::sync::atomic::Ordering::SeqCst);
}

/// While set, a discovery through the shared cache takes five seconds longer,
/// so a test can tell a call that waited for it from one that did not.
#[cfg(test)]
pub(super) static SLOW_DISCOVERY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
fn discover_for_tests() -> HostEnvironment {
    if SLOW_DISCOVERY.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(Duration::from_secs(5));
    }
    discover_host_environment()
}

struct DiscoveryCache {
    state: std::sync::Mutex<CacheState>,
    refreshed: std::sync::Condvar,
    /// Tests move this cache's clock forward instead of sleeping.
    #[cfg(test)]
    clock_offset_ms: std::sync::atomic::AtomicU64,
}

struct CacheState {
    /// The last accepted result and when its discovery finished.
    last: Option<(Instant, HostEnvironment)>,
    /// A discovery is running; at most one runs at a time.
    refreshing: bool,
}

impl DiscoveryCache {
    const fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(CacheState {
                last: None,
                refreshing: false,
            }),
            refreshed: std::sync::Condvar::new(),
            #[cfg(test)]
            clock_offset_ms: std::sync::atomic::AtomicU64::new(0),
        }
    }

    #[cfg(not(test))]
    #[allow(clippy::unused_self)] // the test build reads this cache's clock offset
    fn now(&self) -> Instant {
        Instant::now()
    }

    #[cfg(test)]
    fn now(&self) -> Instant {
        Instant::now()
            + Duration::from_millis(
                self.clock_offset_ms
                    .load(std::sync::atomic::Ordering::SeqCst),
            )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CacheState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn get(&'static self, discover: fn() -> HostEnvironment) -> HostEnvironment {
        let mut state = self.lock();
        loop {
            if let Some((finished, environment)) = &state.last {
                let age = self.now().saturating_duration_since(*finished);
                if age < DISCOVERY_MAX_AGE {
                    let mut environment = environment.clone();
                    environment.discovery_age_ms = millis(age);
                    for stale in stale_ages(&mut environment) {
                        *stale = stale.saturating_add(millis(age));
                    }
                    if age >= DISCOVERY_TTL {
                        self.start_refresh(&mut state, discover);
                    }
                    return environment;
                }
            }
            self.start_refresh(&mut state, discover);
            state = self
                .refreshed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn start_refresh(&'static self, state: &mut CacheState, discover: fn() -> HostEnvironment) {
        if state.refreshing {
            return;
        }
        state.refreshing = true;
        std::thread::spawn(move || {
            let fresh = std::panic::catch_unwind(discover).ok();
            let mut state = self.lock();
            state.refreshing = false;
            if let Some(mut fresh) = fresh
                && !state.last.as_ref().is_some_and(|(finished, previous)| {
                    self.now().saturating_duration_since(*finished) < DISCOVERY_MAX_AGE
                        && lost_answers(previous, &fresh)
                })
            {
                if let Some((finished, previous)) = &state.last {
                    let previous_age = self.now().saturating_duration_since(*finished);
                    carry_confirmed(previous, &mut fresh, millis(previous_age));
                }
                state.last = Some((self.now(), fresh));
            }
            drop(state);
            self.refreshed.notify_all();
        });
    }
}

/// Whether a refresh lost an answer the previous result had: a shell, tool,
/// or WSL probe that completed before timed out now. That is the host being
/// busy, not a change, so the previous result is kept and keeps reporting
/// its real age.
fn lost_answers(previous: &HostEnvironment, fresh: &HostEnvironment) -> bool {
    let lost = |before: &[ProgramProbe], now: &[ProgramProbe]| {
        now.iter().any(|probe| {
            timed_out(probe)
                && before
                    .iter()
                    .any(|old| old.name == probe.name && !timed_out(old))
        })
    };
    lost(&previous.shells, &fresh.shells)
        || lost(&previous.tools, &fresh.tools)
        || (fresh.wsl.execution_status == "timed_out"
            && previous.wsl.execution_status != "timed_out")
}

fn timed_out(probe: &ProgramProbe) -> bool {
    probe.version_status == "timed_out" || probe.execution_status == "timed_out"
}

/// Rule: a shell, tool, or WSL probe that timed out in `fresh` but was
/// confirmed in `previous` reports the previous answers, marked
/// `stale_confirmed` with their age; with nothing confirmed before, the
/// timeout stands. `previous_age_ms` is how old `previous` is now.
fn carry_confirmed(previous: &HostEnvironment, fresh: &mut HostEnvironment, previous_age_ms: u64) {
    let carry = |before: &[ProgramProbe], now: &mut [ProgramProbe]| {
        for probe in now.iter_mut().filter(|probe| timed_out(probe)) {
            if let Some(old) = before.iter().find(|old| {
                old.name == probe.name
                    && !timed_out(old)
                    && (old.version_status == "confirmed" || old.execution_status == "confirmed")
            }) {
                *probe = old.clone();
                "path_confirmed+stale_confirmed".clone_into(&mut probe.evidence);
                probe.stale_confirmed_age_ms =
                    Some(old.stale_confirmed_age_ms.unwrap_or(0) + previous_age_ms);
            }
        }
    };
    carry(&previous.shells, &mut fresh.shells);
    carry(&previous.tools, &mut fresh.tools);
    if fresh.wsl.execution_status == "timed_out" && previous.wsl.execution_status == "confirmed" {
        fresh.wsl = previous.wsl.clone();
        fresh.wsl.stale_confirmed_age_ms =
            Some(previous.wsl.stale_confirmed_age_ms.unwrap_or(0) + previous_age_ms);
    }
    derive_routes(fresh);
}

/// The ages of every carried-over answer in `environment`.
fn stale_ages(environment: &mut HostEnvironment) -> impl Iterator<Item = &mut u64> {
    environment
        .shells
        .iter_mut()
        .chain(environment.tools.iter_mut())
        .filter_map(|probe| probe.stale_confirmed_age_ms.as_mut())
        .chain(environment.wsl.stale_confirmed_age_ms.as_mut())
}

/// `+stale_confirmed` for a route built on a carried-over answer.
const fn stale_suffix(age: Option<u64>) -> &'static str {
    if age.is_some() {
        "+stale_confirmed"
    } else {
        ""
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Discover the current daemon host with a fixed, bounded probe set.
#[must_use]
pub fn discover_host_environment() -> HostEnvironment {
    let started = Instant::now();
    let deadline = started + DISCOVERY_DEADLINE;
    let shell_specs = shell_specs();
    let tool_specs = tool_specs();

    // Every probe runs on its own detached thread, and discovery stops
    // waiting at the deadline; see `DEADLINE_GRACE`. A thread reports the
    // probe as timed out as soon as its program is found, then reports again
    // when the probe finishes.
    let (sender, results) = std::sync::mpsc::channel();
    for (index, &spec) in shell_specs.iter().enumerate() {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let Some(path) = resolve_program(spec.argv0) else {
                let _ = sender.send(Probed::Shell(index, unavailable_probe(spec.name)));
                return;
            };
            let _ = sender.send(Probed::Shell(
                index,
                timed_out_probe(spec, &path, "timed_out"),
            ));
            let _ = sender.send(Probed::Shell(index, probe_shell(spec, &path, deadline)));
        });
    }
    for (index, &spec) in tool_specs.iter().enumerate() {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let Some(path) = resolve_program(spec.argv0) else {
                let _ = sender.send(Probed::Tool(index, unavailable_probe(spec.name)));
                return;
            };
            let _ = sender.send(Probed::Tool(
                index,
                timed_out_probe(spec, &path, "not_probed"),
            ));
            let _ = sender.send(Probed::Tool(index, probe_program(spec, &path, deadline)));
        });
    }
    std::thread::spawn(move || {
        let _ = sender.send(Probed::Wsl(wsl_runs(deadline)));
    });
    let mut shells = vec![None; shell_specs.len()];
    let mut tools = vec![None; tool_specs.len()];
    let mut wsl_runs_done = None;
    let give_up = deadline + DEADLINE_GRACE;
    while let Ok(probed) = results.recv_timeout(give_up.saturating_duration_since(Instant::now())) {
        match probed {
            Probed::Shell(index, probe) => shells[index] = Some(probe),
            Probed::Tool(index, probe) => tools[index] = Some(probe),
            Probed::Wsl(runs) => wsl_runs_done = Some(runs),
        }
    }
    let shells = shells
        .into_iter()
        .zip(&shell_specs)
        .map(|(probe, &spec)| probe.unwrap_or_else(|| unreported_probe(spec, "timed_out")))
        .collect::<Vec<_>>();
    let tools = tools
        .into_iter()
        .zip(&tool_specs)
        .map(|(probe, &spec)| probe.unwrap_or_else(|| unreported_probe(spec, "not_probed")))
        .collect::<Vec<_>>();

    let wsl = wsl_probe(
        &tools,
        wsl_runs_done.unwrap_or(Some((ProbeRun::TimedOut, ProbeRun::TimedOut))),
    );
    let mut environment = HostEnvironment {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        terminal: terminal_probe(),
        shells,
        tools,
        wsl,
        discovery_ms: millis(started.elapsed()),
        ..HostEnvironment::default()
    };
    derive_routes(&mut environment);
    environment
}

/// Rebuild the access routes, beachhead, and preferred shell from the probes.
fn derive_routes(environment: &mut HostEnvironment) {
    let (shells, tools, wsl) = (&environment.shells, &environment.tools, &environment.wsl);
    let mut access_routes = shell_access_routes(shells);
    if let Some(route) = wsl_access_route(wsl, tools, access_routes.len() + 1) {
        access_routes.push(route);
    }
    access_routes.extend(direct_argv_routes(tools, access_routes.len() + 1));
    if let Some(route) = wsl_argv_access_route(wsl, tools, access_routes.len() + 1) {
        access_routes.push(route);
    }
    environment.beachhead = access_routes.first().cloned();
    environment.preferred_shell = access_routes
        .iter()
        .find(|route| route.kind == "shell")
        .map(|route| route.executable.clone());
    environment.access_routes = access_routes;
}

/// Build argv for the actual interpreter family instead of assuming `-lc`.
#[must_use]
pub fn shell_launch_argv(shell: &str, line: &str) -> Vec<String> {
    match shell_family(shell) {
        "powershell" => [
            shell,
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            line,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        "cmd" => [shell, "/D", "/S", "/C", line]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        _ => [shell, "-lc", line]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

/// Resolve the first confirmed default shell without running it, from the
/// same discovery `system_discover` reports.
#[must_use]
pub fn preferred_shell() -> Option<String> {
    cached_host_environment().preferred_shell
}

fn shell_specs() -> Vec<ProbeSpec> {
    if cfg!(windows) {
        vec![
            ProbeSpec {
                name: "pwsh",
                argv0: "pwsh.exe",
                version_args: &[
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "$PSVersionTable.PSEdition + ' ' + $PSVersionTable.PSVersion.ToString()",
                ],
            },
            ProbeSpec {
                name: "bash",
                argv0: "bash.exe",
                version_args: &["--version"],
            },
            ProbeSpec {
                name: "powershell",
                argv0: "powershell.exe",
                version_args: &[
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "$PSVersionTable.PSEdition + ' ' + $PSVersionTable.PSVersion.ToString()",
                ],
            },
            ProbeSpec {
                name: "cmd",
                argv0: "cmd.exe",
                version_args: &["/D", "/C", "ver"],
            },
        ]
    } else {
        vec![
            ProbeSpec {
                name: "bash",
                argv0: "bash",
                version_args: &["--version"],
            },
            ProbeSpec {
                name: "sh",
                argv0: "sh",
                version_args: &["--version"],
            },
            ProbeSpec {
                name: "zsh",
                argv0: "zsh",
                version_args: &["--version"],
            },
            ProbeSpec {
                name: "fish",
                argv0: "fish",
                version_args: &["--version"],
            },
            ProbeSpec {
                name: "pwsh",
                argv0: "pwsh",
                version_args: &[
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "$PSVersionTable.PSEdition + ' ' + $PSVersionTable.PSVersion.ToString()",
                ],
            },
        ]
    }
}

fn shell_family(shell: &str) -> &'static str {
    let family = Path::new(shell)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(shell);
    if family.eq_ignore_ascii_case("pwsh") || family.eq_ignore_ascii_case("powershell") {
        "powershell"
    } else if family.eq_ignore_ascii_case("cmd") {
        "cmd"
    } else {
        "posix"
    }
}

fn shell_access_routes(shells: &[ProgramProbe]) -> Vec<AccessRoute> {
    shells
        .iter()
        .filter(|shell| shell.execution_status == "confirmed")
        .filter_map(|shell| shell.path.as_ref().map(|path| (shell, path)))
        .enumerate()
        .map(|(index, (shell, path))| AccessRoute {
            route_id: format!("shell:{}", shell.name),
            rank: u16::try_from(index + 1).unwrap_or(u16::MAX),
            kind: "shell".to_owned(),
            family: shell_family(path).to_owned(),
            executable: path.clone(),
            argv_template: shell_launch_argv(path, "{command}"),
            version: shell.version.clone(),
            evidence: format!(
                "path_confirmed+version_{}+execution_confirmed{}",
                shell.version_status,
                stale_suffix(shell.stale_confirmed_age_ms)
            ),
        })
        .collect()
}

fn wsl_access_route(wsl: &WslProbe, tools: &[ProgramProbe], rank: usize) -> Option<AccessRoute> {
    let shell = wsl.default_shell.as_ref()?;
    let program = tools
        .iter()
        .find(|probe| probe.name == "wsl" && probe.available)?;
    let executable = program.path.clone()?;
    Some(AccessRoute {
        route_id: format!("wsl:default:{shell}"),
        rank: u16::try_from(rank).unwrap_or(u16::MAX),
        kind: "wsl_shell".to_owned(),
        family: "posix".to_owned(),
        argv_template: vec![
            executable.clone(),
            "-e".to_owned(),
            shell.clone(),
            "-lc".to_owned(),
            "{command}".to_owned(),
        ],
        executable,
        version: wsl.version.clone(),
        evidence: format!(
            "path_confirmed+wsl_execution_confirmed{}",
            stale_suffix(wsl.stale_confirmed_age_ms)
        ),
    })
}

fn direct_argv_routes(tools: &[ProgramProbe], start_rank: usize) -> Vec<AccessRoute> {
    tools
        .iter()
        .filter(|program| {
            program.available && program.version_status == "confirmed" && program.name != "wsl"
        })
        .filter_map(|program| {
            let executable = program.path.clone()?;
            Some((program, executable))
        })
        .enumerate()
        .map(|(index, (program, executable))| AccessRoute {
            route_id: format!("argv:{}", program.name),
            rank: u16::try_from(start_rank + index).unwrap_or(u16::MAX),
            kind: "direct_argv".to_owned(),
            family: "native".to_owned(),
            argv_template: vec![executable.clone(), "{args...}".to_owned()],
            executable,
            version: program.version.clone(),
            evidence: format!(
                "path_confirmed+version_confirmed+direct_argv_structural{}",
                stale_suffix(program.stale_confirmed_age_ms)
            ),
        })
        .collect()
}

fn wsl_argv_access_route(
    wsl: &WslProbe,
    tools: &[ProgramProbe],
    rank: usize,
) -> Option<AccessRoute> {
    if wsl.execution_status != "confirmed" {
        return None;
    }
    let program = tools
        .iter()
        .find(|probe| probe.name == "wsl" && probe.available)?;
    let executable = program.path.clone()?;
    Some(AccessRoute {
        route_id: "wsl:default:argv".to_owned(),
        rank: u16::try_from(rank).unwrap_or(u16::MAX),
        kind: "wsl_argv".to_owned(),
        family: "posix".to_owned(),
        argv_template: vec![
            executable.clone(),
            "-e".to_owned(),
            WSL_PROGRAM_PLACEHOLDER.to_owned(),
            "{args...}".to_owned(),
        ],
        executable,
        version: wsl.version.clone(),
        evidence: format!(
            "path_confirmed+wsl_execution_confirmed+direct_argv_structural{}",
            stale_suffix(wsl.stale_confirmed_age_ms)
        ),
    })
}

const COMMON_TOOL_SPECS: &[(&str, &[&str])] = &[
    ("git", &["--version"]),
    ("rg", &["--version"]),
    ("grep", &["--version"]),
    ("sed", &["--version"]),
    ("awk", &["--version"]),
    ("tail", &["--version"]),
    ("head", &["--version"]),
    ("curl", &["--version"]),
    ("jq", &["--version"]),
    ("python", &["--version"]),
    ("node", &["--version"]),
    ("npm", &["--version"]),
    ("cargo", &["--version"]),
    ("rustc", &["--version"]),
    ("go", &["version"]),
    ("dotnet", &["--version"]),
    ("java", &["--version"]),
    ("cmake", &["--version"]),
    ("make", &["--version"]),
    ("docker", &["--version"]),
    ("gh", &["--version"]),
];

fn tool_specs() -> Vec<ProbeSpec> {
    let mut specs = COMMON_TOOL_SPECS
        .iter()
        .map(|&(name, version_args)| ProbeSpec {
            name,
            argv0: name,
            version_args,
        })
        .collect::<Vec<_>>();
    if cfg!(windows) {
        specs.push(ProbeSpec {
            name: "wsl",
            argv0: "wsl.exe",
            version_args: &["--version"],
        });
    }
    specs
}

fn probe_program(spec: ProbeSpec, path: &Path, deadline: Instant) -> ProgramProbe {
    let run = run_bounded(path, spec.version_args, deadline);
    let (version, version_status) = match run {
        ProbeRun::Complete(output) if output.success => (nonempty(output.text), "confirmed"),
        ProbeRun::Complete(output) => (nonempty(output.text), "failed"),
        ProbeRun::TimedOut => (None, "timed_out"),
        ProbeRun::Failed => (None, "failed"),
    };
    ProgramProbe {
        name: spec.name.to_owned(),
        available: true,
        path: Some(path.to_string_lossy().into_owned()),
        version,
        evidence: "path_confirmed".to_owned(),
        version_status: version_status.to_owned(),
        execution_status: "not_probed".to_owned(),
        stale_confirmed_age_ms: None,
    }
}

fn probe_shell(spec: ProbeSpec, path: &Path, deadline: Instant) -> ProgramProbe {
    let path_text = path.to_string_lossy().into_owned();
    let line = shell_probe_line(&path_text);
    // Not a login shell: that sources the user's startup files, which can be
    // slow, can exit early, and on Linux run the installed autostart, which
    // starts a daemon. The probe only confirms the interpreter runs a line.
    let command_argv = match shell_family(&path_text) {
        "posix" => vec![path_text.clone(), "-c".to_owned(), line.to_owned()],
        _ => shell_launch_argv(&path_text, line),
    };
    let interpreter_args = command_argv
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (version_run, execution_run) = std::thread::scope(|scope| {
        let version_job = scope.spawn(|| run_bounded(path, spec.version_args, deadline));
        let execution_job = scope.spawn(|| run_bounded(path, &interpreter_args, deadline));
        (
            version_job.join().unwrap_or(ProbeRun::Failed),
            execution_job.join().unwrap_or(ProbeRun::Failed),
        )
    });
    let (version, version_status) = match version_run {
        ProbeRun::Complete(output) if output.success => (nonempty(output.text), "confirmed"),
        ProbeRun::Complete(output) => (nonempty(output.text), "failed"),
        ProbeRun::TimedOut => (None, "timed_out"),
        ProbeRun::Failed => (None, "failed"),
    };
    let execution_status = match execution_run {
        ProbeRun::Complete(output) if output.success && output.text == SHELL_SENTINEL => {
            "confirmed"
        }
        ProbeRun::TimedOut => "timed_out",
        _ => "failed",
    };
    ProgramProbe {
        name: spec.name.to_owned(),
        available: true,
        path: Some(path_text),
        version,
        evidence: "path_confirmed".to_owned(),
        version_status: version_status.to_owned(),
        execution_status: execution_status.to_owned(),
        stale_confirmed_age_ms: None,
    }
}

/// A probe whose program was found but that has not finished.
fn timed_out_probe(spec: ProbeSpec, path: &Path, execution_status: &str) -> ProgramProbe {
    ProgramProbe {
        name: spec.name.to_owned(),
        available: true,
        path: Some(path.to_string_lossy().into_owned()),
        version: None,
        evidence: "path_confirmed".to_owned(),
        version_status: "timed_out".to_owned(),
        execution_status: execution_status.to_owned(),
        stale_confirmed_age_ms: None,
    }
}

/// A probe whose thread did not even finish looking up its program by the
/// deadline.
fn unreported_probe(spec: ProbeSpec, execution_status: &str) -> ProgramProbe {
    resolve_program(spec.argv0).map_or_else(
        || unavailable_probe(spec.name),
        |path| timed_out_probe(spec, &path, execution_status),
    )
}

fn unavailable_probe(name: &str) -> ProgramProbe {
    ProgramProbe {
        name: name.to_owned(),
        available: false,
        path: None,
        version: None,
        evidence: "path_not_found".to_owned(),
        version_status: "unavailable".to_owned(),
        execution_status: "unavailable".to_owned(),
        stale_confirmed_age_ms: None,
    }
}

fn shell_probe_line(shell: &str) -> &'static str {
    match shell_family(shell) {
        "powershell" => "Write-Output terminal-commander-shell-probe",
        "cmd" => "echo terminal-commander-shell-probe",
        _ => "printf terminal-commander-shell-probe",
    }
}

/// Run one probe process until it exits, `PROBE_TIMEOUT` after it started,
/// or `deadline`, whichever comes first.
fn run_bounded(program: &Path, args: &[&str], deadline: Instant) -> ProbeRun {
    if Instant::now() >= deadline {
        return ProbeRun::TimedOut;
    }
    let mut command = Command::new(program);
    terminal_commander_core::as_daemon_child(&mut command);
    #[cfg(windows)]
    {
        windows_silent(&mut command);
        // SECURITY: the WSL execution probe runs `wsl.exe -e sh -c`, which
        // launches a Linux process, and wsl.exe forwards every Windows variable
        // NAMED in WSLENV into it. Rebuild WSLENV to the TC-only allowlist so an
        // ambient `WSLENV=SOME_SECRET/u` cannot leak across the boundary.
        let tc_session = std::env::var("TC_SESSION").ok();
        terminal_commander_core::sanitize_wslenv(&mut command, tc_session.as_deref());
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return ProbeRun::Failed;
    };
    let deadline = deadline.min(Instant::now() + PROBE_TIMEOUT);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let Ok(output) = child.wait_with_output() else {
                    return ProbeRun::Failed;
                };
                let bytes = if output.stdout.is_empty() {
                    output.stderr
                } else {
                    output.stdout
                };
                return ProbeRun::Complete(probe_output(status.success(), &bytes));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeRun::TimedOut;
            }
            Err(_) => return ProbeRun::Failed,
        }
    }
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let literal = Path::new(program);
    if program.contains('/') || program.contains('\\') {
        return executable_file(literal).then(|| absolute_path(literal));
    }
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let extensions: Vec<String> = if literal.extension().is_some() {
        vec![String::new()]
    } else {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
            .split(';')
            .map(str::trim)
            .filter(|ext| !ext.is_empty())
            .map(str::to_owned)
            .collect()
    };
    #[cfg(not(windows))]
    let extensions = [String::new()];

    for directory in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = directory.join(format!("{program}{extension}"));
            if executable_file(&candidate) {
                return Some(absolute_path(&candidate));
            }
        }
    }
    None
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

fn executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    true
}

fn terminal_probe() -> TerminalProbe {
    let ci = std::env::var_os("CI").is_some();
    let (kind, name, mut evidence) = if std::env::var_os("WT_SESSION").is_some() {
        (
            "windows_terminal",
            Some("Windows Terminal".to_owned()),
            "WT_SESSION".to_owned(),
        )
    } else if std::env::var_os("TERM_PROGRAM").is_some() {
        (
            "term_program",
            bounded_env_marker("TERM_PROGRAM"),
            "TERM_PROGRAM".to_owned(),
        )
    } else if std::env::var_os("ConEmuPID").is_some() {
        ("conemu", Some("ConEmu".to_owned()), "ConEmuPID".to_owned())
    } else if std::env::var_os("TERM").is_some() {
        ("posix_term", bounded_env_marker("TERM"), "TERM".to_owned())
    } else {
        ("unknown", None, "no_terminal_marker".to_owned())
    };
    let version = bounded_env_marker("TERM_PROGRAM_VERSION");
    if version.is_some() {
        evidence.push_str("+TERM_PROGRAM_VERSION");
    }
    TerminalProbe {
        kind: kind.to_owned(),
        evidence,
        name,
        version,
        interactive: Some(std::io::stdin().is_terminal() || std::io::stdout().is_terminal()),
        ci,
    }
}

fn bounded_env_marker(name: &str) -> Option<String> {
    let value = std::env::var_os(name)?;
    let bounded = value
        .to_string_lossy()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(80)
        .collect::<String>();
    (!bounded.is_empty()).then_some(bounded)
}

/// The WSL list and execution probes, run concurrently. They need only the
/// `wsl.exe` path, so they run alongside the other probes rather than after.
fn wsl_runs(deadline: Instant) -> Option<(ProbeRun, ProbeRun)> {
    if !cfg!(windows) {
        return None;
    }
    let path = resolve_program("wsl.exe")?;
    let path = path.as_path();
    Some(std::thread::scope(|scope| {
        let list_job = scope.spawn(|| run_bounded(path, &["--list", "--quiet"], deadline));
        // `sh -c`, not a login shell, for the reason given in `probe_shell`.
        let execution_job = scope.spawn(|| {
            run_bounded(
                path,
                &["-e", "sh", "-c", &format!("printf {WSL_SENTINEL}")],
                deadline,
            )
        });
        (
            list_job.join().unwrap_or(ProbeRun::Failed),
            execution_job.join().unwrap_or(ProbeRun::Failed),
        )
    }))
}

fn wsl_probe(tools: &[ProgramProbe], runs: Option<(ProbeRun, ProbeRun)>) -> WslProbe {
    if !cfg!(windows) {
        return WslProbe {
            available: false,
            status: "not_windows".to_owned(),
            execution_status: "not_windows".to_owned(),
            ..Default::default()
        };
    }
    let Some(wsl) = tools
        .iter()
        .find(|probe| probe.name == "wsl" && probe.available)
    else {
        return WslProbe {
            available: false,
            status: "path_not_found".to_owned(),
            execution_status: "unavailable".to_owned(),
            ..Default::default()
        };
    };
    let (Some(_), Some((list_run, execution_run))) = (wsl.path.as_deref(), runs) else {
        return WslProbe {
            available: false,
            status: "path_not_found".to_owned(),
            execution_status: "unavailable".to_owned(),
            ..Default::default()
        };
    };
    let distributions = match list_run {
        ProbeRun::Complete(output) if output.success => Some(output.lines),
        _ => None,
    };
    let execution_status = match execution_run {
        ProbeRun::Complete(output) if output.success && output.text == WSL_SENTINEL => "confirmed",
        ProbeRun::TimedOut => "timed_out",
        _ => "failed",
    };
    WslProbe {
        available: true,
        status: if distributions.is_some() {
            "confirmed"
        } else {
            "probe_failed"
        }
        .to_owned(),
        version: wsl.version.clone(),
        distributions: distributions.unwrap_or_default(),
        default_shell: (execution_status == "confirmed").then(|| "sh".to_owned()),
        execution_status: execution_status.to_owned(),
        stale_confirmed_age_ms: None,
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn probe_output(success: bool, bytes: &[u8]) -> ProbeOutput {
    let decoded = String::from_utf8_lossy(bytes).replace('\0', "");
    let lines = decoded
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_WSL_DISTROS)
        .collect::<Vec<_>>();
    ProbeOutput {
        success,
        text: bounded_line(&lines.join("\n")),
        // Each line is bounded on its own: `redact_shell_line` splits on all
        // whitespace and rejoins with spaces, so bounding the joined text
        // turned `wsl --list` output into a single distribution name.
        lines: lines.iter().map(|line| bounded_line(line)).collect(),
    }
}

fn bounded_line(text: &str) -> String {
    crate::command::redact_shell_line(text)
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\n')
        .take(MAX_VERSION_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::apply_execution_policy;
    use super::*;
    use crate::policy::{PolicyCaps, PolicyEngine, PolicyProfile};

    #[test]
    fn shell_disabled_environment_keeps_only_direct_argv_routes() {
        let mut host = discover_host_environment();
        let shell_off = PolicyEngine::with_config_caps(
            PolicyProfile::DeveloperLocal,
            None,
            None,
            PolicyCaps::default(),
        );
        apply_execution_policy(&mut host, &shell_off);

        assert!(
            host.access_routes
                .iter()
                .all(|route| matches!(route.kind.as_str(), "direct_argv" | "wsl_argv")),
            "shell-disabled discovery must not promise a shell route: {:?}",
            host.access_routes
        );
        assert_eq!(host.beachhead, host.access_routes.first().cloned());
        if let Some(beachhead) = &host.beachhead {
            assert!(
                matches!(beachhead.kind.as_str(), "direct_argv" | "wsl_argv"),
                "allow_shell off must not beachhead a native shell route"
            );
            assert_eq!(
                beachhead.argv_template.last().map(String::as_str),
                Some("{args...}"),
                "argv beachhead template must end in {{args...}}"
            );
        }
        assert_eq!(host.preferred_shell, None);
        if host
            .tools
            .iter()
            .any(|tool| tool.available && tool.version_status == "confirmed" && tool.name != "wsl")
        {
            assert!(
                !host.access_routes.is_empty(),
                "confirmed programs must establish at least one direct argv beachhead"
            );
        }
    }

    /// `wsl --list --quiet` writes one name per line as UTF-16LE with CRLF.
    #[cfg(windows)]
    fn wsl_list_bytes(names: &[String]) -> Vec<u8> {
        names
            .iter()
            .flat_map(|name| format!("{name}\r\n").encode_utf16().collect::<Vec<_>>())
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[cfg(windows)]
    fn distributions_from(names: &[String]) -> Vec<String> {
        let tools = [ProgramProbe {
            name: "wsl".to_owned(),
            available: true,
            path: Some(r"C:\Windows\System32\wsl.exe".to_owned()),
            version: None,
            evidence: "path_confirmed".to_owned(),
            version_status: "confirmed".to_owned(),
            execution_status: "not_probed".to_owned(),
            stale_confirmed_age_ms: None,
        }];
        let list = ProbeRun::Complete(probe_output(true, &wsl_list_bytes(names)));
        wsl_probe(&tools, Some((list, ProbeRun::TimedOut))).distributions
    }

    #[cfg(windows)]
    #[test]
    fn wsl_list_output_yields_one_distribution_per_line() {
        let two = vec!["Ubuntu-24.04".to_owned(), "docker-desktop".to_owned()];
        assert_eq!(distributions_from(&two), two);

        let many = (0..MAX_WSL_DISTROS + 4)
            .map(|i| format!("distro-with-a-long-name-{i:02}"))
            .collect::<Vec<_>>();
        assert_eq!(distributions_from(&many), many[..MAX_WSL_DISTROS]);
    }

    /// Version text stays one bounded line, as before.
    #[test]
    fn probe_version_text_stays_one_line() {
        let output = probe_output(true, b"GNU bash, version 5.2\nCopyright (C) 2022\n");
        assert_eq!(output.text, "GNU bash, version 5.2 Copyright (C) 2022");
        assert_eq!(
            output.lines,
            ["GNU bash, version 5.2", "Copyright (C) 2022"]
        );
    }

    use std::sync::atomic::{AtomicU64, Ordering};

    impl DiscoveryCache {
        fn advance(&self, by: Duration) {
            self.clock_offset_ms.fetch_add(millis(by), Ordering::SeqCst);
        }

        /// Wait until no refresh is running.
        fn settle(&self) {
            let mut state = self.lock();
            while state.refreshing {
                state = self.refreshed.wait(state).unwrap();
            }
        }
    }

    fn leaked_cache() -> &'static DiscoveryCache {
        Box::leak(Box::new(DiscoveryCache::new()))
    }

    /// A discovery that takes half a second and numbers its results in
    /// `discovery_ms`.
    fn slow_discovery(runs: &AtomicU64) -> HostEnvironment {
        std::thread::sleep(Duration::from_millis(500));
        HostEnvironment {
            discovery_ms: runs.fetch_add(1, Ordering::SeqCst) + 1,
            ..HostEnvironment::default()
        }
    }

    static STALE_RUNS: AtomicU64 = AtomicU64::new(0);
    fn stale_discovery() -> HostEnvironment {
        slow_discovery(&STALE_RUNS)
    }

    #[test]
    fn a_stale_result_is_served_at_once_and_refreshed_once() {
        let cache = leaked_cache();
        assert_eq!(cache.get(stale_discovery).discovery_ms, 1);
        cache.advance(DISCOVERY_TTL + Duration::from_secs(1));

        let callers = (0..8)
            .map(|_| {
                std::thread::spawn(move || {
                    let started = Instant::now();
                    (cache.get(stale_discovery), started.elapsed())
                })
            })
            .collect::<Vec<_>>();
        for caller in callers {
            let (environment, took) = caller.join().unwrap();
            assert!(
                took < Duration::from_millis(200),
                "a caller waited {took:?}"
            );
            assert_eq!(environment.discovery_ms, 1, "the previous result is served");
            assert!(environment.discovery_age_ms >= millis(DISCOVERY_TTL));
        }

        cache.settle();
        assert_eq!(
            STALE_RUNS.load(Ordering::SeqCst),
            2,
            "one background refresh"
        );
        let refreshed = cache.get(stale_discovery);
        assert_eq!(refreshed.discovery_ms, 2);
        assert!(refreshed.discovery_age_ms < 1_000);
    }

    static OLD_RUNS: AtomicU64 = AtomicU64::new(0);
    fn old_discovery() -> HostEnvironment {
        slow_discovery(&OLD_RUNS)
    }

    #[test]
    fn a_result_past_the_max_age_is_not_served() {
        let cache = leaked_cache();
        assert_eq!(cache.get(old_discovery).discovery_ms, 1);
        cache.advance(DISCOVERY_MAX_AGE + Duration::from_secs(1));
        let fresh = cache.get(old_discovery);
        assert_eq!(
            fresh.discovery_ms, 2,
            "the call waits for a fresh discovery"
        );
        assert!(fresh.discovery_age_ms < 1_000);
    }

    static BUSY_RUNS: AtomicU64 = AtomicU64::new(0);
    /// The first discovery confirms pwsh; later ones time out on it.
    fn busy_discovery() -> HostEnvironment {
        let run = BUSY_RUNS.fetch_add(1, Ordering::SeqCst) + 1;
        let status = if run == 1 { "confirmed" } else { "timed_out" };
        HostEnvironment {
            discovery_ms: run,
            shells: vec![ProgramProbe {
                name: "pwsh".to_owned(),
                available: true,
                path: Some("pwsh".to_owned()),
                version: None,
                evidence: "path_confirmed".to_owned(),
                version_status: status.to_owned(),
                execution_status: status.to_owned(),
                stale_confirmed_age_ms: None,
            }],
            ..HostEnvironment::default()
        }
    }

    #[test]
    fn a_refresh_that_lost_answers_keeps_the_previous_result() {
        let cache = leaked_cache();
        assert_eq!(cache.get(busy_discovery).discovery_ms, 1);
        cache.advance(DISCOVERY_TTL + Duration::from_secs(1));
        let _ = cache.get(busy_discovery);
        cache.settle();
        assert_eq!(BUSY_RUNS.load(Ordering::SeqCst), 2);

        let kept = cache.get(busy_discovery);
        assert_eq!(kept.discovery_ms, 1, "the timed-out refresh is not used");
        assert!(
            kept.discovery_age_ms >= millis(DISCOVERY_TTL),
            "with its real age"
        );
    }

    /// Discovery number `run`: pwsh and WSL confirmed when `confirmed`,
    /// timed out otherwise.
    fn pwsh_and_wsl(run: u64, confirmed: bool) -> HostEnvironment {
        let status = if confirmed { "confirmed" } else { "timed_out" };
        HostEnvironment {
            discovery_ms: run,
            shells: vec![ProgramProbe {
                name: "pwsh".to_owned(),
                available: true,
                path: Some("pwsh".to_owned()),
                version: confirmed.then(|| "PowerShell 7.5".to_owned()),
                evidence: "path_confirmed".to_owned(),
                version_status: status.to_owned(),
                execution_status: status.to_owned(),
                stale_confirmed_age_ms: None,
            }],
            wsl: WslProbe {
                available: true,
                status: if confirmed {
                    "confirmed"
                } else {
                    "probe_failed"
                }
                .to_owned(),
                distributions: if confirmed {
                    vec!["Ubuntu".to_owned()]
                } else {
                    Vec::new()
                },
                execution_status: status.to_owned(),
                ..WslProbe::default()
            },
            ..HostEnvironment::default()
        }
    }

    static IDLE_RUNS: AtomicU64 = AtomicU64::new(0);
    /// The first discovery confirms pwsh and WSL; later ones time out.
    fn idle_discovery() -> HostEnvironment {
        let run = IDLE_RUNS.fetch_add(1, Ordering::SeqCst) + 1;
        pwsh_and_wsl(run, run == 1)
    }

    #[test]
    fn a_timed_out_refresh_reports_the_last_confirmed_answers_as_stale() {
        let cache = leaked_cache();
        assert_eq!(cache.get(idle_discovery).discovery_ms, 1);
        cache.advance(DISCOVERY_MAX_AGE + Duration::from_secs(1));

        let fresh = cache.get(idle_discovery);
        assert_eq!(fresh.discovery_ms, 2, "past the max age, a fresh discovery");
        let pwsh = &fresh.shells[0];
        assert_eq!(
            (pwsh.execution_status.as_str(), pwsh.evidence.as_str()),
            ("confirmed", "path_confirmed+stale_confirmed"),
            "a probe confirmed before keeps its answers, marked stale"
        );
        assert!(pwsh.stale_confirmed_age_ms >= Some(millis(DISCOVERY_MAX_AGE)));
        assert_eq!(fresh.wsl.execution_status, "confirmed");
        assert_eq!(fresh.wsl.distributions, ["Ubuntu"]);
        assert!(fresh.wsl.stale_confirmed_age_ms >= Some(millis(DISCOVERY_MAX_AGE)));
        assert_eq!(
            fresh.preferred_shell.as_deref(),
            Some("pwsh"),
            "routes are rebuilt from the carried answers"
        );
        assert!(
            fresh.access_routes[0]
                .evidence
                .ends_with("+stale_confirmed")
        );

        cache.advance(Duration::from_secs(5));
        let later = cache.get(idle_discovery);
        assert!(
            later.shells[0].stale_confirmed_age_ms
                >= Some(millis(DISCOVERY_MAX_AGE + Duration::from_secs(5))),
            "the stale age keeps growing while served"
        );
    }

    static FIRST_RUNS: AtomicU64 = AtomicU64::new(0);
    fn first_timed_out_discovery() -> HostEnvironment {
        pwsh_and_wsl(FIRST_RUNS.fetch_add(1, Ordering::SeqCst) + 1, false)
    }

    #[test]
    fn a_first_discovery_that_timed_out_stays_timed_out() {
        let first = leaked_cache().get(first_timed_out_discovery);
        assert_eq!(first.shells[0].execution_status, "timed_out");
        assert_eq!(first.shells[0].stale_confirmed_age_ms, None);
        assert_eq!(first.wsl.execution_status, "timed_out");
        assert_eq!(first.wsl.stale_confirmed_age_ms, None);
    }
}
