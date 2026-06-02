// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Environment identifiers for parent → runner routing.

use serde::{Deserialize, Serialize};

/// Which execution environment a probe targets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvironmentSpec {
    /// Probes run in the parent daemon process (default).
    #[default]
    Local,
    /// Linux runtime inside a WSL2 distro (runner daemon in distro).
    WslDistro { distro: String },
    /// Reserved for remote SSH runner (not implemented in M1).
    SshHost { host: String },
}

impl EnvironmentSpec {
    /// Stable string for logs and audit.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Local => "local".to_owned(),
            Self::WslDistro { distro } => format!("wsl:{distro}"),
            Self::SshHost { host } => format!("ssh:{host}"),
        }
    }

    /// Parse harness env `TC_WSL_DISTRO` into a WSL spec when set.
    #[must_use]
    pub fn from_optional_wsl_distro(distro: Option<&str>) -> Self {
        match distro {
            Some(d) if !d.is_empty() => Self::WslDistro {
                distro: d.to_owned(),
            },
            _ => Self::Local,
        }
    }
}
