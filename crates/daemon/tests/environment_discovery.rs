// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use std::time::{Duration, Instant};

use terminal_commanderd::{discover_host_environment, shell_launch_argv};

#[test]
fn host_discovery_is_bounded_and_evidence_backed() {
    let started = Instant::now();
    let host = discover_host_environment();

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(host.os, std::env::consts::OS);
    assert_eq!(host.arch, std::env::consts::ARCH);
    assert!(!host.terminal.kind.is_empty());
    assert!(!host.terminal.evidence.is_empty());
    assert!(
        host.terminal
            .name
            .as_ref()
            .is_none_or(|name| !name.is_empty())
    );
    assert!(
        host.terminal
            .version
            .as_ref()
            .is_none_or(|version| !version.is_empty())
    );
    assert!(!host.shells.is_empty());
    assert!(!host.tools.is_empty());
    for program in host.shells.iter().chain(&host.tools) {
        assert_eq!(program.available, program.path.is_some());
        if program.available {
            assert_eq!(program.evidence, "path_confirmed");
            assert!(matches!(
                program.version_status.as_str(),
                "confirmed" | "failed" | "timed_out" | "not_probed"
            ));
        } else {
            assert_eq!(program.evidence, "path_not_found");
            assert_eq!(program.version_status, "unavailable");
        }
    }

    for shell in &host.shells {
        assert!(matches!(
            shell.execution_status.as_str(),
            "confirmed" | "failed" | "timed_out" | "unavailable"
        ));
    }
    let confirmed_shells = host
        .shells
        .iter()
        .filter(|shell| shell.execution_status == "confirmed")
        .count();
    assert_eq!(
        host.access_routes
            .iter()
            .filter(|route| route.kind == "shell")
            .count(),
        confirmed_shells
    );
    for (index, route) in host.access_routes.iter().enumerate() {
        assert_eq!(usize::from(route.rank), index + 1);
        assert!(matches!(
            route.kind.as_str(),
            "shell" | "wsl_shell" | "direct_argv" | "wsl_argv"
        ));
        assert!(!route.executable.is_empty());
        assert!(std::path::Path::new(&route.executable).is_absolute());
        assert_eq!(route.argv_template.first(), Some(&route.executable));
        let expected_tail = match route.kind.as_str() {
            "shell" | "wsl_shell" => "{command}",
            "direct_argv" | "wsl_argv" => "{args...}",
            other => panic!("unexpected route kind {other}"),
        };
        assert_eq!(
            route.argv_template.last().map(String::as_str),
            Some(expected_tail)
        );
        assert!(route.evidence.contains("confirmed"));
    }

    if host.wsl.execution_status == "confirmed" {
        assert!(
            host.access_routes
                .iter()
                .any(|route| route.kind == "wsl_shell")
        );
        assert!(
            host.access_routes
                .iter()
                .any(|route| route.kind == "wsl_argv")
        );
    }

    assert_eq!(host.beachhead, host.access_routes.first().cloned());
    assert_eq!(
        host.preferred_shell.as_deref(),
        host.access_routes
            .iter()
            .find(|route| route.kind == "shell")
            .map(|route| route.executable.as_str())
    );
}

#[test]
fn shell_launch_uses_the_confirmed_interpreter_family() {
    assert_eq!(
        shell_launch_argv("bash", "printf ok"),
        ["bash", "-lc", "printf ok"]
    );
    assert_eq!(
        shell_launch_argv("pwsh.exe", "Write-Output ok"),
        [
            "pwsh.exe",
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Write-Output ok",
        ]
    );
    assert_eq!(
        shell_launch_argv("cmd.exe", "echo ok"),
        ["cmd.exe", "/D", "/S", "/C", "echo ok"]
    );
}
