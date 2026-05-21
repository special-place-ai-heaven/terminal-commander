// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! [`EventSource`] describes WHO produced a [`SignalEvent`].
//!
//! Wire shape lives in `tests/fixtures/contracts/event.signal.v1.json`
//! under the `source` field.
//!
//! Source-status: live (TC06).
//!
//! [`SignalEvent`]: crate::event::SignalEvent

use serde::{Deserialize, Serialize};

use crate::ids::{JobId, ProbeId};

/// Which kind of probe emitted a frame.
///
/// Mirrors `docs/contracts/enums/probe-kind.md`. The set is closed
/// for MVP; the `Other` variant accepts unknown wire values so
/// consumers stay forward-compatible.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Process,
    Terminal,
    File,
    Directory,
    Journal,
    Artifact,
    /// Unknown probe kind. Producers MUST NOT emit this variant; it
    /// exists only to let consumers parse forward-compatibly.
    #[serde(other)]
    Other,
}

/// Which stream within the source the frame came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceStream {
    Stdout,
    Stderr,
    /// Meta frames (lifecycle markers: command_started/exited).
    Meta,
    /// File or directory data stream.
    File,
    /// Anything not classifiable. Catch-all for forward compat.
    #[serde(other)]
    Other,
}

/// Describes the origin of a single signal event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSource {
    /// Probe that observed the frame.
    pub probe_id: ProbeId,
    /// Probe kind for forward-compatible filtering.
    pub source_type: SourceType,
    /// Which stream within the probe.
    pub stream: SourceStream,
    /// Job context (when the probe was watching a command). May be
    /// absent for file/directory probes that are not tied to a job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<JobId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProbeId;

    #[test]
    fn round_trip_process_event_source() {
        let src = EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::Process,
            stream: SourceStream::Stderr,
            job_id: Some(JobId::new()),
        };
        let j = serde_json::to_value(&src).unwrap();
        assert_eq!(j["source_type"], "process");
        assert_eq!(j["stream"], "stderr");
        assert!(j["probe_id"].as_str().unwrap().starts_with("prb_"));
        assert!(j["job_id"].as_str().unwrap().starts_with("job_"));
        let back: EventSource = serde_json::from_value(j).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn job_id_optional_when_absent() {
        let src = EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::File,
            stream: SourceStream::File,
            job_id: None,
        };
        let j = serde_json::to_string(&src).unwrap();
        assert!(!j.contains("job_id"), "absent job_id should not serialize");
        let back: EventSource = serde_json::from_str(&j).unwrap();
        assert_eq!(back.job_id, None);
    }

    #[test]
    fn unknown_source_type_parses_as_other() {
        let raw = serde_json::json!({
            "probe_id": ProbeId::new().to_wire_string(),
            "source_type": "alien",
            "stream": "stdout"
        });
        let parsed: EventSource = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed.source_type, SourceType::Other);
        assert_eq!(parsed.stream, SourceStream::Stdout);
    }
}
