// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shared IPC handler helpers (audit, error mappers, path policy, scope validation).

use std::sync::Arc;

use terminal_commander_store::AuditEntry;
use terminal_commander_supervisor::identity::PeerIdentity;

use crate::audit::AuditSink;
use crate::command::CommandError;
use crate::ipc::protocol::{IpcError, IpcErrorCode, ShellDenyClass, ShellTeach};
use crate::policy::{PolicyEngine, PolicyProfile};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn emit_audit(
    state: &Arc<DaemonState>,
    action: &str,
    subject: &str,
    decision: &str,
    reason: Option<String>,
    peer: &PeerIdentity,
) {
    emit_audit_rich(state, action, subject, decision, reason, peer, "ipc", None);
}

/// Same row as [`emit_audit`], with an actor label and extra metadata keys.
/// Recipe activate/run rows pass `admin` or `mcp` plus `recipe_id`.
#[allow(clippy::too_many_arguments)] // peer metadata plus one recipe overlay
pub(in crate::ipc::server) fn emit_audit_rich(
    state: &Arc<DaemonState>,
    action: &str,
    subject: &str,
    decision: &str,
    reason: Option<String>,
    peer: &PeerIdentity,
    actor: &str,
    extra: Option<&serde_json::Value>,
) {
    let mut entry = AuditEntry::new(format!("ipc_{action}"), subject, decision).with_actor(actor);
    if let Some(r) = reason {
        entry = entry.with_reason(r);
    }
    let mut meta = peer_metadata(peer);
    if let (Some(obj), Some(serde_json::Value::Object(more))) = (meta.as_object_mut(), extra) {
        for (key, value) in more {
            obj.insert(key.clone(), value.clone());
        }
    }
    entry = entry.with_metadata_json(meta.to_string());
    // Best-effort; audit unhealth must not DOS the IPC path.
    let sink: Arc<dyn AuditSink> = Arc::clone(&state.audit) as Arc<dyn AuditSink>;
    let _ = sink.emit(&entry);
}

fn peer_metadata(peer: &PeerIdentity) -> serde_json::Value {
    match peer {
        PeerIdentity::Unix { uid, gid, pid } => serde_json::json!({
            "kind": "unix",
            "uid": uid,
            "gid": gid,
            "pid": pid,
        }),
        PeerIdentity::Windows { sid, pid, image } => serde_json::json!({
            "kind": "windows",
            "sid": sid,
            "pid": pid,
            "image": image.as_ref().map(|p| p.to_string_lossy().into_owned()),
        }),
        PeerIdentity::Unknown { reason } => serde_json::json!({
            "kind": "unknown",
            "reason": reason,
        }),
    }
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

/// Attach the A2 shell-misuse classification when this error is one of the
/// three teach classes. Other `PolicyDenied` values (paths, sudo scan,
/// non-shell commands) stay plain. An interpreter deny keeps the lane's own
/// text (interpreter, WSL carrier, `allow_shell`, the argv that works) as
/// message and reason; the other classes use the short
/// [`ShellDenyClass::reason`]. A profile that forbids shell says so rather
/// than naming a knob that cannot help there.
pub(in crate::ipc::server) fn enrich_shell_teach(
    policy: &PolicyEngine,
    denied_tool: &str,
    mut err: IpcError,
) -> IpcError {
    let class = match err.code {
        IpcErrorCode::ShellInterpreterDenied if !profile_can_run_shell(policy) => {
            Some(ShellDenyClass::ProfileForbidsShell)
        }
        IpcErrorCode::ShellInterpreterDenied => Some(ShellDenyClass::ShellInterpreterDenied),
        IpcErrorCode::PolicyDenied if err.message.contains("shell execution denied") => {
            Some(classify_shell_policy(policy))
        }
        _ => None,
    };
    let Some(class) = class else {
        return err;
    };
    let hint = crate::policy::profile_change_hint(policy.profile);
    let reason = match class {
        ShellDenyClass::ShellCapabilityOff if policy.shell_withheld_by_allow_roots() => format!(
            "Shell execution denied: {}. Retry with an argv array.",
            crate::policy::SHELL_WITHHELD_BY_ALLOW_ROOTS
        ),
        ShellDenyClass::ShellInterpreterDenied => format!("{} {hint}", err.message),
        _ => format!("{} {hint}", class.reason()),
    };
    err.message.clone_from(&reason);
    err.teach = Some(Box::new(ShellTeach {
        deny_class: class,
        profile: format!("{:?}", policy.profile),
        denied_capability: match class {
            ShellDenyClass::ShellCapabilityOff | ShellDenyClass::ShellInterpreterDenied => {
                Some("allow_shell".to_owned())
            }
            ShellDenyClass::ProfileForbidsShell => None,
        },
        denied_tool: denied_tool.to_owned(),
        reason,
        recipe_id: None,
        recipe_scope: None,
    }));
    err
}

/// When this error is a shell-misuse teach, attach `recipe_id` if exactly
/// one activated recipe matches the denied command. Store failure or no
/// unique match leaves the argv teach in place.
pub(in crate::ipc::server) fn attach_recipe_steer(
    state: &DaemonState,
    intent: terminal_commander_core::RecipeTeachIntent<'_>,
    mut err: IpcError,
) -> IpcError {
    if err.teach.is_none() {
        return err;
    }
    let Ok(active) = state.store.list_active_recipes() else {
        return err;
    };
    // Tombstoned parents are already absent. A job/bucket/probe row whose
    // job has exited is not a steer target.
    let rows: Vec<_> = active
        .into_iter()
        .filter(|row| recipe_scope_runnable(state, row.scope))
        .collect();
    let defs: Vec<_> = rows.iter().map(|row| row.definition.clone()).collect();
    let Some(id) = terminal_commander_core::match_activated_recipe(intent, &defs) else {
        return err;
    };
    // One runnable scope or argv teach. recipe_run has no implicit global,
    // and two scopes would make a single example the wrong call.
    let mut scopes = Vec::new();
    for row in &rows {
        if row.definition.recipe_id == id && !scopes.contains(&row.scope) {
            scopes.push(row.scope);
        }
    }
    let [scope] = scopes.as_slice() else {
        return err;
    };
    if let Some(teach) = err.teach.as_mut() {
        teach.recipe_id = Some(id.to_owned());
        teach.recipe_scope = Some(*scope);
    }
    err
}

/// Profiles where `[policy.caps] allow_shell = true` can enable shell.
const fn profile_can_run_shell(policy: &PolicyEngine) -> bool {
    matches!(
        policy.profile,
        PolicyProfile::DeveloperLocal | PolicyProfile::AdminDebug | PolicyProfile::FullAccess
    )
}

const fn classify_shell_policy(policy: &PolicyEngine) -> ShellDenyClass {
    if profile_can_run_shell(policy) && !policy.caps_allow_shell() {
        ShellDenyClass::ShellCapabilityOff
    } else {
        ShellDenyClass::ProfileForbidsShell
    }
}

pub(in crate::ipc::server) fn map_command_error(e: CommandError) -> IpcError {
    match e {
        CommandError::PolicyDenied(msg) => {
            // The one failsafe (os_guard) reuses the PolicyDenied channel but
            // gets its own typed code so a client sees it is not a knob-gated
            // deny. Detected by the stable reason tag, so no new error variant
            // has to be threaded through the whole command runtime.
            let code = if msg.contains(terminal_commander_core::FAILSAFE_REASON_TAG) {
                IpcErrorCode::OsCriticalPathProtected
            } else {
                IpcErrorCode::PolicyDenied
            };
            IpcError::new(code, msg)
        }
        CommandError::ShellInterpreterDenied(shell) => IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "shell interpreter '{shell}' denied: allow_shell is off. \
                 Run the program directly as argv (e.g. [\"cargo\",\"build\"] instead of \
                 {}).",
                terminal_commander_core::shell_deny::denied_argv_example(&shell)
            ),
        ),
        // US8 (FR-060): a shell smuggled through a wsl carrier. Reuses the
        // ShellInterpreterDenied wire code with a carrier-aware teaching
        // message (policy-wsl.md enforcement contract). The classifier passes
        // `interpreter = "unrecognized construction"` for a fail-closed form.
        CommandError::WslNestedShellDenied {
            interpreter,
            carrier,
        } if interpreter == "unrecognized construction" => IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "unrecognized '{carrier}' construction denied (fail closed): allow_shell is off. \
                 Use a recognized form ({carrier} -e <program> ..., or {carrier} --list / --status)."
            ),
        ),
        CommandError::WslNestedShellDenied {
            interpreter,
            carrier,
        } => IpcError::new(
            IpcErrorCode::ShellInterpreterDenied,
            format!(
                "shell interpreter '{interpreter}' denied inside a '{carrier}' invocation: \
                 allow_shell is off. Run the Linux program directly as argv ({carrier} -e <program> ...)."
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
        CommandError::PosixPathOnWindows { index, path } => IpcError::new(
            IpcErrorCode::PathDenied,
            format!(
                "argv[{index}] '{path}' looks like a Linux/WSL absolute path, but this \
                 daemon runs on Windows — prefix the command with wsl (e.g. \
                 [\"wsl\",\"python3\",\"/home/user/script.py\"]) or use a Windows path"
            ),
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
        // F7: a non-existent program is a CALLER-fixable command attempt
        // (typo / wrong PATH), not a daemon fault. Surface the structured
        // `ProgramNotFound` code (mapped to `invalid_params` at the MCP
        // boundary, carrying `error_kind: "program_not_found"` + `argv0`)
        // instead of the opaque `Internal` the generic `Spawn` arm yields.
        // `argv0` rides as a TYPED field on the IpcError (via
        // `IpcError::program_not_found`), which is what the MCP boundary reads
        // -- so the message wording is no longer load-bearing. The message
        // still names the program and the remedy for humans/logs; an
        // apostrophe in `argv0` no longer breaks recovery now that the value
        // is carried out-of-band rather than parsed from this prose.
        CommandError::ProgramNotFound { argv0 } => IpcError::program_not_found(
            argv0.clone(),
            format!(
                "program not found: '{argv0}'. Remedy: check the spelling of argv[0] and \
                 ensure the program is on the daemon's PATH (or pass an absolute path). \
                 On Windows a bare name is also searched with PATHEXT (.COM/.EXE/.BAT/.CMD), \
                 so .cmd/.bat shims like npm resolve by bare name."
            ),
        ),
        // Genuine server faults (bucket store failure, IO, other spawn
        // failures) stay Internal.
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

/// F2: true when `path` looks like a POSIX absolute path (`/home/...`), not a
/// Windows drive or UNC path. On non-Windows hosts this is always false.
#[cfg(windows)]
pub(in crate::ipc::server) fn looks_like_posix_absolute(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with('/') && !s.starts_with("//")
}

#[cfg(not(windows))]
pub(in crate::ipc::server) const fn looks_like_posix_absolute(_path: &std::path::Path) -> bool {
    false
}

fn posix_path_on_windows_error(path: &std::path::Path) -> IpcError {
    IpcError::new(
        IpcErrorCode::PathDenied,
        format!(
            "path '{}' looks like a Linux/WSL absolute path, but this daemon runs on \
             Windows — use a Windows path (e.g. C:\\Users\\...) or read the file via \
             `run_and_watch` with argv [\"wsl\",\"cat\",\"/home/...\"] instead of files.read",
            path.display()
        ),
    )
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
    // F2: POSIX-looking paths on a Windows host — before the relative gate
    // (on Windows `/home/...` is not `is_absolute()` and would mislead).
    if looks_like_posix_absolute(path) {
        return Err(posix_path_on_windows_error(path));
    }

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
    if looks_like_posix_absolute(path) {
        return Err(posix_path_on_windows_error(path));
    }

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

    // (5) Build the target. If it already exists, canonicalize the target
    // itself before gating and writing. This resolves file symlinks, Windows
    // 8.3 aliases, and case aliases to the real on-disk name. A missing target
    // is a legitimate create, so only NotFound falls back to the canonical
    // parent plus requested filename.
    let canonical_target = canonical_parent.join(file_name);
    let authorized_target = match std::fs::canonicalize(&canonical_target) {
        Ok(existing) => existing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => canonical_target,
        Err(e) => {
            return Err(IpcError::new(
                IpcErrorCode::Internal,
                format!("resolve write target '{}': {e}", canonical_target.display()),
            ));
        }
    };
    let verdict = state
        .policy
        .evaluate(&crate::policy::PolicyAction::FileWrite {
            path: &authorized_target,
        });
    if verdict.decision == crate::policy::PolicyDecision::Deny {
        return Err(IpcError::new(IpcErrorCode::PathDenied, verdict.reason));
    }

    Ok(authorized_target)
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
    state: &DaemonState,
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

/// Recipe list, run, and teach steer. Global is always runnable. A
/// job, bucket, or probe scope is runnable only while that id is live,
/// so a leftover row cannot stay runnable after the job exits.
pub(in crate::ipc::server) fn recipe_scope_runnable(
    state: &DaemonState,
    scope: terminal_commander_core::ActivationScope,
) -> bool {
    validate_scope_against_live_jobs(state, scope).is_ok()
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

    fn shell_exec_line(state: &DaemonState) -> crate::policy::PolicyVerdict {
        state
            .policy
            .evaluate(&crate::policy::PolicyAction::CommandShellStart {
                shell_line: "echo a | wc -c",
                cwd: std::path::Path::new("."),
                shell: "/bin/sh",
            })
    }

    fn allow_roots_cfg(data: &std::path::Path) -> crate::config::DaemonConfig {
        let mut cfg = crate::config::DaemonConfig::defaults_in(data);
        cfg.policy.commands = Some(crate::config::PolicyCommandsSection {
            allow_roots: vec!["git".to_owned()],
        });
        cfg
    }

    /// `allow_roots` set, `allow_shell` unset: the developer_local shell
    /// default is withheld and the deny says why and how to opt in.
    #[test]
    fn allow_roots_withholds_default_shell_with_teaching_text() {
        use crate::ipc::protocol::ShellDenyClass;
        let data = unique_data_dir("roots-shell-off");
        let state = DaemonState::bootstrap(allow_roots_cfg(&data)).expect("bootstrap");
        assert!(!state.policy.caps_allow_shell());
        let verdict = shell_exec_line(&state);
        assert_eq!(verdict.decision, crate::policy::PolicyDecision::Deny);
        let err = enrich_shell_teach(
            &state.policy,
            "shell_exec",
            IpcError::new(IpcErrorCode::PolicyDenied, verdict.reason),
        );
        assert!(
            err.message.contains(
                "allow_shell is off because [policy.commands] allow_roots confines commands; \
                 set [policy.caps] allow_shell = true explicitly to enable shell_exec"
            ),
            "deny must say allow_roots withheld the shell; got: {}",
            err.message
        );
        let teach = err.teach.expect("cap-off shell deny carries teach");
        assert_eq!(teach.deny_class, ShellDenyClass::ShellCapabilityOff);
        assert_eq!(teach.denied_capability.as_deref(), Some("allow_shell"));
        let _ = std::fs::remove_dir_all(&data);
    }

    /// `allow_roots` set, `allow_shell = true` explicit: the operator opt-in
    /// wins and shell_exec is allowed (it does not consult allow_roots).
    #[test]
    fn allow_roots_with_explicit_allow_shell_true_allows_shell() {
        let data = unique_data_dir("roots-shell-on");
        let mut cfg = allow_roots_cfg(&data);
        cfg.policy.caps = Some(crate::config::PolicyCapsSection {
            allow_shell: Some(true),
            ..Default::default()
        });
        let state = DaemonState::bootstrap(cfg).expect("bootstrap");
        assert!(state.policy.caps_allow_shell());
        assert_eq!(
            shell_exec_line(&state).decision,
            crate::policy::PolicyDecision::AllowWithAudit
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    fn state_for(data: &std::path::Path) -> Arc<DaemonState> {
        let cfg = crate::config::DaemonConfig::defaults_in(data);
        Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap"))
    }

    /// `developer_local`: the hardened profile that keeps the sensitive-path deny.
    fn hardened_state_for(data: &std::path::Path) -> Arc<DaemonState> {
        let mut cfg = crate::config::DaemonConfig::defaults_in(data);
        cfg.policy.profile = PolicyProfile::DeveloperLocal;
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

    /// Sensitive-path policy must be enforced on the platform-native canonical
    /// path, including Windows backslash separators, under a hardened profile.
    /// The default `full_access` profile reads the same file.
    #[test]
    fn native_sensitive_path_is_denied_before_file_access() {
        let data = unique_data_dir("sensitive");
        let ssh_dir = data.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).expect("create fake sensitive parent");
        let secret = ssh_dir.join("id_rsa");
        std::fs::write(&secret, b"FAKE TEST KEY\n").expect("create fake sensitive file");
        resolve_and_authorize_file(&state_for(&data), &secret, false)
            .expect("full_access default reads a sensitive path");
        let state = hardened_state_for(&data);

        let read_err = resolve_and_authorize_file(&state, &secret, false)
            .expect_err("native sensitive read path must be denied");
        assert_eq!(read_err.code, IpcErrorCode::PathDenied);

        let write_err = resolve_and_authorize_file_write(&state, &secret, false)
            .expect_err("native sensitive write path must be denied");
        assert_eq!(write_err.code, IpcErrorCode::PathDenied);

        let _ = std::fs::remove_dir_all(&data);
    }

    #[cfg(unix)]
    #[test]
    fn existing_write_symlink_is_authorized_by_its_real_target() {
        use std::os::unix::fs::symlink;

        let data = unique_data_dir("write-symlink");
        let state = hardened_state_for(&data);
        let ssh_dir = data.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).expect("create fake sensitive parent");
        let secret = ssh_dir.join("id_rsa");
        std::fs::write(&secret, b"FAKE TEST KEY\n").expect("create fake sensitive file");
        let alias = data.join("harmless-output");
        symlink(&secret, &alias).expect("create write symlink");

        let err = resolve_and_authorize_file_write(&state, &alias, false)
            .expect_err("write authorization must gate the existing symlink target");
        assert_eq!(err.code, IpcErrorCode::PathDenied);

        let _ = std::fs::remove_dir_all(&data);
    }

    /// F2: POSIX absolute paths on a Windows host daemon get a platform-mismatch
    /// error, not silent resolution to `C:\home\...`.
    #[cfg(windows)]
    #[test]
    fn posix_absolute_path_is_rejected_on_windows_daemon() {
        let data = unique_data_dir("posix-win");
        let state = state_for(&data);

        let err = resolve_and_authorize_file(
            &state,
            std::path::Path::new("/home/robert/somefile"),
            false,
        )
        .expect_err("posix path must be rejected on windows daemon");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("Linux/WSL"),
            "expected platform-mismatch guidance, got: {}",
            err.message
        );
        assert!(
            !err.message.contains("must be absolute"),
            "must not use the misleading relative-path message: {}",
            err.message
        );

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

    /// F7 (cross-platform): a non-existent program is a CALLER-fixable
    /// command attempt, so it maps to the structured `ProgramNotFound`
    /// code (which the MCP boundary surfaces as `invalid_params` with an
    /// `error_kind`/`argv0` data payload), NEVER the opaque server-fault
    /// `Internal`. The teaching message must name the offending argv0.
    #[test]
    fn program_not_found_maps_to_program_not_found_code_naming_argv0() {
        let err = map_command_error(CommandError::ProgramNotFound {
            argv0: "tc_nonexistent_program_f7_xyz".to_owned(),
        });
        assert_eq!(err.code, IpcErrorCode::ProgramNotFound);
        assert!(
            err.message.contains("tc_nonexistent_program_f7_xyz"),
            "the offending argv0 must be surfaced in the message, got: {}",
            err.message
        );
        // F7: the typed `argv0` field -- not the prose -- is the authoritative
        // carrier the MCP boundary reads. It must hold the exact argv0.
        assert_eq!(
            err.argv0.as_deref(),
            Some("tc_nonexistent_program_f7_xyz"),
            "the typed argv0 field must carry the offending program name"
        );
    }

    /// F7: an `argv0` containing an apostrophe must still ride verbatim on the
    /// TYPED field. The old prose quote-count parse could not recover this; the
    /// typed carrier makes the message wording irrelevant to recovery.
    #[test]
    fn program_not_found_typed_argv0_carries_apostrophe_program_name() {
        let err = map_command_error(CommandError::ProgramNotFound {
            argv0: "my'prog".to_owned(),
        });
        assert_eq!(err.code, IpcErrorCode::ProgramNotFound);
        assert_eq!(
            err.argv0.as_deref(),
            Some("my'prog"),
            "an apostrophe-bearing argv0 must survive verbatim on the typed field"
        );
    }

    /// F7 (cross-platform): a GENERIC spawn failure (not program-not-found)
    /// must stay `Internal`. The `ProgramNotFound` carve-out must not widen
    /// to swallow other spawn faults.
    /// The `env -S` deny names a real denied argv, not `["env -S","-c",...]`.
    #[test]
    fn env_split_string_deny_example_is_a_real_argv() {
        let err = map_command_error(CommandError::ShellInterpreterDenied(
            terminal_commander_core::shell_deny::ENV_SPLIT_STRING_DENY.to_owned(),
        ));
        assert!(
            !err.message.contains("[\"env -S\""),
            "example must be a real argv: {}",
            err.message
        );
        assert!(err.message.contains("[\"env\",\"-S\","), "{}", err.message);
        let err = map_command_error(CommandError::ShellInterpreterDenied("bash".to_owned()));
        assert!(err.message.contains("[\"bash\",\"-c\",\"cargo build\"]"));
    }

    #[test]
    fn generic_spawn_error_still_maps_to_internal() {
        let err = map_command_error(CommandError::Spawn(
            terminal_commander_probes::ProcessProbeError::Io(std::io::Error::other(
                "spawn permission denied",
            )),
        ));
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

    #[test]
    fn shell_teach_classifies_cap_off_profile_forbid_and_interpreter() {
        use crate::ipc::protocol::ShellDenyClass;
        use crate::policy::{PolicyCaps, PolicyEngine, PolicyProfile};

        // developer_local hardened with an explicit allow_shell = false.
        let local = PolicyEngine::with_config_caps(
            PolicyProfile::DeveloperLocal,
            None,
            None,
            PolicyCaps::default(),
        );
        let cap_off = enrich_shell_teach(
            &local,
            "shell_exec",
            IpcError::new(
                IpcErrorCode::PolicyDenied,
                "shell execution denied: allow_shell capability is off or profile forbids shell",
            ),
        );
        let teach = cap_off.teach.expect("cap-off shell deny carries teach");
        assert_eq!(teach.deny_class, ShellDenyClass::ShellCapabilityOff);
        assert_eq!(teach.profile, "DeveloperLocal");
        assert_eq!(teach.denied_capability.as_deref(), Some("allow_shell"));
        assert_eq!(teach.denied_tool, "shell_exec");
        assert!(teach.recipe_id.is_none());
        assert_eq!(
            teach.reason,
            format!(
                "{} {}",
                ShellDenyClass::ShellCapabilityOff.reason(),
                crate::policy::profile_change_hint(PolicyProfile::DeveloperLocal)
            )
        );
        assert!(!teach.reason.contains("set allow_shell"));
        assert!(!teach.reason.to_ascii_lowercase().contains("enable shell"));

        let repo = PolicyEngine::with_config_caps(
            PolicyProfile::RepoOnly,
            None,
            None,
            PolicyCaps {
                allow_shell: true,
                ..PolicyCaps::default()
            },
        );
        let forbidden = enrich_shell_teach(
            &repo,
            "shell_exec",
            IpcError::new(
                IpcErrorCode::PolicyDenied,
                "shell execution denied: allow_shell capability is off or profile forbids shell",
            ),
        );
        let teach = forbidden.teach.expect("profile forbid carries teach");
        assert_eq!(teach.deny_class, ShellDenyClass::ProfileForbidsShell);
        assert_eq!(teach.profile, "RepoOnly");
        assert!(teach.denied_capability.is_none());

        let interpreter = enrich_shell_teach(
            &local,
            "command_start_combed",
            map_command_error(CommandError::ShellInterpreterDenied("sh".to_owned())),
        );
        let teach = interpreter.teach.expect("interpreter deny carries teach");
        assert_eq!(teach.deny_class, ShellDenyClass::ShellInterpreterDenied);
        assert_eq!(teach.denied_capability.as_deref(), Some("allow_shell"));
        assert!(
            interpreter
                .message
                .contains(r#"[policy] profile = "full_access""#),
            "mcp-facing message must name the profile switch; got: {}",
            interpreter.message
        );

        let path = enrich_shell_teach(
            &local,
            "file_read_window",
            IpcError::new(IpcErrorCode::PolicyDenied, "path denied"),
        );
        assert!(path.teach.is_none(), "non-shell policy denies stay plain");
    }

    /// The argv lane's own deny text (interpreter, WSL carrier, knob, working
    /// argv) reaches the MCP caller; a profile that forbids shell says so
    /// instead of pointing at a knob that cannot help there.
    #[test]
    fn shell_teach_keeps_lane_text_and_names_profile_forbid() {
        use crate::ipc::protocol::ShellDenyClass;
        use crate::policy::{PolicyCaps, PolicyEngine, PolicyProfile};

        let local = PolicyEngine::with_config_caps(
            PolicyProfile::DeveloperLocal,
            None,
            None,
            PolicyCaps::default(),
        );
        let wsl = enrich_shell_teach(
            &local,
            "command_start_combed",
            map_command_error(CommandError::WslNestedShellDenied {
                interpreter: "bash".to_owned(),
                carrier: "wsl.exe".to_owned(),
            }),
        );
        let teach = wsl.teach.expect("interpreter deny carries teach");
        assert_eq!(teach.deny_class, ShellDenyClass::ShellInterpreterDenied);
        assert_eq!(teach.denied_capability.as_deref(), Some("allow_shell"));
        for text in [&wsl.message, &teach.reason] {
            for needle in [
                "'bash'",
                "'wsl.exe'",
                r#"[policy] profile = "full_access""#,
                "wsl.exe -e <program>",
            ] {
                assert!(text.contains(needle), "{needle} missing from: {text}");
            }
        }

        let direct = enrich_shell_teach(
            &local,
            "command_start_combed",
            map_command_error(CommandError::ShellInterpreterDenied("bash".to_owned())),
        );
        // Same text as `reason` in crates/mcp/tests/fixtures/a2/shell_interpreter_denied.json.
        assert_eq!(
            direct.message,
            "shell interpreter 'bash' denied: allow_shell is off. Run the program directly as argv (e.g. [\"cargo\",\"build\"] instead of [\"bash\",\"-c\",\"cargo build\"]). This daemon runs the `developer_local` profile (default is full_access, which allows everything); to change it set `[policy] profile = \"full_access\"` (and drop any `[policy.caps]` false override) in the daemon config (the `--config` file, else terminal-commander.toml in the data dir)."
        );
        assert_eq!(direct.teach.expect("teach").reason, direct.message);

        for profile in [PolicyProfile::RepoOnly, PolicyProfile::ReadOnlyObserver] {
            let engine = PolicyEngine::with_config_caps(profile, None, None, PolicyCaps::default());
            let err = enrich_shell_teach(
                &engine,
                "command_start_combed",
                map_command_error(CommandError::ShellInterpreterDenied("bash".to_owned())),
            );
            let teach = err.teach.expect("profile forbid carries teach");
            assert_eq!(teach.deny_class, ShellDenyClass::ProfileForbidsShell);
            assert!(teach.denied_capability.is_none(), "{profile:?}");
            assert_eq!(
                teach.reason,
                format!(
                    "{} {}",
                    ShellDenyClass::ProfileForbidsShell.reason(),
                    crate::policy::profile_change_hint(profile)
                )
            );
            assert!(err.message.contains("forbids shell"), "{}", err.message);
            assert!(!err.message.contains("allow_shell"), "{}", err.message);
        }
    }
}
