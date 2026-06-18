// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use terminal_commander_store::AuditEntry;

use super::common::{
    require_regular_file, resolve_and_authorize_file, resolve_and_authorize_file_write,
};
use crate::audit::AuditSink;
use crate::ipc::protocol::{
    DEFAULT_FILE_READ_BYTES, DEFAULT_FILE_READ_LINES, DEFAULT_FILE_SEARCH_MATCHES,
    DEFAULT_FILE_SEARCH_SNIPPET_BYTES, FileLine, FileReadWindowParams, FileReadWindowResponse,
    FileSearchMatch, FileSearchParams, FileSearchResponse, FileWatchListEntry,
    FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse, FileWatchStopParams,
    FileWatchStopResponse, FileWriteParams, FileWriteResponse, IpcError, IpcErrorCode, IpcResponse,
    MAX_COMMAND_INLINE_RULES, MAX_FILE_READ_BYTES, MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES,
    MAX_FILE_SEARCH_SCAN_BYTES, MAX_FILE_SEARCH_SNIPPET_BYTES, MAX_FILE_WRITE_BYTES,
};
use crate::state::DaemonState;

/// Emit a dedicated `file_write` domain audit row through the persistent
/// audit sink (constitution V). This is SEPARATE from the dispatch-level
/// `ipc_file_write` row: the domain row is emitted by the handler itself so
/// it can land BEFORE the write (the `allow` row precedes any filesystem
/// mutation) and so a denied write is recorded under the `file_write` action,
/// mirroring `WatchRuntime`'s `file_watch_start` audit pattern.
fn emit_file_write_audit(
    state: &Arc<DaemonState>,
    subject: &str,
    decision: &str,
    reason: Option<String>,
    bytes: Option<u64>,
) {
    let mut entry = AuditEntry::new("file_write", subject, decision)
        .with_actor("file_runtime")
        .with_profile(format!("{:?}", state.policy.profile));
    if let Some(r) = reason {
        entry = entry.with_reason(r);
    }
    if let Some(b) = bytes {
        entry = entry.with_metadata_json(format!(r#"{{"bytes_written":{b}}}"#));
    }
    let sink: Arc<dyn AuditSink> = Arc::clone(&state.audit) as Arc<dyn AuditSink>;
    let _ = sink.emit(&entry);
}

pub(in crate::ipc::server) fn handle_file_read_window(
    state: &Arc<DaemonState>,
    params: &FileReadWindowParams,
) -> Result<IpcResponse, IpcError> {
    use std::io::{BufRead, BufReader};

    // Resolve to a canonical, policy-authorized path (absolute-only +
    // symlink-safe default-deny), then open THAT exact path so a symlink
    // to a denied target cannot slip through between check and open.
    let resolved = resolve_and_authorize_file(state, &params.path, false)?;
    let meta = require_regular_file(&resolved)?;
    let file_bytes = meta.len();

    let start_line = params.start_line.unwrap_or(1).max(1);
    let max_lines = params
        .max_lines
        .unwrap_or(DEFAULT_FILE_READ_LINES)
        .min(MAX_FILE_READ_LINES);
    let max_bytes = params
        .max_bytes
        .unwrap_or(DEFAULT_FILE_READ_BYTES)
        .min(MAX_FILE_READ_BYTES);

    let f = std::fs::File::open(&resolved)
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("open: {e}")))?;
    let mut reader = BufReader::new(f);
    let mut byte_offset: u64 = 0;
    let mut line_no: u64 = 0;
    let mut out_lines: Vec<FileLine> = Vec::new();
    let mut total_bytes: usize = 0;
    let mut truncated = false;
    let mut buf = String::new();
    let next_byte_offset: u64;

    loop {
        buf.clear();
        let read = reader.read_line(&mut buf).map_err(|e| {
            if matches!(e.kind(), std::io::ErrorKind::InvalidData) {
                IpcError::new(
                    IpcErrorCode::FileBinary,
                    format!("'{}' contains non-UTF-8 bytes", params.path.display()),
                )
            } else {
                IpcError::new(IpcErrorCode::Internal, format!("read_line: {e}"))
            }
        })?;
        if read == 0 {
            next_byte_offset = byte_offset;
            break;
        }
        line_no = line_no.saturating_add(1);
        let line_start = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);
        if line_no < start_line {
            continue;
        }
        let trimmed = buf.trim_end_matches('\n').trim_end_matches('\r').to_owned();
        let line_size = trimmed.len();
        if total_bytes.saturating_add(line_size) > max_bytes {
            truncated = true;
            next_byte_offset = line_start;
            break;
        }
        total_bytes = total_bytes.saturating_add(line_size);
        out_lines.push(FileLine {
            line: line_no,
            byte_offset: line_start,
            text: trimmed,
        });
        if u32::try_from(out_lines.len()).unwrap_or(u32::MAX) >= max_lines {
            truncated = true;
            next_byte_offset = byte_offset;
            break;
        }
    }

    Ok(IpcResponse::FileReadWindow(FileReadWindowResponse {
        path: params.path.clone(),
        lines: out_lines,
        file_bytes,
        truncated,
        next_byte_offset,
    }))
}

pub(in crate::ipc::server) fn handle_file_search(
    state: &Arc<DaemonState>,
    params: &FileSearchParams,
) -> Result<IpcResponse, IpcError> {
    use std::io::{BufRead, BufReader};

    if params.query.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            "query must be non-empty",
        ));
    }
    // Resolve to a canonical, policy-authorized path (absolute-only +
    // symlink-safe default-deny), then open THAT exact path.
    let resolved = resolve_and_authorize_file(state, &params.path, false)?;
    require_regular_file(&resolved)?;

    let max_matches = params
        .max_matches
        .unwrap_or(DEFAULT_FILE_SEARCH_MATCHES)
        .min(MAX_FILE_SEARCH_MATCHES);
    let max_snippet = params
        .max_snippet_bytes
        .unwrap_or(DEFAULT_FILE_SEARCH_SNIPPET_BYTES)
        .min(MAX_FILE_SEARCH_SNIPPET_BYTES);
    let case_insensitive = params.case_insensitive.unwrap_or(false);
    let needle_lower = params.query.to_ascii_lowercase();

    let f = std::fs::File::open(&resolved)
        .map_err(|e| IpcError::new(IpcErrorCode::Internal, format!("open: {e}")))?;
    let mut reader = BufReader::new(f);
    let mut matches: Vec<FileSearchMatch> = Vec::new();
    let mut bytes_scanned: u64 = 0;
    let mut byte_offset: u64 = 0;
    let mut line_no: u64 = 0;
    let mut truncated = false;
    let mut buf = String::new();

    loop {
        buf.clear();
        let read = reader.read_line(&mut buf).map_err(|e| {
            if matches!(e.kind(), std::io::ErrorKind::InvalidData) {
                IpcError::new(
                    IpcErrorCode::FileBinary,
                    format!("'{}' contains non-UTF-8 bytes", params.path.display()),
                )
            } else {
                IpcError::new(IpcErrorCode::Internal, format!("read_line: {e}"))
            }
        })?;
        if read == 0 {
            break;
        }
        line_no = line_no.saturating_add(1);
        bytes_scanned = bytes_scanned.saturating_add(read as u64);
        let line_start = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);

        let line = buf.trim_end_matches('\n').trim_end_matches('\r');
        let pos = if case_insensitive {
            line.to_ascii_lowercase().find(&needle_lower)
        } else {
            line.find(&params.query)
        };
        if let Some(col) = pos {
            let snippet = if line.len() > max_snippet {
                let mut end = max_snippet;
                while !line.is_char_boundary(end) && end > 0 {
                    end -= 1;
                }
                line[..end].to_owned()
            } else {
                line.to_owned()
            };
            matches.push(FileSearchMatch {
                line: line_no,
                byte_offset: line_start.saturating_add(col as u64),
                snippet,
            });
            if u32::try_from(matches.len()).unwrap_or(u32::MAX) >= max_matches {
                truncated = true;
                break;
            }
        }
        if bytes_scanned >= MAX_FILE_SEARCH_SCAN_BYTES {
            truncated = true;
            break;
        }
    }

    Ok(IpcResponse::FileSearch(FileSearchResponse {
        path: params.path.clone(),
        matches,
        truncated,
        bytes_scanned,
    }))
}

/// `file_write` (TC22 A3): write UTF-8 `content` to a single regular file
/// under the `paths.write_allow` policy gate. Audit-before-write, bounded
/// size, atomic (temp file + rename). MUTATING + non-idempotent.
///
/// Order of operations (security-critical, do not reorder):
///  1. BOUND the content size BEFORE any filesystem touch (oversize ->
///     `OversizedRequest`, no write). The refusal lands a `file_write` DENY
///     audit row so the domain stream is self-complete (never an audit-allow).
///  2. RESOLVE + AUTHORIZE the canonical target via
///     `resolve_and_authorize_file_write` (absolute-only, no `..`, symlink-safe
///     via canonical-parent, gated against `write_allow` deny-first). EVERY
///     refusal (PathDenied, FileNotFound, the not-a-regular-file refusal, or a
///     server-fault Internal) emits a `file_write` audit row (`deny`, or `error`
///     for Internal) and returns with NO write performed.
///  3. AUDIT the allow decision BEFORE the write (constitution V): the
///     `allow` row is emitted, then the bytes are written. A crash between
///     the two leaves an audited intent with no file -- never an unaudited
///     write.
///  4. WRITE ATOMICALLY: content goes to a temp file in the SAME directory,
///     which is then renamed over the target. A reader never observes a
///     partial/torn write, and the rename is atomic on the same filesystem.
pub(in crate::ipc::server) fn handle_file_write(
    state: &Arc<DaemonState>,
    params: &FileWriteParams,
) -> Result<IpcResponse, IpcError> {
    use std::io::Write as _;

    // (1) Bound the content size BEFORE touching the filesystem. A write
    // larger than the cap is rejected outright; mirrors the read-window /
    // search-scan byte budgets.
    let content_len = params.content.len();
    if content_len > MAX_FILE_WRITE_BYTES {
        let msg = format!(
            "content is {content_len} bytes; file_write cap is {MAX_FILE_WRITE_BYTES} bytes"
        );
        // Record the refusal under the `file_write` action so the domain audit
        // stream is self-complete (not just the dispatch-level `ipc_file_write`
        // row). Audit-before-any-write is preserved: no byte is written here.
        emit_file_write_audit(
            state,
            &params.path.display().to_string(),
            "deny",
            Some(msg.clone()),
            None,
        );
        return Err(IpcError::new(IpcErrorCode::OversizedRequest, msg));
    }

    // (2) Resolve + authorize the canonical target. On a policy deny, record
    // a `file_write` deny audit row (mirrors WatchRuntime) and return the
    // error WITHOUT writing.
    let resolved = match resolve_and_authorize_file_write(state, &params.path, params.create_dirs) {
        Ok(p) => p,
        Err(e) => {
            // Record EVERY resolve refusal under the `file_write` action so the
            // domain audit stream is self-complete: a policy deny (PathDenied),
            // a missing-parent (FileNotFound), and a server-fault (Internal) all
            // land a row. `Internal` is logged as `error` (server fault); every
            // caller-facing refusal as `deny`. No byte is written on any path.
            let decision = if e.code == IpcErrorCode::Internal {
                "error"
            } else {
                "deny"
            };
            emit_file_write_audit(
                state,
                &params.path.display().to_string(),
                decision,
                Some(e.message.clone()),
                None,
            );
            return Err(e);
        }
    };

    // If the target exists, it must be a regular file -- refuse to clobber a
    // directory or special file (the atomic rename would fail anyway, but a
    // typed error is clearer than an opaque IO error).
    if let Ok(meta) = std::fs::symlink_metadata(&resolved)
        && !meta.file_type().is_file()
    {
        let msg = format!(
            "'{}' exists and is not a regular file; refusing to overwrite",
            resolved.display()
        );
        // Record this refusal under the `file_write` action too (self-complete
        // stream). No byte is written.
        emit_file_write_audit(
            state,
            &resolved.display().to_string(),
            "deny",
            Some(msg.clone()),
            None,
        );
        return Err(IpcError::new(IpcErrorCode::PathDenied, msg));
    }

    // (3) Audit-before-write (constitution V): the `allow` row precedes the
    // mutation, so an audited intent always exists before any byte lands.
    emit_file_write_audit(
        state,
        &resolved.display().to_string(),
        "allow",
        None,
        Some(content_len as u64),
    );

    // (4) Atomic write: stage in a temp file in the SAME directory, then
    // rename over the target. `persist`-style temp+rename avoids a torn
    // write and is atomic on the same filesystem. We build a unique temp
    // name from pid + nanos + the target file name.
    let parent = resolved.parent().ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::Internal,
            format!("resolved target '{}' has no parent", resolved.display()),
        )
    })?;
    let tmp_name = {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let stem = resolved
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file_write");
        format!(".{stem}.tc-write-{pid}-{nanos}.tmp")
    };
    let tmp_path = parent.join(tmp_name);

    // Write the temp file, flush + sync, then rename. On any IO error we best-
    // effort remove the temp file so a failed write leaves no debris.
    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(params.content.as_bytes())?;
        f.flush()?;
        f.sync_all()?;
        // Atomic replace. On Unix `rename` over an existing file is atomic.
        std::fs::rename(&tmp_path, &resolved)?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("write '{}': {e}", resolved.display()),
        ));
    }

    Ok(IpcResponse::FileWrite(FileWriteResponse {
        path: resolved,
        bytes_written: content_len as u64,
    }))
}

pub(in crate::ipc::server) fn handle_file_watch_start(
    state: &Arc<DaemonState>,
    params: &FileWatchStartParams,
) -> Result<IpcResponse, IpcError> {
    if params.rules.len() > MAX_COMMAND_INLINE_RULES {
        return Err(IpcError::new(
            IpcErrorCode::OversizedRequest,
            format!(
                "rules has {} items; cap is {MAX_COMMAND_INLINE_RULES}",
                params.rules.len()
            ),
        ));
    }
    // Resolve to a canonical, policy-authorized path (absolute-only +
    // symlink-safe default-deny) before starting the watch, so the probe
    // follows the exact target that policy authorized.
    let resolved = resolve_and_authorize_file(state, &params.path, true)?;
    let bucket_cfg = params.bucket_config.clone().unwrap_or_default();
    let follow_from_beginning = params.follow_from_beginning.unwrap_or(false);
    match state.watch.start(
        resolved,
        bucket_cfg,
        params.rules.clone(),
        follow_from_beginning,
        params.tag.clone(),
    ) {
        Ok((watch_id, bucket_id, probe_id)) => {
            Ok(IpcResponse::FileWatchStart(FileWatchStartResponse {
                watch_id,
                bucket_id,
                probe_id,
                cursor: 0,
            }))
        }
        Err(crate::file_watch::WatchError::PolicyDenied(reason)) => {
            Err(IpcError::new(IpcErrorCode::PathDenied, reason))
        }
        Err(crate::file_watch::WatchError::NotFound(p)) => Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!("'{}' is not a regular file", p.display()),
        )),
        Err(crate::file_watch::WatchError::Sifter(e)) => {
            Err(IpcError::new(IpcErrorCode::RuleInvalid, e))
        }
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("file_watch_start: {other}"),
        )),
    }
}

pub(in crate::ipc::server) fn handle_file_watch_stop(
    state: &Arc<DaemonState>,
    params: &FileWatchStopParams,
) -> Result<IpcResponse, IpcError> {
    match state.watch.stop(params.watch_id) {
        Ok((bucket_id, m)) => Ok(IpcResponse::FileWatchStop(FileWatchStopResponse {
            watch_id: params.watch_id,
            bucket_id,
            frames_total: m.frames_total,
            events_emitted: m.events_emitted,
            bytes_total: m.bytes_total,
        })),
        Err(crate::file_watch::WatchError::UnknownWatch(id)) => Err(IpcError::new(
            IpcErrorCode::UnknownWatch,
            format!("watch id '{}' is not live", id.to_wire_string()),
        )),
        Err(other) => Err(IpcError::new(
            IpcErrorCode::Internal,
            format!("file_watch_stop: {other}"),
        )),
    }
}
pub(in crate::ipc::server) fn handle_file_watch_list(state: &Arc<DaemonState>) -> IpcResponse {
    let entries: Vec<FileWatchListEntry> = state
        .watch
        .list()
        .into_iter()
        .map(
            |(watch_id, bucket_id, probe_id, path, m)| FileWatchListEntry {
                watch_id,
                bucket_id,
                probe_id,
                path,
                frames_total: m.frames_total,
                events_emitted: m.events_emitted,
                bytes_total: m.bytes_total,
            },
        )
        .collect();
    IpcResponse::FileWatchList(FileWatchListResponse { entries })
}
