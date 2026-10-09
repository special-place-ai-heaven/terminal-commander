// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Identity shared by the embedded engine, IPC discovery and health.

use serde::{Deserialize, Serialize};

/// Revision of the typed engine contract, independent of package releases.
pub const ENGINE_API_VERSION: u32 = 1;
/// Revision of the engine's serialized response schema.
pub const ENGINE_SCHEMA_VERSION: u32 = 1;

/// Content-free correlation for a lost job; the current instance is the reader,
/// not a claim about which instance started the job or why its owner ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JobLostDetails {
    pub job_id: terminal_commander_core::JobId,
    pub current_instance_id: String,
    pub api_version: u32,
    pub build_fingerprint: String,
}

/// Immutable build inputs. This is a discriminator, not an artifact checksum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildIdentity {
    /// FNV-1a-128 over sorted TC source/manifests/lock inputs and compiler settings.
    /// This non-cryptographic fingerprint is not a security attestation.
    pub source_fingerprint: String,
    pub target: String,
    pub compiler: String,
    pub profile: String,
    pub features: String,
    /// Explicit build-system provenance, absent when no provenance was supplied.
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineIdentity {
    pub api_version: u32,
    pub schema_version: u32,
    pub engine_version: String,
    pub build: BuildIdentity,
    /// A fresh UUID for each engine bootstrap, including two engines in one process.
    pub instance_id: String,
}

/// Typed health view returned by the embedded facade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineHealth {
    pub identity: EngineIdentity,
    pub uptime_secs: u64,
    pub idle_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineFeature {
    Commands,
    IsolatedCommands,
    Shell,
    Pty,
    ShellSessions,
    Files,
    FileWatches,
    Sifters,
    Buckets,
    Context,
    Tails,
    Registry,
    Recipes,
    Subscriptions,
    Policy,
    Audit,
    ResourceLimits,
    ProcessObservation,
    WholeJobCpu,
    OwnerCredentials,
    RemoteTargets,
}

/// Availability never bypasses request-specific policy or input validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureAvailability {
    Available,
    DeniedByPolicy,
    UnsupportedPlatform,
    HostPermissionRequired,
    HostTransportRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCapability {
    pub feature: EngineFeature,
    pub availability: FeatureAvailability,
}
