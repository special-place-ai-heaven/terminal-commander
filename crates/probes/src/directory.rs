// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Directory probe (TC20).
//!
//! Polls a directory at a configurable interval, emits structured
//! `DirectoryEvent`s for created / modified / deleted entries. Move
//! events are best-effort and may surface as delete+create on
//! polling-only filesystems.
//!
//! Artifact probes: a tiny JUnit-XML test-result summary is included
//! to satisfy TC20's acceptance criterion of "at least one artifact
//! summary." More formats land in follow-up goals.
//!
//! Source-status: live (TC20). Native inotify-based watching is
//! deferred to a follow-up POSIX harness.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::oneshot;

/// Default polling interval for the directory probe.
pub const DEFAULT_DIR_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Per-entry kind emitted by the directory probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryEventKind {
    Created,
    Modified,
    Deleted,
}

/// A single directory event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEvent {
    pub kind: DirectoryEventKind,
    pub path: PathBuf,
    pub size: u64,
}

/// Per-probe config.
#[derive(Debug, Clone)]
pub struct DirectoryProbeConfig {
    pub directory: PathBuf,
    pub poll_interval: Duration,
}

impl DirectoryProbeConfig {
    #[must_use]
    pub const fn watch(directory: PathBuf) -> Self {
        Self {
            directory,
            poll_interval: DEFAULT_DIR_POLL_INTERVAL,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoryProbeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("probe cancelled")]
    Cancelled,
}

/// Sink for directory events.
pub trait DirectorySink: Send + Sync + 'static {
    fn emit(&self, event: DirectoryEvent);
}

/// Default in-memory sink.
#[derive(Debug, Default, Clone)]
pub struct InMemoryDirectorySink {
    inner: Arc<Mutex<Vec<DirectoryEvent>>>,
}

impl InMemoryDirectorySink {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn drain(&self) -> Vec<DirectoryEvent> {
        std::mem::take(&mut *self.inner.lock())
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl DirectorySink for InMemoryDirectorySink {
    fn emit(&self, event: DirectoryEvent) {
        self.inner.lock().push(event);
    }
}

/// Directory probe handle.
#[derive(Debug)]
pub struct DirectoryProbe {
    cancel_tx: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<Result<(), DirectoryProbeError>>>,
}

impl DirectoryProbe {
    pub fn spawn(
        config: DirectoryProbeConfig,
        sink: Arc<dyn DirectorySink>,
    ) -> Result<Self, DirectoryProbeError> {
        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(run(config, sink, rx));
        Ok(Self {
            cancel_tx: Some(tx),
            join: Some(join),
        })
    }

    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
    }

    pub async fn wait(&mut self) -> Result<(), DirectoryProbeError> {
        let Some(j) = self.join.take() else {
            return Err(DirectoryProbeError::Cancelled);
        };
        match j.await {
            Ok(r) => r,
            Err(e) => Err(DirectoryProbeError::Io(std::io::Error::other(
                e.to_string(),
            ))),
        }
    }
}

async fn run(
    config: DirectoryProbeConfig,
    sink: Arc<dyn DirectorySink>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<(), DirectoryProbeError> {
    let mut prev: HashMap<PathBuf, u64> = HashMap::new();
    let mut first = true;
    loop {
        if cancel_rx.try_recv().is_ok() {
            return Err(DirectoryProbeError::Cancelled);
        }
        let mut now: HashMap<PathBuf, u64> = HashMap::new();
        if let Ok(rd) = std::fs::read_dir(&config.directory) {
            for entry in rd.flatten() {
                let path = entry.path();
                let size = entry.metadata().ok().map_or(0, |m| m.len());
                now.insert(path, size);
            }
        }
        if !first {
            for (path, size) in &now {
                match prev.get(path) {
                    None => sink.emit(DirectoryEvent {
                        kind: DirectoryEventKind::Created,
                        path: path.clone(),
                        size: *size,
                    }),
                    Some(prev_size) if prev_size != size => sink.emit(DirectoryEvent {
                        kind: DirectoryEventKind::Modified,
                        path: path.clone(),
                        size: *size,
                    }),
                    _ => {}
                }
            }
            for (path, prev_size) in &prev {
                if !now.contains_key(path) {
                    sink.emit(DirectoryEvent {
                        kind: DirectoryEventKind::Deleted,
                        path: path.clone(),
                        size: *prev_size,
                    });
                }
            }
        }
        first = false;
        prev = now;
        tokio::select! {
            () = tokio::time::sleep(config.poll_interval) => {}
            _ = &mut cancel_rx => return Err(DirectoryProbeError::Cancelled),
        }
    }
}

/// JUnit XML test-result summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JunitSummary {
    pub total: u32,
    pub failures: u32,
    pub errors: u32,
    pub skipped: u32,
}

impl JunitSummary {
    /// Best-effort parse of a JUnit-XML payload. We only read the
    /// attributes on the outer `testsuite` (or `testsuites`) element.
    #[must_use]
    pub fn parse(xml: &str) -> Self {
        let mut summary = Self::default();
        for attr in ["tests", "failures", "errors", "skipped"] {
            if let Some(value) = extract_attr(xml, "testsuites", attr)
                .or_else(|| extract_attr(xml, "testsuite", attr))
                .and_then(|s| s.parse::<u32>().ok())
            {
                match attr {
                    "tests" => summary.total = value,
                    "failures" => summary.failures = value,
                    "errors" => summary.errors = value,
                    "skipped" => summary.skipped = value,
                    _ => {}
                }
            }
        }
        summary
    }
}

fn extract_attr(xml: &str, element: &str, attr: &str) -> Option<String> {
    let needle_open = format!("<{element}");
    let start = xml.find(&needle_open)?;
    let end_of_open_tag = xml[start..].find('>')?;
    let header = &xml[start..start + end_of_open_tag];
    let attr_needle = format!(" {attr}=\"");
    let attr_start = header.find(&attr_needle)?;
    let value_start = attr_start + attr_needle.len();
    let after = &header[value_start..];
    let value_end = after.find('"')?;
    Some(after[..value_end].to_owned())
}

/// Convenience: spawn a probe with an in-memory sink for tests.
pub fn spawn_with_in_memory_sink(
    config: DirectoryProbeConfig,
) -> Result<(DirectoryProbe, Arc<InMemoryDirectorySink>), DirectoryProbeError> {
    let sink = Arc::new(InMemoryDirectorySink::new());
    let arc_dyn: Arc<dyn DirectorySink> = sink.clone();
    let probe = DirectoryProbe::spawn(config, arc_dyn)?;
    Ok((probe, sink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("tc-dir-probe-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn directory_probe_detects_created_modified_deleted() {
        let runtime = rt();
        runtime.block_on(async {
            let dir = temp_dir("crud");
            let mut cfg = DirectoryProbeConfig::watch(dir.clone());
            cfg.poll_interval = Duration::from_millis(80);
            let (mut probe, sink) = spawn_with_in_memory_sink(cfg).unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            let f = dir.join("a.txt");
            {
                let mut h = std::fs::File::create(&f).unwrap();
                writeln!(h, "hello").unwrap();
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            {
                let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
                writeln!(h, "world").unwrap();
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            std::fs::remove_file(&f).unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            probe.cancel();
            let _ = probe.wait().await;
            let events = sink.drain();
            assert!(
                events.iter().any(|e| e.kind == DirectoryEventKind::Created),
                "no created event"
            );
            assert!(
                events
                    .iter()
                    .any(|e| e.kind == DirectoryEventKind::Modified),
                "no modified event"
            );
            assert!(
                events.iter().any(|e| e.kind == DirectoryEventKind::Deleted),
                "no deleted event"
            );
            std::fs::remove_dir_all(&dir).ok();
        });
    }

    #[test]
    fn junit_summary_parses_testsuite() {
        let xml = r#"<?xml version="1.0"?>
            <testsuite tests="10" failures="2" errors="1" skipped="0">
              <testcase name="a"/>
            </testsuite>"#;
        let s = JunitSummary::parse(xml);
        assert_eq!(
            s,
            JunitSummary {
                total: 10,
                failures: 2,
                errors: 1,
                skipped: 0
            }
        );
    }

    #[test]
    fn junit_summary_parses_testsuites() {
        let xml = r#"<?xml version="1.0"?>
            <testsuites tests="20" failures="3" errors="0" skipped="1">
              <testsuite tests="20" failures="3" errors="0" skipped="1"/>
            </testsuites>"#;
        let s = JunitSummary::parse(xml);
        assert_eq!(
            s,
            JunitSummary {
                total: 20,
                failures: 3,
                errors: 0,
                skipped: 1
            }
        );
    }

    #[test]
    fn junit_summary_missing_attrs_defaults_to_zero() {
        let xml = "<testsuite tests=\"5\"/>";
        let s = JunitSummary::parse(xml);
        assert_eq!(s.total, 5);
        assert_eq!(s.failures, 0);
    }
}
