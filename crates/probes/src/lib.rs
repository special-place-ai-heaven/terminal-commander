// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Process probe (TC15). Spawns a non-interactive command, reads
//! stdout + stderr concurrently line-by-line, normalizes to
//! `SourceFrame`s, feeds the sifter runtime, and emits `EventDraft`s
//! through an `EventSink`.
//!
//! Implementation note: MVP uses `tokio::process::Command` directly.
//! `process-wrap`'s POSIX process-group integration is named in the
//! TC15 mini-spec; the swap is recorded in the goal-file decision
//! lock and deferred to a follow-up. Cancellation here is best-effort
//! via `Child::start_kill` (SIGKILL on Unix, TerminateProcess on
//! Windows); a graceful SIGTERM-first ladder lands when the
//! process-wrap swap happens.
//!
//! Source-status: live (TC15) for non-interactive process probing
//! and event emission. Job lifecycle + exit events land in TC16.

pub mod directory;
pub mod file;
pub mod process;
pub mod pty;

pub use directory::{
    DEFAULT_DIR_POLL_INTERVAL, DirectoryEvent, DirectoryEventKind, DirectoryProbe,
    DirectoryProbeConfig, DirectoryProbeError, DirectorySink, InMemoryDirectorySink, JunitSummary,
    spawn_with_in_memory_sink as spawn_directory_probe_with_sink,
};

pub use file::{
    DEFAULT_POLL_INTERVAL, FileProbe, FileProbeConfig, FileProbeError, FileProbeMetrics,
    FileProbeMode, spawn_with_sink as spawn_file_probe_with_sink,
};
pub use process::{
    DEFAULT_GRACE, EventSink, InMemorySink, ProcessProbe, ProcessProbeConfig, ProcessProbeError,
    ProcessProbeMetrics,
};
pub use pty::{AnsiNormalizer, PromptDetector, PromptKind};
#[cfg(unix)]
pub use pty::{
    DEFAULT_PTY_GRACE, MAX_PTY_STDIN_BYTES, PtyProbe, PtyProbeConfig, PtyProbeError,
    PtyProbeMetrics, WriteStdinError,
};
