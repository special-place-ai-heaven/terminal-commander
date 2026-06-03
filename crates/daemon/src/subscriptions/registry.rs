// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Bounded, in-memory [`SubscriptionRegistry`] (subscriptions §2).
//!
//! Holds every open subscription for the daemon session, keyed by the OPAQUE
//! per-open `sub_id`. Bounded by [`MAX_SUBSCRIPTIONS`]: opening beyond it
//! returns [`IpcErrorCode::SubscriptionLimitExceeded`]. Reset wholesale on
//! daemon restart (registry + buckets + offsets reset together — the
//! `boot_id` / `UnknownSubscription` pair is the restart signal).
//!
//! Consumer isolation (spec C1, AC8): two opens with an identical predicate
//! mint DISTINCT `sub_id`s with INDEPENDENT offset maps, so one consumer's
//! pull never advances another's cursor.

use std::collections::HashMap;
use std::time::Instant;

use parking_lot::RwLock;
use terminal_commander_core::BucketId;
use terminal_commander_ipc::{IpcError, IpcErrorCode, MAX_SUBSCRIPTIONS};
use uuid::Uuid;

use super::model::{Predicate, Subscription};

/// A bounded snapshot row describing one open subscription. Daemon-internal
/// (the wire `SubscriptionSummary` is Task 9, out of this batch).
#[derive(Debug, Clone)]
pub struct SubscriptionSummary {
    /// Opaque per-open handle.
    pub sub_id: Uuid,
    /// Normalized predicate hash.
    pub predicate_hash: u64,
    /// The predicate (for display / re-recognition).
    pub predicate: Predicate,
    /// Number of buckets this subscription currently tracks an offset for.
    pub source_count: usize,
    /// When the subscription was opened.
    pub created_at: Instant,
    /// When the subscription was last pulled.
    pub last_pull_at: Option<Instant>,
}

/// In-memory registry of open subscriptions.
#[derive(Debug, Default)]
pub struct SubscriptionRegistry {
    subs: RwLock<HashMap<Uuid, Subscription>>,
}

impl SubscriptionRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new subscription with a freshly-minted opaque `sub_id` and the
    /// given initial offsets (from-now tails for already-in-scope buckets).
    ///
    /// # Errors
    /// Returns [`IpcErrorCode::SubscriptionLimitExceeded`] if the registry is
    /// already at [`MAX_SUBSCRIPTIONS`].
    pub fn open(
        &self,
        predicate: Predicate,
        initial_offsets: HashMap<BucketId, u64>,
    ) -> Result<Uuid, IpcError> {
        let mut guard = self.subs.write();
        if guard.len() >= MAX_SUBSCRIPTIONS {
            return Err(IpcError::new(
                IpcErrorCode::SubscriptionLimitExceeded,
                format!(
                    "subscription registry full ({MAX_SUBSCRIPTIONS} open); close one and retry"
                ),
            ));
        }
        let sub = Subscription::new(predicate, initial_offsets);
        let id = sub.sub_id;
        guard.insert(id, sub);
        Ok(id)
    }

    /// Close a subscription. Returns `true` if it existed and was removed.
    pub fn close(&self, id: Uuid) -> bool {
        self.subs.write().remove(&id).is_some()
    }

    /// Current number of open subscriptions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.subs.read().len()
    }

    /// Whether the registry holds no subscriptions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.subs.read().is_empty()
    }

    /// Snapshot all open subscriptions as bounded summary rows.
    #[must_use]
    pub fn list(&self) -> Vec<SubscriptionSummary> {
        self.subs
            .read()
            .values()
            .map(|s| SubscriptionSummary {
                sub_id: s.sub_id,
                predicate_hash: s.predicate_hash,
                predicate: s.predicate.clone(),
                source_count: s.offsets.len(),
                created_at: s.created_at,
                last_pull_at: s.last_pull_at,
            })
            .collect()
    }

    /// Read a subscription under the lock and copy out a value derived from it.
    ///
    /// # Errors
    /// [`IpcErrorCode::UnknownSubscription`] if the id is not present.
    pub fn with_sub<R>(&self, id: Uuid, f: impl FnOnce(&Subscription) -> R) -> Result<R, IpcError> {
        let guard = self.subs.read();
        guard.get(&id).map(f).ok_or_else(|| unknown(id))
    }

    /// Mutate a subscription under the write lock and return a value.
    ///
    /// # Errors
    /// [`IpcErrorCode::UnknownSubscription`] if the id is not present.
    pub fn with_sub_mut<R>(
        &self,
        id: Uuid,
        f: impl FnOnce(&mut Subscription) -> R,
    ) -> Result<R, IpcError> {
        let mut guard = self.subs.write();
        guard.get_mut(&id).map(f).ok_or_else(|| unknown(id))
    }
}

/// Build the typed `UnknownSubscription` error for a missing `sub_id`.
fn unknown(id: Uuid) -> IpcError {
    IpcError::new(
        IpcErrorCode::UnknownSubscription,
        format!("unknown subscription: {id}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscriptions::model::SourceSel;

    fn all_predicate() -> Predicate {
        Predicate {
            severity_min: None,
            kind: None,
            sources: SourceSel::All,
            tag: None,
        }
    }

    #[test]
    fn two_opens_same_predicate_get_distinct_ids_and_independent_offsets() {
        let reg = SubscriptionRegistry::new();
        let b = BucketId::new();
        let a_id = reg.open(all_predicate(), HashMap::new()).unwrap();
        let b_id = reg.open(all_predicate(), HashMap::new()).unwrap();
        assert_ne!(a_id, b_id, "AC8: distinct opaque sub_ids");

        // Advance A's offset for bucket `b`; B must not see it.
        reg.with_sub_mut(a_id, |s| {
            s.offsets.insert(b, 42);
        })
        .unwrap();
        let a_off = reg.with_sub(a_id, |s| s.offsets.get(&b).copied()).unwrap();
        let b_off = reg.with_sub(b_id, |s| s.offsets.get(&b).copied()).unwrap();
        assert_eq!(a_off, Some(42), "A's offset advanced");
        assert_eq!(b_off, None, "AC8: B's offset is independent (untouched)");

        // Same predicate -> same predicate_hash, distinct handles.
        let a_hash = reg.with_sub(a_id, |s| s.predicate_hash).unwrap();
        let b_hash = reg.with_sub(b_id, |s| s.predicate_hash).unwrap();
        assert_eq!(
            a_hash, b_hash,
            "identical predicates share a predicate_hash"
        );
    }

    #[test]
    fn open_beyond_cap_returns_limit_exceeded() {
        let reg = SubscriptionRegistry::new();
        for _ in 0..MAX_SUBSCRIPTIONS {
            reg.open(all_predicate(), HashMap::new()).unwrap();
        }
        let err = reg.open(all_predicate(), HashMap::new()).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::SubscriptionLimitExceeded);
        assert_eq!(reg.len(), MAX_SUBSCRIPTIONS, "cap not exceeded");
    }

    #[test]
    fn close_removes_and_reports_presence() {
        let reg = SubscriptionRegistry::new();
        let id = reg.open(all_predicate(), HashMap::new()).unwrap();
        assert!(reg.close(id), "close of a live sub returns true");
        assert!(!reg.close(id), "double close returns false");
        assert!(reg.is_empty());
    }

    #[test]
    fn with_sub_unknown_returns_unknown_subscription() {
        let reg = SubscriptionRegistry::new();
        let err = reg.with_sub(Uuid::new_v4(), |_| ()).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::UnknownSubscription);
        let err_mut = reg.with_sub_mut(Uuid::new_v4(), |_| ()).unwrap_err();
        assert_eq!(err_mut.code, IpcErrorCode::UnknownSubscription);
    }
}
