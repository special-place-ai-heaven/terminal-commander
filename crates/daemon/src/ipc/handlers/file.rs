// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::sync::Arc;

use super::common::{map_path_policy, require_regular_file};
use crate::ipc::protocol::{
    DEFAULT_FILE_READ_BYTES, DEFAULT_FILE_READ_LINES, DEFAULT_FILE_SEARCH_MATCHES,
    DEFAULT_FILE_SEARCH_SNIPPET_BYTES, FileLine, FileReadWindowParams, FileReadWindowResponse,
    FileSearchMatch, FileSearchParams, FileSearchResponse, FileWatchListEntry,
    FileWatchListResponse, FileWatchStartParams, FileWatchStartResponse, FileWatchStopParams,
    FileWatchStopResponse, IpcError, IpcErrorCode, IpcResponse, MAX_COMMAND_INLINE_RULES,
    MAX_FILE_READ_BYTES, MAX_FILE_READ_LINES, MAX_FILE_SEARCH_MATCHES, MAX_FILE_SEARCH_SCAN_BYTES,
    MAX_FILE_SEARCH_SNIPPET_BYTES,
};
use crate::state::DaemonState;

pub(in crate::ipc::server) fn handle_file_read_window(
    state: &Arc<DaemonState>,
    params: &FileReadWindowParams,
) -> Result<IpcResponse, IpcError> {
    use std::io::{BufRead, BufReader};

    map_path_policy(state, &params.path, false)?;
    let meta = require_regular_file(&params.path)?;
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

    let f = std::fs::File::open(&params.path)
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
    map_path_policy(state, &params.path, false)?;
    require_regular_file(&params.path)?;

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

    let f = std::fs::File::open(&params.path)
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
    let bucket_cfg = params.bucket_config.clone().unwrap_or_default();
    let follow_from_beginning = params.follow_from_beginning.unwrap_or(false);
    match state.watch.start(
        params.path.clone(),
        bucket_cfg,
        params.rules.clone(),
        follow_from_beginning,
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
