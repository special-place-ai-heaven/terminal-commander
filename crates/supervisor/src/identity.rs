// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Platform-neutral peer identity. Replaces the prior
// `PeerCred { uid: 0, gid: 0, pid: None }` constant on Windows.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PeerIdentity {
    Unix {
        uid: u32,
        gid: u32,
        pid: Option<i32>,
    },
    Windows {
        sid: String,
        pid: Option<u32>,
        image: Option<PathBuf>,
    },
    Unknown {
        reason: Option<String>,
    },
}

impl PeerIdentity {
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, PeerIdentity::Unknown { .. })
    }

    #[must_use]
    pub fn unknown() -> Self {
        PeerIdentity::Unknown { reason: None }
    }

    #[must_use]
    pub fn unknown_because(reason: impl Into<String>) -> Self {
        PeerIdentity::Unknown {
            reason: Some(reason.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_identity_is_not_known() {
        assert!(!PeerIdentity::unknown().is_known());
        assert!(!PeerIdentity::unknown_because("test reason").is_known());
    }

    #[test]
    fn unix_identity_is_known() {
        let id = PeerIdentity::Unix {
            uid: 1000,
            gid: 1000,
            pid: Some(42),
        };
        assert!(id.is_known());
    }

    #[test]
    fn windows_identity_is_known() {
        let id = PeerIdentity::Windows {
            sid: "S-1-5-21-1-2-3-1001".into(),
            pid: Some(42),
            image: None,
        };
        assert!(id.is_known());
    }

    #[test]
    fn round_trip_windows_with_image_path() {
        let id = PeerIdentity::Windows {
            sid: "S-1-5-21-1-2-3-1001".into(),
            pid: Some(42),
            image: Some(PathBuf::from(r"C:\Program Files\cursor\Cursor.exe")),
        };
        let s = serde_json::to_string(&id).unwrap();
        let back: PeerIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn round_trip_unix() {
        let id = PeerIdentity::Unix {
            uid: 1000,
            gid: 1000,
            pid: Some(42),
        };
        let s = serde_json::to_string(&id).unwrap();
        let back: PeerIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn round_trip_unknown_with_reason() {
        let id = PeerIdentity::unknown_because("GetTokenInformation failed");
        let s = serde_json::to_string(&id).unwrap();
        let back: PeerIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn round_trip_unknown_without_reason() {
        let id = PeerIdentity::unknown();
        let s = serde_json::to_string(&id).unwrap();
        let back: PeerIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }
}
