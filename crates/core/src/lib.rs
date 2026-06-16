// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Terminal Commander core domain types.
//!
//! This crate holds the identifier, severity, event, source, and
//! pointer models that every other crate references. It performs
//! no I/O.
//!
//! Source-status (TC06): live for `Severity`, typed IDs,
//! `EventSource`, `SourcePointer`, `RuleRef`, and `SignalEvent`.
//! Serialization round-trips against the TC05 contract fixtures.
//! Storage, MCP, and probe wiring land in later goals (TC07+).

#![doc(html_no_source)]

pub mod activation;
pub mod bucket;
pub mod context;
pub mod environment;
pub mod error;
pub mod event;
pub mod ids;
pub mod job;
#[cfg(windows)]
pub mod platform;
pub mod pointer;
pub mod rule;
pub mod severity;
pub mod source;

pub use activation::ActivationScope;
pub use environment::EnvironmentSpec;
pub use job::{DEFAULT_JOB_GRACE, JobConfig, JobExitInfo, JobManager, JobRecord, JobState};
#[cfg(windows)]
pub use platform::{sanitize_wslenv, windows_silent, wslenv_overlay_value};

pub use bucket::{
    BucketConfig, BucketError, BucketManager, BucketReadRequest, BucketReadResponse, BucketState,
    BucketSummary, BucketWaitRequest, BucketWaitResponse, BySeverity, DEFAULT_MAX_EVENTS,
    DEFAULT_READ_LIMIT, DEFAULT_TTL, MAX_READ_LIMIT,
};
pub use context::{
    ContextError, ContextLine, ContextRingConfig, ContextRingManager, ContextWindowRequest,
    ContextWindowResponse, DEFAULT_RING_BYTES, DEFAULT_RING_FRAMES, MAX_FRAME_BYTES,
    MAX_WINDOW_BYTES, MAX_WINDOW_FRAMES, RingTail, SourceFrame,
};
pub use error::{CoreError, Result};
pub use event::{Captures, EventDraft, RuleRef, SignalEvent};
pub use ids::{
    ActivationId, AuditId, BucketId, EventId, FrameId, JobId, ProbeId, RuleId, SessionId, SourceId,
    TypedId,
};
pub use pointer::SourcePointer;
pub use rule::{
    CANONICAL_MATCH_KEY, ContextHint, MAX_CONTEXT_LINES, MAX_EXAMPLES,
    MAX_FULL_MATCH_SUMMARY_BYTES, MAX_PATTERN_BYTES, MAX_RULE_ID_BYTES, MAX_TAG_BYTES, MAX_TAGS,
    MAX_TEMPLATE_BYTES, REGEX_DFA_SIZE_LIMIT, REGEX_SIZE_LIMIT, RESERVED_MATCH_KEYS, RenderError,
    RenderedSummary, RuleDefinition, RuleError, RuleExample, RuleExampleExpect, RuleHandle,
    RuleStatus, RuleTestRequest, RuleTestResult, RuleType, clamp_full_match, compile_bounded_regex,
    compile_bounded_regex_set, is_reserved_match_key,
};
pub use severity::Severity;
pub use source::{EventSource, SourceStream, SourceType};
