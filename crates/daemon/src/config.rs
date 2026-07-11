// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon configuration (TC36).
//!
//! Loads `terminal-commanderd.toml` from a known path (operator-
//! supplied via `--config`, or the platform-default
//! `$XDG_CONFIG_HOME/terminal-commanderd/terminal-commanderd.toml`).
//! All fields have safe defaults so `terminal-commanderd start`
//! works without any config file present.
//!
//! Validation rejects:
//! - Empty / non-absolute `data_dir`.
//! - Unknown policy profiles.
//! - Negative or zero retention values.
//! - Response limits above the codebase hard caps
//!   (`MAX_FILE_WINDOW_BYTES`, `MAX_READ_LIMIT`).
//! - `runtime_mode` values not in the closed set.
//!
//! Errors are deliberately short and do NOT echo back the offending
//! value when the field could carry a secret-looking path. The path
//! itself is fine to surface; the goal is to keep loud value echoes
//! out of logs by default.
//!
//! Source-status: live (TC36) for parse + validate + apply-defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::policy::{PolicyCaps, PolicyProfile};

/// Hard cap on a single file_read_window response. Mirrors the value
/// already exported by `terminal-commander-mcp::MAX_FILE_WINDOW_BYTES`.
/// Defined here because the daemon must not depend on the mcp crate.
pub const HARD_MAX_FILE_WINDOW_BYTES: usize = 64 * 1024;

/// Hard cap on a single bucket read. Mirrors `terminal_commander_store::
/// MAX_READ_LIMIT`.
pub const HARD_MAX_READ_LIMIT: usize = 10_000;

/// Default per-bucket count cap.
pub const DEFAULT_BUCKET_MAX_EVENTS: u64 = 100_000;

/// Default per-bucket TTL (24 hours), in seconds.
pub const DEFAULT_BUCKET_TTL_SECONDS: u64 = 86_400;

/// Default per-call file_read_window cap (operator may lower; never above hard cap).
pub const DEFAULT_FILE_WINDOW_BYTES: usize = HARD_MAX_FILE_WINDOW_BYTES;

/// Default bucket read response cap (operator may lower; never above hard cap).
pub const DEFAULT_READ_LIMIT: usize = 200;

/// Default audit retention (days). Advisory at MVP; the daemon does
/// not yet evict old audit rows.
pub const DEFAULT_AUDIT_RETENTION_DAYS: u32 = 30;

/// Default idle TTL in seconds before the daemon self-reaps via
/// `trigger_shutdown`. `0` disables the idle-timer entirely.
pub const DEFAULT_IDLE_TTL_SECS: u64 = 1800;

/// Default maximum number of concurrent persistent shell sessions
/// (P1 / TC50). Bounds resource use; sessions past this cap are refused
/// with a bounded explanatory error, never a silent hang.
pub const DEFAULT_MAX_SESSIONS: usize = 16;

/// Default per-session idle TTL in seconds (P1 / TC50).
///
/// A session with no exec/status activity for longer than this is reaped
/// (its shell torn down, resources reclaimed). `0` disables the
/// per-session reaper.
pub const DEFAULT_SESSION_IDLE_TTL_SECS: u64 = 900;

/// Closed set of runtime modes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    /// Initialize state, run a bounded self-check, exit. Used by
    /// `terminal-commanderd check` and by tests. Never opens
    /// sockets, never spawns commands.
    #[default]
    SelfCheck,
    /// Initialize state and wait for shutdown signal. Foreground
    /// only. No UDS, no MCP transport, no command execution. This
    /// is the TC36 bootstrap mode.
    ForegroundIdle,
    /// Initialize state, bind the local UDS, accept connections,
    /// wait for shutdown signal. No TCP. No command execution.
    /// Method set is the TC37 minimum (system_discover / health /
    /// policy_status / self_check). TC38+ extend it.
    IpcServer,
}

/// Errors raised during config load / validate.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config IO error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("config TOML parse error: {0}")]
    Parse(String),
    #[error("config validation error: {0}")]
    Validate(String),
}

/// Result alias for this module.
pub type Result<T> = core::result::Result<T, ConfigError>;

/// `[daemon]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSection {
    /// Data directory holding the SQLite DB and any future
    /// daemon-local files. MUST NOT be `/mnt/c/...` on WSL2.
    pub data_dir: PathBuf,
    /// Local socket path. Reserved for TC37. Not opened by TC36.
    #[serde(default)]
    pub socket_path: Option<PathBuf>,
    /// Runtime mode. Defaults to `self_check`.
    #[serde(default)]
    pub runtime_mode: RuntimeMode,
    /// Idle TTL (seconds) before the daemon self-reaps by calling
    /// `trigger_shutdown`. `0` disables the idle-timer entirely.
    /// May be overridden at runtime via the `TC_IDLE_TTL_SECS` env var.
    #[serde(default = "default_idle_ttl_secs")]
    pub idle_ttl_secs: u64,
}

const fn default_idle_ttl_secs() -> u64 {
    DEFAULT_IDLE_TTL_SECS
}

/// `[shell_session]` section (P1 / TC50): bounds on persistent shell sessions.
///
/// The capability itself is gated separately by `[policy.caps]
/// allow_session` (default false); this section only sizes the runtime
/// once sessions are permitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSessionSection {
    /// Maximum concurrent live sessions. Enforced BEFORE spawn.
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
    /// Per-session idle TTL (seconds). `0` disables the reaper.
    #[serde(default = "default_session_idle_ttl_secs")]
    pub idle_ttl_secs: u64,
}

impl Default for ShellSessionSection {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            idle_ttl_secs: DEFAULT_SESSION_IDLE_TTL_SECS,
        }
    }
}

const fn default_max_sessions() -> usize {
    DEFAULT_MAX_SESSIONS
}

const fn default_session_idle_ttl_secs() -> u64 {
    DEFAULT_SESSION_IDLE_TTL_SECS
}

fn default_shell_session() -> ShellSessionSection {
    ShellSessionSection::default()
}

/// `[sifters]` section (US2 / FR-009): tuning for the sifter runtime.
///
/// `universal_extractors` (default FALSE) enables always-on,
/// LOW-severity baseline extractors (error/warning/exit/progress) that
/// emit bounded structured signal for ANY command even when no
/// tool-specific rule pack is active. Off by default so the signal
/// stream is exactly what the operator opted into; on, a command with
/// no pack still surfaces a baseline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SiftersSection {
    /// Enable always-on baseline universal extractors. Default false.
    #[serde(default)]
    pub universal_extractors: bool,
}

fn default_sifters() -> SiftersSection {
    SiftersSection::default()
}

/// `[policy]` section (declarative profile schema, POLICY.md section 4).
///
/// The `profile` + `profile_version` keys are TC36-era; the `repo_root`,
/// `[policy.paths]`, `[policy.commands]`, and `[policy.probes]` blocks are
/// TC22.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySection {
    pub profile: PolicyProfile,
    #[serde(default = "default_profile_version")]
    pub profile_version: String,
    /// `$REPO_ROOT` for `repo_only` containment (POLICY.md section 2.2).
    /// REQUIRED when `profile = repo_only`; ignored for other profiles.
    /// Validated in `validate_and_clamp`.
    #[serde(default)]
    pub repo_root: Option<PathBuf>,
    /// `[policy.commands]` block (POLICY.md section 4).
    #[serde(default)]
    pub commands: Option<PolicyCommandsSection>,
    /// `[policy.paths]` block (POLICY.md section 4). Parsed and retained
    /// for forward-compat; path allow-list enforcement beyond repo_only
    /// containment is a later phase.
    #[serde(default)]
    pub paths: Option<PolicyPathsSection>,
    /// `[policy.probes]` block (POLICY.md section 4 / 6; TC22 A2). ENFORCED:
    /// threaded into the engine at bootstrap and applied deny-first to the
    /// three probe-creating ops via `PolicyAction::ProbeCreate`.
    #[serde(default)]
    pub probes: Option<PolicyProbesSection>,
    /// `[policy.caps]` block (Hybrid trust model). Optional tri-state overrides:
    /// omitted entries inherit the profile default, while explicit false revokes it.
    #[serde(default)]
    pub caps: Option<PolicyCapsSection>,
}

/// `[policy.commands]` (POLICY.md section 4). `allow_roots` is the
/// per-profile command allow-list (matched by argv[0] basename).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyCommandsSection {
    /// Allowed command basenames. When absent/empty, default-deny is NOT
    /// applied: both exec profiles allow any command surviving the
    /// structural deny set (and, for repo_only, path containment). A
    /// non-empty list opts in to default-deny and is enforced verbatim
    /// for both developer_local and repo_only.
    #[serde(default)]
    pub allow_roots: Vec<String>,
}

/// `[policy.caps]` (Hybrid trust model -- reconciliation Decision 1/5).
///
/// Granular capability overrides. Omitted values inherit the selected profile;
/// explicit true/false values override that profile's preset.
// 4 independent opt-in capability flags; a bitfield/enum would hurt the config/serde surface
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyCapsSection {
    #[serde(default)]
    pub allow_shell: Option<bool>,
    #[serde(default)]
    pub allow_session: Option<bool>,
    #[serde(default)]
    pub allow_privileged: Option<bool>,
    #[serde(default)]
    pub allow_remote: Option<bool>,
}

/// `[policy.paths]` (POLICY.md section 4 / section 5).
///
/// `read_allow`, `watch_allow`, `write_allow`, and `deny_extra` ARE enforced
/// by the engine for `FileRead` / `FileWatch` / `FileWrite` (TC22 A1/A3):
/// compiled to anchored glob-regexes at engine construction and applied as
/// deny-first allow/deny layers on top of repo_only containment and the
/// default-deny suffix list.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyPathsSection {
    #[serde(default)]
    pub read_allow: Vec<String>,
    /// `[policy.paths] write_allow` (TC22 A3). Enforced by the engine for
    /// `FileWrite`: empty == not configured == allow (zero-config writes stay
    /// usable); a non-empty list is authoritative and a write path matching no
    /// glob is denied (`no_allow_rule`). Independent from `read_allow`.
    ///
    /// OPERATOR WARNING (zero-config write posture): with the DEFAULT config
    /// (profile `DeveloperLocal`, no `repo_root`, EMPTY `write_allow`),
    /// `file_write` can write ANYWHERE except the default-deny sensitive-suffix
    /// list -- there is no path containment. This is intentional for a single
    /// local developer, but in ANY shared or agent-facing context an operator
    /// enabling the write lane MUST set a non-empty `write_allow` (or use the
    /// `repo_only` profile with a `repo_root`) to confine writes. Leaving this
    /// empty in a shared/agent deployment is an open write surface.
    #[serde(default)]
    pub write_allow: Vec<String>,
    #[serde(default)]
    pub watch_allow: Vec<String>,
    #[serde(default)]
    pub deny_extra: Vec<String>,
}

/// `[policy.probes]` allow/deny KIND lists (POLICY.md section 4 / 6; TC22 A2).
///
/// ENFORCED: threaded into the policy engine at bootstrap (`with_probe_kinds`)
/// and applied as a deny-first secondary filter on the three probe-creating ops
/// (`command_start_combed` -> `command`, `pty_command_start` /
/// `shell_session_start` -> `pty`, `file_watch_start` -> `file_watch`) via
/// `PolicyAction::ProbeCreate`. Kinds are CASE-SENSITIVE snake_case from the
/// closed set {command, file_watch, pty}; deny beats allow; an EMPTY
/// `allow_kinds` is "not configured" == allow (zero-config stays usable),
/// matching the `[policy.paths]` allow-list posture.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyProbesSection {
    #[serde(default)]
    pub allow_kinds: Vec<String>,
    #[serde(default)]
    pub deny_kinds: Vec<String>,
}

fn default_profile_version() -> String {
    "1".to_owned()
}

/// `[retention]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionSection {
    #[serde(default = "default_bucket_max_events")]
    pub max_events: u64,
    #[serde(default = "default_bucket_ttl_seconds")]
    pub ttl_seconds: u64,
}

const fn default_bucket_max_events() -> u64 {
    DEFAULT_BUCKET_MAX_EVENTS
}

const fn default_bucket_ttl_seconds() -> u64 {
    DEFAULT_BUCKET_TTL_SECONDS
}

/// `[audit]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSection {
    #[serde(default = "default_audit_retention_days")]
    pub retention_days: u32,
}

const fn default_audit_retention_days() -> u32 {
    DEFAULT_AUDIT_RETENTION_DAYS
}

/// `[limits]` section. Per-call response caps. All values are
/// validated against the codebase hard caps and clamped if the
/// operator set them too high.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsSection {
    #[serde(default = "default_file_window_bytes")]
    pub file_window_bytes: usize,
    #[serde(default = "default_read_limit")]
    pub bucket_read_limit: usize,
}

const fn default_file_window_bytes() -> usize {
    DEFAULT_FILE_WINDOW_BYTES
}

const fn default_read_limit() -> usize {
    DEFAULT_READ_LIMIT
}

/// Full daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub daemon: DaemonSection,
    pub policy: PolicySection,
    #[serde(default = "default_retention")]
    pub retention: RetentionSection,
    #[serde(default = "default_audit")]
    pub audit: AuditSection,
    #[serde(default = "default_limits")]
    pub limits: LimitsSection,
    #[serde(default = "default_shell_session")]
    pub shell_session: ShellSessionSection,
    #[serde(default = "default_sifters")]
    pub sifters: SiftersSection,
}

const fn default_retention() -> RetentionSection {
    RetentionSection {
        max_events: DEFAULT_BUCKET_MAX_EVENTS,
        ttl_seconds: DEFAULT_BUCKET_TTL_SECONDS,
    }
}

const fn default_audit() -> AuditSection {
    AuditSection {
        retention_days: DEFAULT_AUDIT_RETENTION_DAYS,
    }
}

const fn default_limits() -> LimitsSection {
    LimitsSection {
        file_window_bytes: DEFAULT_FILE_WINDOW_BYTES,
        bucket_read_limit: DEFAULT_READ_LIMIT,
    }
}

impl DaemonConfig {
    /// Build a config with safe defaults rooted at the given data
    /// directory. Useful for tests and for first-run when no file
    /// exists.
    #[must_use]
    pub fn defaults_in(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            daemon: DaemonSection {
                data_dir: data_dir.into(),
                socket_path: None,
                runtime_mode: RuntimeMode::default(),
                idle_ttl_secs: DEFAULT_IDLE_TTL_SECS,
            },
            policy: PolicySection {
                profile: PolicyProfile::default(),
                profile_version: "1".to_owned(),
                repo_root: None,
                commands: None,
                paths: None,
                probes: None,
                caps: None,
            },
            retention: default_retention(),
            audit: default_audit(),
            limits: default_limits(),
            shell_session: default_shell_session(),
            sifters: default_sifters(),
        }
    }

    /// Load a `DaemonConfig` from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let p = path.as_ref();
        let raw = std::fs::read_to_string(p).map_err(|source| ConfigError::Io {
            path: p.to_path_buf(),
            source,
        })?;
        let mut cfg: Self = toml::from_str(&raw).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate_and_clamp()?;
        Ok(cfg)
    }

    /// Parse a config from an in-memory string. Used by tests.
    pub fn from_toml(s: &str) -> Result<Self> {
        let mut cfg: Self = toml::from_str(s).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate_and_clamp()?;
        Ok(cfg)
    }

    /// Resolve the effective capability set fed to the policy engine.
    ///
    /// Starts from the selected profile's preset and applies every explicitly
    /// configured true/false value. Omitted entries inherit the preset. This
    /// pure derivation never mutates stored TOML or bypasses `evaluate()`.
    #[must_use]
    pub const fn resolved_caps(&self) -> PolicyCaps {
        let mut caps = PolicyCaps::default_for_profile(self.policy.profile);
        if let Some(overrides) = &self.policy.caps {
            if let Some(value) = overrides.allow_shell {
                caps.allow_shell = value;
            }
            if let Some(value) = overrides.allow_session {
                caps.allow_session = value;
            }
            if let Some(value) = overrides.allow_privileged {
                caps.allow_privileged = value;
            }
            if let Some(value) = overrides.allow_remote {
                caps.allow_remote = value;
            }
        }
        caps
    }

    /// Resolve the database path. Always `<data_dir>/terminal-commander.db`.
    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        self.daemon.data_dir.join("terminal-commander.db")
    }

    /// Resolve the UDS socket path. Uses `daemon.socket_path` if
    /// configured, otherwise `<data_dir>/terminal-commanderd.sock`.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.daemon
            .socket_path
            .clone()
            .unwrap_or_else(|| self.daemon.data_dir.join("terminal-commanderd.sock"))
    }

    /// Windows named-pipe path for parent IPC (`\\.\pipe\...`).
    ///
    /// Delegates to `supervisor::paths::resolve_socket_path` for the
    /// non-custom case so the daemon binds EXACTLY the name the client
    /// (mcp/cli) resolves: TC_SOCKET > TC_SESSION token > username default.
    #[must_use]
    pub fn pipe_name(&self) -> String {
        if let Some(ref custom) = self.daemon.socket_path {
            return custom.to_string_lossy().into_owned();
        }
        terminal_commander_supervisor::paths::resolve_socket_path()
            .to_string_lossy()
            .into_owned()
    }

    /// Validate the loaded config. Clamps soft per-call limits down
    /// to the hard caps. Rejects clearly-broken values.
    fn validate_and_clamp(&mut self) -> Result<()> {
        // data_dir is required and must be non-empty.
        if self.daemon.data_dir.as_os_str().is_empty() {
            return Err(ConfigError::Validate(
                "daemon.data_dir must not be empty".to_owned(),
            ));
        }
        // Reject `/mnt/c/...` on principle even before the store
        // does its own /proc/self/mountinfo check. This is a fast
        // pre-check; the store retains the authoritative WSL 9P
        // rejection.
        if let Some(s) = self.daemon.data_dir.to_str()
            && (s.starts_with("/mnt/c/") || s.starts_with("/mnt/C/"))
        {
            return Err(ConfigError::Validate(
                "daemon.data_dir must not be under /mnt/c on WSL2 (9P unsafe)".to_owned(),
            ));
        }
        if self.retention.max_events == 0 {
            return Err(ConfigError::Validate(
                "retention.max_events must be > 0".to_owned(),
            ));
        }
        if self.retention.ttl_seconds == 0 {
            return Err(ConfigError::Validate(
                "retention.ttl_seconds must be > 0".to_owned(),
            ));
        }
        if self.audit.retention_days == 0 {
            return Err(ConfigError::Validate(
                "audit.retention_days must be > 0".to_owned(),
            ));
        }
        if self.limits.file_window_bytes == 0 {
            return Err(ConfigError::Validate(
                "limits.file_window_bytes must be > 0".to_owned(),
            ));
        }
        if self.limits.bucket_read_limit == 0 {
            return Err(ConfigError::Validate(
                "limits.bucket_read_limit must be > 0".to_owned(),
            ));
        }
        // TC22: `repo_only` cannot boot without a confinement root. An
        // unrooted repo_only engine fail-safe denies every path/cwd
        // action, which is safe but useless; reject it at config load so
        // the operator gets a clear error instead of a dead daemon.
        if matches!(self.policy.profile, PolicyProfile::RepoOnly)
            && self
                .policy
                .repo_root
                .as_ref()
                .is_none_or(|p| p.as_os_str().is_empty())
        {
            return Err(ConfigError::Validate(
                "policy.repo_root is required when profile = repo_only".to_owned(),
            ));
        }
        // Clamp per-call limits down to hard caps.
        if self.limits.file_window_bytes > HARD_MAX_FILE_WINDOW_BYTES {
            self.limits.file_window_bytes = HARD_MAX_FILE_WINDOW_BYTES;
        }
        if self.limits.bucket_read_limit > HARD_MAX_READ_LIMIT {
            self.limits.bucket_read_limit = HARD_MAX_READ_LIMIT;
        }
        Ok(())
    }
}

/// Render the active config back to TOML. Used by
/// `terminal-commanderd print-config`. Does NOT echo any secret
/// material (the daemon does not hold any).
pub fn to_toml(cfg: &DaemonConfig) -> String {
    // toml::to_string never fails for well-typed structs; surface
    // the error rather than panic, for hygiene.
    toml::to_string(cfg).unwrap_or_else(|e| format!("# serialization error: {e}\n"))
}

/// Transport used to reach a remote daemon (P5, FR-019..FR-021).
///
/// Constitution IV: the daemon binds a LOCAL endpoint only; remote reach
/// is achieved by tunnelling to an existing local socket, never by
/// opening a public TCP listener. The only transport variant is
/// `ssh_forward`: the OPERATOR establishes an `ssh -L` local-forward from
/// `local_forward_socket` (a path on this host) to the remote daemon's
/// UDS. Terminal Commander then dials that purely-local socket path -- it
/// never speaks SSH or TCP itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTransport {
    /// Operator-established SSH local-forward to the remote daemon's UDS.
    /// The adapter dials `local_forward_socket` (a local path); the SSH
    /// tunnel is owned by the operator, NOT spawned by Terminal Commander.
    SshForward,
}

/// A registered remote federation target (P5, FR-019).
///
/// Parsed from `targets.toml`. `local_forward_socket` is a path on THIS
/// host that an operator-run `ssh -L` (or equivalent) has forwarded to the
/// remote daemon's UDS. Routing a tool with `target_id` set dials that
/// local socket path; combing still happens on the remote daemon, so the
/// bounded/structured signal contract is identical local vs remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTarget {
    /// Stable operator-chosen id, referenced by the `target_id` tool field.
    pub target_id: String,
    /// Transport. Only `ssh_forward` is supported (see [`RemoteTransport`]).
    pub transport: RemoteTransport,
    /// Human-facing host label (e.g. `build-01.internal`). Informational;
    /// the adapter never dials this -- it dials `local_forward_socket`.
    pub host: String,
    /// Optional SSH identity file the OPERATOR uses to establish the
    /// forward. Recorded for documentation / tooling only; Terminal
    /// Commander never reads it or spawns ssh.
    #[serde(default)]
    pub identity_file: Option<PathBuf>,
    /// Optional remote-side UDS path (documentation: what the operator's
    /// `ssh -L` forwards TO on the remote host). Not dialed locally.
    #[serde(default)]
    pub remote_socket: Option<PathBuf>,
    /// The LOCAL socket path this host dials to reach the remote daemon.
    /// An operator-established forward must terminate here. This is the
    /// only field the adapter actually connects to.
    pub local_forward_socket: PathBuf,
}

/// Parsed `targets.toml`. Default (no file / empty file) = NO targets =
/// local-only, preserving exact backward compatibility.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetsConfig {
    /// Registered remote targets. Empty by default.
    #[serde(default)]
    pub targets: Vec<RemoteTarget>,
}

impl TargetsConfig {
    /// Parse a `targets.toml` from an in-memory string. Validates that
    /// `target_id`s are non-empty and unique and that no two targets share
    /// a `local_forward_socket`.
    pub fn from_toml(s: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(s).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load `targets.toml` from disk.
    ///
    /// A MISSING file is NOT an error: it resolves to an empty
    /// (local-only) config, since remote federation is opt-in. Any other
    /// IO error (permission denied, etc.) is surfaced.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let p = path.as_ref();
        match std::fs::read_to_string(p) {
            Ok(raw) => Self::from_toml(&raw),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: p.to_path_buf(),
                source,
            }),
        }
    }

    /// Look up a target by id.
    #[must_use]
    pub fn get(&self, target_id: &str) -> Option<&RemoteTarget> {
        self.targets.iter().find(|t| t.target_id == target_id)
    }

    /// True when no targets are registered (the default local-only state).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    fn validate(&self) -> Result<()> {
        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_sockets = std::collections::HashSet::new();
        for t in &self.targets {
            if t.target_id.trim().is_empty() {
                return Err(ConfigError::Validate(
                    "remote target has an empty target_id".to_owned(),
                ));
            }
            if !seen_ids.insert(t.target_id.as_str()) {
                return Err(ConfigError::Validate(format!(
                    "duplicate remote target_id: {}",
                    t.target_id
                )));
            }
            if t.local_forward_socket.as_os_str().is_empty() {
                return Err(ConfigError::Validate(format!(
                    "remote target '{}' has an empty local_forward_socket",
                    t.target_id
                )));
            }
            if !seen_sockets.insert(t.local_forward_socket.clone()) {
                return Err(ConfigError::Validate(format!(
                    "remote target '{}' reuses a local_forward_socket already bound to another target",
                    t.target_id
                )));
            }
        }
        Ok(())
    }
}

/// Resolve the default `targets.toml` path:
/// `<XDG_CONFIG_HOME or ~/.config>/terminal-commander/targets.toml`.
///
/// Honors `TC_TARGETS_CONFIG` (full path override, used by tests) first.
#[must_use]
pub fn default_targets_config_path() -> PathBuf {
    if let Ok(explicit) = std::env::var("TC_TARGETS_CONFIG")
        && !explicit.is_empty()
    {
        return PathBuf::from(explicit);
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("terminal-commander").join("targets.toml")
}

/// Load the targets registry from the default (or `TC_TARGETS_CONFIG`)
/// path. A missing file resolves to an empty (local-only) registry.
pub fn load_targets() -> Result<TargetsConfig> {
    TargetsConfig::load(default_targets_config_path())
}

/// The daemon's default data dir when no `--config`/`--data-dir`/`TC_DATA`
/// is supplied (F5).
///
/// Delegates to `supervisor::paths::resolve_state_dir` so the daemon (the
/// pidfile WRITER) and the supervisor (the pidfile READER + socket prober)
/// can never resolve different directories. Before this, the daemon used a
/// `%USERPROFILE%\.terminal-commanderd` default on Windows while the
/// supervisor probed `%LOCALAPPDATA%\terminal-commanderd\state`, so a daemon
/// started without `--data-dir` (manual `start`, or `update`/`restart`) wrote
/// its pidfile where the reader never looked.
#[must_use]
pub fn default_state_dir() -> PathBuf {
    terminal_commander_supervisor::paths::resolve_state_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_toml() {
        let cfg = DaemonConfig::defaults_in("/tmp/tc-test-data");
        let s = to_toml(&cfg);
        let back = DaemonConfig::from_toml(&s).unwrap();
        assert_eq!(back.daemon.data_dir, cfg.daemon.data_dir);
        assert_eq!(back.policy.profile, cfg.policy.profile);
    }

    #[test]
    fn default_state_dir_matches_supervisor_resolver() {
        // F5: the daemon's default data dir MUST equal the path the
        // supervisor (pidfile reader, socket prober) resolves, or a
        // daemon started without --data-dir writes its pidfile where the
        // reader never looks. Single source of truth: delegate.
        assert_eq!(
            super::default_state_dir(),
            terminal_commander_supervisor::paths::resolve_state_dir(),
            "daemon default data dir must match supervisor::paths::resolve_state_dir"
        );
    }

    #[test]
    fn loads_minimal_toml_with_defaults() {
        let s = r#"
            [daemon]
            data_dir = "/tmp/tc-min"

            [policy]
            profile = "developer_local"
        "#;
        let cfg = DaemonConfig::from_toml(s).unwrap();
        assert_eq!(cfg.policy.profile, PolicyProfile::DeveloperLocal);
        assert_eq!(cfg.retention.max_events, DEFAULT_BUCKET_MAX_EVENTS);
        assert_eq!(cfg.limits.file_window_bytes, DEFAULT_FILE_WINDOW_BYTES);
        assert_eq!(cfg.daemon.runtime_mode, RuntimeMode::SelfCheck);
    }

    #[test]
    fn rejects_empty_data_dir() {
        let s = r#"
            [daemon]
            data_dir = ""

            [policy]
            profile = "developer_local"
        "#;
        let err = DaemonConfig::from_toml(s).unwrap_err();
        match err {
            ConfigError::Validate(m) => assert!(m.contains("data_dir")),
            other => panic!("expected Validate error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_mnt_c_data_dir() {
        let s = r#"
            [daemon]
            data_dir = "/mnt/c/Users/x/tc-data"

            [policy]
            profile = "developer_local"
        "#;
        let err = DaemonConfig::from_toml(s).unwrap_err();
        match err {
            ConfigError::Validate(m) => assert!(m.contains("9P")),
            other => panic!("expected Validate error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_zero_retention() {
        let s = r#"
            [daemon]
            data_dir = "/tmp/x"

            [policy]
            profile = "developer_local"

            [retention]
            max_events = 0
            ttl_seconds = 60
        "#;
        let err = DaemonConfig::from_toml(s).unwrap_err();
        assert!(matches!(err, ConfigError::Validate(_)));
    }

    #[test]
    fn clamps_file_window_to_hard_cap() {
        let s = r#"
            [daemon]
            data_dir = "/tmp/x"

            [policy]
            profile = "developer_local"

            [limits]
            file_window_bytes = 999_999_999
            bucket_read_limit = 50
        "#;
        let cfg = DaemonConfig::from_toml(s).unwrap();
        assert_eq!(cfg.limits.file_window_bytes, HARD_MAX_FILE_WINDOW_BYTES);
        assert_eq!(cfg.limits.bucket_read_limit, 50);
    }

    #[test]
    fn clamps_bucket_read_limit_to_hard_cap() {
        let s = r#"
            [daemon]
            data_dir = "/tmp/x"

            [policy]
            profile = "developer_local"

            [limits]
            file_window_bytes = 4096
            bucket_read_limit = 999_999
        "#;
        let cfg = DaemonConfig::from_toml(s).unwrap();
        assert_eq!(cfg.limits.bucket_read_limit, HARD_MAX_READ_LIMIT);
    }

    #[test]
    fn unknown_policy_profile_is_a_parse_error() {
        let s = r#"
            [daemon]
            data_dir = "/tmp/x"

            [policy]
            profile = "totally_bogus"
        "#;
        let err = DaemonConfig::from_toml(s).unwrap_err();
        // Either Parse or Validate is acceptable; both keep the bad
        // value out of the running daemon.
        assert!(matches!(
            err,
            ConfigError::Parse(_) | ConfigError::Validate(_)
        ));
    }

    #[test]
    fn db_path_under_data_dir() {
        let cfg = DaemonConfig::defaults_in("/tmp/tc-dbp");
        assert_eq!(
            cfg.db_path(),
            PathBuf::from("/tmp/tc-dbp/terminal-commander.db")
        );
    }

    // --- TC22 Phase 2: profile schema + repo_root validation (AC10) ---

    #[test]
    fn repo_only_without_repo_root_is_rejected() {
        let toml = "[daemon]\ndata_dir = \"/tmp/tc-ro\"\n[policy]\nprofile = \"repo_only\"\n";
        let err = DaemonConfig::from_toml(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::Validate(_)),
            "expected Validate error, got {err:?}"
        );
    }

    #[test]
    fn repo_only_with_repo_root_loads() {
        let toml = "[daemon]\ndata_dir = \"/tmp/tc-ro\"\n[policy]\nprofile = \"repo_only\"\nrepo_root = \"/tmp/tc-ro/repo\"\n";
        let cfg = DaemonConfig::from_toml(toml).expect("repo_only + repo_root must load");
        assert_eq!(cfg.policy.profile, PolicyProfile::RepoOnly);
        assert!(cfg.policy.repo_root.is_some());
    }

    #[test]
    fn policy_commands_section_parses() {
        let toml = "[daemon]\ndata_dir = \"/tmp/tc-c\"\n[policy]\nprofile = \"developer_local\"\n[policy.commands]\nallow_roots = [\"cargo\", \"git\", \"npm\"]\n";
        let cfg = DaemonConfig::from_toml(toml).expect("must parse [policy.commands]");
        let roots = cfg.policy.commands.expect("commands present").allow_roots;
        assert_eq!(roots, vec!["cargo", "git", "npm"]);
    }

    #[test]
    fn parses_policy_caps_block() {
        let toml = "[daemon]\ndata_dir = \"/tmp/tc-caps\"\n\
                    [policy]\nprofile = \"developer_local\"\n\
                    [policy.caps]\nallow_shell = true\n";
        let cfg = DaemonConfig::from_toml(toml).expect("must parse [policy.caps]");
        let caps = cfg.policy.caps.as_ref().expect("caps present");
        assert_eq!(caps.allow_shell, Some(true));
        assert_eq!(caps.allow_session, None);
        assert_eq!(caps.allow_privileged, None);
        assert_eq!(caps.allow_remote, None);
    }

    #[test]
    fn caps_absent_is_none_not_error() {
        let toml =
            "[daemon]\ndata_dir = \"/tmp/tc-nocaps\"\n[policy]\nprofile = \"developer_local\"\n";
        let cfg = DaemonConfig::from_toml(toml).expect("parse");
        assert!(cfg.policy.caps.is_none());
    }

    #[test]
    fn developer_local_profile_presets_allow_shell_false() {
        let toml =
            "[daemon]\ndata_dir = \"/tmp/tc-dev-shell\"\n[policy]\nprofile = \"developer_local\"\n";
        let cfg = DaemonConfig::from_toml(toml).expect("parse developer_local defaults");
        let caps = cfg.resolved_caps();
        assert!(!caps.allow_shell);
        assert!(!caps.allow_session);
        assert!(!caps.allow_privileged);
        assert!(!caps.allow_remote);
    }

    #[test]
    fn full_access_profile_presets_all_caps_true() {
        let toml = "[daemon]\ndata_dir = \"/tmp/tc-fa\"\n[policy]\nprofile = \"full_access\"\n";
        let cfg = DaemonConfig::from_toml(toml).expect("parse full_access");
        let caps = cfg.resolved_caps();
        assert!(
            caps.allow_shell && caps.allow_session && caps.allow_privileged && caps.allow_remote
        );
    }

    #[test]
    fn full_access_explicit_false_revokes_shell() {
        let toml = "[daemon]\ndata_dir = \"/tmp/tc-fa-revoke\"\n[policy]\nprofile = \"full_access\"\n[policy.caps]\nallow_shell = false\n";
        let cfg = DaemonConfig::from_toml(toml).expect("parse full_access override");
        let caps = cfg.resolved_caps();
        assert!(!caps.allow_shell);
        assert!(caps.allow_session);
        assert!(caps.allow_privileged);
        assert!(caps.allow_remote);
    }

    #[test]
    fn developer_local_without_repo_root_still_loads() {
        let mut cfg = DaemonConfig::defaults_in("/tmp/tc-dev");
        cfg.validate_and_clamp().expect("dev_local must validate");
    }

    #[test]
    fn committed_example_toml_parses() {
        // AC7: the shipped example config must always load. Path is
        // relative to the daemon crate root (where tests run).
        let raw = std::fs::read_to_string("../../config/terminal-commanderd.example.toml")
            .expect("read example toml");
        let cfg = DaemonConfig::from_toml(&raw).expect("example toml must parse + validate");
        assert_eq!(cfg.policy.profile, PolicyProfile::DeveloperLocal);
    }

    #[test]
    fn committed_targets_example_toml_parses() {
        // P5: the shipped targets.toml example must always load + validate.
        // Path is relative to the daemon crate root (where tests run).
        let raw = std::fs::read_to_string("../../config/targets.example.toml")
            .expect("read targets example toml");
        let cfg =
            TargetsConfig::from_toml(&raw).expect("targets example toml must parse + validate");
        let t = cfg.get("build-01").expect("example target present");
        assert_eq!(t.transport, RemoteTransport::SshForward);
        assert_eq!(
            t.local_forward_socket,
            PathBuf::from("/tmp/tc-fwd-build-01.sock")
        );
    }

    #[test]
    fn sifters_section_defaults_off_and_parses_when_present() {
        // Absent: defaults to false (US2 / FR-009).
        let raw = "[daemon]\ndata_dir = \"/tmp/tc-sifters-default\"\n[policy]\nprofile = \"developer_local\"\nprofile_version = \"1\"\n";
        let cfg = DaemonConfig::from_toml(raw).expect("minimal toml parses");
        assert!(
            !cfg.sifters.universal_extractors,
            "universal_extractors must default OFF"
        );

        // Present + true: round-trips.
        let raw_on = "[daemon]\ndata_dir = \"/tmp/tc-sifters-on\"\n[policy]\nprofile = \"developer_local\"\nprofile_version = \"1\"\n[sifters]\nuniversal_extractors = true\n";
        let cfg_on = DaemonConfig::from_toml(raw_on).expect("sifters toml parses");
        assert!(cfg_on.sifters.universal_extractors);
    }

    // ---- P5 (T049): targets.toml / RemoteTarget parsing ----

    #[test]
    fn targets_default_is_empty_local_only() {
        // The whole point of the default: no targets => local-only, exact
        // backward compatibility for every existing tool.
        let cfg = TargetsConfig::default();
        assert!(cfg.is_empty());
        assert!(cfg.targets.is_empty());
        assert!(cfg.get("anything").is_none());
    }

    #[test]
    fn targets_empty_toml_parses_to_no_targets() {
        let cfg = TargetsConfig::from_toml("").expect("empty targets.toml parses");
        assert!(cfg.is_empty());
    }

    #[test]
    fn targets_parse_ssh_forward_target() {
        let raw = r#"
    [[targets]]
    target_id = "build-01"
    transport = "ssh_forward"
    host = "build-01.internal"
    identity_file = "/home/op/.ssh/id_ed25519"
    remote_socket = "/run/user/1000/terminal-commanderd.sock"
    local_forward_socket = "/tmp/tc-fwd-build-01.sock"
    "#;
        let cfg = TargetsConfig::from_toml(raw).expect("ssh_forward target parses");
        assert_eq!(cfg.targets.len(), 1);
        let t = cfg.get("build-01").expect("target present");
        assert_eq!(t.transport, RemoteTransport::SshForward);
        assert_eq!(t.host, "build-01.internal");
        assert_eq!(
            t.identity_file.as_deref(),
            Some(Path::new("/home/op/.ssh/id_ed25519"))
        );
        assert_eq!(
            t.local_forward_socket,
            PathBuf::from("/tmp/tc-fwd-build-01.sock")
        );
    }

    #[test]
    fn targets_optional_fields_default_to_none() {
        let raw = r#"
    [[targets]]
    target_id = "minimal"
    transport = "ssh_forward"
    host = "h"
    local_forward_socket = "/tmp/tc-fwd-minimal.sock"
    "#;
        let cfg = TargetsConfig::from_toml(raw).expect("minimal target parses");
        let t = cfg.get("minimal").expect("target present");
        assert!(t.identity_file.is_none());
        assert!(t.remote_socket.is_none());
    }

    #[test]
    fn targets_reject_duplicate_target_id() {
        let raw = r#"
    [[targets]]
    target_id = "dup"
    transport = "ssh_forward"
    host = "a"
    local_forward_socket = "/tmp/tc-a.sock"
    [[targets]]
    target_id = "dup"
    transport = "ssh_forward"
    host = "b"
    local_forward_socket = "/tmp/tc-b.sock"
    "#;
        let err = TargetsConfig::from_toml(raw).expect_err("duplicate id rejected");
        assert!(matches!(err, ConfigError::Validate(_)));
    }

    #[test]
    fn targets_reject_duplicate_forward_socket() {
        let raw = r#"
    [[targets]]
    target_id = "a"
    transport = "ssh_forward"
    host = "a"
    local_forward_socket = "/tmp/tc-shared.sock"
    [[targets]]
    target_id = "b"
    transport = "ssh_forward"
    host = "b"
    local_forward_socket = "/tmp/tc-shared.sock"
    "#;
        let err = TargetsConfig::from_toml(raw).expect_err("duplicate socket rejected");
        assert!(matches!(err, ConfigError::Validate(_)));
    }

    #[test]
    fn targets_reject_empty_target_id() {
        let raw = r#"
    [[targets]]
    target_id = ""
    transport = "ssh_forward"
    host = "a"
    local_forward_socket = "/tmp/tc-a.sock"
    "#;
        let err = TargetsConfig::from_toml(raw).expect_err("empty id rejected");
        assert!(matches!(err, ConfigError::Validate(_)));
    }

    #[test]
    fn targets_load_missing_file_is_local_only_not_error() {
        // Remote federation is opt-in: a missing targets.toml must resolve to
        // the empty (local-only) registry, NOT a hard error.
        let mut p = std::env::temp_dir();
        p.push(format!(
            "tc-targets-absent-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        assert!(!p.exists());
        let cfg = TargetsConfig::load(&p).expect("missing file => empty config");
        assert!(cfg.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn pipe_name_matches_supervisor_client_resolution() {
        // Cross-side invariant: the daemon's bind name must equal what the
        // client (supervisor::paths) resolves for the same env. With no custom
        // socket_path and an unseeded env, both must be the legacy username pipe.
        let cfg = DaemonConfig::defaults_in("C:\\tmp\\tc-pipe-test");
        let client = terminal_commander_supervisor::paths::resolve_socket_path();
        assert_eq!(
            cfg.pipe_name(),
            client.to_string_lossy(),
            "daemon pipe_name() must equal client resolve_socket_path()"
        );
    }
}
