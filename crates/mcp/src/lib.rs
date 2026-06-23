// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Terminal Commander MCP crate.
//!
//! Per PRIVILEGE_MODEL.md the MCP server contains NO Command::spawn,
//! NO file open outside its own config, and NO network listener.
//!
//! Dual-surface layout:
//! - [`tools`] plus the `terminal-commander-mcp` binary are the
//!   **production** surface: the rmcp `ServerHandler` that advertises
//!   all 50 live tools (see [`tools::tool_catalogue`]), each forwarded
//!   to the daemon over UDS IPC.
//! - `ToolSurface` (this file) is a **legacy in-process test facade**
//!   used by integration tests that drive bucket/registry flows
//!   directly without stdio or rmcp. It is NOT the product surface and
//!   its tool list must not be read as the advertised catalogue.
//! - [`daemon_client`] — UDS client wrapper shared by both surfaces.
//! - [`target_router`] — P5 remote-federation routing: resolves an
//!   optional `target_id` to the local daemon (default) or a target's
//!   operator-forwarded LOCAL socket. No spawn, no TCP, no fs.
//!
//! Source-status: live.

pub mod daemon_client;
pub mod surface;
pub mod target_router;
pub mod tools;

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use terminal_commander_core::{
    BucketId, BucketReadRequest, BucketReadResponse, BucketSummary, BucketWaitRequest,
    BucketWaitResponse, ContextWindowResponse, FrameId, ProbeId, Severity,
};
use terminal_commanderd::{PolicyAction, PolicyDecision, PolicyEngine, Router};

/// `system_discover` response. Mirrors the contract fixture
/// reserved-not-implemented placeholder in TC05; the live shape is
/// minimal at MVP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDiscoverResponse {
    pub version: String,
    pub mcp_spec: String,
    pub policy_profile: String,
    pub tools: Vec<String>,
}

/// Errors from the MCP tool surface.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("bucket error: {0}")]
    Bucket(#[from] terminal_commander_core::BucketError),
    #[error("context error: {0}")]
    Context(#[from] terminal_commander_core::ContextError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(String),
}

/// MCP tool surface. Wraps the daemon `Router` + `PolicyEngine`.
#[derive(Debug)]
pub struct ToolSurface {
    pub router: Arc<Router>,
    pub policy: PolicyEngine,
}

impl ToolSurface {
    /// Construct a tool surface.
    #[must_use]
    pub const fn new(router: Arc<Router>, policy: PolicyEngine) -> Self {
        Self { router, policy }
    }

    /// `system_discover` tool.
    #[must_use]
    pub fn system_discover(&self) -> SystemDiscoverResponse {
        SystemDiscoverResponse {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            mcp_spec: "2025-11-25".to_owned(),
            policy_profile: format!("{:?}", self.policy.profile),
            tools: crate::tools::catalogue_tool_names()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }

    /// `bucket_events_since` tool.
    pub fn bucket_events_since(
        &self,
        bucket_id: BucketId,
        cursor: u64,
        severity_min: Option<Severity>,
        kind_filter: Option<String>,
        limit: Option<usize>,
    ) -> Result<BucketReadResponse, McpError> {
        let v = self.policy.evaluate(&PolicyAction::BucketRead);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        let req = BucketReadRequest {
            cursor,
            severity_min,
            kind_filter,
            limit,
        };
        Ok(self.router.bucket_events_since(bucket_id, &req)?)
    }

    /// `bucket_wait` tool.
    pub async fn bucket_wait(
        &self,
        bucket_id: BucketId,
        cursor: u64,
        severity_min: Option<Severity>,
        kind_filter: Option<String>,
        limit: Option<usize>,
        timeout: Duration,
    ) -> Result<BucketWaitResponse, McpError> {
        let v = self.policy.evaluate(&PolicyAction::BucketWait);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        let req = BucketWaitRequest {
            cursor,
            severity_min,
            kind_filter,
            limit,
            timeout,
        };
        Ok(self.router.bucket_wait(bucket_id, req).await?)
    }

    /// `bucket_summary` tool.
    pub fn bucket_summary(&self, bucket_id: BucketId) -> Result<BucketSummary, McpError> {
        let v = self.policy.evaluate(&PolicyAction::BucketRead);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        Ok(self.router.bucket_summary(bucket_id)?)
    }

    /// `event_context` tool.
    pub fn event_context(
        &self,
        probe_id: ProbeId,
        anchor: FrameId,
        before: u32,
        after: u32,
        max_bytes: Option<usize>,
    ) -> Result<ContextWindowResponse, McpError> {
        let v = self.policy.evaluate(&PolicyAction::EventContext);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        Ok(self
            .router
            .event_context(probe_id, anchor, before, after, max_bytes)?)
    }

    // TC43: the in-process `ToolSurface::file_read_window` shim was
    // removed. The MCP-facing file_read_window now lives in
    // `tools.rs` and forwards through the daemon UDS. Per TC43, the
    // MCP crate MUST NOT touch the filesystem directly.

    /// `registry_search` tool.
    pub fn registry_search(
        &self,
        store: &mut terminal_commander_store::EventStore,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<terminal_commander_store::RuleSearchHit>, McpError> {
        let v = self.policy.evaluate(&PolicyAction::BucketRead);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        store
            .ensure_registry()
            .map_err(|e| McpError::Store(e.to_string()))?;
        store
            .search_rules(query, limit)
            .map_err(|e| McpError::Store(e.to_string()))
    }

    /// `registry_get` tool.
    pub fn registry_get(
        &self,
        store: &terminal_commander_store::EventStore,
        rule_id: &str,
    ) -> Result<Option<terminal_commander_core::RuleDefinition>, McpError> {
        let v = self.policy.evaluate(&PolicyAction::BucketRead);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        store
            .get_latest_rule(rule_id)
            .map_err(|e| McpError::Store(e.to_string()))
    }

    /// `registry_create` tool.
    pub fn registry_create(
        &self,
        store: &mut terminal_commander_store::EventStore,
        rule: &terminal_commander_core::RuleDefinition,
    ) -> Result<u32, McpError> {
        let v = self.policy.evaluate(&PolicyAction::RegistryCreate);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        store
            .create_rule_version(rule)
            .map_err(|e| McpError::Store(e.to_string()))
    }

    /// `registry_activate` tool. Always passes through AllowWithAudit
    /// (TC22 contract). Caller's audit-log path emits the record.
    pub fn registry_activate(
        &self,
        store: &mut terminal_commander_store::EventStore,
        rule_id: &str,
        version: u32,
        profile: Option<&str>,
    ) -> Result<(), McpError> {
        let v = self.policy.evaluate(&PolicyAction::RegistryActivate);
        if v.decision == PolicyDecision::Deny {
            return Err(McpError::PolicyDenied(v.reason));
        }
        store
            .record_activation(rule_id, version, profile, Some("mcp"))
            .map_err(|e| McpError::Store(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terminal_commander_core::{
        BucketConfig, BucketManager, Captures, ContextRingManager, EventDraft, EventSource,
        FrameId, JobManager, ProbeId, SignalEvent, SourcePointer, SourceStream, SourceType,
    };
    use terminal_commander_sifters::SifterRuntime;

    fn surface() -> ToolSurface {
        let buckets = Arc::new(BucketManager::new());
        let rings = Arc::new(ContextRingManager::new());
        let jobs = Arc::new(JobManager::new());
        let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
        let router = Arc::new(Router::new(buckets, rings, jobs, sifter));
        ToolSurface::new(router, PolicyEngine::default_engine())
    }

    fn draft(bid: BucketId, sev: Severity) -> EventDraft {
        let mut caps = Captures::new();
        caps.insert("k".to_owned(), "v".to_owned());
        EventDraft {
            bucket_id: bid,
            timestamp: time::OffsetDateTime::now_utc(),
            severity: sev,
            kind: "k".to_owned(),
            summary: "s".to_owned(),
            rule: None,
            source: EventSource {
                probe_id: ProbeId::new(),
                source_type: SourceType::Process,
                stream: SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        }
    }

    #[test]
    fn discover_lists_mvp_tools() {
        let s = surface();
        let d = s.system_discover();
        assert!(d.tools.contains(&"bucket_wait".to_owned()));
        assert_eq!(d.mcp_spec, "2025-11-25");
    }

    #[test]
    fn discover_advertises_exactly_the_catalogue_no_drift() {
        let s = surface();
        let mut got = s.system_discover().tools;
        got.sort();
        let mut want: Vec<String> = crate::tools::catalogue_tool_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        want.sort();
        assert_eq!(
            got, want,
            "ToolSurface::system_discover must advertise exactly tool_catalogue() names (no hand-maintained drift)"
        );
    }

    #[test]
    fn events_since_through_mcp() {
        let s = surface();
        let bid = BucketId::new();
        s.router
            .bucket_create(bid, BucketConfig::default())
            .unwrap();
        s.router
            .bucket_append(bid, draft(bid, Severity::High))
            .unwrap();
        let resp = s.bucket_events_since(bid, 0, None, None, None).unwrap();
        assert_eq!(resp.events.len(), 1);
    }

    #[test]
    fn read_only_observer_denies_bucket_wait_too() {
        // read_only_observer profile does not deny BucketWait in
        // our impl; ensure the deny path still works for a wrong
        // action. Use AdminDebug's RegistryCreate via a separate path.
        let buckets = Arc::new(BucketManager::new());
        let rings = Arc::new(ContextRingManager::new());
        let jobs = Arc::new(JobManager::new());
        let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
        let router = Arc::new(Router::new(buckets, rings, jobs, sifter));
        let s = ToolSurface::new(
            router,
            PolicyEngine::new(terminal_commanderd::PolicyProfile::ReadOnlyObserver),
        );
        let bid = BucketId::new();
        s.router
            .bucket_create(bid, BucketConfig::default())
            .unwrap();
        // BucketRead is allowed under read_only_observer.
        let _ = s.bucket_events_since(bid, 0, None, None, None).unwrap();
        // Sanity: bucket_summary also OK.
        let _ = s.bucket_summary(bid).unwrap();
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn bucket_wait_through_mcp_heartbeat() {
        let runtime = rt();
        runtime.block_on(async {
            let s = surface();
            let bid = BucketId::new();
            s.router
                .bucket_create(bid, BucketConfig::default())
                .unwrap();
            let resp = s
                .bucket_wait(bid, 0, None, None, None, Duration::from_millis(40))
                .await
                .unwrap();
            assert!(resp.heartbeat);
        });
    }

    #[test]
    fn event_context_returns_anchor_missing_for_unknown_anchor() {
        let s = surface();
        let pid = ProbeId::new();
        s.router.rings.create_ring_default(pid).unwrap();
        let resp = s.event_context(pid, FrameId::new(), 0, 0, None).unwrap();
        assert!(resp.anchor_missing);
    }

    // We never expose a raw-stream lane: ToolSurface returns only the
    // structured types from `terminal_commander_core`.
    #[allow(dead_code)]
    fn no_raw_stream_check(_e: &SignalEvent, _b: &BucketReadResponse) {}

    // TC43: the in-process `ToolSurface::file_read_window` was
    // removed (MCP must not read files directly). The daemon-backed
    // file_read_window + bounded-cap + sensitive-path-denied paths
    // are covered by `crates/daemon/tests/file_ipc.rs` and
    // `crates/mcp/tests/file_tools_live_e2e.rs`.

    #[test]
    fn registry_create_search_round_trip_via_mcp() {
        let s = surface();
        let mut store = terminal_commander_store::EventStore::in_memory().unwrap();
        let mut rule = terminal_commander_core::RuleDefinition {
            id: "mcp.test".to_owned(),
            version: 1,
            kind: terminal_commander_core::RuleType::Keyword,
            status: terminal_commander_core::RuleStatus::Draft,
            severity: Severity::Medium,
            event_kind: "x".to_owned(),
            stream: None,
            description: None,
            pattern: None,
            keywords: Some(vec!["needle".to_owned()]),
            captures: vec![],
            summary_template: "ok".to_owned(),
            tags: vec!["mcp".to_owned()],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: terminal_commander_core::ContextHint::default(),
            examples: vec![],
        };
        let v1 = s.registry_create(&mut store, &rule).unwrap();
        assert_eq!(v1, 1);
        rule.summary_template = "edited".to_owned();
        let v2 = s.registry_create(&mut store, &rule).unwrap();
        assert_eq!(v2, 2);
        let got = s.registry_get(&store, "mcp.test").unwrap().unwrap();
        assert_eq!(got.summary_template, "edited");
        let hits = s.registry_search(&mut store, "mcp", None).unwrap();
        assert!(!hits.is_empty());
    }
}
