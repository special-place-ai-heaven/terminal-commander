// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use terminal_commander_store::AuditEntry;

use super::common::{
    require_regular_file, resolve_and_authorize_file, resolve_and_authorize_file_write,
};
use crate::audit::AuditSink;
use crate::ipc::protocol::{
    DEFAULT_FILE_LIST_ENTRIES, DEFAULT_FILE_READ_BYTES, DEFAULT_FILE_READ_LINES,
    DEFAULT_FILE_SEARCH_MATCHES, DEFAULT_FILE_SEARCH_SNIPPET_BYTES, DirEntry, DirEntryKind,
    FileLine, FileListDirParams, FileListDirResponse, FileReadWindowParams, FileReadWindowResponse,
    FileSearchMatch, FileSearchParams, FileSearchResponse, FileWatchListEntry,
    FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse, FileWatchStopParams,
    FileWatchStopResponse, FileWriteParams, FileWriteResponse, IpcError, IpcErrorCode, IpcResponse,
    MAX_COMMAND_INLINE_RULES, MAX_FILE_LIST_ENTRIES, MAX_FILE_READ_BYTES, MAX_FILE_READ_LINES,
    MAX_FILE_SEARCH_MATCHES, MAX_FILE_SEARCH_SCAN_BYTES, MAX_FILE_SEARCH_SNIPPET_BYTES,
    MAX_FILE_WRITE_BYTES,
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

/// Build a bounded snippet that CONTAINS the match at byte offset `col`
/// within `line`.
///
/// The window is centered on the match rather than always taken from column 0
/// (F12): a match sitting past `max_snippet` bytes into a long line must still
/// appear in the returned snippet. We aim to start roughly `max_snippet / 2`
/// before `col`, then clamp so the window never starts before 0 nor runs past
/// the end of the line; when the match is near the end the window slides left
/// so it still fills `max_snippet` bytes. Both window edges are snapped DOWN to
/// UTF-8 char boundaries so the slice can never split a multi-byte character.
///
/// Invariant: `start <= col` always holds for this formula, so the BEGINNING of
/// the match is always visible -- even when the matched term is itself longer
/// than `max_snippet` (a huge query), in which case only the head of the match
/// fits, which is the best we can do without growing the snippet budget.
fn center_snippet(line: &str, col: usize, max_snippet: usize) -> String {
    if line.len() <= max_snippet {
        return line.to_owned();
    }
    // Center the window on the match, then clamp into [0, line.len()] while
    // keeping it `max_snippet` wide. `start <= col` is preserved: the left edge
    // is at most `col`, so the match start is never clipped off.
    let half = max_snippet / 2;
    let centered_start = col.saturating_sub(half);
    let mut end = (centered_start + max_snippet).min(line.len());
    let mut start = end.saturating_sub(max_snippet);

    // Snap BOTH edges DOWN to char boundaries so we never slice through a
    // multi-byte char. Walking down keeps `start <= col` (col is a boundary)
    // and can only shrink the window.
    while start > 0 && !line.is_char_boundary(start) {
        start -= 1;
    }
    while end > start && !line.is_char_boundary(end) {
        end -= 1;
    }
    // Defensive: boundary walking can only move `end` toward `start`, so this
    // clamp is belt-and-suspenders, never a real slice-order violation.
    if end < start {
        end = start;
    }
    line[start..end].to_owned()
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
            let snippet = center_snippet(line, col, max_snippet);
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

/// Map a filesystem [`std::fs::FileType`] to the wire [`DirEntryKind`]. A
/// symlink is reported as `symlink` REGARDLESS of its target: the caller stat'd
/// with `symlink_metadata`, so the link is never followed.
fn dir_entry_kind(ft: std::fs::FileType) -> DirEntryKind {
    if ft.is_symlink() {
        DirEntryKind::Symlink
    } else if ft.is_dir() {
        DirEntryKind::Dir
    } else {
        DirEntryKind::File
    }
}

/// Convert a `SystemTime` to milliseconds since the Unix epoch. A time before
/// the epoch yields a negative value; an out-of-`i64`-range value yields `None`.
fn system_time_to_millis(t: std::time::SystemTime) -> Option<i64> {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).ok(),
        Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|v| -v),
    }
}

/// `file_list_dir` (US3 FR-020/021): bounded, single-level directory listing.
///
/// Uses the IDENTICAL read gate as `file_read_window`
/// (`resolve_and_authorize_file` -> absolute-only + canonicalize +
/// `PolicyAction::FileRead`), so a policy-denied path returns the SAME denial
/// shape (FR-021) by construction, and a relative path / missing directory
/// reuse the existing typed errors. The listing is single level: entries are
/// stat'd with `symlink_metadata` (a symlink/reparse point is reported by kind
/// and NEVER followed), sorted dirs-first then files/symlinks (each group
/// lexicographic by name), and capped with a truthful `total_entries` +
/// `truncated` flag (Constitution III). An entry that vanishes between
/// enumeration and stat is returned with partial metadata (or omitted when even
/// its readdir file-type is gone) -- never a whole-listing error. The dispatch
/// layer emits the `ipc_file_list_dir` audit row (consistent with read/search).
pub(in crate::ipc::server) fn handle_file_list_dir(
    state: &Arc<DaemonState>,
    params: &FileListDirParams,
) -> Result<IpcResponse, IpcError> {
    // Same read gate as file_read_window: absolute-only, canonicalize (resolving
    // symlinks), then PolicyAction::FileRead on the canonical target. A denied
    // path yields the identical PathDenied shape file_read produces (FR-021).
    let resolved = resolve_and_authorize_file(state, std::path::Path::new(&params.path), false)?;

    // The target must be a directory. A regular-file (or special-file) target
    // teaches the `read` action rather than returning an odd/empty listing.
    if !resolved.is_dir() {
        return Err(IpcError::new(
            IpcErrorCode::FileNotFound,
            format!(
                "'{}' is not a directory; use the files `read` action (file_read_window) \
                 to read a file",
                resolved.display()
            ),
        ));
    }

    // Clamp the entry cap into [1, MAX_FILE_LIST_ENTRIES]; omitted = default.
    let cap = params
        .max_entries
        .map_or(DEFAULT_FILE_LIST_ENTRIES, |n| (n as usize).max(1))
        .min(MAX_FILE_LIST_ENTRIES);

    let rd = std::fs::read_dir(&resolved).map_err(|e| {
        IpcError::new(
            IpcErrorCode::Internal,
            format!("read_dir '{}': {e}", resolved.display()),
        )
    })?;

    let mut collected: Vec<DirEntry> = Vec::new();
    for ent in rd {
        // A per-entry readdir error (rare) is skipped, not a whole-listing fault.
        let Ok(ent) = ent else { continue };
        let name = ent.file_name().to_string_lossy().into_owned();
        // `symlink_metadata`: NEVER follow symlinks / reparse points.
        if let Ok(meta) = std::fs::symlink_metadata(ent.path()) {
            let kind = dir_entry_kind(meta.file_type());
            let size_bytes = if kind == DirEntryKind::File {
                Some(meta.len())
            } else {
                None
            };
            let mtime_ms = meta.modified().ok().and_then(system_time_to_millis);
            collected.push(DirEntry {
                name,
                kind,
                size_bytes,
                mtime_ms,
            });
        } else if let Ok(ft) = ent.file_type() {
            // Race: the entry vanished between enumeration and stat. Fall back to
            // the cheap readdir file-type (captured without a separate stat and
            // NOT symlink-following) with metadata absent. If even that is gone,
            // omit the entry entirely (never counted).
            collected.push(DirEntry {
                name,
                kind: dir_entry_kind(ft),
                size_bytes: None,
                mtime_ms: None,
            });
        }
    }

    // Deterministic order: dirs first, then files/symlinks together; each group
    // lexicographic by name.
    collected.sort_by(|a, b| {
        let a_dir = matches!(a.kind, DirEntryKind::Dir);
        let b_dir = matches!(b.kind, DirEntryKind::Dir);
        b_dir.cmp(&a_dir).then_with(|| a.name.cmp(&b.name))
    });

    let total_entries = collected.len() as u64;
    let truncated = collected.len() > cap;
    collected.truncate(cap);

    Ok(IpcResponse::FileListDir(FileListDirResponse {
        path: resolved.display().to_string(),
        entries: collected,
        total_entries,
        truncated,
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

#[cfg(test)]
mod tests {
    use super::center_snippet;

    /// F12 regression: when the match sits far past `max_snippet` bytes from
    /// column 0, the centered window must still CONTAIN the matched substring.
    /// The old code always sliced `line[..max_snippet]`, so a match deep in a
    /// long line was invisible in the returned snippet.
    #[test]
    fn center_snippet_contains_match_far_from_column_zero() {
        let max = 20;
        let prefix = "x".repeat(200);
        let needle = "NEEDLE";
        let line = format!("{prefix}{needle}{}", "y".repeat(200));
        let col = line.find(needle).expect("needle present");
        assert!(
            col > max,
            "match must be past the snippet budget to exercise F12"
        );

        let snippet = center_snippet(&line, col, max);
        assert!(
            snippet.contains(needle),
            "centered snippet must contain the match; got {snippet:?}"
        );
        assert!(
            snippet.len() <= max,
            "snippet must stay within the byte budget"
        );
    }

    /// A match near column 0 still works: the window starts at 0 and the match
    /// is visible at the head of the snippet.
    #[test]
    fn center_snippet_match_near_column_zero() {
        let max = 20;
        let needle = "HIT";
        let line = format!("{needle}{}", "z".repeat(200));
        let col = line.find(needle).expect("needle present");
        assert_eq!(col, 0);

        let snippet = center_snippet(&line, col, max);
        assert!(snippet.starts_with(needle), "got {snippet:?}");
        assert!(snippet.len() <= max);
    }

    /// A short line (<= max_snippet) is returned whole and unchanged.
    #[test]
    fn center_snippet_short_line_unchanged() {
        let max = 240;
        let line = "a short line with the WORD in it";
        let col = line.find("WORD").expect("needle present");
        let snippet = center_snippet(line, col, max);
        assert_eq!(snippet, line);
    }

    /// A multi-byte UTF-8 line whose RAW window edges land MID-CHARACTER. This
    /// fixture is built so the unclamped centering math puts both edges inside
    /// a multi-byte char, forcing BOTH boundary-snapping `while` loops to walk
    /// down -- a regression that broke the snapping would panic here (mid-char
    /// slice) instead of silently passing. We assert no panic, the result still
    /// sits on char boundaries (valid UTF-8 + recoverable as a substring), and
    /// the match is inside the window.
    ///
    /// Fixture math (prefix 'é' = 2 bytes, suffix 'り' = 3 bytes):
    ///   prefix = "é"*80  -> bytes [0, 160), boundaries on every EVEN offset.
    ///   needle = "TARGET" (ASCII) at col = 160, occupying [160, 166).
    ///   suffix = "り"*40  -> boundaries at 166, 169, 172, 175, 178, 181, ...
    ///   max_snippet = 43 -> half = 21. Unclamped: start = col - 21 = 139 (ODD
    ///   -> mid-'é'), end = 139 + 43 = 182 (16 bytes into the suffix, between
    ///   181 and 184 -> mid-'り'). Both `while` loops MUST run: 139->138 and
    ///   182->181, yielding the on-boundary window [138, 181).
    #[test]
    fn center_snippet_utf8_snaps_offboundary_edges() {
        let prefix = "é".repeat(80); // 160 bytes
        let needle = "TARGET";
        let suffix = "り".repeat(40); // 120 bytes, 3-byte chars
        let line = format!("{prefix}{needle}{suffix}");
        let col = line.find(needle).expect("needle present");
        assert_eq!(col, 160, "fixture assumes the match sits at byte 160");

        let max = 43;
        // Sanity-check the fixture itself: the RAW (unsnapped) edges this helper
        // computes must both be mid-character, or the test would be vacuous.
        let raw_start = col - max / 2; // 139
        let raw_end = raw_start + max; // 182
        assert!(
            !line.is_char_boundary(raw_start),
            "fixture must put the raw start edge mid-char to exercise the snap loop"
        );
        assert!(
            !line.is_char_boundary(raw_end),
            "fixture must put the raw end edge mid-char to exercise the snap loop"
        );

        // No panic on a mid-char raw window == the snap loops did their job.
        let snippet = center_snippet(&line, col, max);
        // The result is valid UTF-8 (String guarantees it only because we sliced
        // on boundaries) and must be locatable back in the line on boundaries.
        let snip_start = line
            .find(snippet.as_str())
            .expect("snippet is a substring of line");
        let snip_end = snip_start + snippet.len();
        assert!(
            line.is_char_boundary(snip_start),
            "result start must be on a char boundary"
        );
        assert!(
            line.is_char_boundary(snip_end),
            "result end must be on a char boundary"
        );
        // Edges were snapped DOWN from the mid-char raw edges.
        assert_eq!(snip_start, 138, "start should snap 139 -> 138");
        assert_eq!(snip_end, 181, "end should snap 182 -> 181");
        assert!(
            snippet.len() <= max,
            "got len {} for {snippet:?}",
            snippet.len()
        );
        assert!(
            snippet.contains(needle),
            "centered snippet should hold the match"
        );
    }

    /// Huge-query edge: the matched term itself is longer than `max_snippet`.
    /// We cannot show the whole match, but the invariant `start <= col` must
    /// hold so the FIRST byte of the match falls inside the window (the head of
    /// the match is visible) -- and we must not panic on the slice.
    #[test]
    fn center_snippet_query_longer_than_budget() {
        let max = 8;
        let needle = "A_VERY_LONG_QUERY_STRING";
        assert!(needle.len() > max);
        let pad = "p".repeat(50);
        let line = format!("{pad}{needle}{}", "q".repeat(50));
        let col = line.find(needle).expect("needle present");

        let snippet = center_snippet(&line, col, max);
        assert!(snippet.len() <= max);
        // The match's first byte is inside the returned window: the snippet
        // contains the slice [col, col+1), i.e. the head of the match. Confirm
        // by locating the snippet back in the line and checking it spans `col`.
        let snip_start = line
            .find(snippet.as_str())
            .expect("snippet is a substring of line");
        let snip_end = snip_start + snippet.len();
        assert!(
            snip_start <= col && col < snip_end,
            "window [{snip_start}, {snip_end}) must contain match at {col}; got {snippet:?}"
        );
    }
}
