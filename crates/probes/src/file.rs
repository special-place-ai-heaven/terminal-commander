// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! File probe (TC18). Polls a file path, handles create-after-start,
//! truncation, and rotation; emits normalized SourceFrame instances
//! to the sifter runtime via the same EventSink as the process probe.
//!
//! Source-status: live (TC18) using a portable polling
//! implementation. The notify/notify-debouncer-full path noted in
//! the TC18 mini-spec is deferred to a follow-up.

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

/// Default polling interval.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);

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

#[allow(clippy::too_many_arguments)]
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
    let mut noise_pipeline = ProbeNoisePipeline::with_default_policy();

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
                let size = f.metadata().await?.len();
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
                // Truncation / rotation detection: size decreased.
                if let Some(prev) = last_size
                    && size < prev
                {
                    let mut m = metrics.lock();
                    if size == 0 {
                        m.truncations_detected = m.truncations_detected.saturating_add(1);
                    } else {
                        m.rotations_detected = m.rotations_detected.saturating_add(1);
                    }
                    pos = 0;
                }
                last_size = Some(size);

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
        // Wait for the next poll tick or cancellation.
        tokio::select! {
            () = tokio::time::sleep(config.poll_interval) => {}
            _ = &mut cancel_rx => return Err(FileProbeError::Cancelled),
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
            assert!(!sink.is_empty());
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
}
