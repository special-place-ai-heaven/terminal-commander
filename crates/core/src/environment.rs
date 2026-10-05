// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// The `environment` field of the start requests, and the environment the
// daemon's children get.

use serde::{Deserialize, Serialize};

/// Variables naming the daemon's OWN endpoint, removed from every command the
/// daemon runs (all lanes) and from its discovery probes.
///
/// `TC_SOCKET` is always set by the supervisor, `TC_DATA` when the daemon's
/// launcher had one. A terminal-commander process started beneath the daemon
/// would otherwise attach to this daemon's socket and data dir -- or, like the
/// installed Linux autostart run from a login shell, start a second daemon on
/// them. A value a caller passes explicitly is applied after the removal and
/// still wins. `TC_SESSION` is not here: it names a session, not this daemon's
/// files, and crosses the WSL bridge.
pub const DAEMON_ENDPOINT_ENV: [&str; 2] = ["TC_SOCKET", "TC_DATA"];

/// Set to `1` in every process the daemon starts, so a script (the Linux
/// autostart) can tell it runs inside a TC-managed process tree.
pub const DAEMON_CHILD_ENV: &str = "TC_DAEMON_CHILD";

/// Which execution environment a start request targets.
///
/// Only [`EnvironmentSpec::Local`] is supported. Environment runners were never
/// built; the other variants stay so that a request naming one still decodes
/// and the daemon can refuse it with a precise error instead of a decode
/// failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvironmentSpec {
    /// Probes run in the daemon process (default).
    #[default]
    Local,
    /// Unsupported: a Linux runtime inside a WSL2 distro.
    WslDistro { distro: String },
    /// Unsupported: a remote SSH host.
    SshHost { host: String },
}
