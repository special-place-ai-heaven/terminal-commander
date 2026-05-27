// SPDX-License-Identifier: Apache-2.0
// Shared daemon-supervisor library used by the MCP adapter and the
// operator CLI. Owns: daemon endpoint resolution, daemon health
// probing, structured Unavailable diagnostics, peer-identity model.

pub mod ensure;
pub mod identity;
pub mod paths;
pub mod pidfile;
pub mod replace;
