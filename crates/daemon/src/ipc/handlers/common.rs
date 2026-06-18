// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
                "shell interpreter '{shell}' denied; the argv lane is not a shell bridge. \
                 Remedy: invoke the program directly as argv (e.g. [\"cargo\",\"build\"] \
                 instead of [\"{shell}\",\"-c\",\"cargo build\"]); for pipelines/redirects \
                 use the shell_exec tool, which is gated by the allow_shell policy cap."
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
        // An inline rule that fails to compile is a CALLER-fixable error:
        // the operator passed a bad regex / kind-keywords mismatch / empty
        // id. Surface `RuleInvalid` (the same teaching code `registry_*`
        // uses for rule validation) so the client can fix the rule, rather
        // than the server-fault `Internal`. The bucket is allocated AFTER
        // this compile in `start_combed`, so this path leaks nothing.
        CommandError::Sifter(msg) => IpcError::new(
            IpcErrorCode::RuleInvalid,
            format!("inline rule compile failed: {msg}"),
        ),
        // Genuine server faults (bucket store failure, IO) stay Internal.
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

/// Resolve a client-supplied WRITE target to a canonical, policy-authorized
/// path (TC22 A3). Unlike [`resolve_and_authorize_file`], the target file
/// MAY NOT EXIST yet, so `std::fs::canonicalize` on the target itself would
/// wrongly return `FileNotFound`. Instead we canonicalize the PARENT
/// directory (resolving every symlink in it) and append the target's
/// file name, forming a canonical target whose parent is the real on-disk
/// directory. We then policy-gate THAT canonical target.
///
/// `create_dirs` lets the caller create missing parent directories, but only
/// WITHIN an allowed path: the parent the write lands in must still pass the
/// `FileWrite` policy gate. We therefore (1) compute the canonical parent
/// (creating it under policy when `create_dirs`), (2) build the canonical
/// target, and (3) gate the canonical target. `create_dirs` never widens the
/// allow-list -- it only saves a separate mkdir for a path policy permits.
///
/// SECURITY mirror of the read path:
///  - ABSOLUTE-ONLY: a relative path is rejected (the daemon has no workspace
///    root) before any filesystem touch.
///  - NO `..`: a target containing any `..` (parent-dir) component is rejected
///    UP FRONT, before the policy gate / `create_dir_all` / `canonicalize`. A
///    write target never needs `..`, and rejecting it early prevents
///    `create_dir_all` from building directories outside the allow-list before
///    the canonical gate would deny (the create-then-deny asymmetry).
///  - SYMLINK-SAFE: canonicalizing the parent resolves a symlinked directory
///    to its real target, so a write through `/tmp/link -> ~/.ssh` is gated on
///    `~/.ssh/...`, not the innocuous link name. The default-deny suffix check
///    + `write_allow` then run on the real canonical target.
///  - NO TOCTOU widening: the returned path is the SAME canonical path the
///    caller then writes, so there is no re-resolution between gate and write.
pub(in crate::ipc::server) fn resolve_and_authorize_file_write(
    state: &Arc<DaemonState>,
    path: &std::path::Path,
    create_dirs: bool,
) -> Result<std::path::PathBuf, IpcError> {
    // (1) Absolute-only: the daemon has no workspace root.
    if !path.is_absolute() {
        return Err(IpcError::new(
            IpcErrorCode::PathDenied,
            format!(
                "path '{}' must be absolute (e.g. /home/u/project/out.txt); \
                 the daemon has no workspace root and would otherwise resolve \
                 it against its own working directory",
                path.display()
            ),
        ));
    }

    // (1b) SECURITY: reject any `..` (parent-dir) component UP FRONT, before the
    // step-3 policy gate, `create_dir_all`, or `canonicalize`. A write target
    // never legitimately needs `..`. Without this guard the step-3 gate sees the
    // RAW parent (still carrying literal `..`); the policy engine collapses `..`
    // lexically before matching and may DENY, but `create_dir_all` honors the raw
    // `..` and builds directories OUTSIDE the allow-list before the final
    // canonical gate runs -- a create-then-deny asymmetry that leaves an
    // out-of-allow-list directory artifact on disk. Rejecting `..` here removes
    // that asymmetry on EVERY platform. The canonical-form final gate (step 5)
    // stays intact as defense in depth.
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(IpcError::new(
            IpcErrorCode::PathDenied,
            format!(
                "path '{}' contains '..' (parent-dir traversal not permitted for writes)",
                path.display()
            ),
        ));
    }

    // (2) Split off the file name; the parent is what we canonicalize.
    let file_name = path.file_name().ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::PathDenied,
            format!(
                "path '{}' has no file name component; file_write needs a target file path",
                path.display()
            ),
        )
    })?;
    let parent = path.parent().ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::PathDenied,
            format!("path '{}' has no parent directory", path.display()),
        )
    })?;

    // (3) Optionally create the parent BEFORE canonicalize so a fresh tree is
    // writable -- but gate it FIRST so create_dirs never builds a directory
    // outside an allowed path. We gate the requested parent (canonicalized
    // tolerant of non-existence via the policy engine's own
    // `canonicalize_lexical`) then create it, then canonicalize the real dir.
    if create_dirs && !parent.exists() {
        // Gate the parent-as-target so a mkdir cannot escape the allow-list.
        // `..` has already been rejected in step 1b, so `parent` here is free of
        // parent-dir components and `create_dir_all` cannot climb outside the
        // gated tree. The engine still canonicalizes lexically before matching
        // (defense in depth), so a future change cannot silently reintroduce a
        // traversal escape past this gate.
        let verdict = state
            .policy
            .evaluate(&crate::policy::PolicyAction::FileWrite { path: parent });
        if verdict.decision == crate::policy::PolicyDecision::Deny {
            return Err(IpcError::new(IpcErrorCode::PathDenied, verdict.reason));
        }
        std::fs::create_dir_all(parent).map_err(|e| {
            IpcError::new(
                IpcErrorCode::Internal,
                format!("create_dirs '{}': {e}", parent.display()),
            )
        })?;
    }

    // (4) Canonicalize the parent (now guaranteed to exist if create_dirs was
    // set). A missing parent without create_dirs is an honest FileNotFound.
    let canonical_parent = std::fs::canonicalize(parent).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IpcError::new(
            IpcErrorCode::FileNotFound,
            format!(
                "parent directory '{}' does not exist (pass create_dirs to create it within an allowed path)",
                parent.display()
            ),
        ),
        _ => IpcError::new(
            IpcErrorCode::Internal,
            format!("resolve parent '{}': {e}", parent.display()),
        ),
    })?;
    if !canonical_parent.is_dir() {
        return Err(IpcError::new(
            IpcErrorCode::PathDenied,
            format!("parent '{}' is not a directory", canonical_parent.display()),
        ));
    }

    // (5) Build the canonical target and gate IT. A symlinked parent is now
    // resolved, so the gate sees the real target tree.
    let canonical_target = canonical_parent.join(file_name);
    let verdict = state
        .policy
        .evaluate(&crate::policy::PolicyAction::FileWrite {
            path: &canonical_target,
        });
    if verdict.decision == crate::policy::PolicyDecision::Deny {
        return Err(IpcError::new(IpcErrorCode::PathDenied, verdict.reason));
    }

    Ok(canonical_target)
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

    /// FIX 1 (cross-platform): an inline-rule compile failure is a
    /// CALLER-fixable error and must map to `RuleInvalid`, not the
    /// server-fault `Internal`. The caller can fix their rule.
    #[test]
    fn sifter_rule_compile_failure_maps_to_rule_invalid() {
        let err = map_command_error(CommandError::Sifter("bad regex".to_owned()));
        assert_eq!(err.code, IpcErrorCode::RuleInvalid);
        assert!(
            err.message.contains("bad regex"),
            "expected the underlying reason to be surfaced, got: {}",
            err.message
        );
    }

    /// FIX 1 (cross-platform): genuine server faults (IO) still map to
    /// `Internal`. The RuleInvalid carve-out must not swallow real
    /// server-side failures.
    #[test]
    fn io_error_still_maps_to_internal() {
        let err = map_command_error(CommandError::Io(std::io::Error::other("disk gone")));
        assert_eq!(err.code, IpcErrorCode::Internal);
    }

    /// FIX 1 (Medium finding, cross-platform): a `..` write target is rejected
    /// up front with a teaching `PathDenied`, BEFORE any `create_dir_all` or
    /// canonicalize. We prove the placement directly: with `create_dirs: true`
    /// the would-be-escaped sibling directory does NOT exist after the call, so
    /// no out-of-allow-list directory artifact was created (the create-then-deny
    /// asymmetry is closed). Covers both `create_dirs` values.
    #[test]
    fn dotdot_write_target_rejected_before_any_filesystem_touch() {
        let data = unique_data_dir("dotdot");
        std::fs::create_dir_all(&data).expect("data dir");
        let state = state_for(&data);

        // Absolute target whose `..` would climb out of `data/inner` into a
        // SIBLING `data/escaped` directory.
        let escaped_dir = data.join("escaped");
        let dotdot_target = data
            .join("inner")
            .join("..")
            .join("escaped")
            .join("out.txt");
        assert!(dotdot_target.is_absolute(), "target must be absolute");
        assert!(
            !escaped_dir.exists(),
            "precondition: escaped sibling dir must not pre-exist"
        );

        for create_dirs in [true, false] {
            let err = resolve_and_authorize_file_write(&state, &dotdot_target, create_dirs)
                .expect_err("`..` write target must be rejected");
            assert_eq!(
                err.code,
                IpcErrorCode::PathDenied,
                "create_dirs={create_dirs}"
            );
            assert!(
                err.message.contains(".."),
                "teaching `..` reason expected (create_dirs={create_dirs}): {}",
                err.message
            );
            // The reject precedes create_dir_all: no escaped artifact exists.
            assert!(
                !escaped_dir.exists(),
                "no out-of-allow-list directory artifact (create_dirs={create_dirs})"
            );
        }

        let _ = std::fs::remove_dir_all(&data);
    }
}
