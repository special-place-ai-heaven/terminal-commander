// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Error type for `terminal-commander-core`.
//!
//! Source-status: live (TC06). Variants will grow as other goals
//! introduce constructors; downstream crates wrap or re-export.

use thiserror::Error;

/// Errors that can arise inside `terminal-commander-core`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    /// A typed identifier was constructed from an empty or
    /// otherwise unusable string.
    #[error("invalid identifier: {reason}")]
    InvalidId { reason: String },

    /// A typed identifier could not be parsed from a wire string
    /// (wrong prefix, malformed UUID, etc.).
    #[error("could not parse identifier '{value}': {reason}")]
    IdParse { value: String, reason: String },

    /// A severity string was not one of the canonical seven values.
    #[error(
        "unknown severity '{value}' (expected one of trace/debug/info/low/medium/high/critical)"
    )]
    UnknownSeverity { value: String },

    /// A `SignalEvent` was constructed without either a
    /// `SourcePointer` or a `pointer_unavailable_reason`.
    ///
    /// Enforces the TC02 invariant: every signal event preserves a
    /// bounded source pointer OR explains why no pointer can exist.
    #[error(
        "event '{event_id}' (severity {severity}) has neither pointer nor pointer_unavailable_reason"
    )]
    PointerInvariantViolation { event_id: String, severity: String },
}

/// Convenience alias for `Result<T, CoreError>`.
pub type Result<T> = core::result::Result<T, CoreError>;
