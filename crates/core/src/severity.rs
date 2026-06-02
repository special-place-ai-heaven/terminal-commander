// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Severity enum (seven canonical values).
//!
//! Wire form: lowercase snake_case strings exactly matching the
//! variant name (one word, no underscores). Numeric rank governs
//! ordering; consumers MUST sort by rank, not lexicographically.
//!
//! Source-status: live (TC06). See
//! `docs/contracts/enums/severity.md` for the canonical doctrine.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Severity of a [`SignalEvent`].
///
/// Total ordering is `Trace < Debug < Info < Low < Medium < High <
/// Critical`. Derived `PartialOrd`/`Ord` honor declaration order.
///
/// [`SignalEvent`]: crate::event::SignalEvent
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Diagnostic frames. Never a default for an event.
    Trace,
    /// Verbose detail for development.
    Debug,
    /// Routine informational events.
    Info,
    /// Informational events an operator may want to see.
    Low,
    /// Default for unrecognized errors.
    Medium,
    /// Things that usually need human follow-up.
    High,
    /// Things that have stopped progress.
    Critical,
}

impl Severity {
    /// Integer rank for sorting and SQLite ORDER BY storage.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Low => 3,
            Self::Medium => 4,
            Self::High => 5,
            Self::Critical => 6,
        }
    }

    /// Lowercase wire-form string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    /// Parse the wire-form string. Returns
    /// [`CoreError::UnknownSeverity`] for any unrecognized value.
    pub fn parse(value: &str) -> crate::Result<Self> {
        match value {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(CoreError::UnknownSeverity {
                value: other.to_owned(),
            }),
        }
    }
}

impl core::fmt::Display for Severity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_matches_doc_order() {
        let order = [
            Severity::Trace,
            Severity::Debug,
            Severity::Info,
            Severity::Low,
            Severity::Medium,
            Severity::High,
            Severity::Critical,
        ];
        for w in order.windows(2) {
            assert!(w[0].rank() < w[1].rank());
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn round_trip_serde() {
        let cases = [
            "trace", "debug", "info", "low", "medium", "high", "critical",
        ];
        for c in cases {
            let parsed = Severity::parse(c).unwrap();
            assert_eq!(parsed.as_str(), c);
            let json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, format!("\"{c}\""));
            let back: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, parsed);
        }
    }

    #[test]
    fn parse_unknown_errors_with_value() {
        let err = Severity::parse("PANIC").unwrap_err();
        match err {
            CoreError::UnknownSeverity { value } => assert_eq!(value, "PANIC"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn ordering_filters_by_minimum() {
        let events = [
            Severity::Low,
            Severity::High,
            Severity::Trace,
            Severity::Critical,
        ];
        let min = Severity::Medium;
        let kept: Vec<_> = events.iter().copied().filter(|s| *s >= min).collect();
        assert_eq!(kept, vec![Severity::High, Severity::Critical]);
    }
}
