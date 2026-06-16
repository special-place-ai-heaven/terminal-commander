// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Single-writer store actor (ROB-11, Findings #4 + #7).
//!
//! Replaces the `Arc<Mutex<EventStore>>` pattern with a dedicated OS
//! thread that owns the `EventStore`. All SQLite I/O runs on the
//! actor thread; callers communicate via a bounded `sync_channel` of
//! typed `StoreOp` requests and receive a typed reply over a one-shot
//! ack channel. The trait surface of [`crate::audit::AuditSink`]
//! does not change (ADR Option C — sync `emit`, internal mpsc).
//!
//! ## Why
//! - Eliminates Mutex convoy on shared `EventStore` (Finding #4).
//! - Gets blocking SQLite off the Tokio worker pool (Finding #7).
//! - Preserves read-your-writes semantics: callers block on the ack
//!   channel so subsequent calls see the result.
//!
//! ## Shutdown
//! [`StoreClient::shutdown`] sends `StoreOp::Shutdown`; the actor
//! thread drains all queued ops, runs `PRAGMA wal_checkpoint(FULL)`
//! via [`EventStore::wal_checkpoint_full`], and exits. After join,
//! every subsequent `call` returns
//! [`EventStoreError::Unavailable`] with the closed-channel
//! message.
//!
//! Production callers: [`crate::state::DaemonState::store`],
//! [`crate::audit::PersistentAudit`], IPC registry handlers, and
//! [`crate::runtime::run_ipc_server`] shutdown (WAL checkpoint).

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use parking_lot::Mutex;
use terminal_commander_core::{ActivationScope, RuleDefinition};
use terminal_commander_store::{
    ActiveRuleDef, AuditEntry, AuditReadRequest, AuditRow, EventStore, EventStoreError,
    ImportResult, RuleSearchHit,
};

/// Channel depth. Burst capacity before `call` blocks on `send`;
/// in steady state the actor thread drains within a microsecond.
pub const CHANNEL_CAPACITY: usize = 1024;

/// Typed request envelope. Every variant maps 1:1 to an
/// `EventStore` method that today lives behind `store.lock()`.
///
/// Variants intentionally own their input (`String`, etc.) so the
/// op can cross the thread boundary without lifetime gymnastics.
#[derive(Debug, Clone)]
pub enum StoreOp {
    /// `EventStore::ensure_audit()`
    EnsureAudit,
    /// `EventStore::ensure_registry()`
    EnsureRegistry,
    /// `EventStore::record_audit(&entry)` -> audit row id
    RecordAudit { entry: AuditEntry },
    /// `EventStore::audit_since(&request)` -> rows
    AuditSince { request: AuditReadRequest },
    /// `EventStore::audit_count()` -> count
    AuditCount,
    /// `EventStore::list_active_rule_defs_scoped()`
    ListActiveRuleDefsScoped,
    /// `EventStore::search_rules(&query, limit)` -> hits
    SearchRules { query: String, limit: Option<usize> },
    /// `EventStore::get_rule_version(rule_id, version)`
    GetRuleVersion { rule_id: String, version: u32 },
    /// `EventStore::get_latest_rule(rule_id)`
    GetLatestRule { rule_id: String },
    /// `EventStore::create_rule_version(&definition)` -> assigned version
    CreateRuleVersion { definition: RuleDefinition },
    /// `EventStore::record_activation_scoped(rule_id, version, scope, profile, actor)`
    RecordActivationScoped {
        rule_id: String,
        version: u32,
        scope: ActivationScope,
        profile: Option<String>,
        actor: Option<String>,
    },
    /// `EventStore::import_rule_pack_by_name(name, promote_active)`
    ImportRulePackByName { name: String, promote_active: bool },
    /// `EventStore::deactivate_rule_scoped(rule_id, version, scope)` -> bool
    DeactivateRuleScoped {
        rule_id: String,
        version: u32,
        scope: ActivationScope,
    },
    /// `EventStore::ensure_workspace()` (P1 / TC50)
    EnsureWorkspace,
    /// `EventStore::create_workspace_snapshot(...)` -> snapshot id
    CreateWorkspaceSnapshot {
        snapshot_id: String,
        name: Option<String>,
        source_session_id: Option<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
    },
    /// `EventStore::get_workspace_snapshot(snapshot_id)` -> optional row
    GetWorkspaceSnapshot { snapshot_id: String },
    /// Drain queue, run `wal_checkpoint(FULL)`, exit thread.
    Shutdown,
}

/// Typed reply matched to the sending `StoreOp` variant. The actor
/// thread guarantees the reply variant matches the request shape;
/// callers downcast via the `expect_*` helpers on [`StoreClient`].
#[derive(Debug)]
pub enum StoreReply {
    Unit,
    AuditId(u64),
    AuditRows(Vec<AuditRow>),
    AuditCount(u64),
    ActiveRuleDefs(Vec<ActiveRuleDef>),
    RuleSearchHits(Vec<RuleSearchHit>),
    OptionalRuleDef(Option<RuleDefinition>),
    Version(u32),
    Import(ImportResult),
    Bool(bool),
    SnapshotId(String),
    OptionalSnapshot(Option<terminal_commander_store::WorkspaceSnapshotRow>),
}

/// Internal envelope on the wire: an op plus the ack channel back
/// to the caller. `StoreReply` is wrapped in `Result` so the actor
/// thread can surface per-op errors without poisoning the channel.
struct StoreMsg {
    op: StoreOp,
    ack: SyncSender<Result<StoreReply, EventStoreError>>,
}

/// Handle to the writer thread. Cheap to clone (`Arc<Inner>`).
#[derive(Clone)]
pub struct StoreClient {
    inner: Arc<Inner>,
}

struct Inner {
    tx: SyncSender<StoreMsg>,
    /// `Some` until `shutdown` joins the thread. Behind a `Mutex`
    /// purely so `shutdown(&self)` (not `self`) can take the join
    /// handle exactly once.
    join: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for StoreClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreClient")
            .field("channel_capacity", &CHANNEL_CAPACITY)
            .finish()
    }
}

impl StoreClient {
    /// Open the DB at `path` in writer mode and spawn the store
    /// thread. The returned client is cloneable; every clone shares
    /// the same actor.
    pub fn open_writer(path: &Path) -> Result<Self, EventStoreError> {
        let mut store = EventStore::with_writer(path)?;
        // Apply the well-known migrations eagerly so the first real
        // call is not the one paying schema cost. Matches the
        // bootstrap discipline in `state.rs`.
        store.ensure_audit()?;
        store.ensure_registry()?;
        store.ensure_workspace()?;
        Self::spawn_with_store(store)
    }

    /// Test/internal: spawn directly over an already-opened store.
    /// Exposed `pub(crate)` so tests using `EventStore::open_writer`
    /// + temp dirs can construct a client without re-opening.
    pub(crate) fn spawn_with_store(store: EventStore) -> Result<Self, EventStoreError> {
        let (tx, rx) = sync_channel::<StoreMsg>(CHANNEL_CAPACITY);
        let join = thread::Builder::new()
            .name("tc-store-actor".to_owned())
            .spawn(move || actor_loop(store, rx))
            .map_err(|e| EventStoreError::Unavailable(format!("spawn store-actor thread: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner {
                tx,
                join: Mutex::new(Some(join)),
            }),
        })
    }

    /// Blocking RPC: send the op, wait for the ack. Returns the
    /// typed reply or the per-op error. Channel-closed surfaces as
    /// `EventStoreError::Unavailable`.
    pub fn call(&self, op: StoreOp) -> Result<StoreReply, EventStoreError> {
        let (ack_tx, ack_rx) = sync_channel::<Result<StoreReply, EventStoreError>>(1);
        self.inner
            .tx
            .send(StoreMsg { op, ack: ack_tx })
            .map_err(|_| {
                EventStoreError::Unavailable("store actor thread is no longer running".into())
            })?;
        ack_rx
            .recv()
            .map_err(|_| EventStoreError::Unavailable("store actor dropped reply channel".into()))?
    }

    /// Idempotent shutdown. Sends `Shutdown` (best-effort — the
    /// thread may already be exiting if the channel was closed),
    /// then joins. Safe to call exactly once per cloned-tree; later
    /// calls observe `join = None` and return `Ok(())` without
    /// re-sending.
    pub fn shutdown(&self) -> Result<(), EventStoreError> {
        let mut guard = self.inner.join.lock();
        let Some(handle) = guard.take() else {
            return Ok(());
        };
        // Best-effort: if the receiver was already dropped (e.g. via
        // explicit channel close in tests) the send fails silently.
        let (ack_tx, _ack_rx) = sync_channel::<Result<StoreReply, EventStoreError>>(1);
        let _ = self.inner.tx.send(StoreMsg {
            op: StoreOp::Shutdown,
            ack: ack_tx,
        });
        handle.join().map_err(|_| {
            EventStoreError::Unavailable("store actor thread panicked during shutdown".into())
        })?;
        Ok(())
    }

    /// Apply the registry migration eagerly (idempotent).
    pub fn ensure_registry(&self) -> Result<(), EventStoreError> {
        match self.call(StoreOp::EnsureRegistry)? {
            StoreReply::Unit => Ok(()),
            other => Err(unexpected_store_reply("EnsureRegistry", &other)),
        }
    }

    /// Count persisted audit rows.
    pub fn audit_count(&self) -> Result<u64, EventStoreError> {
        match self.call(StoreOp::AuditCount)? {
            StoreReply::AuditCount(n) => Ok(n),
            other => Err(unexpected_store_reply("AuditCount", &other)),
        }
    }

    /// Read audit rows since `request.cursor`.
    pub fn audit_since(
        &self,
        request: &AuditReadRequest,
    ) -> Result<Vec<AuditRow>, EventStoreError> {
        match self.call(StoreOp::AuditSince {
            request: request.clone(),
        })? {
            StoreReply::AuditRows(rows) => Ok(rows),
            other => Err(unexpected_store_reply("AuditSince", &other)),
        }
    }

    /// Active rule definitions with activation scope.
    pub fn list_active_rule_defs_scoped(&self) -> Result<Vec<ActiveRuleDef>, EventStoreError> {
        match self.call(StoreOp::ListActiveRuleDefsScoped)? {
            StoreReply::ActiveRuleDefs(defs) => Ok(defs),
            other => Err(unexpected_store_reply("ListActiveRuleDefsScoped", &other)),
        }
    }

    /// Full-text rule search.
    pub fn search_rules(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<RuleSearchHit>, EventStoreError> {
        match self.call(StoreOp::SearchRules {
            query: query.to_owned(),
            limit,
        })? {
            StoreReply::RuleSearchHits(hits) => Ok(hits),
            other => Err(unexpected_store_reply("SearchRules", &other)),
        }
    }

    /// Fetch a specific rule version.
    pub fn get_rule_version(
        &self,
        rule_id: &str,
        version: u32,
    ) -> Result<Option<RuleDefinition>, EventStoreError> {
        match self.call(StoreOp::GetRuleVersion {
            rule_id: rule_id.to_owned(),
            version,
        })? {
            StoreReply::OptionalRuleDef(def) => Ok(def),
            other => Err(unexpected_store_reply("GetRuleVersion", &other)),
        }
    }

    /// Fetch the latest rule version.
    pub fn get_latest_rule(
        &self,
        rule_id: &str,
    ) -> Result<Option<RuleDefinition>, EventStoreError> {
        match self.call(StoreOp::GetLatestRule {
            rule_id: rule_id.to_owned(),
        })? {
            StoreReply::OptionalRuleDef(def) => Ok(def),
            other => Err(unexpected_store_reply("GetLatestRule", &other)),
        }
    }

    /// Persist a new rule version.
    pub fn create_rule_version(&self, definition: &RuleDefinition) -> Result<u32, EventStoreError> {
        match self.call(StoreOp::CreateRuleVersion {
            definition: definition.clone(),
        })? {
            StoreReply::Version(v) => Ok(v),
            other => Err(unexpected_store_reply("CreateRuleVersion", &other)),
        }
    }

    /// Record a scoped activation row.
    pub fn record_activation_scoped(
        &self,
        rule_id: &str,
        version: u32,
        scope: ActivationScope,
        profile: Option<&str>,
        actor: Option<&str>,
    ) -> Result<(), EventStoreError> {
        match self.call(StoreOp::RecordActivationScoped {
            rule_id: rule_id.to_owned(),
            version,
            scope,
            profile: profile.map(str::to_owned),
            actor: actor.map(str::to_owned),
        })? {
            StoreReply::Unit => Ok(()),
            other => Err(unexpected_store_reply("RecordActivationScoped", &other)),
        }
    }

    /// Import a named rule pack.
    pub fn import_rule_pack_by_name(
        &self,
        pack: &str,
        activate: bool,
    ) -> Result<ImportResult, EventStoreError> {
        match self.call(StoreOp::ImportRulePackByName {
            name: pack.to_owned(),
            promote_active: activate,
        })? {
            StoreReply::Import(result) => Ok(result),
            other => Err(unexpected_store_reply("ImportRulePackByName", &other)),
        }
    }

    /// Deactivate a scoped rule; returns whether a row changed.
    pub fn deactivate_rule_scoped(
        &self,
        rule_id: &str,
        version: u32,
        scope: ActivationScope,
    ) -> Result<bool, EventStoreError> {
        match self.call(StoreOp::DeactivateRuleScoped {
            rule_id: rule_id.to_owned(),
            version,
            scope,
        })? {
            StoreReply::Bool(changed) => Ok(changed),
            other => Err(unexpected_store_reply("DeactivateRuleScoped", &other)),
        }
    }

    /// Ensure the workspace-snapshot schema (V0006) exists. Idempotent.
    pub fn ensure_workspace(&self) -> Result<(), EventStoreError> {
        match self.call(StoreOp::EnsureWorkspace)? {
            StoreReply::Unit => Ok(()),
            other => Err(unexpected_store_reply("EnsureWorkspace", &other)),
        }
    }

    /// Persist a workspace snapshot (P1 / TC50). Returns the snapshot id.
    pub fn create_workspace_snapshot(
        &self,
        snapshot_id: &str,
        name: Option<&str>,
        source_session_id: Option<&str>,
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> Result<String, EventStoreError> {
        match self.call(StoreOp::CreateWorkspaceSnapshot {
            snapshot_id: snapshot_id.to_owned(),
            name: name.map(ToOwned::to_owned),
            source_session_id: source_session_id.map(ToOwned::to_owned),
            cwd: cwd.map(ToOwned::to_owned),
            env: env.to_vec(),
        })? {
            StoreReply::SnapshotId(id) => Ok(id),
            other => Err(unexpected_store_reply("CreateWorkspaceSnapshot", &other)),
        }
    }

    /// Fetch a workspace snapshot by id. `Ok(None)` if unknown.
    pub fn get_workspace_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<terminal_commander_store::WorkspaceSnapshotRow>, EventStoreError> {
        match self.call(StoreOp::GetWorkspaceSnapshot {
            snapshot_id: snapshot_id.to_owned(),
        })? {
            StoreReply::OptionalSnapshot(row) => Ok(row),
            other => Err(unexpected_store_reply("GetWorkspaceSnapshot", &other)),
        }
    }
}

fn unexpected_store_reply(op: &str, reply: &StoreReply) -> EventStoreError {
    EventStoreError::Unavailable(format!(
        "store actor {op}: unexpected reply variant {reply:?}"
    ))
}

/// Owner of the `EventStore`. Runs on a dedicated OS thread.
///
/// Takes `rx` by value so the channel is dropped exactly when the
/// thread exits (closes any straggler senders on the way out).
#[allow(clippy::needless_pass_by_value)]
fn actor_loop(mut store: EventStore, rx: Receiver<StoreMsg>) {
    while let Ok(msg) = rx.recv() {
        if matches!(msg.op, StoreOp::Shutdown) {
            // Drain remaining queued ops so callers waiting on acks
            // see a definite reply instead of a closed channel.
            drain_remaining(&mut store, &rx);
            // Best-effort checkpoint. If it errors we still ack the
            // shutdown so the caller's join completes.
            let _ = store.wal_checkpoint_full();
            let _ = msg.ack.send(Ok(StoreReply::Unit));
            return;
        }
        let reply = execute_guarded(&mut store, msg.op);
        let _ = msg.ack.send(reply);
    }
    // Channel closed without explicit shutdown (every client
    // dropped). Run the same drain + checkpoint discipline so we
    // never lose pending writes.
    drain_remaining(&mut store, &rx);
    let _ = store.wal_checkpoint_full();
}

fn drain_remaining(store: &mut EventStore, rx: &Receiver<StoreMsg>) {
    while let Ok(msg) = rx.try_recv() {
        if matches!(msg.op, StoreOp::Shutdown) {
            let _ = msg.ack.send(Ok(StoreReply::Unit));
            continue;
        }
        let reply = execute_guarded(store, msg.op);
        let _ = msg.ack.send(reply);
    }
}

fn execute(store: &mut EventStore, op: StoreOp) -> Result<StoreReply, EventStoreError> {
    match op {
        StoreOp::EnsureAudit => store.ensure_audit().map(|()| StoreReply::Unit),
        StoreOp::EnsureRegistry => store.ensure_registry().map(|()| StoreReply::Unit),
        StoreOp::RecordAudit { entry } => store.record_audit(&entry).map(StoreReply::AuditId),
        StoreOp::AuditSince { request } => store.audit_since(&request).map(StoreReply::AuditRows),
        StoreOp::AuditCount => store.audit_count().map(StoreReply::AuditCount),
        StoreOp::ListActiveRuleDefsScoped => store
            .list_active_rule_defs_scoped()
            .map(StoreReply::ActiveRuleDefs),
        StoreOp::SearchRules { query, limit } => store
            .search_rules(&query, limit)
            .map(StoreReply::RuleSearchHits),
        StoreOp::GetRuleVersion { rule_id, version } => store
            .get_rule_version(&rule_id, version)
            .map(StoreReply::OptionalRuleDef),
        StoreOp::GetLatestRule { rule_id } => store
            .get_latest_rule(&rule_id)
            .map(StoreReply::OptionalRuleDef),
        StoreOp::CreateRuleVersion { definition } => store
            .create_rule_version(&definition)
            .map(StoreReply::Version),
        StoreOp::RecordActivationScoped {
            rule_id,
            version,
            scope,
            profile,
            actor,
        } => store
            .record_activation_scoped(
                &rule_id,
                version,
                scope,
                profile.as_deref(),
                actor.as_deref(),
            )
            .map(|()| StoreReply::Unit),
        StoreOp::ImportRulePackByName {
            name,
            promote_active,
        } => store
            .import_rule_pack_by_name(&name, promote_active)
            .map(StoreReply::Import),
        StoreOp::DeactivateRuleScoped {
            rule_id,
            version,
            scope,
        } => store
            .deactivate_rule_scoped(&rule_id, version, scope)
            .map(StoreReply::Bool),
        StoreOp::EnsureWorkspace => store.ensure_workspace().map(|()| StoreReply::Unit),
        StoreOp::CreateWorkspaceSnapshot {
            snapshot_id,
            name,
            source_session_id,
            cwd,
            env,
        } => store
            .create_workspace_snapshot(
                &snapshot_id,
                name.as_deref(),
                source_session_id.as_deref(),
                cwd.as_deref(),
                &env,
            )
            .map(StoreReply::SnapshotId),
        StoreOp::GetWorkspaceSnapshot { snapshot_id } => store
            .get_workspace_snapshot(&snapshot_id)
            .map(StoreReply::OptionalSnapshot),
        StoreOp::Shutdown => Ok(StoreReply::Unit),
    }
}

/// Panic-isolated wrapper around [`execute`]. A panic inside any
/// `EventStore` method would otherwise unwind out of [`actor_loop`] and
/// kill the single store-writer thread, turning every subsequent
/// [`StoreClient::call`] into an `Unavailable` error -- silently
/// disabling ALL persistence and audit for the daemon's life. Catching
/// the unwind converts one bad op into a typed error for that one caller
/// while the loop keeps serving. Any in-flight rusqlite transaction
/// borrows `&mut conn` and rolls back on Drop during the unwind, so the
/// connection remains usable.
fn execute_guarded(store: &mut EventStore, op: StoreOp) -> Result<StoreReply, EventStoreError> {
    guard_panic(|| execute(store, op))
}

/// Run a store closure with `catch_unwind`, converting a panic into a
/// typed [`EventStoreError::Unavailable`] (and logging it so the
/// otherwise-silent degradation is observable). Factored out from
/// [`execute_guarded`] so the isolation contract is unit-testable
/// without a real panicking store op.
fn guard_panic<F>(f: F) -> Result<StoreReply, EventStoreError>
where
    F: FnOnce() -> Result<StoreReply, EventStoreError>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(reply) => reply,
        Err(panic) => {
            let detail = panic_message(&*panic);
            eprintln!("terminal-commanderd: store op panicked, store actor survived: {detail}");
            Err(EventStoreError::Unavailable(format!(
                "store operation panicked: {detail}"
            )))
        }
    }
}

/// Best-effort extraction of a panic payload's message string.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use terminal_commander_store::AuditEntry;

    fn temp_db() -> tempfile::NamedTempFile {
        tempfile::Builder::new()
            .prefix("tc-store-actor-")
            .suffix(".db")
            .tempfile()
            .expect("temp file")
    }

    fn sample_entry(seq: usize) -> AuditEntry {
        let mut e = AuditEntry::new("rob-11_test", format!("subject-{seq}"), "allow");
        e.reason = Some(format!("seq={seq}"));
        e
    }

    #[test]
    fn open_writer_and_round_trip_record_audit() {
        let f = temp_db();
        let client = StoreClient::open_writer(f.path()).expect("open writer");

        let entry = sample_entry(1);
        let reply = client
            .call(StoreOp::RecordAudit { entry })
            .expect("record_audit");
        let id = match reply {
            StoreReply::AuditId(id) => id,
            other => panic!("unexpected reply: {other:?}"),
        };
        assert!(id >= 1, "audit id must be >= 1, got {id}");

        let count_reply = client.call(StoreOp::AuditCount).expect("audit_count");
        let count = match count_reply {
            StoreReply::AuditCount(n) => n,
            other => panic!("unexpected reply: {other:?}"),
        };
        assert_eq!(count, 1);

        client.shutdown().expect("shutdown");
    }

    #[test]
    fn audit_since_returns_rows_in_order() {
        let f = temp_db();
        let client = StoreClient::open_writer(f.path()).expect("open writer");

        for i in 1..=5 {
            let _ = client
                .call(StoreOp::RecordAudit {
                    entry: sample_entry(i),
                })
                .expect("record_audit");
        }

        let reply = client
            .call(StoreOp::AuditSince {
                request: AuditReadRequest::new(0),
            })
            .expect("audit_since");
        let rows = match reply {
            StoreReply::AuditRows(r) => r,
            other => panic!("unexpected reply: {other:?}"),
        };
        assert_eq!(rows.len(), 5, "expected 5 rows, got {}", rows.len());
        for (i, row) in rows.iter().enumerate() {
            let want = format!("subject-{}", i + 1);
            assert_eq!(row.subject, want, "row {i} subject mismatch");
        }

        client.shutdown().expect("shutdown");
    }

    #[test]
    fn call_after_shutdown_errors_cleanly() {
        let f = temp_db();
        let client = StoreClient::open_writer(f.path()).expect("open writer");
        client.shutdown().expect("shutdown");

        let err = client
            .call(StoreOp::AuditCount)
            .expect_err("call after shutdown must error");
        match err {
            // A closed channel after shutdown is a backend-unavailable
            // fault, not a caller-fixable InvalidPayload (so the IPC layer
            // reports Internal, not RuleInvalid).
            EventStoreError::Unavailable(msg) => {
                assert!(
                    msg.contains("store actor") || msg.contains("no longer running"),
                    "unexpected error message: {msg}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn concurrent_emit_no_deadlock_and_monotonic_ids() {
        // 10 threads x 100 emits each (issue's required regression).
        let f = temp_db();
        let client = StoreClient::open_writer(f.path()).expect("open writer");

        let n_threads = 10usize;
        let per_thread = 100usize;
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for t in 0..n_threads {
            let client = client.clone();
            let counter = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                let mut ids = Vec::with_capacity(per_thread);
                for i in 0..per_thread {
                    let seq = t * per_thread + i;
                    counter.fetch_add(1, Ordering::SeqCst);
                    let reply = client
                        .call(StoreOp::RecordAudit {
                            entry: sample_entry(seq),
                        })
                        .expect("record_audit in contention");
                    if let StoreReply::AuditId(id) = reply {
                        ids.push(id);
                    } else {
                        panic!("unexpected reply variant");
                    }
                }
                ids
            }));
        }
        let mut all_ids = Vec::new();
        for h in handles {
            all_ids.extend(h.join().expect("thread join"));
        }
        assert_eq!(
            all_ids.len(),
            n_threads * per_thread,
            "lost emits under contention"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            n_threads * per_thread,
            "counter mismatch"
        );

        // IDs strictly increasing when sorted (FIFO queue + single
        // writer guarantees monotonic assignment).
        all_ids.sort_unstable();
        for w in all_ids.windows(2) {
            assert!(w[0] < w[1], "audit ids not strictly increasing: {w:?}");
        }

        // Final count matches.
        let reply = client.call(StoreOp::AuditCount).expect("audit_count");
        if let StoreReply::AuditCount(n) = reply {
            assert_eq!(n, (n_threads * per_thread) as u64);
        } else {
            panic!("unexpected reply variant");
        }

        client.shutdown().expect("shutdown");
    }

    #[test]
    fn panicking_op_is_isolated_as_unavailable_not_thread_death() {
        // A panic inside a store op must be caught and surfaced as a
        // typed `Unavailable` error so the single-writer actor thread
        // survives instead of dying and disabling all persistence for
        // the daemon's life. Silence the default panic hook so the
        // deliberate panic does not spam test output.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let boom = guard_panic(|| panic!("boom-42"));
        let pass = guard_panic(|| Ok(StoreReply::Unit));
        std::panic::set_hook(prev);

        match boom {
            Err(EventStoreError::Unavailable(m)) => {
                assert!(m.contains("boom-42"), "panic detail must propagate: {m}");
            }
            other => panic!("a panicking op must yield Unavailable, got {other:?}"),
        }
        assert!(
            matches!(pass, Ok(StoreReply::Unit)),
            "a non-panicking op must pass through unchanged"
        );
    }
}
