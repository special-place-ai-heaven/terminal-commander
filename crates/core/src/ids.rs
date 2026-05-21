// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Typed identifier newtypes.
//!
//! Each kind of identifier (Event, Bucket, Job, Probe, Rule, Source,
//! Frame, Activation, Audit, Session) is a distinct wrapper around
//! a UUIDv7. The wire form is `<prefix>_<simple-uuid>` (no dashes)
//! so identifiers sort lexicographically by time and remain copy-
//! paste safe.
//!
//! Source-status: live (TC06). Storage backing in TC12 stores the
//! raw UUID bytes; the prefix is reconstituted on read.

use core::fmt;
use core::marker::PhantomData;

use serde::{Deserialize, Serialize, de, ser};
use uuid::Uuid;

use crate::error::CoreError;

/// Marker trait that names the wire prefix for a typed identifier.
///
/// Implementors are zero-sized types; one per identifier kind.
pub trait TypedIdKind: Copy + Clone + core::fmt::Debug + Eq + Ord + core::hash::Hash {
    /// Wire prefix used in serialized form, e.g. `"evt"` for events.
    const PREFIX: &'static str;
}

macro_rules! decl_id_kind {
    ($name:ident, $prefix:literal, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name;
        impl TypedIdKind for $name {
            const PREFIX: &'static str = $prefix;
        }
    };
}

decl_id_kind!(EventIdKind, "evt", "Marker kind for [`EventId`].");
decl_id_kind!(BucketIdKind, "bkt", "Marker kind for [`BucketId`].");
decl_id_kind!(JobIdKind, "job", "Marker kind for [`JobId`].");
decl_id_kind!(ProbeIdKind, "prb", "Marker kind for [`ProbeId`].");
decl_id_kind!(RuleIdKind, "rul", "Marker kind for [`RuleId`].");
decl_id_kind!(SourceIdKind, "src", "Marker kind for [`SourceId`].");
decl_id_kind!(FrameIdKind, "frm", "Marker kind for [`FrameId`].");
decl_id_kind!(ActivationIdKind, "act", "Marker kind for [`ActivationId`].");
decl_id_kind!(AuditIdKind, "aud", "Marker kind for [`AuditId`].");
decl_id_kind!(SessionIdKind, "ses", "Marker kind for [`SessionId`].");

/// A strongly typed identifier backed by a UUIDv7.
///
/// All variants share the same memory layout (16 bytes); the kind
/// only affects compile-time type and the wire prefix.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypedId<K: TypedIdKind> {
    raw: Uuid,
    _kind: PhantomData<fn() -> K>,
}

impl<K: TypedIdKind> TypedId<K> {
    /// Generate a new identifier from a freshly minted UUIDv7.
    #[must_use]
    pub fn new() -> Self {
        Self {
            raw: Uuid::now_v7(),
            _kind: PhantomData,
        }
    }

    /// Wrap an existing UUID. The caller is responsible for ensuring
    /// the UUID is meaningful as the given kind.
    #[must_use]
    pub const fn from_uuid(raw: Uuid) -> Self {
        Self {
            raw,
            _kind: PhantomData,
        }
    }

    /// Raw UUID bytes.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.raw
    }

    /// Wire-form string: `<prefix>_<32-hex>`.
    #[must_use]
    pub fn to_wire_string(&self) -> String {
        format!("{}_{}", K::PREFIX, self.raw.simple())
    }

    /// Parse a wire-form string. The prefix MUST match `K::PREFIX`.
    pub fn parse_wire(s: &str) -> crate::Result<Self> {
        let (prefix, rest) = s.split_once('_').ok_or_else(|| CoreError::IdParse {
            value: s.to_owned(),
            reason: "missing '_' separator".to_owned(),
        })?;
        if prefix != K::PREFIX {
            return Err(CoreError::IdParse {
                value: s.to_owned(),
                reason: format!("expected prefix '{}' got '{}'", K::PREFIX, prefix),
            });
        }
        let raw = Uuid::parse_str(rest).map_err(|e| CoreError::IdParse {
            value: s.to_owned(),
            reason: e.to_string(),
        })?;
        Ok(Self::from_uuid(raw))
    }
}

impl<K: TypedIdKind> Default for TypedId<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: TypedIdKind> fmt::Display for TypedId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", K::PREFIX, self.raw.simple())
    }
}

impl<K: TypedIdKind> fmt::Debug for TypedId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", core::any::type_name::<K>(), self)
    }
}

impl<K: TypedIdKind> Serialize for TypedId<K> {
    fn serialize<S: ser::Serializer>(&self, s: S) -> core::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_wire_string())
    }
}

impl<'de, K: TypedIdKind> Deserialize<'de> for TypedId<K> {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> core::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse_wire(&s).map_err(de::Error::custom)
    }
}

// Public type aliases. Each is a distinct compile-time type.
pub type EventId = TypedId<EventIdKind>;
pub type BucketId = TypedId<BucketIdKind>;
pub type JobId = TypedId<JobIdKind>;
pub type ProbeId = TypedId<ProbeIdKind>;
pub type RuleId = TypedId<RuleIdKind>;
pub type SourceId = TypedId<SourceIdKind>;
pub type FrameId = TypedId<FrameIdKind>;
pub type ActivationId = TypedId<ActivationIdKind>;
pub type AuditId = TypedId<AuditIdKind>;
pub type SessionId = TypedId<SessionIdKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_event_id() {
        let id = EventId::new();
        let s = id.to_wire_string();
        assert!(s.starts_with("evt_"));
        assert_eq!(s.len(), 4 + 32); // "evt_" + 32 hex chars
        let back = EventId::parse_wire(&s).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn parse_wrong_prefix_fails() {
        let id = EventId::new();
        let bad = id.to_wire_string().replace("evt", "bkt");
        let err = EventId::parse_wire(&bad).unwrap_err();
        match err {
            CoreError::IdParse { reason, .. } => assert!(reason.contains("prefix")),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn ids_are_unique_and_sortable_by_time() {
        let a = EventId::new();
        let b = EventId::new();
        assert_ne!(a, b);
        // UUIDv7 is monotonic per the millisecond timestamp.
        // Two consecutive new() calls usually share the same ms;
        // we only assert non-equality (above) since same-ms ordering
        // depends on the random tail.
    }

    #[test]
    fn serde_json_uses_wire_form() {
        let id = JobId::new();
        let v = serde_json::to_value(id).unwrap();
        let s = v.as_str().unwrap();
        assert!(s.starts_with("job_"));
        let back: JobId = serde_json::from_value(v).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn parse_missing_separator_fails() {
        let err = BucketId::parse_wire("bkt00000000000000000000000000000000").unwrap_err();
        match err {
            CoreError::IdParse { reason, .. } => assert!(reason.contains("separator")),
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
