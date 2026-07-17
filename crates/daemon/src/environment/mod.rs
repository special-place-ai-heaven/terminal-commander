// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Parent → environment runner orchestration.

mod probe;
mod router;
#[cfg(windows)]
pub mod wsl;

pub use probe::{discover_host_environment, preferred_shell, shell_launch_argv};

pub use router::{EnvironmentRouter, RouteError, RouteOutcome};

/// Remove execution routes that the active policy cannot honor, then recompute
/// the public ranking and beachhead from the surviving routes.
pub(crate) fn apply_execution_policy(
    environment: &mut crate::ipc::protocol::HostEnvironment,
    policy: &crate::policy::PolicyEngine,
) {
    use crate::policy::{PolicyAction, PolicyDecision};

    let command_probe_allowed = policy
        .evaluate(&PolicyAction::ProbeCreate { kind: "command" })
        .decision
        != PolicyDecision::Deny;
    let Some(cwd) = policy.environment_discovery_cwd() else {
        environment.access_routes.clear();
        environment.beachhead = None;
        environment.preferred_shell = None;
        return;
    };
    environment.access_routes.retain(|route| {
        if !command_probe_allowed {
            return false;
        }

        if matches!(route.kind.as_str(), "shell" | "wsl_shell")
            && policy
                .evaluate(&PolicyAction::CommandShellStart {
                    shell_line: "{command}",
                    cwd,
                    shell: &route.executable,
                })
                .decision
                == PolicyDecision::Deny
        {
            return false;
        }

        route.kind == "shell"
            || policy
                .evaluate(&PolicyAction::CommandStart {
                    argv: &route.argv_template,
                    cwd,
                })
                .decision
                != PolicyDecision::Deny
    });
    for (index, route) in environment.access_routes.iter_mut().enumerate() {
        route.rank = u16::try_from(index + 1).unwrap_or(u16::MAX);
    }
    environment.beachhead = environment.access_routes.first().cloned();
    environment.preferred_shell = environment
        .access_routes
        .iter()
        .find(|route| route.kind == "shell")
        .map(|route| route.executable.clone());
}
