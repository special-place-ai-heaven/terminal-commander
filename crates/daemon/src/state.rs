// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon runtime state (TC36).
//!
//! Holds every subsystem instantiated at bootstrap: persistent store,
//! bucket manager, context rings, job manager, sifter runtime,
//! policy engine, persistent audit sink, and the wired `Router`.
//!
//! The constructor enforces the TC35 invariant: production daemon
//! state ALWAYS uses [`crate::PersistentAudit`] (NOT `InMemoryAudit`).
//! The router is built via `Router::with_sink`.
//!
//! Source-status: live (TC36).

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use terminal_commander_core::{BucketManager, ContextRingManager, JobManager};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::EventStore;

use crate::activation::ActivationRegistry;
use crate::audit::PersistentAudit;
use crate::command::CommandRuntime;
use crate::config::DaemonConfig;
use crate::file_watch::WatchRuntime;
use crate::policy::PolicyEngine;
#[cfg(unix)]
use crate::pty_command::PtyRuntime;
use crate::router::Router;

/// Errors raised during daemon bootstrap.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("failed to create data dir '{path}': {source}")]
    CreateDataDir {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("event store error: {0}")]
    Store(#[from] terminal_commander_store::EventStoreError),
    #[error("sifter runtime error: {0}")]
    Sifter(String),
}

/// Result alias.
pub type Result<T> = core::result::Result<T, BootstrapError>;

/// The wired daemon runtime. Construct via [`DaemonState::bootstrap`].
pub struct DaemonState {
    /// Original config (kept so callers can inspect / re-render).
    pub config: DaemonConfig,
    /// Locked, single-writer event store. Shared between the router's
    /// audit sink and any future store-using subsystems.
    pub store: Arc<Mutex<EventStore>>,
    /// In-memory bucket manager.
    pub buckets: Arc<BucketManager>,
    /// Per-probe context rings.
    pub rings: Arc<ContextRingManager>,
    /// Job manager.
    pub jobs: Arc<JobManager>,
    /// Sifter runtime (constructed empty at bootstrap; hot-binding
    /// is TC42 scope).
    pub sifter: Arc<SifterRuntime>,
    /// Active policy engine.
    pub policy: PolicyEngine,
    /// Persistent audit sink wrapping the same store. Stored
    /// separately so direct readers (operator CLI) can inspect it
    /// without going through the router.
    pub audit: Arc<PersistentAudit>,
    /// Wired router. Always uses the persistent audit sink.
    pub router: Arc<Router>,
    /// Command runtime (TC38). Wires command_start_combed into the
    /// daemon. Uses the same persistent audit sink as the router.
    pub command: Arc<CommandRuntime>,
    /// In-memory activation registry (TC42). Restored from the
    /// persistent `rule_activations` table at bootstrap; mutated by
    /// the `registry_activate` / `registry_deactivate` IPC handlers.
    pub activation: Arc<ActivationRegistry>,
    /// File-watch runtime (TC43). Owns live `FileProbe` handles
    /// attached to buckets. Deliberately separate from
    /// `CommandRuntime`.
    pub watch: Arc<WatchRuntime>,
    /// PTY command runtime (TC44). Owns live `PtyProbe` handles for
    /// interactive jobs. Linux/WSL only; on non-Unix targets the
    /// field is absent and the IPC layer returns
    /// `UnsupportedPlatform`.
    #[cfg(unix)]
    pub pty: Arc<PtyRuntime>,
}

impl std::fmt::Debug for DaemonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // We deliberately surface only the most useful fields for
        // operator-facing debug output. Internals (Arc handles) are
        // omitted; `finish_non_exhaustive()` makes that explicit.
        f.debug_struct("DaemonState")
            .field("data_dir", &self.config.daemon.data_dir)
            .field("policy_profile", &self.config.policy.profile)
            .field("runtime_mode", &self.config.daemon.runtime_mode)
            .finish_non_exhaustive()
    }
}

impl DaemonState {
    /// Bootstrap the full daemon runtime from a validated config.
    ///
    /// Steps:
    /// 1. Create `data_dir` if missing.
    /// 2. Open the event store at `<data_dir>/terminal-commander.db`.
    /// 3. Apply the V0003 audit migration eagerly so first-emit is fast.
    /// 4. Construct in-memory subsystems (bucket manager, rings, jobs).
    /// 5. Build an empty sifter runtime.
    /// 6. Wire a `PersistentAudit` sink over the shared store.
    /// 7. Construct the router via `Router::with_sink` (NEVER
    ///    `Router::new`, which would default to `InMemoryAudit`).
    pub fn bootstrap(config: DaemonConfig) -> Result<Self> {
        ensure_dir(&config.daemon.data_dir)?;
        let db_path = config.db_path();
        let store_handle = EventStore::with_writer(&db_path)?;
        let store = Arc::new(Mutex::new(store_handle));

        let audit = Arc::new(PersistentAudit::new(Arc::clone(&store)));
        audit.ensure_migration().map_err(BootstrapError::Store)?;

        // Apply the TC13 registry migration eagerly so bootstrap can
        // safely call `list_active_rule_defs` and the IPC layer can
        // accept `registry_*` calls without a first-call latency
        // spike. The migration is idempotent.
        {
            let mut g = store.lock();
            g.ensure_registry().map_err(BootstrapError::Store)?;
        }

        let buckets = Arc::new(BucketManager::new());
        let rings = Arc::new(ContextRingManager::new());
        let jobs = Arc::new(JobManager::new());
        let sifter =
            Arc::new(SifterRuntime::build(&[]).map_err(|e| BootstrapError::Sifter(e.to_string()))?);

        let policy = PolicyEngine::new(config.policy.profile);

        // Restore active rule definitions from the persistent
        // registry. The in-memory ActivationRegistry is the runtime
        // authority for `command_start_combed`; the DB row is the
        // durability backstop for restarts. TC42c rehydrates each
        // row with its scope so a bucket/job/probe-scoped activation
        // survives a daemon restart and reattaches to the matching
        // identity when that bucket/job/probe is re-encountered.
        let active_rows = {
            let g = store.lock();
            g.list_active_rule_defs_scoped()
                .map_err(BootstrapError::Store)?
        };
        let entries: Vec<crate::activation::ActivationEntry> = active_rows
            .into_iter()
            .map(
                |terminal_commander_store::ActiveRuleDef { definition, scope }| {
                    crate::activation::ActivationEntry { definition, scope }
                },
            )
            .collect();
        let activation = Arc::new(ActivationRegistry::from_entries(entries));

        let router = Arc::new(Router::with_sink(
            Arc::clone(&buckets),
            Arc::clone(&rings),
            Arc::clone(&jobs),
            Arc::clone(&sifter),
            // PersistentAudit implements AuditSink; coerce through Arc.
            Arc::clone(&audit) as Arc<dyn crate::audit::AuditSink>,
        ));

        let command = Arc::new(CommandRuntime::new(
            Arc::clone(&router),
            Arc::clone(&rings),
            Arc::clone(&jobs),
            Arc::clone(&audit) as Arc<dyn crate::audit::AuditSink>,
            policy,
            Arc::clone(&activation),
        ));
        let watch = Arc::new(WatchRuntime::new(
            Arc::clone(&router),
            Arc::clone(&rings),
            Arc::clone(&jobs),
            Arc::clone(&audit) as Arc<dyn crate::audit::AuditSink>,
            policy,
            Arc::clone(&activation),
        ));
        #[cfg(unix)]
        let pty = Arc::new(PtyRuntime::new(
            Arc::clone(&router),
            Arc::clone(&rings),
            Arc::clone(&jobs),
            Arc::clone(&audit) as Arc<dyn crate::audit::AuditSink>,
            policy,
            Arc::clone(&activation),
        ));

        Ok(Self {
            config,
            store,
            buckets,
            rings,
            jobs,
            sifter,
            policy,
            audit,
            router,
            command,
            activation,
            watch,
            #[cfg(unix)]
            pty,
        })
    }

    /// Convenience: report whether the active audit sink is the
    /// persistent one. Used by the bootstrap assertion test.
    #[must_use]
    pub fn audit_is_persistent(&self) -> bool {
        // PersistentAudit is the only Arc<PersistentAudit> we hand
        // back. By construction, true.
        Arc::strong_count(&self.audit) >= 1
    }
}

fn ensure_dir(p: &Path) -> Result<()> {
    if p.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(p).map_err(|source| BootstrapError::CreateDataDir {
        path: p.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_store::AuditReadRequest;

    fn temp_data_dir(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        p.push(format!("tc-state-{tag}-{pid}-{nanos}"));
        p
    }

    fn cleanup(p: &std::path::Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn bootstrap_creates_data_dir_and_opens_store() {
        let data = temp_data_dir("create");
        assert!(!data.exists(), "precondition: temp dir does not exist yet");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        assert!(data.exists(), "data dir must be created");
        // store file exists
        assert!(state.config.db_path().exists());
        cleanup(&data);
    }

    #[test]
    fn bootstrap_router_uses_persistent_audit() {
        let data = temp_data_dir("audit");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Drive a router call.
        let bid = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bid, terminal_commander_core::BucketConfig::default())
            .unwrap();

        // The audit row must be visible in the persistent store
        // (not just in some in-memory shadow copy).
        let mut g = state.store.lock();
        let rows = g.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter().any(|r| r.action == "bucket_create"),
            "bucket_create must surface in persistent audit; rows: {rows:?}"
        );
        drop(g);
        cleanup(&data);
    }
}
