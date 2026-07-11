// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! File probe (TC18). Follows a file path, handles create-after-start,
//! truncation, and rotation; emits normalized SourceFrame instances
//! to the sifter runtime via the same EventSink as the process probe.
//!
//! Source-status: live. Native filesystems use an event-driven `notify`
//! backend (US3b / T041) so a change is observed within OS-notification
//! latency; WSL `/mnt/c` (9p/drvfs) keeps the re-stat polling backend,
//! because inotify is silently non-functional across the 9P boundary
//! (microsoft/WSL#4739). The backend is chosen per-path via
//! `/proc/self/mountinfo` (see `select_backend_for_path`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use terminal_commander_core::{BucketId, ContextRingManager, ProbeId, SourceFrame, SourceStream};
use terminal_commander_sifters::SifterRuntime;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};
use tokio::sync::oneshot;

use crate::noise_pipeline::ProbeNoisePipeline;
use crate::process::{EventSink, InMemorySink};

/// Stable identity of an open file, used to detect rotation that a
/// size comparison alone misses (H2). A logrotate `create` /
/// atomic-rename whose replacement file already reached or passed the
/// old read position is NOT a size shrink, so without identity we
/// would seek into the middle of the NEW file and emit a corrupt
/// mid-line fragment. Captured from the OPEN handle so the identity
/// reflects the file we are actually reading.
///
/// - Unix: `(st_dev, st_ino)` from the metadata.
/// - Windows: `(dwVolumeSerialNumber, nFileIndexHigh:nFileIndexLow)`
///   read via `GetFileInformationByHandle` on the open handle. The std
///   `MetadataExt` accessors for these are unstable
///   (`windows_by_handle`, rust-lang/rust#63010) and unusable on
///   stable, so we read the same kernel fields directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    /// Device / volume the file lives on.
    device: u64,
    /// Inode / file-index uniquely identifying the file on that device.
    inode: u64,
}

impl FileIdentity {
    /// Capture a stable identity for the open file. `meta` is used on
    /// Unix; the open `file` handle is used on Windows. Returns `None`
    /// when the platform cannot supply an identity; a `None` identity
    /// is treated as "unknown" and never triggers a false rotation.
    #[cfg(unix)]
    // Option return is required for cross-platform API uniformity: the
    // Windows path can legitimately yield `None`.
    #[allow(clippy::unnecessary_wraps)]
    fn capture(_file: &File, meta: &std::fs::Metadata) -> Option<Self> {
        use std::os::unix::fs::MetadataExt as _;
        Some(Self {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }

    #[cfg(windows)]
    fn capture(file: &File, _meta: &std::fs::Metadata) -> Option<Self> {
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };

        let handle = file.as_raw_handle() as HANDLE;
        // SAFETY: `handle` is a valid, open file handle for the lifetime
        // of `file` (borrowed for this call). `GetFileInformationByHandle`
        // only reads from the handle and writes into `info`, which is a
        // properly aligned, fully owned `BY_HANDLE_FILE_INFORMATION`. We
        // check the BOOL result before reading any field.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe { GetFileInformationByHandle(handle, &raw mut info) };
        if ok == 0 {
            return None;
        }
        let inode = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Some(Self {
            device: u64::from(info.dwVolumeSerialNumber),
            inode,
        })
    }

    #[cfg(not(any(unix, windows)))]
    fn capture(_file: &File, _meta: &std::fs::Metadata) -> Option<Self> {
        None
    }
}

/// Default polling interval.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Which low-level mechanism the file probe uses to learn that a watched
/// file changed.
///
/// US3b (T041): native filesystems get an event-driven `notify` watcher so
/// a change is observed within OS-notification latency instead of waiting
/// out a poll interval. WSL `/mnt/c` (9p/drvfs) keeps the re-stat poll loop:
/// inotify is silently non-functional across the 9P boundary
/// (microsoft/WSL#4739) -- `inotify_add_watch` succeeds but no events are
/// ever delivered, so an event-driven watcher there would hang forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileWatchBackend {
    /// Event-driven OS notifications (inotify / FSEvents / `ReadDirectoryChangesW`).
    Notify,
    /// Periodic re-stat polling. The WSL 9P fallback and the universal
    /// platform floor.
    Poll,
}

/// Decide the watch backend for `target` from a `/proc/self/mountinfo`-format
/// string.
///
/// Pure and fixture-testable: the same longest-mount-point-prefix match the
/// store's 9P guard uses, so a path whose owning mount is filesystem type `9p`
/// selects [`FileWatchBackend::Poll`] and everything else selects
/// [`FileWatchBackend::Notify`].
///
/// `mountinfo` lines look like:
/// `36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue`
/// where field index 4 is the mount point and the token immediately after
/// the `-` separator is the filesystem type.
#[must_use]
pub fn backend_from_mountinfo(mountinfo: &str, target: &std::path::Path) -> FileWatchBackend {
    let target_str = target.to_string_lossy();
    let mut best: Option<(&str, &str)> = None;
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(dash_idx) = fields.iter().position(|&f| f == "-") else {
            continue;
        };
        if fields.len() < 5 || dash_idx + 1 >= fields.len() {
            continue;
        }
        let mount_point = fields[4];
        let fs_type = fields[dash_idx + 1];
        // Longest-prefix match: a path under `/mnt/c` must bind to the
        // `/mnt/c` 9p mount, not the `/` 9p (or ext4) root above it.
        if path_has_mount_prefix(&target_str, mount_point) {
            match best {
                None => best = Some((mount_point, fs_type)),
                Some((bmp, _)) if mount_point.len() > bmp.len() => {
                    best = Some((mount_point, fs_type));
                }
                _ => {}
            }
        }
    }
    match best {
        // WSL drvfs mounts report fs type `9p`; treat it as poll-only.
        Some((_, "9p")) => FileWatchBackend::Poll,
        _ => FileWatchBackend::Notify,
    }
}

/// True when `mount_point` is a path-component prefix of `target`. Avoids
/// the false match where `/mnt/c2` would otherwise be claimed by `/mnt/c`
/// under a naive `starts_with`. The root mount `/` matches everything.
fn path_has_mount_prefix(target: &str, mount_point: &str) -> bool {
    if mount_point == "/" {
        return true;
    }
    target
        .strip_prefix(mount_point)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Choose the watch backend for `path` on this host.
///
/// Reads `/proc/self/mountinfo` when present (Linux/WSL) and applies
/// [`backend_from_mountinfo`]; on every other platform there is no 9P
/// boundary to worry about, so the native event-driven backend is used.
#[must_use]
pub fn select_backend_for_path(path: &std::path::Path) -> FileWatchBackend {
    let mountinfo_path = std::path::Path::new("/proc/self/mountinfo");
    let Ok(mountinfo) = std::fs::read_to_string(mountinfo_path) else {
        // Not Linux/WSL (or unreadable): no 9P boundary, use notify.
        return FileWatchBackend::Notify;
    };
    // Resolve to an absolute path so the mount-point prefix match is
    // meaningful; fall back to the raw path if canonicalize fails (e.g.
    // a watch-for-create path that does not exist yet -- its parent is
    // what matters, and the raw absolute form still prefix-matches).
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    backend_from_mountinfo(&mountinfo, &abs)
}

/// File probe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileProbeMode {
    /// Scan from the current position once and stop.
    ScanOnce,
    /// Follow the file, starting from the end.
    FollowEnd,
    /// Follow the file, starting from the beginning.
    FollowBeginning,
}

/// Per-probe configuration.
#[derive(Debug, Clone)]
pub struct FileProbeConfig {
    pub probe_id: Option<ProbeId>,
    pub bucket_id: BucketId,
    pub path: PathBuf,
    pub mode: FileProbeMode,
    pub poll_interval: Duration,
    /// Which low-level change-detection mechanism the probe drives. US3b
    /// (T041): defaults to event-driven [`FileWatchBackend::Notify`]; the
    /// daemon overrides it with [`select_backend_for_path`] so WSL `/mnt/c`
    /// (9p) watches fall back to polling.
    pub backend: FileWatchBackend,
}

impl FileProbeConfig {
    /// Construct a follow-end config.
    #[must_use]
    pub const fn follow_end(path: PathBuf, bucket_id: BucketId) -> Self {
        Self {
            probe_id: None,
            bucket_id,
            path,
            mode: FileProbeMode::FollowEnd,
            poll_interval: DEFAULT_POLL_INTERVAL,
            backend: FileWatchBackend::Notify,
        }
    }
    /// Construct a follow-beginning config.
    #[must_use]
    pub const fn follow_beginning(path: PathBuf, bucket_id: BucketId) -> Self {
        Self {
            probe_id: None,
            bucket_id,
            path,
            mode: FileProbeMode::FollowBeginning,
            poll_interval: DEFAULT_POLL_INTERVAL,
            backend: FileWatchBackend::Notify,
        }
    }
}

/// Counters.
#[derive(Debug, Default, Clone)]
pub struct FileProbeMetrics {
    pub frames_total: u64,
    pub bytes_total: u64,
    pub events_emitted: u64,
    pub rotations_detected: u64,
    pub truncations_detected: u64,
    pub frames_suppressed: u64,
    pub frames_suppressed_progress: u64,
    pub frames_suppressed_dedupe: u64,
}

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum FileProbeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("probe cancelled")]
    Cancelled,
}

/// Handle to a running file probe.
#[derive(Debug)]
pub struct FileProbe {
    probe_id: ProbeId,
    metrics: Arc<Mutex<FileProbeMetrics>>,
    cancel_tx: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<Result<(), FileProbeError>>>,
}

impl FileProbe {
    /// Spawn a probe.
    pub fn spawn(
        config: FileProbeConfig,
        rings: Arc<ContextRingManager>,
        runtime: Arc<SifterRuntime>,
        sink: Arc<dyn EventSink>,
    ) -> Result<Self, FileProbeError> {
        let probe_id = config.probe_id.unwrap_or_default();
        rings
            .create_ring_default(probe_id)
            .map_err(|e| FileProbeError::Io(std::io::Error::other(e.to_string())))?;
        let metrics = Arc::new(Mutex::new(FileProbeMetrics::default()));
        let metrics_for_task = Arc::clone(&metrics);
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let join = tokio::spawn(run(
            config,
            probe_id,
            rings,
            runtime,
            sink,
            metrics_for_task,
            cancel_rx,
        ));
        Ok(Self {
            probe_id,
            metrics,
            cancel_tx: Some(cancel_tx),
            join: Some(join),
        })
    }

    #[must_use]
    pub const fn id(&self) -> ProbeId {
        self.probe_id
    }

    #[must_use]
    pub fn metrics(&self) -> FileProbeMetrics {
        self.metrics.lock().clone()
    }

    /// Whether the probe task has reached a terminal state.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.join
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
    }

    pub async fn wait(&mut self) -> Result<(), FileProbeError> {
        let Some(j) = self.join.take() else {
            return Err(FileProbeError::Cancelled);
        };
        match j.await {
            Ok(r) => r,
            Err(e) => Err(FileProbeError::Io(std::io::Error::other(e.to_string()))),
        }
    }
}

/// Safety-net re-stat interval used by the event-driven backend. Even with
/// a healthy `notify` watcher the probe still wakes this often to (a) catch
/// the brief window before a watch-for-create directory watch is armed, (b)
/// survive a dropped/over-flowed watcher, and (c) make the FollowEnd initial
/// seek + first read happen without depending on an event. It is far longer
/// than the poll interval -- events do the fast work; this is only a floor.
const NOTIFY_SAFETY_NET_INTERVAL: Duration = Duration::from_secs(1);

/// Event-driven change signal backing [`FileWatchBackend::Notify`].
///
/// Owns a `notify::RecommendedWatcher` (inotify / FSEvents /
/// `ReadDirectoryChangesW`) that watches the PARENT DIRECTORY of the target,
/// not the file itself: a logrotate `create` / atomic-rename swaps the inode,
/// which a file-level inotify watch loses but a directory watch still sees as
/// a create/rename event. Every raw event is coalesced into a single readiness
/// signal on a bounded channel -- the probe re-stats and reads on each signal,
/// so collapsing a burst of N events into one wake-up loses nothing.
struct FileChangeWatcher {
    /// Held to keep the OS watch alive; dropping it stops notifications.
    _watcher: notify::RecommendedWatcher,
    /// Readiness signals. Capacity 1 with a non-blocking sender: a backlog
    /// is meaningless (the probe always re-reads everything new), so extra
    /// events are dropped rather than queued.
    rx: tokio::sync::mpsc::Receiver<()>,
}

impl FileChangeWatcher {
    /// Build a directory watch for `path`'s parent. Returns `None` if a
    /// watcher cannot be created or armed (caller then falls back to the
    /// poll wait, never failing the probe).
    fn new(path: &std::path::Path) -> Option<Self> {
        use notify::Watcher as _;

        let (event_tx, rx) = tokio::sync::mpsc::channel::<()>(1);
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                // Any successful event = "something changed, go re-stat".
                // A try_send failure means a signal is already pending
                // (capacity 1) -- exactly the coalescing we want. Errors
                // are dropped; the safety-net interval still re-reads.
                if res.is_ok() {
                    let _ = event_tx.try_send(());
                }
            })
            .ok()?;

        // Watch the parent directory non-recursively so create / rename /
        // modify of the target file are all observed. Fall back to watching
        // the file's own path if it has no parent (e.g. a bare filename).
        let watch_target = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| path.to_path_buf(), std::path::Path::to_path_buf);
        watcher
            .watch(&watch_target, notify::RecursiveMode::NonRecursive)
            .ok()?;

        Some(Self {
            _watcher: watcher,
            rx,
        })
    }
}

#[allow(clippy::too_many_arguments)]
// The follow loop is one tightly-coupled lifecycle (open, detect
// rotation/truncation, read new lines, wait for the next change signal);
// splitting it would obscure the shared `pos`/`last_size`/`last_identity`
// state more than the length costs.
#[allow(clippy::too_many_lines)]
async fn run(
    config: FileProbeConfig,
    probe_id: ProbeId,
    rings: Arc<ContextRingManager>,
    runtime: Arc<SifterRuntime>,
    sink: Arc<dyn EventSink>,
    metrics: Arc<Mutex<FileProbeMetrics>>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<(), FileProbeError> {
    let mut pos: u64 = 0;
    let mut line_no: u64 = 0;
    let mut last_size: Option<u64> = None;
    // H2: track the identity of the file we last read so a rotation
    // that keeps (or grows) the size — a logrotate `create` /
    // atomic-rename — is still detected and `pos` reset to 0.
    let mut last_identity: Option<FileIdentity> = None;
    let mut noise_pipeline = ProbeNoisePipeline::with_default_policy();

    // US3b (T041): arm the event-driven watcher on native filesystems. On
    // the poll backend (WSL 9P) or when a watcher cannot be created, this
    // stays `None` and the loop uses the short re-stat tick. A `None` here
    // is always safe -- it degrades to the existing polling behavior.
    let mut watcher = match config.backend {
        FileWatchBackend::Notify => FileChangeWatcher::new(&config.path),
        FileWatchBackend::Poll => None,
    };

    loop {
        if cancel_rx.try_recv().is_ok() {
            return Err(FileProbeError::Cancelled);
        }
        // Attempt to open the file.
        match File::open(&config.path).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if matches!(config.mode, FileProbeMode::ScanOnce) {
                    return Ok(());
                }
                // Watch-for-create; sleep until next poll.
            }
            Err(e) => return Err(FileProbeError::Io(e)),
            Ok(mut f) => {
                let meta = f.metadata().await?;
                if !meta.is_file() {
                    return Err(FileProbeError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "watched path '{}' is no longer a regular file",
                            config.path.display()
                        ),
                    )));
                }
                let size = meta.len();
                let identity = FileIdentity::capture(&f, &meta);
                // Initial seek for FollowEnd on first open.
                if last_size.is_none() {
                    match config.mode {
                        FileProbeMode::FollowEnd => {
                            pos = size;
                        }
                        FileProbeMode::FollowBeginning | FileProbeMode::ScanOnce => {
                            pos = 0;
                        }
                    }
                }
                // Rotation / truncation detection. Two independent
                // signals, checked identity-first:
                //
                // H2 (rotation): the file we are now reading is a
                //   DIFFERENT file than last poll (a logrotate
                //   `create` / atomic-rename). This is missed by a
                //   size comparison when the new file already reached
                //   or passed the old `pos`; without it we would seek
                //   into the middle of the new file and emit a corrupt
                //   mid-line fragment. Only fires when BOTH identities
                //   are known and differ.
                //
                // L2 (truncation): the same file shrank in place
                //   (`size < prev`). ANY in-place shrink is a
                //   truncation, not only a shrink to exactly 0 (a
                //   truncate-then-write leaves a small non-zero size).
                //
                // Both reset `pos = 0` so the next read starts at the
                // head of the (new or truncated) content.
                let rotated = match (last_identity, identity) {
                    (Some(prev), Some(curr)) => prev != curr,
                    _ => false,
                };
                if rotated {
                    let mut m = metrics.lock();
                    m.rotations_detected = m.rotations_detected.saturating_add(1);
                    pos = 0;
                } else if let Some(prev) = last_size
                    && size < prev
                {
                    let mut m = metrics.lock();
                    m.truncations_detected = m.truncations_detected.saturating_add(1);
                    pos = 0;
                }
                last_size = Some(size);
                last_identity = identity;

                if pos < size {
                    f.seek(SeekFrom::Start(pos)).await?;
                    let mut reader = BufReader::new(f);
                    let scan_once = matches!(config.mode, FileProbeMode::ScanOnce);
                    // Read whole, newline-terminated lines and advance
                    // `pos` by the exact raw on-disk byte count
                    // (terminator included) so byte offsets stay aligned
                    // across CRLF lines and re-opens. A trailing line
                    // with no `\n` is emitted only in ScanOnce; in follow
                    // mode it is left unconsumed (pos not advanced) so a
                    // still-growing final line is read exactly once, when
                    // complete -- never mis-split or re-emitted, and a
                    // stray non-UTF-8 byte never aborts the probe.
                    while let Some(line) = read_file_line(&mut reader).await? {
                        if !line.terminated && !scan_once {
                            break;
                        }
                        line_no = line_no.saturating_add(1);
                        let frame = SourceFrame::new(probe_id, SourceStream::File, line.text)
                            .with_line(line_no)
                            .with_byte_offset(pos);
                        pos = pos.saturating_add(line.raw_len);
                        let _ = rings.append_frame(probe_id, frame.clone());
                        {
                            let mut m = metrics.lock();
                            m.frames_total = m.frames_total.saturating_add(1);
                            m.bytes_total = m.bytes_total.saturating_add(line.raw_len);
                        }
                        let mut events_emitted = metrics.lock().events_emitted;
                        {
                            let mut m = metrics.lock();
                            noise_pipeline.process_frame(
                                &frame,
                                &terminal_commander_core::SourceType::File,
                                config.bucket_id,
                                &runtime,
                                sink.as_ref(),
                                &mut *m,
                                &mut events_emitted,
                                std::iter::empty(),
                            );
                            m.events_emitted = events_emitted;
                        }
                    }
                }
                if matches!(config.mode, FileProbeMode::ScanOnce) {
                    return Ok(());
                }
            }
        }
        // Wait for the next change signal or cancellation. The event-driven
        // backend wakes on an OS notification (within notification latency)
        // OR a long safety-net interval OR cancel; the poll backend keeps
        // the short re-stat tick (WSL 9P, where notify never fires).
        if let Some(w) = watcher.as_mut() {
            tokio::select! {
                _ = w.rx.recv() => {}
                () = tokio::time::sleep(NOTIFY_SAFETY_NET_INTERVAL) => {}
                _ = &mut cancel_rx => return Err(FileProbeError::Cancelled),
            }
        } else {
            tokio::select! {
                () = tokio::time::sleep(config.poll_interval) => {}
                _ = &mut cancel_rx => return Err(FileProbeError::Cancelled),
            }
        }
    }
}

/// Maximum bytes retained for a single file line's decoded text. A
/// pathological newline-less file cannot grow the per-line buffer past
/// this; excess text bytes are dropped, but `raw_len` still reflects the
/// true on-disk byte count so `pos` stays aligned.
const MAX_FILE_LINE_BYTES: usize = 64 * 1024;

/// One line read from a followed file.
struct FileLineRead {
    /// Lossily-decoded line text with any trailing `\r` (CRLF) removed.
    text: String,
    /// Exact raw on-disk bytes this line occupied, terminator included.
    /// `pos` advances by this so byte offsets stay aligned regardless of
    /// CRLF endings or capped/lossy text.
    raw_len: u64,
    /// Whether a `\n` terminator was seen. `false` = a still-growing
    /// final line; follow mode leaves it for the next poll.
    terminated: bool,
}

/// Read one line from `reader`, splitting on `\n`. Decodes with
/// `from_utf8_lossy` so a stray non-UTF-8 byte never aborts the probe
/// (the prior `lines()`/`next_line()` path returned `Err` and silently
/// stopped following). Returns `None` only at EOF with nothing buffered.
async fn read_file_line<R>(reader: &mut R) -> std::io::Result<Option<FileLineRead>>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut text: Vec<u8> = Vec::new();
    let mut raw_len: u64 = 0;
    let mut saw_input = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            // EOF. Surface a buffered, newline-less tail as unterminated.
            if saw_input {
                return Ok(Some(FileLineRead {
                    text: decode_file_line(&text),
                    raw_len,
                    terminated: false,
                }));
            }
            return Ok(None);
        }
        saw_input = true;
        if let Some(idx) = chunk.iter().position(|&b| b == b'\n') {
            push_capped(&mut text, &chunk[..idx]);
            // raw_len counts the line bytes plus the `\n` itself.
            raw_len = raw_len.saturating_add(idx as u64).saturating_add(1);
            reader.consume(idx + 1);
            return Ok(Some(FileLineRead {
                text: decode_file_line(&text),
                raw_len,
                terminated: true,
            }));
        }
        let take = chunk.len();
        push_capped(&mut text, chunk);
        raw_len = raw_len.saturating_add(take as u64);
        reader.consume(take);
    }
}

/// Append `more` to `buf`, retaining at most [`MAX_FILE_LINE_BYTES`].
/// Excess text is dropped (the raw byte count is tracked separately by
/// the caller so `pos` accounting is unaffected by the cap).
fn push_capped(buf: &mut Vec<u8>, more: &[u8]) {
    let room = MAX_FILE_LINE_BYTES.saturating_sub(buf.len());
    let take = room.min(more.len());
    buf.extend_from_slice(&more[..take]);
}

/// Decode raw line bytes as lossy UTF-8 and strip a single trailing
/// `\r` so a CRLF-terminated line yields the same text as an LF line.
fn decode_file_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\r')
        .to_owned()
}

/// Test-friendly factory that returns an InMemorySink alongside the
/// probe handle.
pub fn spawn_with_sink(
    config: FileProbeConfig,
    rings: Arc<ContextRingManager>,
    runtime: Arc<SifterRuntime>,
) -> Result<(FileProbe, Arc<InMemorySink>), FileProbeError> {
    let sink = Arc::new(InMemorySink::new());
    let arc_dyn: Arc<dyn EventSink> = sink.clone();
    let probe = FileProbe::spawn(config, rings, runtime, arc_dyn)?;
    Ok((probe, sink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use terminal_commander_core::{ContextHint, RuleDefinition, RuleStatus, RuleType, Severity};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn rule_error() -> RuleDefinition {
        RuleDefinition {
            id: "test.err".to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::High,
            event_kind: "kw_err".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["ERROR".to_owned()]),
            captures: vec![],
            summary_template: "matched".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tc-file-probe-{}-{}", std::process::id(), name))
    }

    #[test]
    fn read_file_line_splits_lf_and_reports_raw_len() {
        rt().block_on(async {
            let mut src: &[u8] = b"abc\ndef\n";
            let l1 = read_file_line(&mut src).await.unwrap().unwrap();
            assert_eq!(l1.text, "abc");
            assert_eq!(l1.raw_len, 4); // "abc\n"
            assert!(l1.terminated);
            let l2 = read_file_line(&mut src).await.unwrap().unwrap();
            assert_eq!(l2.text, "def");
            assert_eq!(l2.raw_len, 4);
            assert!(l2.terminated);
            assert!(read_file_line(&mut src).await.unwrap().is_none());
        });
    }

    #[test]
    fn read_file_line_crlf_offset_counts_cr_byte() {
        rt().block_on(async {
            // CRLF: the `\r` is stripped from the text but MUST be counted
            // in raw_len, else `pos` drifts one byte per line and the next
            // poll re-reads/duplicates content.
            let mut src: &[u8] = b"win\r\nnext\r\n";
            let l1 = read_file_line(&mut src).await.unwrap().unwrap();
            assert_eq!(l1.text, "win");
            assert_eq!(l1.raw_len, 5); // "win\r\n" == 5 on-disk bytes
            assert!(l1.terminated);
        });
    }

    #[test]
    fn read_file_line_unterminated_tail_is_flagged() {
        rt().block_on(async {
            let mut src: &[u8] = b"done\npartial";
            let l1 = read_file_line(&mut src).await.unwrap().unwrap();
            assert!(l1.terminated);
            let l2 = read_file_line(&mut src).await.unwrap().unwrap();
            assert_eq!(l2.text, "partial");
            assert_eq!(l2.raw_len, 7);
            assert!(
                !l2.terminated,
                "a newline-less tail must be flagged so follow mode holds it back"
            );
        });
    }

    #[test]
    fn read_file_line_non_utf8_does_not_abort() {
        rt().block_on(async {
            // 0xFF is invalid UTF-8. The old next_line() path returned Err
            // and silently stopped the probe; lossy decode must keep going.
            let mut src: &[u8] = b"ab\xffcd\n";
            let l1 = read_file_line(&mut src).await.unwrap().unwrap();
            assert!(l1.terminated);
            assert_eq!(l1.raw_len, 6); // 5 line bytes + '\n'
            assert!(
                l1.text.contains('\u{FFFD}'),
                "invalid byte replaced with U+FFFD, not fatal: {:?}",
                l1.text
            );
        });
    }

    #[test]
    fn file_probe_scan_once_existing_file() {
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("scan-once");
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "an ERROR line").unwrap();
                writeln!(f, "ordinary").unwrap();
            }
            let rings = Arc::new(ContextRingManager::new());
            let sifter = Arc::new(SifterRuntime::build(&[rule_error()]).unwrap());
            let mut cfg = FileProbeConfig::follow_beginning(p.clone(), BucketId::new());
            cfg.mode = FileProbeMode::ScanOnce;
            let (mut probe, sink) = spawn_with_sink(cfg, rings, sifter).unwrap();
            let _ = probe.wait().await;
            let m = probe.metrics();
            assert!(m.frames_total >= 2, "frames: {}", m.frames_total);
            assert!(m.events_emitted >= 1, "events: {}", m.events_emitted);
            let events = sink.drain();
            assert!(!events.is_empty());
            assert!(
                events
                    .iter()
                    .all(|event| event.source.source_type
                        == terminal_commander_core::SourceType::File),
                "file probe emitted a non-file source: {events:?}"
            );
            let _ = std::fs::remove_file(&p);
        });
    }

    #[test]
    fn file_probe_follow_end_skips_existing_then_picks_up_appends() {
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("follow-end");
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "ERROR before-start").unwrap(); // skipped
            }
            let rings = Arc::new(ContextRingManager::new());
            let sifter = Arc::new(SifterRuntime::build(&[rule_error()]).unwrap());
            let cfg = FileProbeConfig::follow_end(p.clone(), BucketId::new());
            let (mut probe, _sink) = spawn_with_sink(cfg, rings, sifter).unwrap();

            tokio::time::sleep(Duration::from_millis(300)).await; // let probe seek to end
            {
                let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
                writeln!(f, "ERROR after-start").unwrap();
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            probe.cancel();
            let _ = probe.wait().await;
            let m = probe.metrics();
            assert!(m.frames_total >= 1, "should pick up appended line");
            assert!(m.events_emitted >= 1);
            let _ = std::fs::remove_file(&p);
        });
    }

    #[test]
    fn file_probe_watch_for_create() {
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("watch-create");
            let _ = std::fs::remove_file(&p);
            let rings = Arc::new(ContextRingManager::new());
            let sifter = Arc::new(SifterRuntime::build(&[rule_error()]).unwrap());
            let cfg = FileProbeConfig::follow_beginning(p.clone(), BucketId::new());
            let (mut probe, _sink) = spawn_with_sink(cfg, rings, sifter).unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "ERROR new").unwrap();
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            probe.cancel();
            let _ = probe.wait().await;
            let m = probe.metrics();
            assert!(
                m.frames_total >= 1,
                "create-after-start should be picked up"
            );
            let _ = std::fs::remove_file(&p);
        });
    }

    #[test]
    fn file_probe_truncation_detected() {
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("truncate");
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "ERROR before").unwrap();
            }
            let rings = Arc::new(ContextRingManager::new());
            let sifter = Arc::new(SifterRuntime::build(&[rule_error()]).unwrap());
            let cfg = FileProbeConfig::follow_beginning(p.clone(), BucketId::new());
            let (mut probe, _sink) = spawn_with_sink(cfg, rings, sifter).unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            // Truncate the file in place to zero bytes.
            std::fs::File::create(&p).unwrap();
            tokio::time::sleep(Duration::from_millis(600)).await;
            probe.cancel();
            let _ = probe.wait().await;
            let m = probe.metrics();
            assert!(
                m.truncations_detected >= 1,
                "truncations: {}",
                m.truncations_detected
            );
            let _ = std::fs::remove_file(&p);
        });
    }

    #[test]
    fn file_probe_nonzero_shrink_counts_as_truncation_not_rotation() {
        // L2: a truncate-then-write that leaves a SMALL NON-ZERO size is
        // an in-place truncation, not a rotation. `set_len` shrinks the
        // file in place (same inode, no 0-byte transition), so the probe
        // observes a direct large -> small-non-zero step. The old code
        // counted any non-zero shrink as a rotation; it must now be a
        // truncation, and never a rotation (identity is unchanged).
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("nonzero-shrink");
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "ERROR a fairly long first line of content").unwrap();
            }
            let rings = Arc::new(ContextRingManager::new());
            let sifter = Arc::new(SifterRuntime::build(&[rule_error()]).unwrap());
            let cfg = FileProbeConfig::follow_beginning(p.clone(), BucketId::new());
            let (mut probe, _sink) = spawn_with_sink(cfg, rings, sifter).unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            // Shrink in place to a non-zero size via a single ftruncate
            // (no intermediate 0-byte state, same inode).
            {
                let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
                f.set_len(8).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(600)).await;
            probe.cancel();
            let _ = probe.wait().await;
            let m = probe.metrics();
            assert!(
                m.truncations_detected >= 1,
                "a non-zero in-place shrink must be a truncation; truncations: {}",
                m.truncations_detected
            );
            assert_eq!(
                m.rotations_detected, 0,
                "a same-inode in-place shrink must NOT be a rotation; rotations: {}",
                m.rotations_detected
            );
            let _ = std::fs::remove_file(&p);
        });
    }

    #[cfg(unix)]
    #[test]
    fn file_probe_rotation_same_or_larger_size_detected_via_identity() {
        // H2: replace the watched file with a NEW inode whose size is >=
        // the old size (an atomic-rename / logrotate `create`). A size
        // comparison alone misses this (size did not shrink), so without
        // identity tracking the probe would seek to the old `pos` in the
        // NEW file, drop its leading bytes, and emit a corrupt mid-line
        // fragment. With identity tracking the rotation is detected, `pos`
        // resets to 0, and the new file's FIRST line (which contains a
        // keyword living entirely within the dropped region) is read
        // cleanly and fires an event.
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("rotate-identity");
            let dir = p.parent().unwrap().to_path_buf();
            // First-generation file: read fully so `pos` advances to its
            // end. Does NOT contain the SECONDGEN keyword.
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "first generation only, no marker here at all").unwrap();
            }
            let old_size = std::fs::metadata(&p).unwrap().len();

            let rings = Arc::new(ContextRingManager::new());
            // Rule keyed to a token that exists ONLY at the very start of
            // the NEW file's first line — inside the byte range the buggy
            // mid-file seek would drop.
            let rule = {
                let mut r = rule_error();
                r.id = "test.secondgen".to_owned();
                r.event_kind = "kw_secondgen".to_owned();
                r.keywords = Some(vec!["SECONDGEN".to_owned()]);
                r
            };
            let sifter = Arc::new(SifterRuntime::build(&[rule]).unwrap());
            let cfg = FileProbeConfig::follow_beginning(p.clone(), BucketId::new());
            let (mut probe, sink) = spawn_with_sink(cfg, rings, sifter).unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;

            // Build a replacement file (NEW inode) whose size is >= the
            // old size, with SECONDGEN at the head, then atomically rename
            // it over the watched path.
            let new_path = dir.join(format!("tc-rotate-new-{}", std::process::id()));
            {
                let mut f = std::fs::File::create(&new_path).unwrap();
                let mut line = String::from("SECONDGEN marker leads the new file");
                // Pad so the new file is at least as large as the old one,
                // ensuring `size < prev` is FALSE (the H2 case).
                while (line.len() as u64) < old_size + 8 {
                    line.push('x');
                }
                writeln!(f, "{line}").unwrap();
            }
            std::fs::rename(&new_path, &p).unwrap();

            tokio::time::sleep(Duration::from_millis(700)).await;
            probe.cancel();
            let _ = probe.wait().await;

            let m = probe.metrics();
            assert!(
                m.rotations_detected >= 1,
                "same-or-larger-size rotation must be detected via identity; rotations: {}",
                m.rotations_detected
            );
            let drained = sink.drain();
            assert!(
                drained.iter().any(|d| d.kind == "kw_secondgen"),
                "the new file's leading line must be read from the head (no dropped \
                 bytes / corrupt fragment); emitted kinds: {:?}",
                drained.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>()
            );
            let _ = std::fs::remove_file(&p);
        });
    }

    // ---- US3b (T037): notify backend + 9P poll-fallback selection ----

    #[test]
    fn backend_selection_9p_mountinfo_selects_poll() {
        // T037(b): the WSL 9P fixture must select the POLL fallback. We
        // cannot mount a real 9P filesystem in CI, so we assert the
        // backend-SELECTION decision against the recorded mountinfo
        // fixture -- the same detection the daemon wires in `start`.
        let mountinfo =
            include_str!("../../../tests/fixtures/probes/wsl-mountinfo/wsl2-9p-drvfs.mountinfo");
        // A path under /mnt/c (a `9p drvfs` mount in the fixture) -> Poll.
        assert_eq!(
            backend_from_mountinfo(mountinfo, std::path::Path::new("/mnt/c/Users/dev/app.log")),
            FileWatchBackend::Poll,
            "a /mnt/c path on the 9p fixture must fall back to polling"
        );
        // The 9P root `/` in this fixture is also `9p` -> Poll.
        assert_eq!(
            backend_from_mountinfo(mountinfo, std::path::Path::new("/home/dev/native.log")),
            FileWatchBackend::Poll,
            "a path on the 9p root mount must fall back to polling"
        );
    }

    #[test]
    fn backend_selection_native_mountinfo_selects_notify() {
        // T037(b) negative: native ext4 / tmpfs roots must select the
        // event-driven notify backend.
        let native =
            include_str!("../../../tests/fixtures/probes/wsl-mountinfo/native-linux.mountinfo");
        assert_eq!(
            backend_from_mountinfo(native, std::path::Path::new("/home/dev/app.log")),
            FileWatchBackend::Notify,
            "an ext4 path must use the event-driven backend"
        );
        let ext4 = include_str!("../../../tests/fixtures/probes/wsl-mountinfo/wsl2-ext4.mountinfo");
        assert_eq!(
            backend_from_mountinfo(ext4, std::path::Path::new("/home/dev/projects/app.log")),
            FileWatchBackend::Notify,
            "a WSL2 ext4 (Linux-native) path must use the event-driven backend"
        );
    }

    #[test]
    fn backend_selection_longest_prefix_not_naive_substring() {
        // `/mnt/c` (9p) must not steal a `/mnt/c2` path (a hypothetical
        // sibling mount). The component-aware prefix match guards this.
        let mountinfo = "\
    1 0 0:1 / / rw - ext4 /dev/root rw
    2 1 0:2 / /mnt/c rw - 9p drvfs rw
    3 1 0:3 / /mnt/c2 rw - ext4 /dev/sdc rw
    ";
        assert_eq!(
            backend_from_mountinfo(mountinfo, std::path::Path::new("/mnt/c2/app.log")),
            FileWatchBackend::Notify,
            "/mnt/c2 (ext4) must not be captured by the /mnt/c (9p) mount"
        );
        assert_eq!(
            backend_from_mountinfo(mountinfo, std::path::Path::new("/mnt/c/app.log")),
            FileWatchBackend::Poll,
            "/mnt/c (9p) itself must still select poll"
        );
    }

    #[test]
    fn notify_backend_picks_up_native_append() {
        // T037(a): a change on a NATIVE filesystem must yield a prompt
        // event through the event-driven `notify` backend -- no dependence
        // on the slow poll interval. The probe is spawned with
        // `backend = Notify` and a deliberately LONG poll_interval so a
        // pass proves the notify path (not a poll tick) drove the read.
        let runtime = rt();
        runtime.block_on(async {
            let p = temp_path("notify-append");
            let _ = std::fs::remove_file(&p);
            {
                let mut f = std::fs::File::create(&p).unwrap();
                writeln!(f, "ERROR before-start").unwrap(); // skipped (FollowEnd)
            }
            let rings = Arc::new(ContextRingManager::new());
            let sifter = Arc::new(SifterRuntime::build(&[rule_error()]).unwrap());
            let mut cfg = FileProbeConfig::follow_end(p.clone(), BucketId::new());
            cfg.backend = FileWatchBackend::Notify;
            // Long poll interval: if this test passes, the notify watcher
            // (not a poll tick) is what woke the probe.
            cfg.poll_interval = Duration::from_secs(30);
            let (mut probe, _sink) = spawn_with_sink(cfg, rings, sifter).unwrap();

            // Let the probe seek to end and arm the directory watch.
            tokio::time::sleep(Duration::from_millis(300)).await;
            {
                let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
                writeln!(f, "ERROR after-start").unwrap();
                f.flush().unwrap();
            }
            // Poll the probe metrics for up to ~3s; a notify-driven read
            // completes in well under that. Far below the 30s poll tick and
            // the nextest 5-min terminate budget, so a real failure fails
            // fast instead of hanging.
            let mut picked_up = false;
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if probe.metrics().frames_total >= 1 {
                    picked_up = true;
                    break;
                }
            }
            probe.cancel();
            let _ = probe.wait().await;
            let m = probe.metrics();
            assert!(
                picked_up && m.events_emitted >= 1,
                "event-driven notify backend must pick up a native-fs append \
                 without waiting out the poll interval; frames={}, events={}",
                m.frames_total,
                m.events_emitted
            );
            let _ = std::fs::remove_file(&p);
        });
    }
}
