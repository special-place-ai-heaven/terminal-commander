// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon shell-exec runtime (TC49).
//!
//! [`ShellRuntime`] is a thin facade over [`CommandRuntime`]: it is the
//! daemon-level entry for the gated `shell_exec` lane. It runs ONE shell
//! line (pipelines / compounds / redirects) through the existing
//! comb/bucket pipeline, behind the `allow_shell` capability — without
//! weakening the argv-first default.
//!
//! Pipeline:
//!
//! ```text
//! ShellRuntime::exec
//!   -> validate shell_line (non-empty, <= MAX_SHELL_LINE_BYTES)
//!   -> resolve shell (req.shell or default_shell)
//!   -> argv = [shell, "-lc", shell_line]
//!   -> CommandRuntime::start_combed_shell   // StartLane::Shell
//!        -> SKIP the argv-lane shell-interpreter guard
//!        -> PolicyAction::CommandShellStart  // allow_shell cap gate
//!        -> command_shell_start audit row (redacted line)
//!        -> shared spawn core (bucket / probe / waiter)
//! ```
//!
//! The argv lane ([`CommandRuntime::start_combed`]) is UNCHANGED: it keeps
//! `SHELL_INTERPRETERS_DENY` as a hard deny. The shell lane assembles
//! `argv[0]` = the chosen interpreter ON PURPOSE and is gated instead by
//! [`PolicyAction::CommandShellStart`](crate::policy::PolicyAction).
//!
//! SYNC: [`CommandRuntime::start_combed_shell`] is synchronous (it never
//! awaits — the spawn closure is enqueued onto a `JoinSet`), so
//! [`ShellRuntime::exec`] is synchronous too. It MUST be called from
//! within a tokio runtime; `ProcessProbe::spawn` uses
//! `tokio::process::Command`. Async IPC / MCP handlers call `exec`
//! inline.

use std::path::PathBuf;
use std::sync::Arc;

use terminal_commander_core::{BucketConfig, RuleDefinition};

use crate::command::{CommandError, CommandRuntime, CommandStartRequest, CommandStartResponse};

/// Maximum byte length of a single `shell_line`.
///
/// Equal to [`MAX_ARGV_ITEM_BYTES`](crate::command::MAX_ARGV_ITEM_BYTES):
/// the shell lane assembles `argv = [shell, "-lc", shell_line]`, so
/// `shell_line` lands as `argv[2]` and is bounded by the same per-item
/// argv cap that [`CommandRuntime::start_combed_shell`] enforces via
/// `validate_argv`. A larger cap here would lie — `validate_argv` would
/// reject the oversize line as `ArgvItemTooLong { index: 2, .. }`. Raising
/// this later requires a lane-aware validator that exempts `argv[2]` under
/// the shell lane; that is an explicit follow-up, NOT TC49.
pub const MAX_SHELL_LINE_BYTES: usize = crate::command::MAX_ARGV_ITEM_BYTES;

/// Default shell used when the request does not name one.
///
/// `/bin/bash` on Unix (the platform the shell lane targets); a bare
/// `bash` elsewhere (resolved via `PATH`). The lane is Unix-first; a
/// non-Unix host without `bash` on `PATH` fails at spawn, not here.
#[must_use]
fn default_shell() -> String {
    if cfg!(unix) {
        "/bin/bash".to_owned()
    } else {
        "bash".to_owned()
    }
}

/// A daemon-level request to run ONE shell line through the comb pipeline.
///
/// Mirrors [`CommandStartRequest`] field-for-field where the lanes share
/// inputs, but takes a dedicated `shell_line` instead of a raw `argv`: the
/// runtime assembles `argv = [shell, "-lc", shell_line]` itself, so the
/// caller never hand-builds an interpreter argv (which the argv lane would
/// hard-deny). `shell` is the interpreter override; `None` resolves to
/// [`default_shell`].
#[derive(Debug, Clone)]
pub struct ShellExecRequest {
    /// The shell line to run (pipelines / compounds / redirects allowed).
    /// Becomes `argv[2]`; bounded by [`MAX_SHELL_LINE_BYTES`].
    pub shell_line: String,
    /// Interpreter override. `None` -> [`default_shell`].
    pub shell: Option<String>,
    /// Working directory for the spawned child. `None` inherits the
    /// daemon's cwd (the policy gate may reject paths outside the project
    /// root on containment profiles).
    pub cwd: Option<PathBuf>,
    /// Explicit environment to set on the child. Empty means inherit.
    pub env: Vec<(String, String)>,
    /// Optional rule set to bind for combing this job's output.
    pub rules: Vec<RuleDefinition>,
    /// Bucket config (max_events / TTL). Defaults applied if `None`.
    pub bucket_config: Option<BucketConfig>,
    /// Optional per-bucket tag for subscription routing.
    pub tag: Option<String>,
}

impl ShellExecRequest {
    /// Build a minimal request that runs `line` with the default shell and
    /// no overrides. Primarily a test/seam convenience; production callers
    /// construct the struct directly to set `cwd` / `env` / `rules`.
    #[must_use]
    pub fn line(line: impl Into<String>) -> Self {
        Self {
            shell_line: line.into(),
            shell: None,
            cwd: None,
            env: Vec::new(),
            rules: Vec::new(),
            bucket_config: None,
            tag: None,
        }
    }
}

/// Thin facade over [`CommandRuntime`] for the gated `shell_exec` lane.
///
/// Holds an `Arc<CommandRuntime>` and forwards to the shared spawn core via
/// [`CommandRuntime::start_combed_shell`]. Owns no probe / bucket state of
/// its own; the command runtime remains the single owner of the live job
/// table and the dedup guard.
#[derive(Debug, Clone)]
pub struct ShellRuntime {
    command: Arc<CommandRuntime>,
}

impl ShellRuntime {
    /// Wrap a shared [`CommandRuntime`].
    #[must_use]
    pub const fn new(command: Arc<CommandRuntime>) -> Self {
        Self { command }
    }

    /// Run ONE shell line through the comb pipeline behind the
    /// `allow_shell` capability.
    ///
    /// Validates `shell_line` (non-empty after trim, `<=`
    /// [`MAX_SHELL_LINE_BYTES`]), resolves the shell, assembles
    /// `argv = [shell, "-lc", shell_line]`, and forwards to
    /// [`CommandRuntime::start_combed_shell`] — which gates on
    /// [`PolicyAction::CommandShellStart`](crate::policy::PolicyAction)
    /// (denied by default) and, on allow, emits a `command_shell_start`
    /// audit row before spawning.
    ///
    /// SYNC: `start_combed_shell` never awaits, so the borrows it holds on
    /// `shell_line` / `shell` live only for this call. MUST be called from
    /// within a tokio runtime.
    ///
    /// # Errors
    /// - [`CommandError::EmptyArgv`] if `shell_line` is empty / whitespace.
    /// - [`CommandError::ArgvItemTooLong`] (`index: 0`) if `shell_line`
    ///   exceeds [`MAX_SHELL_LINE_BYTES`]. `index: 0` reports the
    ///   `shell_line` itself (the request's single user input), not its
    ///   downstream `argv[2]` position.
    /// - [`CommandError::PolicyDenied`] if the `allow_shell` cap is off or
    ///   the profile forbids shell.
    /// - any other [`CommandError`] propagated from the spawn core.
    pub fn exec(&self, req: ShellExecRequest) -> Result<CommandStartResponse, CommandError> {
        if req.shell_line.trim().is_empty() {
            return Err(CommandError::EmptyArgv);
        }
        if req.shell_line.len() > MAX_SHELL_LINE_BYTES {
            return Err(CommandError::ArgvItemTooLong {
                index: 0,
                len: req.shell_line.len(),
            });
        }

        let shell = req.shell.clone().unwrap_or_else(default_shell);
        let argv = vec![shell.clone(), "-lc".to_owned(), req.shell_line.clone()];

        let cmd_req = CommandStartRequest {
            argv,
            cwd: req.cwd,
            env: req.env,
            bucket_config: req.bucket_config,
            rules: req.rules,
            grace: None,
            tag: req.tag,
            dedup_nonce: None,
            peer_discriminator: None,
            // TC-B1: the shell lane is combed output too; strip color codes
            // so anchored rules match and summaries stay clean. Raw bytes
            // remain in the frame store.
            strip_ansi: true,
        };

        self.command
            .start_combed_shell(cmd_req, &req.shell_line, &shell)
    }
}
