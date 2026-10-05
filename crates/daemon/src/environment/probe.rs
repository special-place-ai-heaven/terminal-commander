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
/// How long a discovery result is reused. Shells and tools rarely change
/// while the daemon runs, but WSL can: a distro starts, or one is installed,
/// and a probe that timed out on a busy host can succeed later. Reusing a
/// result this briefly keeps such a change, or a timed-out probe, from
/// being reported for long.
const DISCOVERY_TTL: Duration = Duration::from_secs(30);

/// The last discovery result and when it finished.
static DISCOVERY_CACHE: std::sync::Mutex<Option<(Instant, HostEnvironment)>> =
    std::sync::Mutex::new(None);
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
    text: String,
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

/// The host environment, reusing a discovery for `DISCOVERY_TTL`.
///
/// Otherwise this discovers now. Concurrent callers share one discovery, so
/// a call waits at most `DISCOVERY_DEADLINE` + `DEADLINE_GRACE`.
#[must_use]
pub fn cached_host_environment() -> HostEnvironment {
    let mut cache = DISCOVERY_CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((finished, environment)) = cache.as_ref()
        && finished.elapsed() < DISCOVERY_TTL
    {
        return environment.clone();
    }
    let environment = discover_host_environment();
    *cache = Some((Instant::now(), environment.clone()));
    environment
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
    let mut access_routes = shell_access_routes(&shells);
    if let Some(route) = wsl_access_route(&wsl, &tools, access_routes.len() + 1) {
        access_routes.push(route);
    }
    access_routes.extend(direct_argv_routes(&tools, access_routes.len() + 1));
    if let Some(route) = wsl_argv_access_route(&wsl, &tools, access_routes.len() + 1) {
        access_routes.push(route);
    }
    let beachhead = access_routes.first().cloned();
    let preferred_shell = access_routes
        .iter()
        .find(|route| route.kind == "shell")
        .map(|route| route.executable.clone());

    HostEnvironment {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        terminal: terminal_probe(),
        shells,
        tools,
        wsl,
        access_routes,
        beachhead,
        preferred_shell,
        discovery_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
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
            evidence: if shell.version_status == "confirmed" {
                "path_confirmed+version_confirmed+execution_confirmed".to_owned()
            } else {
                format!(
                    "path_confirmed+version_{}+execution_confirmed",
                    shell.version_status
                )
            },
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
        evidence: "path_confirmed+wsl_execution_confirmed".to_owned(),
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
            evidence: "path_confirmed+version_confirmed+direct_argv_structural".to_owned(),
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
        evidence: "path_confirmed+wsl_execution_confirmed+direct_argv_structural".to_owned(),
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
    }
}

fn probe_shell(spec: ProbeSpec, path: &Path, deadline: Instant) -> ProgramProbe {
    let path_text = path.to_string_lossy().into_owned();
    let command_argv = shell_launch_argv(&path_text, shell_probe_line(&path_text));
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
    #[cfg(windows)]
    windows_silent(&mut command);
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
                return ProbeRun::Complete(ProbeOutput {
                    success: status.success(),
                    text: bounded_text(&bytes),
                });
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
        let execution_job = scope.spawn(|| {
            run_bounded(
                path,
                &["-e", "sh", "-lc", &format!("printf {WSL_SENTINEL}")],
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
        ProbeRun::Complete(output) if output.success => Some(
            output
                .text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(MAX_WSL_DISTROS)
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        ),
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
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn bounded_text(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes).replace('\0', "");
    let joined = decoded
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_WSL_DISTROS)
        .collect::<Vec<_>>()
        .join("\n");
    crate::command::redact_shell_line(&joined)
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
}
