// SPDX-License-Identifier: Apache-2.0
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

pub mod bucket;
pub mod context;
pub mod error;
pub mod event;
pub mod ids;
pub mod job;
pub mod pointer;
pub mod rule;
pub mod severity;
pub mod source;

pub use job::{DEFAULT_JOB_GRACE, JobConfig, JobExitInfo, JobManager, JobRecord, JobState};

pub use bucket::{
    BucketConfig, BucketError, BucketManager, BucketReadRequest, BucketReadResponse, BucketState,
    BucketSummary, BucketWaitRequest, BucketWaitResponse, BySeverity, DEFAULT_MAX_EVENTS,
    DEFAULT_READ_LIMIT, DEFAULT_TTL, MAX_READ_LIMIT,
};
pub use context::{
    ContextError, ContextLine, ContextRingConfig, ContextRingManager, ContextWindowRequest,
    ContextWindowResponse, DEFAULT_RING_BYTES, DEFAULT_RING_FRAMES, MAX_FRAME_BYTES,
    MAX_WINDOW_BYTES, MAX_WINDOW_FRAMES, SourceFrame,
};
pub use error::{CoreError, Result};
pub use event::{Captures, EventDraft, RuleRef, SignalEvent};
pub use ids::{
    ActivationId, AuditId, BucketId, EventId, FrameId, JobId, ProbeId, RuleId, SessionId, SourceId,
    TypedId,
};
pub use pointer::SourcePointer;
pub use rule::{
    ContextHint, MAX_CONTEXT_LINES, MAX_EXAMPLES, MAX_PATTERN_BYTES, MAX_RULE_ID_BYTES,
    MAX_TAG_BYTES, MAX_TAGS, MAX_TEMPLATE_BYTES, RenderError, RenderedSummary, RuleDefinition,
    RuleError, RuleExample, RuleExampleExpect, RuleHandle, RuleStatus, RuleTestRequest,
    RuleTestResult, RuleType,
};
pub use severity::Severity;
pub use source::{EventSource, SourceStream, SourceType};
