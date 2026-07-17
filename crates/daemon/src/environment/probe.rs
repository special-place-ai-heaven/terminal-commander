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

/// Discover the current daemon host with a fixed, bounded probe set.
#[must_use]
pub fn discover_host_environment() -> HostEnvironment {
    let started = Instant::now();
    let shell_specs = shell_specs();
    let tool_specs = tool_specs();

    let (shells, tools) = std::thread::scope(|scope| {
        let mut shell_jobs = Vec::with_capacity(shell_specs.len());
        for &spec in &shell_specs {
            shell_jobs.push(scope.spawn(move || probe_shell(spec)));
        }
        let mut tool_jobs = Vec::with_capacity(tool_specs.len());
        for &spec in &tool_specs {
            tool_jobs.push(scope.spawn(move || probe_program(spec)));
        }
        let shells = shell_jobs
            .into_iter()
            .map(|job| job.join().unwrap_or_else(|_| unavailable_probe("unknown")))
            .collect::<Vec<_>>();
        let tools = tool_jobs
            .into_iter()
            .map(|job| job.join().unwrap_or_else(|_| unavailable_probe("unknown")))
            .collect::<Vec<_>>();
        (shells, tools)
    });

    let wsl = wsl_probe(&tools);
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

/// Resolve the first confirmed default shell without running it.
#[must_use]
pub fn preferred_shell() -> Option<String> {
    shell_specs()
        .into_iter()
        .map(probe_shell)
        .find(|probe| probe.execution_status == "confirmed")
        .and_then(|probe| probe.path)
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

fn probe_program(spec: ProbeSpec) -> ProgramProbe {
    let Some(path) = resolve_program(spec.argv0) else {
        return unavailable_probe(spec.name);
    };
    let run = run_bounded(&path, spec.version_args);
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

fn probe_shell(spec: ProbeSpec) -> ProgramProbe {
    let Some(path) = resolve_program(spec.argv0) else {
        return unavailable_probe(spec.name);
    };
    let path_text = path.to_string_lossy().into_owned();
    let command_argv = shell_launch_argv(&path_text, shell_probe_line(&path_text));
    let interpreter_args = command_argv
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (version_run, execution_run) = std::thread::scope(|scope| {
        let version_job = scope.spawn(|| run_bounded(&path, spec.version_args));
        let execution_job = scope.spawn(|| run_bounded(&path, &interpreter_args));
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

fn run_bounded(program: &Path, args: &[&str]) -> ProbeRun {
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
    let deadline = Instant::now() + PROBE_TIMEOUT;
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

fn wsl_probe(tools: &[ProgramProbe]) -> WslProbe {
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
    let Some(path) = wsl.path.as_deref().map(Path::new) else {
        return WslProbe {
            available: false,
            status: "path_not_found".to_owned(),
            execution_status: "unavailable".to_owned(),
            ..Default::default()
        };
    };
    let (list_run, execution_run) = std::thread::scope(|scope| {
        let list_job = scope.spawn(|| run_bounded(path, &["--list", "--quiet"]));
        let execution_job = scope.spawn(|| {
            run_bounded(
                path,
                &["-e", "sh", "-lc", &format!("printf {WSL_SENTINEL}")],
            )
        });
        (
            list_job.join().unwrap_or(ProbeRun::Failed),
            execution_job.join().unwrap_or(ProbeRun::Failed),
        )
    });
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
    use crate::policy::{PolicyEngine, PolicyProfile};

    #[test]
    fn shell_disabled_environment_keeps_only_direct_argv_routes() {
        let mut host = discover_host_environment();
        apply_execution_policy(&mut host, &PolicyEngine::new(PolicyProfile::DeveloperLocal));

        assert!(
            host.access_routes
                .iter()
                .all(|route| matches!(route.kind.as_str(), "direct_argv" | "wsl_argv")),
            "shell-disabled discovery must not promise a shell route: {:?}",
            host.access_routes
        );
        assert_eq!(host.beachhead, host.access_routes.first().cloned());
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
