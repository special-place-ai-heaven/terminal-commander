// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Parent → environment runner orchestration.

mod router;
#[cfg(windows)]
pub mod wsl;

pub use router::{EnvironmentRouter, RouteError, RouteOutcome};
