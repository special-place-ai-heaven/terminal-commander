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
    Unknown,
}

impl PeerIdentity {
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, PeerIdentity::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_identity_is_not_known() {
        assert!(!PeerIdentity::Unknown.is_known());
    }

    #[test]
    fn unix_identity_is_known() {
        let id = PeerIdentity::Unix { uid: 1000, gid: 1000, pid: Some(42) };
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
    fn round_trip_through_serde_json() {
        let id = PeerIdentity::Windows {
            sid: "S-1-5-21-1-2-3-1001".into(),
            pid: Some(42),
            image: None,
        };
        let s = serde_json::to_string(&id).unwrap();
        let back: PeerIdentity = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }
}
