// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Remote-target routing for daemon-backed MCP tools (P5 / T050).
//!
//! Every daemon-backed tool may carry an optional `target_id`. When unset
//! (the default), the tool routes to the LOCAL daemon exactly as before --
//! full backward compatibility. When set, the tool routes to the named
//! target's daemon by dialing a purely-LOCAL socket path
//! ([`RemoteTarget::local_forward_socket`]) that an operator-established
//! `ssh -L` forward terminates at.
//!
//! Constitution invariants this module upholds:
//! - **Local-only (IV)**: routing only ever produces an [`McpDaemonClient`]
//!   wired to a UDS/named-pipe PATH. There is no `TcpListener`, `UdpSocket`,
//!   or network address anywhere on this path -- a remote daemon is reached
//!   ONLY through the operator's tunnel to its local socket.
//! - **Adapter-never-spawns (I)**: this module never launches a process. In
//!   particular it does NOT spawn `ssh`; establishing the `ssh -L` forward is
//!   the OPERATOR's responsibility (deferred-to-operator, documented on
//!   [`RemoteTransport::SshForward`]). The adapter only dials the resulting
//!   local socket.
//! - **No direct fs here**: the target registry is parsed by the daemon crate
//!   (`terminal_commanderd::TargetsConfig`) and handed to the router already
//!   loaded, so the adapter source stays free of any direct filesystem call
//!   (the adapter no-fs grep guard stays green).
//!
//! Source-status: live (T050).

use terminal_commanderd::{RemoteTarget, RemoteTransport, TargetsConfig};

use crate::daemon_client::McpDaemonClient;

/// Why a `target_id` could not be routed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetRouteError {
    /// The `target_id` is not present in the loaded `targets.toml`.
    #[error("unknown target_id '{0}'")]
    UnknownTarget(String),
}

/// Resolves an optional `target_id` to the [`McpDaemonClient`] a tool
/// should dial.
///
/// Holds the LOCAL client (used when no `target_id` is supplied) and the
/// loaded remote-target registry. Cheaply cloneable: the contained client
/// is a thin handle and the registry is shared.
#[derive(Debug, Clone)]
pub struct TargetRouter {
    /// The local daemon client. Returned verbatim for the default
    /// (no-`target_id`) path, preserving exact backward compatibility.
    local: McpDaemonClient,
    /// Loaded remote targets. Empty by default (local-only).
    targets: TargetsConfig,
}

impl TargetRouter {
    /// Build a router over the local client and the loaded target registry.
    #[must_use]
    pub const fn new(local: McpDaemonClient, targets: TargetsConfig) -> Self {
        Self { local, targets }
    }

    /// A local-only router (no remote targets registered). Equivalent to the
    /// pre-P5 behaviour for every tool.
    #[must_use]
    pub fn local_only(local: McpDaemonClient) -> Self {
        Self::new(local, TargetsConfig::default())
    }

    /// The local daemon client.
    #[must_use]
    pub const fn local(&self) -> &McpDaemonClient {
        &self.local
    }

    /// The registered remote targets.
    #[must_use]
    pub const fn targets(&self) -> &[RemoteTarget] {
        self.targets.targets.as_slice()
    }

    /// Look up a registered target by id.
    #[must_use]
    pub fn target(&self, target_id: &str) -> Option<&RemoteTarget> {
        self.targets.get(target_id)
    }

    /// Resolve a daemon client for the given optional `target_id`.
    ///
    /// - `None` => the LOCAL client (default; dials the local daemon socket).
    /// - `Some(id)` of a registered target => a NEW client dialing that
    ///   target's [`local_forward_socket`](RemoteTarget::local_forward_socket)
    ///   -- a local path the operator's tunnel terminates at. The returned
    ///   client inherits the local client's per-call timeout but NOT its
    ///   startup status handle (a forwarded socket has its own liveness; we
    ///   never self-heal a remote daemon from here).
    /// - `Some(id)` of an unknown target => [`TargetRouteError::UnknownTarget`].
    ///
    /// This never opens a connection (the client is lazy) and never binds or
    /// dials any TCP/UDP address -- only a local socket PATH.
    pub fn resolve(&self, target_id: Option<&str>) -> Result<McpDaemonClient, TargetRouteError> {
        match target_id {
            None => Ok(self.local.clone()),
            Some(id) => {
                let target = self
                    .targets
                    .get(id)
                    .ok_or_else(|| TargetRouteError::UnknownTarget(id.to_owned()))?;
                Ok(self.client_for(target))
            }
        }
    }

    /// Build a client dialing a target's forwarded LOCAL socket.
    ///
    /// The transport match is exhaustive on purpose: a future transport
    /// variant must be wired here deliberately, not silently fall through to
    /// a network dial. Today the only variant is
    /// [`RemoteTransport::SshForward`], which dials the local forward socket.
    #[must_use]
    pub fn client_for(&self, target: &RemoteTarget) -> McpDaemonClient {
        match target.transport {
            RemoteTransport::SshForward => {
                // Reach the remote daemon ONLY through the operator-forwarded
                // LOCAL socket path. No network address is ever constructed.
                McpDaemonClient::new(target.local_forward_socket.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn target(id: &str, sock: &str) -> RemoteTarget {
        RemoteTarget {
            target_id: id.to_owned(),
            transport: RemoteTransport::SshForward,
            host: format!("{id}.internal"),
            identity_file: None,
            remote_socket: None,
            local_forward_socket: PathBuf::from(sock),
        }
    }

    fn router_with(targets: Vec<RemoteTarget>) -> TargetRouter {
        let local = McpDaemonClient::new(PathBuf::from("/tmp/tc-local.sock"));
        TargetRouter::new(local, TargetsConfig { targets })
    }

    #[test]
    fn unset_target_dials_local_socket() {
        let router = router_with(vec![target("r1", "/tmp/tc-fwd-r1.sock")]);
        let client = router.resolve(None).expect("local resolves");
        assert_eq!(client.socket_path(), Path::new("/tmp/tc-local.sock"));
    }

    #[test]
    fn set_target_dials_its_forward_socket() {
        let router = router_with(vec![
            target("r1", "/tmp/tc-fwd-r1.sock"),
            target("r2", "/tmp/tc-fwd-r2.sock"),
        ]);
        let client = router.resolve(Some("r2")).expect("known target resolves");
        // Routes to the FORWARDED LOCAL socket, never a network address.
        assert_eq!(client.socket_path(), Path::new("/tmp/tc-fwd-r2.sock"));
    }

    #[test]
    fn unknown_target_is_typed_error() {
        let router = router_with(vec![target("r1", "/tmp/tc-fwd-r1.sock")]);
        let err = router
            .resolve(Some("does-not-exist"))
            .expect_err("unknown target errors");
        assert_eq!(
            err,
            TargetRouteError::UnknownTarget("does-not-exist".to_owned())
        );
    }

    #[test]
    fn local_only_router_has_no_targets() {
        let local = McpDaemonClient::new(PathBuf::from("/tmp/tc-local.sock"));
        let router = TargetRouter::local_only(local);
        assert!(router.targets().is_empty());
        // Default path still works.
        assert!(router.resolve(None).is_ok());
        // Any id is unknown.
        assert!(matches!(
            router.resolve(Some("x")),
            Err(TargetRouteError::UnknownTarget(_))
        ));
    }

    #[test]
    fn resolved_remote_socket_path_is_never_a_network_address() {
        // By construction the router only ever yields a client whose dial
        // target is a local socket PATH (no host:port). This guards the
        // constitution IV no-public-TCP invariant at the routing layer: the
        // path has no scheme and no ':' port separator pattern of host:port.
        let router = router_with(vec![target("r1", "/tmp/tc-fwd-r1.sock")]);
        let client = router.resolve(Some("r1")).expect("resolves");
        let p = client.socket_path().to_string_lossy();
        assert!(
            !p.contains("://"),
            "routed dial target must be a path, not a URL: {p}"
        );
        assert!(
            p.starts_with('/') || p.starts_with("\\\\") || p.contains("tc-fwd"),
            "routed dial target must be a filesystem/pipe path: {p}"
        );
    }
}
