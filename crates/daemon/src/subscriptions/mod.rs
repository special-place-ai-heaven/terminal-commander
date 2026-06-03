// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Predicate-routed Subscriptions (Phase 1).
//!
//! A subscription is ONE multiplexed, lossless, bounded consumer over many
//! buckets. The pieces:
//!
//! - [`source`] — a per-bucket source side-table written at `bucket_create`,
//!   the routing substrate (buckets carry no source identity of their own).
//! - [`model`] — the [`model::Predicate`] grammar + the per-open
//!   [`model::Subscription`] state (opaque `sub_id`, independent offsets).
//! - [`registry`] — the bounded in-memory [`registry::SubscriptionRegistry`]
//!   keyed by opaque `sub_id` (consumer isolation: same predicate, distinct
//!   cursors).
//! - [`pull`] — the lossless multiplexed read. Correctness rests on
//!   enroll-before-recheck: each in-scope bucket's `Notify` waiter is enrolled
//!   via `Notified::enable()` BEFORE any offset read, because the bucket signals
//!   with the permit-less `notify_waiters()`. The cursor/seq is the source of
//!   truth; `Notify` is only a latency hint.

pub mod model;
pub mod pull;
pub mod registry;
pub mod source;
