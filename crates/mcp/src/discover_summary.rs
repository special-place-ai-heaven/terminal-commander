// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! The default `system_discover` reply: what a model needs to choose how to
//! run a command. `detail: "full"` returns the whole payload instead.

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use terminal_commanderd::ipc::protocol::{AccessRoute, ProgramProbe, TerminalProbe, WslProbe};

use crate::tools::{OmniStatus, SystemDiscoverPayload};

/// How much `system_discover` returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverDetail {
    /// What is needed to choose how to run a command.
    #[default]
    Summary,
    /// Everything, including every tool's description and the daemon's
    /// method list.
    Full,
}

/// MCP-facing parameters for `system_discover`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct McpSystemDiscoverParams {
    /// `summary` (default) or `full`.
    #[serde(default)]
    pub detail: DiscoverDetail,
}

/// Tool version strings longer than this are cut, and end in `...`.
pub const SUMMARY_VERSION_CHARS: usize = 60;

const NOTE: &str = "Summary. Call system_discover with {\"detail\":\"full\"} for every \
tool's description, the daemon method list, the per-tool direct_argv routes, and full \
version strings.";

const ARGV_RULE: &str = "Each argv_tools entry has a direct_argv route: run it with an argv \
action as [<that tool's path from tools>, <args>...].";

#[derive(Debug, Serialize)]
pub struct DiscoverSummary<'a> {
    pub detail: &'static str,
    pub note: &'static str,
    pub adapter_version: &'static str,
    pub mcp_spec: &'static str,
    pub daemon_available: bool,
    pub daemon: Option<DaemonSummary<'a>>,
    pub daemon_error: Option<&'a str>,
    pub tool_catalogue: CatalogueSummary<'a>,
    pub omni_status: &'a OmniStatus,
}

#[derive(Debug, Serialize)]
pub struct DaemonSummary<'a> {
    pub version: &'a str,
    pub policy_profile: &'a str,
    /// The daemon's IPC method list is in `detail: "full"`.
    pub method_count: usize,
    pub environment: EnvironmentSummary<'a>,
}

#[derive(Debug, Serialize)]
pub struct EnvironmentSummary<'a> {
    pub os: &'a str,
    pub arch: &'a str,
    pub terminal: &'a TerminalProbe,
    pub shells: &'a [ProgramProbe],
    pub wsl: &'a WslProbe,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_shell: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beachhead: Option<&'a AccessRoute>,
    /// Every access route except the per-tool `direct_argv` ones.
    pub routes: Vec<&'a AccessRoute>,
    /// Tools that have a `direct_argv` route.
    pub argv_tools: Vec<&'a str>,
    pub argv_rule: &'static str,
    pub tools: Vec<ToolProbeSummary<'a>>,
    pub discovery_ms: u64,
    pub discovery_age_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct ToolProbeSummary<'a> {
    pub name: &'a str,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Cow<'a, str>>,
    pub version_status: &'a str,
    pub evidence: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_confirmed_age_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct CatalogueSummary<'a> {
    pub count: usize,
    /// Every tool that cannot be called now, with the reason.
    pub unavailable: Vec<UnavailableTool<'a>>,
}

#[derive(Debug, Serialize)]
pub struct UnavailableTool<'a> {
    pub name: &'static str,
    pub unavailable_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steer: Option<&'a crate::teach::ArgvSteer>,
}

/// The summary view of a full `system_discover` payload.
#[must_use]
pub fn summarize(payload: &SystemDiscoverPayload) -> DiscoverSummary<'_> {
    DiscoverSummary {
        detail: "summary",
        note: NOTE,
        adapter_version: payload.adapter_version,
        mcp_spec: payload.mcp_spec,
        daemon_available: payload.daemon_available,
        daemon: payload.daemon.as_ref().map(|daemon| {
            let environment = &daemon.environment;
            DaemonSummary {
                version: &daemon.version,
                policy_profile: &daemon.policy_profile,
                method_count: daemon.methods.len(),
                environment: EnvironmentSummary {
                    os: &environment.os,
                    arch: &environment.arch,
                    terminal: &environment.terminal,
                    shells: &environment.shells,
                    wsl: &environment.wsl,
                    preferred_shell: environment.preferred_shell.as_deref(),
                    beachhead: environment.beachhead.as_ref(),
                    routes: environment
                        .access_routes
                        .iter()
                        .filter(|route| route.kind != "direct_argv")
                        .collect(),
                    argv_tools: environment
                        .access_routes
                        .iter()
                        .filter(|route| route.kind == "direct_argv")
                        .map(|route| {
                            route
                                .route_id
                                .strip_prefix("argv:")
                                .unwrap_or(&route.route_id)
                        })
                        .collect(),
                    argv_rule: ARGV_RULE,
                    tools: environment.tools.iter().map(tool_summary).collect(),
                    discovery_ms: environment.discovery_ms,
                    discovery_age_ms: environment.discovery_age_ms,
                },
            }
        }),
        daemon_error: payload.daemon_error.as_deref(),
        tool_catalogue: CatalogueSummary {
            count: payload.tools.len(),
            unavailable: payload
                .tools
                .iter()
                .filter(|tool| !tool.available)
                .map(|tool| UnavailableTool {
                    name: tool.name,
                    unavailable_reason: tool.unavailable_reason,
                    steer: tool.steer.as_ref(),
                })
                .collect(),
        },
        omni_status: &payload.omni_status,
    }
}

fn tool_summary(probe: &ProgramProbe) -> ToolProbeSummary<'_> {
    ToolProbeSummary {
        name: &probe.name,
        available: probe.available,
        path: probe.path.as_deref(),
        version: probe.version.as_deref().map(short_version),
        version_status: &probe.version_status,
        evidence: &probe.evidence,
        stale_confirmed_age_ms: probe.stale_confirmed_age_ms,
    }
}

fn short_version(version: &str) -> Cow<'_, str> {
    if version.chars().count() <= SUMMARY_VERSION_CHARS {
        return Cow::Borrowed(version);
    }
    let kept: String = version.chars().take(SUMMARY_VERSION_CHARS - 3).collect();
    Cow::Owned(format!("{kept}..."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{
        OmniMatrix, PrivilegedHelperStatus, PtyStatus, RemoteTargetsStatus, SessionsStatus,
        ShellExecStatus, discovered_tools,
    };

    fn payload(daemon_available: bool) -> SystemDiscoverPayload {
        SystemDiscoverPayload {
            adapter_version: "0.0.0",
            mcp_spec: "2026-07-28",
            daemon_available,
            daemon: None,
            daemon_error: (!daemon_available).then(|| "daemon unavailable".to_owned()),
            tools: discovered_tools(daemon_available, None),
            omni_status: OmniStatus {
                program_version: "0.0.0",
                matrix: OmniMatrix {
                    shell_exec: ShellExecStatus {
                        available: daemon_available,
                        reason: None,
                        steer: None,
                    },
                    sessions: SessionsStatus { available: false },
                    pty: PtyStatus {
                        available: daemon_available,
                        platform: "posix",
                    },
                    privileged_helper: PrivilegedHelperStatus {
                        available: false,
                        reason: "threat_review_pending",
                    },
                    remote_targets: RemoteTargetsStatus {
                        count: 0,
                        reachable: 0,
                    },
                },
            },
        }
    }

    #[test]
    fn every_unavailable_tool_is_in_the_summary() {
        for daemon_available in [false, true] {
            let payload = payload(daemon_available);
            let summary = summarize(&payload);
            assert_eq!(summary.tool_catalogue.count, payload.tools.len());
            let listed: Vec<&str> = summary
                .tool_catalogue
                .unavailable
                .iter()
                .map(|tool| tool.name)
                .collect();
            let unavailable: Vec<&str> = payload
                .tools
                .iter()
                .filter(|tool| !tool.available)
                .map(|tool| tool.name)
                .collect();
            assert_eq!(listed, unavailable);
            if !daemon_available {
                assert!(listed.contains(&"health"), "{listed:?}");
            }
        }
    }

    #[test]
    fn long_versions_are_cut_visibly() {
        let long = "x".repeat(SUMMARY_VERSION_CHARS + 1);
        let cut = short_version(&long);
        assert_eq!(cut.chars().count(), SUMMARY_VERSION_CHARS);
        assert!(cut.ends_with("..."));
        let exact = "y".repeat(SUMMARY_VERSION_CHARS);
        assert_eq!(short_version(&exact), exact);
    }
}
