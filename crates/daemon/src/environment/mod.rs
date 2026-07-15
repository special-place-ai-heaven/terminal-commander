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

/// Remove execution routes that the active shell capability cannot honor,
/// then recompute the public ranking and beachhead from the surviving routes.
pub(crate) fn apply_shell_capability(
    environment: &mut crate::ipc::protocol::HostEnvironment,
    allow_shell: bool,
) {
    if !allow_shell {
        environment
            .access_routes
            .retain(|route| matches!(route.kind.as_str(), "direct_argv" | "wsl_argv"));
    }
    for (index, route) in environment.access_routes.iter_mut().enumerate() {
        route.rank = u16::try_from(index + 1).unwrap_or(u16::MAX);
    }
    environment.beachhead = environment.access_routes.first().cloned();
    environment.preferred_shell = if allow_shell {
        environment
            .access_routes
            .iter()
            .find(|route| route.kind == "shell")
            .map(|route| route.executable.clone())
    } else {
        None
    };
}
