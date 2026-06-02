// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Shared IPC handler helpers (audit, error mappers, path policy, scope validation).

use std::sync::Arc;

use terminal_commander_store::AuditEntry;
use terminal_commander_supervisor::identity::PeerIdentity;

use crate::audit::AuditSink;
use crate::command::CommandError;
use crate::ipc::protocol::{IpcError, IpcErrorCode};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn emit_audit(
    state: &Arc<DaemonState>,
    action: &str,
    subject: &str,
    decision: &str,
    reason: Option<String>,
    peer: &PeerIdentity,
) {
    let mut entry = AuditEntry::new(format!("ipc_{action}"), subject, decision).with_actor("ipc");
    if let Some(r) = reason {
        entry = entry.with_reason(r);
    }
    // Attach peer metadata as pre-serialized JSON. Stays well inside
    // MAX_AUDIT_METADATA_BYTES.
    let meta = match peer {
        PeerIdentity::Unix { uid, gid, pid } => format!(
            r#"{{"kind":"unix","uid":{},"gid":{},"pid":{}}}"#,
            uid,
            gid,
            pid.map_or_else(|| "null".to_owned(), |x| x.to_string())
        ),
        PeerIdentity::Windows { sid, pid, image } => format!(
            r#"{{"kind":"windows","sid":{},"pid":{},"image":{}}}"#,
            serde_json::to_string(sid).unwrap_or_else(|_| "null".to_owned()),
            pid.map_or_else(|| "null".to_owned(), |x| x.to_string()),
            image
                .as_deref()
                .and_then(|p| p.to_str())
                .and_then(|s| serde_json::to_string(s).ok())
                .unwrap_or_else(|| "null".to_owned()),
        ),
        PeerIdentity::Unknown { reason: r } => format!(
            r#"{{"kind":"unknown","reason":{}}}"#,
            r.as_deref()
                .and_then(|s| serde_json::to_string(s).ok())
                .unwrap_or_else(|| "null".to_owned()),
        ),
    };
    entry = entry.with_metadata_json(meta);
    // Best-effort; audit unhealth must not DOS the IPC path.
    let sink: Arc<dyn AuditSink> = Arc::clone(&state.audit) as Arc<dyn AuditSink>;
    let _ = sink.emit(&entry);
}

pub(in crate::ipc::server) fn identity_audit_subject(identity: &PeerIdentity) -> String {
    match identity {
        PeerIdentity::Unix { uid, pid, .. } => {
            format!("uid={uid}:pid={}", pid.map_or(0, |p| p))
        }
        PeerIdentity::Windows { sid, pid, .. } => {
            format!("sid={sid}:pid={}", pid.map_or(0, |p| p))
        }
        PeerIdentity::Unknown { .. } => "unknown_peer".to_owned(),
    }
}

#[cfg(unix)]
pub(in crate::ipc::server) fn emit_audit_internal_error(
    state: &Arc<DaemonState>,
    action: &str,
    message: &str,
) {
    let entry = AuditEntry::new(format!("ipc_{action}"), "internal", "error")
        .with_actor("ipc")
        .with_reason(message);
    let sink: Arc<dyn AuditSink> = Arc::clone(&state.audit) as Arc<dyn AuditSink>;
    let _ = sink.emit(&entry);
}

pub(in crate::ipc::server) fn map_bucket_error(
    e: terminal_commander_core::BucketError,
) -> IpcError {
    use terminal_commander_core::BucketError;
    match e {
        BucketError::NotFound(_) => IpcError::new(IpcErrorCode::BucketNotFound, e.to_string()),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

pub(in crate::ipc::server) fn map_command_error(e: CommandError) -> IpcError {
    match e {
        CommandError::PolicyDenied(msg) => IpcError::new(IpcErrorCode::PolicyDenied, msg),
        CommandError::ShellInterpreterDenied(shell) => IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "shell interpreter '{shell}' denied; command_start_combed is not a shell bridge"
            ),
        ),
        CommandError::EmptyArgv => {
            IpcError::new(IpcErrorCode::ArgvInvalid, "argv must not be empty")
        }
        CommandError::ArgvTooLong(n) => {
            IpcError::new(IpcErrorCode::ArgvInvalid, format!("argv too long: {n}"))
        }
        CommandError::ArgvItemTooLong { index, len } => IpcError::new(
            IpcErrorCode::ArgvInvalid,
            format!("argv[{index}] is {len} bytes; exceeds per-item cap"),
        ),
        CommandError::UnknownJob(id) => {
            IpcError::new(IpcErrorCode::UnknownJob, format!("unknown job: {id}"))
        }
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

pub(in crate::ipc::server) fn map_store_error(
    e: terminal_commander_store::EventStoreError,
) -> IpcError {
    use terminal_commander_store::EventStoreError;
    match e {
        EventStoreError::InvalidPayload(msg) => IpcError::new(IpcErrorCode::RuleInvalid, msg),
        // A backend/actor fault (dead writer thread, dropped reply
        // channel, unexpected reply, or an isolated op panic) is NOT
        // caller-fixable: surface it as a server-fault Internal, never
        // RuleInvalid, so an agent whose rule is valid is not told to
        // "fix" it while the store is actually down.
        EventStoreError::Unavailable(msg) => IpcError::new(IpcErrorCode::Internal, msg),
        other => IpcError::new(IpcErrorCode::Internal, other.to_string()),
    }
}

pub(in crate::ipc::server) fn map_path_policy(
    state: &Arc<DaemonState>,
    path: &std::path::Path,
    is_watch: bool,
) -> Result<(), IpcError> {
    let action = if is_watch {
        crate::policy::PolicyAction::FileWatch { path }
    } else {
        crate::policy::PolicyAction::FileRead { path }
    };
    let verdict = state.policy.evaluate(&action);
    if verdict.decision == crate::policy::PolicyDecision::Deny {
        return Err(IpcError::new(IpcErrorCode::PathDenied, verdict.reason));
    }
    Ok(())
}

/// Resolve a client-supplied file path to a canonical, policy-authorized
/// path that callers then open directly.
///
/// This closes two path-handling holes (external review TC22 I5):
///
/// 1. ABSOLUTE-ONLY (trust/correctness): the daemon has no workspace
///    root, so a relative path would silently resolve against the
///    daemon's process CWD - not the client's "repo". Relative paths are
///    rejected up front with a teaching [`IpcErrorCode::PathDenied`].
///
/// 2. SYMLINK-SAFE DEFAULT-DENY (security): the default-deny suffix check
///    inside [`PolicyEngine::evaluate`] matches on the path STRING, but
///    `File::open` follows symlinks. A symlink whose own name does not
///    match a sensitive suffix (e.g. `/tmp/x -> ~/.ssh/id_rsa`) would
///    pass the string check and then read the secret target. We
///    canonicalize FIRST (resolving every symlink) and run the policy
///    check on the real target, then return that canonical path so the
///    caller opens the SAME path it authorized (closing the TOCTOU
///    window - no re-resolution between check and open).
///
/// `canonicalize` requires the target to exist; for `file_read_window` /
/// `file_search` / `file_watch_start` the target MUST exist, so a missing
/// path is an honest [`IpcErrorCode::FileNotFound`], not a bypass.
pub(in crate::ipc::server) fn resolve_and_authorize_file(
    state: &Arc<DaemonState>,
    path: &std::path::Path,
    is_watch: bool,
) -> Result<std::path::PathBuf, IpcError> {
    // (1) Absolute-only: the daemon has no workspace root.
    if !path.is_absolute() {
        return Err(IpcError::new(
            IpcErrorCode::PathDenied,
            format!(
                "path '{}' must be absolute (e.g. /home/u/project/Cargo.toml); \
                 the daemon has no workspace root and would otherwise resolve \
                 it against its own working directory",
                path.display()
            ),
        ));
    }

    // (2) Canonicalize BEFORE the policy check so symlinks resolve to
    // their real target and the default-deny suffix check sees it.
    let canonical = std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' does not exist", path.display()),
        ),
        _ => IpcError::new(
            IpcErrorCode::Internal,
            format!("resolve '{}': {e}", path.display()),
        ),
    })?;

    // (3) Policy-gate the CANONICAL path. A symlink to a denied target is
    // now caught because `canonical` is the real target.
    map_path_policy(state, &canonical, is_watch)?;

    Ok(canonical)
}

pub(in crate::ipc::server) fn require_regular_file(
    path: &std::path::Path,
) -> Result<std::fs::Metadata, IpcError> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(m),
        Ok(_) => Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' is not a regular file", path.display()),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' does not exist", path.display()),
        )),
        Err(e) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("stat '{}': {e}", path.display()),
        )),
    }
}

/// Validate that a caller-supplied [`ActivationScope`] resolves to a
/// known live entity (where applicable). `Global` is always valid.
/// A `Bucket` / `Job` / `Probe` scope referring to an id the daemon
/// does not currently have a live job for is rejected with
/// [`IpcErrorCode::ScopeInvalid`] instead of silently widening to
/// `Global`.
///
/// Note on liveness: we deliberately only check against the
/// command-runtime's live-job map. A scope referring to a future
/// bucket/job/probe id that has not been started yet is not
/// legitimately scopeable; the operator can create the command
/// first, then activate. A scope referring to a recently-exited job
/// is treated as invalid for the same reason.
pub(in crate::ipc::server) fn validate_scope_against_live_jobs(
    state: &Arc<DaemonState>,
    scope: terminal_commander_core::ActivationScope,
) -> Result<(), IpcError> {
    use terminal_commander_core::ActivationScope;
    match scope {
        ActivationScope::Global => Ok(()),
        ActivationScope::Bucket { bucket_id } => {
            let in_command = state
                .command
                .live_jobs()
                .iter()
                .any(|j| j.bucket_id == bucket_id);
            let in_watch = state
                .watch
                .live_watches()
                .iter()
                .any(|w| w.bucket_id == bucket_id);
            #[cfg(unix)]
            let in_pty = state
                .pty
                .live_jobs()
                .iter()
                .any(|j| j.bucket_id == bucket_id);
            #[cfg(not(unix))]
            let in_pty = false;
            if in_command || in_watch || in_pty {
                Ok(())
            } else {
                Err(IpcError::new(
                    IpcErrorCode::ScopeInvalid,
                    format!(
                        "scope bucket_id={} does not resolve to a live job, watch, or pty",
                        bucket_id.to_wire_string()
                    ),
                ))
            }
        }
        ActivationScope::Job { job_id } => {
            let in_command = state.command.live_jobs().iter().any(|j| j.job_id == job_id);
            let in_watch = state
                .watch
                .live_watches()
                .iter()
                .any(|w| w.watch_id == job_id);
            #[cfg(unix)]
            let in_pty = state.pty.live_jobs().iter().any(|j| j.job_id == job_id);
            #[cfg(not(unix))]
            let in_pty = false;
            if in_command || in_watch || in_pty {
                Ok(())
            } else {
                Err(IpcError::new(
                    IpcErrorCode::ScopeInvalid,
                    format!(
                        "scope job_id={} does not resolve to a live job, watch, or pty",
                        job_id.to_wire_string()
                    ),
                ))
            }
        }
        ActivationScope::Probe { probe_id } => {
            let in_command = state
                .command
                .live_jobs()
                .iter()
                .any(|j| j.probe_id == probe_id);
            let in_watch = state
                .watch
                .live_watches()
                .iter()
                .any(|w| w.probe_id == probe_id);
            #[cfg(unix)]
            let in_pty = state.pty.live_jobs().iter().any(|j| j.probe_id == probe_id);
            #[cfg(not(unix))]
            let in_pty = false;
            if in_command || in_watch || in_pty {
                Ok(())
            } else {
                Err(IpcError::new(
                    IpcErrorCode::ScopeInvalid,
                    format!(
                        "scope probe_id={} does not resolve to a live job, watch, or pty",
                        probe_id.to_wire_string()
                    ),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::IpcErrorCode;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_data_dir(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        p.push(format!("tc-resolve-{tag}-{pid}-{nanos}-{n}"));
        p
    }

    fn state_for(data: &std::path::Path) -> Arc<DaemonState> {
        let cfg = crate::config::DaemonConfig::defaults_in(data);
        Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"))
    }

    /// BUG 2 (cross-platform): a relative path is rejected with a teaching
    /// `PathDenied` instead of being silently resolved against the
    /// daemon's process CWD. The daemon has no workspace root.
    #[test]
    fn relative_path_is_rejected_with_teaching_error() {
        let data = unique_data_dir("rel");
        let state = state_for(&data);

        let err = resolve_and_authorize_file(&state, std::path::Path::new("Cargo.toml"), false)
            .expect_err("relative path must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("must be absolute"),
            "teaching message expected, got: {}",
            err.message
        );

        let _ = std::fs::remove_dir_all(&data);
    }

    /// BUG 2 (cross-platform): an absolute path to an existing regular
    /// file is authorized and resolves to a canonical path.
    #[test]
    fn absolute_existing_path_is_authorized() {
        let data = unique_data_dir("abs");
        let state = state_for(&data);
        let file = data.join("ok.txt");
        {
            let mut f = std::fs::File::create(&file).expect("create");
            f.write_all(b"hello\n").expect("write");
        }
        assert!(file.is_absolute(), "temp file path must be absolute");

        let resolved =
            resolve_and_authorize_file(&state, &file, false).expect("absolute path authorized");
        // Canonical form points at the same file (compare canonicalized
        // both sides to tolerate the Windows `\\?\` verbatim prefix).
        let expect = std::fs::canonicalize(&file).expect("canonicalize");
        assert_eq!(resolved, expect);

        let _ = std::fs::remove_dir_all(&data);
    }
}
