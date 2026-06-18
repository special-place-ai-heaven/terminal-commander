// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Process probe (TC15). Spawns a non-interactive command, reads
//! stdout + stderr concurrently line-by-line, normalizes to
//! `SourceFrame`s, feeds the sifter runtime, and emits `EventDraft`s
//! through an `EventSink`.
//!
//! Implementation note: MVP uses `tokio::process::Command` directly.
//! The child is made its own process-group leader (`process_group(0)`)
//! so cancellation can signal the whole tree.
//!
//! Cancellation is a GRACE LADDER (US3b / T042 / FR-015): SIGTERM the
//! child's process group, wait up to `ProcessProbeConfig::grace` for a
//! cooperative exit, then escalate to a process-group SIGKILL. On
//! Windows there is no SIGTERM equivalent for a Job Object, so the
//! forced `TerminateJobObject` is both steps (documented asymmetry --
//! see `terminate_process_tree_graceful`). The same ladder backs the
//! unix PTY probe's cancel, so command / PTY / shell-session stop share
//! one contract.
//!
//! Source-status: live (TC15) for non-interactive process probing and
//! event emission; grace-ladder cancel live on unix (US3b), Windows
//! forced-only by platform constraint. Job lifecycle + exit events in TC16.

pub mod ansi;
pub mod file;
pub mod noise_pipeline;
pub mod process;
pub mod pty;

pub use ansi::strip_ansi;
pub use file::{
    DEFAULT_POLL_INTERVAL, FileProbe, FileProbeConfig, FileProbeError, FileProbeMetrics,
    FileProbeMode, FileWatchBackend, backend_from_mountinfo, select_backend_for_path,
    spawn_with_sink as spawn_file_probe_with_sink,
};
pub use noise_pipeline::{
    PASSWORD_PROMPT_KIND, ProbeNoisePipeline, SharedProbeNoisePipeline, SuppressionCounter,
    SuppressionMetrics, password_prompt_draft,
};
pub use process::{
    DEFAULT_GRACE, EventSink, InMemorySink, ProcessProbe, ProcessProbeConfig, ProcessProbeError,
    ProcessProbeMetrics,
};
pub use pty::{AnsiNormalizer, PromptDetector, PromptKind};
// PTY probe surface is available on every host with a PTY backend (unix
// `pty-process` and Windows ConPTY via `portable-pty`).
#[cfg(any(unix, windows))]
pub use pty::{
    DEFAULT_PTY_GRACE, MAX_PTY_STDIN_BYTES, PtyExitOutcome, PtyProbe, PtyProbeConfig,
    PtyProbeError, PtyProbeMetrics, WriteStdinError,
};
