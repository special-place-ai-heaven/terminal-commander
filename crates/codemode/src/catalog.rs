// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Canonical 60-op Code Mode function catalog, `OpClass` call-budget tags,
//! Python `TypedDict` stub renderer, and self-contained agent idioms manual.

use serde::{Deserialize, Serialize};

/// Call-budget classification for a sandbox host operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpClass {
    /// Read-only inspection or search (counts toward `max_host_calls` only).
    Read,
    /// Blocking wait or subscription cursor control (counts toward `max_host_calls`;
    /// waits clamp to remaining wall-clock budget).
    Wait,
    /// State-mutating operation (counts toward both `max_host_calls` and `max_mutation_calls`).
    Mutate,
}

/// Every host operation callable inside the Terminal Commander Code Mode Python sandbox.
///
/// Exact 1-to-1 match with the 60 `full`-surface tools in `crates/mcp/src/tools.rs::tool_catalogue()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TcCodeOp {
    // Status, Discovery, Audit, Runtime & Targets (10)
    SystemDiscover,
    Health,
    PolicyStatus,
    SelfCheck,
    AuditSince,
    RuntimeState,
    ProbeList,
    ProbeStatus,
    TargetList,
    TargetProbe,
    // Command & Shell (6)
    CommandStartCombed,
    RunAndWatch,
    CommandStatus,
    CommandStop,
    ShellExec,
    CommandOutputTail,
    // Buckets & Context (4)
    BucketEventsSince,
    BucketWait,
    BucketSummary,
    EventContext,
    // Rule Registry (9)
    RegistrySearch,
    RegistryGet,
    RegistryUpsert,
    RegistryTest,
    RegistryActivate,
    RegistryImportPack,
    RegistryDeactivate,
    RegistryListActive,
    RegistrySuggestFromSamples,
    // Argv Recipes (8)
    RecipeSearch,
    RecipeGet,
    RecipeUpsert,
    RecipeTest,
    RecipeActivate,
    RecipeDeactivate,
    RecipeListActive,
    RecipeRun,
    // Files & File Watches (6)
    FileReadWindow,
    FileSearch,
    FileWrite,
    FileWatchStart,
    FileWatchStop,
    FileWatchList,
    // PTY & Credentials (5)
    PtyCommandStart,
    PtyCommandWriteStdin,
    PtyCommandStop,
    PtyCommandList,
    CredentialRequest,
    // Persistent Shell Sessions & Workspace Snapshots (7)
    ShellSessionStart,
    ShellSessionExec,
    ShellSessionStatus,
    ShellSessionStop,
    ShellSessionList,
    WorkspaceSnapshotCreate,
    WorkspaceSnapshotApply,
    // Subscriptions (5)
    SubscriptionOpen,
    SubscriptionPull,
    SubscriptionList,
    SubscriptionClose,
    SubscriptionSeek,
}

/// Metadata for one keyword parameter on a sandbox function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub py_type: &'static str,
    pub required: bool,
    pub default_repr: &'static str,
    pub doc: &'static str,
}

/// Canonical descriptor for one sandbox function, its domain group, `OpClass`, and `TypedDict` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeOpDescriptor {
    pub op: TcCodeOp,
    pub name: &'static str,
    pub domain: &'static str,
    pub op_class: OpClass,
    pub summary: &'static str,
    pub params: &'static [ParamSpec],
    pub return_type: &'static str,
    pub typed_dicts: &'static str,
    pub keywords: &'static [&'static str],
}

impl TcCodeOp {
    /// All 60 variants in canonical order.
    pub const ALL: &'static [Self] = &[
        Self::SystemDiscover,
        Self::Health,
        Self::PolicyStatus,
        Self::SelfCheck,
        Self::AuditSince,
        Self::RuntimeState,
        Self::ProbeList,
        Self::ProbeStatus,
        Self::TargetList,
        Self::TargetProbe,
        Self::CommandStartCombed,
        Self::RunAndWatch,
        Self::CommandStatus,
        Self::CommandStop,
        Self::ShellExec,
        Self::CommandOutputTail,
        Self::BucketEventsSince,
        Self::BucketWait,
        Self::BucketSummary,
        Self::EventContext,
        Self::RegistrySearch,
        Self::RegistryGet,
        Self::RegistryUpsert,
        Self::RegistryTest,
        Self::RegistryActivate,
        Self::RegistryImportPack,
        Self::RegistryDeactivate,
        Self::RegistryListActive,
        Self::RegistrySuggestFromSamples,
        Self::RecipeSearch,
        Self::RecipeGet,
        Self::RecipeUpsert,
        Self::RecipeTest,
        Self::RecipeActivate,
        Self::RecipeDeactivate,
        Self::RecipeListActive,
        Self::RecipeRun,
        Self::FileReadWindow,
        Self::FileSearch,
        Self::FileWrite,
        Self::FileWatchStart,
        Self::FileWatchStop,
        Self::FileWatchList,
        Self::PtyCommandStart,
        Self::PtyCommandWriteStdin,
        Self::PtyCommandStop,
        Self::PtyCommandList,
        Self::CredentialRequest,
        Self::ShellSessionStart,
        Self::ShellSessionExec,
        Self::ShellSessionStatus,
        Self::ShellSessionStop,
        Self::ShellSessionList,
        Self::WorkspaceSnapshotCreate,
        Self::WorkspaceSnapshotApply,
        Self::SubscriptionOpen,
        Self::SubscriptionPull,
        Self::SubscriptionList,
        Self::SubscriptionClose,
        Self::SubscriptionSeek,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemDiscover => "system_discover",
            Self::Health => "health",
            Self::PolicyStatus => "policy_status",
            Self::SelfCheck => "self_check",
            Self::AuditSince => "audit_since",
            Self::RuntimeState => "runtime_state",
            Self::ProbeList => "probe_list",
            Self::ProbeStatus => "probe_status",
            Self::TargetList => "target_list",
            Self::TargetProbe => "target_probe",
            Self::CommandStartCombed => "command_start_combed",
            Self::RunAndWatch => "run_and_watch",
            Self::CommandStatus => "command_status",
            Self::CommandStop => "command_stop",
            Self::ShellExec => "shell_exec",
            Self::CommandOutputTail => "command_output_tail",
            Self::BucketEventsSince => "bucket_events_since",
            Self::BucketWait => "bucket_wait",
            Self::BucketSummary => "bucket_summary",
            Self::EventContext => "event_context",
            Self::RegistrySearch => "registry_search",
            Self::RegistryGet => "registry_get",
            Self::RegistryUpsert => "registry_upsert",
            Self::RegistryTest => "registry_test",
            Self::RegistryActivate => "registry_activate",
            Self::RegistryImportPack => "registry_import_pack",
            Self::RegistryDeactivate => "registry_deactivate",
            Self::RegistryListActive => "registry_list_active",
            Self::RegistrySuggestFromSamples => "registry_suggest_from_samples",
            Self::RecipeSearch => "recipe_search",
            Self::RecipeGet => "recipe_get",
            Self::RecipeUpsert => "recipe_upsert",
            Self::RecipeTest => "recipe_test",
            Self::RecipeActivate => "recipe_activate",
            Self::RecipeDeactivate => "recipe_deactivate",
            Self::RecipeListActive => "recipe_list_active",
            Self::RecipeRun => "recipe_run",
            Self::FileReadWindow => "file_read_window",
            Self::FileSearch => "file_search",
            Self::FileWrite => "file_write",
            Self::FileWatchStart => "file_watch_start",
            Self::FileWatchStop => "file_watch_stop",
            Self::FileWatchList => "file_watch_list",
            Self::PtyCommandStart => "pty_command_start",
            Self::PtyCommandWriteStdin => "pty_command_write_stdin",
            Self::PtyCommandStop => "pty_command_stop",
            Self::PtyCommandList => "pty_command_list",
            Self::CredentialRequest => "credential_request",
            Self::ShellSessionStart => "shell_session_start",
            Self::ShellSessionExec => "shell_session_exec",
            Self::ShellSessionStatus => "shell_session_status",
            Self::ShellSessionStop => "shell_session_stop",
            Self::ShellSessionList => "shell_session_list",
            Self::WorkspaceSnapshotCreate => "workspace_snapshot_create",
            Self::WorkspaceSnapshotApply => "workspace_snapshot_apply",
            Self::SubscriptionOpen => "subscription_open",
            Self::SubscriptionPull => "subscription_pull",
            Self::SubscriptionList => "subscription_list",
            Self::SubscriptionClose => "subscription_close",
            Self::SubscriptionSeek => "subscription_seek",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "system_discover" => Some(Self::SystemDiscover),
            "health" => Some(Self::Health),
            "policy_status" => Some(Self::PolicyStatus),
            "self_check" => Some(Self::SelfCheck),
            "audit_since" => Some(Self::AuditSince),
            "runtime_state" => Some(Self::RuntimeState),
            "probe_list" => Some(Self::ProbeList),
            "probe_status" => Some(Self::ProbeStatus),
            "target_list" => Some(Self::TargetList),
            "target_probe" => Some(Self::TargetProbe),
            "command_start_combed" => Some(Self::CommandStartCombed),
            "run_and_watch" => Some(Self::RunAndWatch),
            "command_status" => Some(Self::CommandStatus),
            "command_stop" => Some(Self::CommandStop),
            "shell_exec" => Some(Self::ShellExec),
            "command_output_tail" => Some(Self::CommandOutputTail),
            "bucket_events_since" => Some(Self::BucketEventsSince),
            "bucket_wait" => Some(Self::BucketWait),
            "bucket_summary" => Some(Self::BucketSummary),
            "event_context" => Some(Self::EventContext),
            "registry_search" => Some(Self::RegistrySearch),
            "registry_get" => Some(Self::RegistryGet),
            "registry_upsert" => Some(Self::RegistryUpsert),
            "registry_test" => Some(Self::RegistryTest),
            "registry_activate" => Some(Self::RegistryActivate),
            "registry_import_pack" => Some(Self::RegistryImportPack),
            "registry_deactivate" => Some(Self::RegistryDeactivate),
            "registry_list_active" => Some(Self::RegistryListActive),
            "registry_suggest_from_samples" => Some(Self::RegistrySuggestFromSamples),
            "recipe_search" => Some(Self::RecipeSearch),
            "recipe_get" => Some(Self::RecipeGet),
            "recipe_upsert" => Some(Self::RecipeUpsert),
            "recipe_test" => Some(Self::RecipeTest),
            "recipe_activate" => Some(Self::RecipeActivate),
            "recipe_deactivate" => Some(Self::RecipeDeactivate),
            "recipe_list_active" => Some(Self::RecipeListActive),
            "recipe_run" => Some(Self::RecipeRun),
            "file_read_window" => Some(Self::FileReadWindow),
            "file_search" => Some(Self::FileSearch),
            "file_write" => Some(Self::FileWrite),
            "file_watch_start" => Some(Self::FileWatchStart),
            "file_watch_stop" => Some(Self::FileWatchStop),
            "file_watch_list" => Some(Self::FileWatchList),
            "pty_command_start" => Some(Self::PtyCommandStart),
            "pty_command_write_stdin" => Some(Self::PtyCommandWriteStdin),
            "pty_command_stop" => Some(Self::PtyCommandStop),
            "pty_command_list" => Some(Self::PtyCommandList),
            "credential_request" => Some(Self::CredentialRequest),
            "shell_session_start" => Some(Self::ShellSessionStart),
            "shell_session_exec" => Some(Self::ShellSessionExec),
            "shell_session_status" => Some(Self::ShellSessionStatus),
            "shell_session_stop" => Some(Self::ShellSessionStop),
            "shell_session_list" => Some(Self::ShellSessionList),
            "workspace_snapshot_create" => Some(Self::WorkspaceSnapshotCreate),
            "workspace_snapshot_apply" => Some(Self::WorkspaceSnapshotApply),
            "subscription_open" => Some(Self::SubscriptionOpen),
            "subscription_pull" => Some(Self::SubscriptionPull),
            "subscription_list" => Some(Self::SubscriptionList),
            "subscription_close" => Some(Self::SubscriptionClose),
            "subscription_seek" => Some(Self::SubscriptionSeek),
            _ => None,
        }
    }

    #[must_use]
    pub const fn op_class(self) -> OpClass {
        match self {
            Self::SystemDiscover
            | Self::Health
            | Self::PolicyStatus
            | Self::SelfCheck
            | Self::AuditSince
            | Self::RuntimeState
            | Self::ProbeList
            | Self::ProbeStatus
            | Self::TargetList
            | Self::TargetProbe
            | Self::CommandStatus
            | Self::CommandOutputTail
            | Self::BucketEventsSince
            | Self::BucketSummary
            | Self::EventContext
            | Self::RegistrySearch
            | Self::RegistryGet
            | Self::RegistryTest
            | Self::RegistryListActive
            | Self::RegistrySuggestFromSamples
            | Self::RecipeSearch
            | Self::RecipeGet
            | Self::RecipeTest
            | Self::RecipeListActive
            | Self::FileReadWindow
            | Self::FileSearch
            | Self::FileWatchList
            | Self::PtyCommandList
            | Self::ShellSessionStatus
            | Self::ShellSessionList
            | Self::SubscriptionList => OpClass::Read,

            Self::BucketWait
            | Self::SubscriptionOpen
            | Self::SubscriptionPull
            | Self::SubscriptionClose
            | Self::SubscriptionSeek => OpClass::Wait,

            Self::CommandStartCombed
            | Self::RunAndWatch
            | Self::CommandStop
            | Self::ShellExec
            | Self::RegistryUpsert
            | Self::RegistryActivate
            | Self::RegistryImportPack
            | Self::RegistryDeactivate
            | Self::RecipeUpsert
            | Self::RecipeActivate
            | Self::RecipeDeactivate
            | Self::RecipeRun
            | Self::FileWrite
            | Self::FileWatchStart
            | Self::FileWatchStop
            | Self::PtyCommandStart
            | Self::PtyCommandWriteStdin
            | Self::PtyCommandStop
            | Self::CredentialRequest
            | Self::ShellSessionStart
            | Self::ShellSessionExec
            | Self::ShellSessionStop
            | Self::WorkspaceSnapshotCreate
            | Self::WorkspaceSnapshotApply => OpClass::Mutate,
        }
    }

    #[must_use]
    pub fn descriptor(self) -> &'static CodeOpDescriptor {
        TC_CODE_OPS
            .iter()
            .find(|desc| desc.op == self)
            .expect("every TcCodeOp has a canonical descriptor")
    }

    #[must_use]
    pub fn signature_one_liner(self) -> String {
        render_signature_line(self.descriptor())
    }
}

impl std::fmt::Display for TcCodeOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const SIGNAL_TYPED_DICTS: &str = "\
class SignalEvent(TypedDict):
    event_id: str          # pass to event_context(event_id=...)
    bucket_id: str
    seq: int
    timestamp: str
    severity: str          # trace|debug|info|low|medium|high|critical
    kind: str
    summary: str
    rule: dict | None      # {'id': str, 'version': int, 'registry_id'?: str}
    source: dict           # {'probe_id': str, 'source_type': str, 'stream': str, 'job_id'?: str}
    captures: dict[str, str] | None
    pointer: dict | None   # {'frame_id': str, 'line'?: int, 'stream'?: str, 'context_available': bool}
    pointer_unavailable_reason: str | None";

const COMMAND_START_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "argv",
        py_type: "list[str]",
        required: true,
        default_repr: "",
        doc: "Non-empty argv array, e.g. ['cargo', 'test'].",
    },
    ParamSpec {
        name: "cwd",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional working directory.",
    },
    ParamSpec {
        name: "env",
        py_type: "list[dict[str, str]]",
        required: false,
        default_repr: "[]",
        doc: "Array of {'key': 'NAME', 'value': 'VAL'} objects (merged onto inherited env).",
    },
    ParamSpec {
        name: "grace_ms",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "Grace window (ms) between graceful and forced stop.",
    },
    ParamSpec {
        name: "bucket_config_json",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional JSON string {'max_events': N, 'ttl': seconds}.",
    },
    ParamSpec {
        name: "rules",
        py_type: "list[dict] | None",
        required: false,
        default_repr: "None",
        doc: "Inline comb rules, minimal [{'pattern': 'ERROR'}] or [{'keywords': ['FAILED']}].",
    },
    ParamSpec {
        name: "rules_json",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Deprecated JSON-array string form of rules; prefer rules=[...].",
    },
    ParamSpec {
        name: "tag",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional per-bucket tag for subscription routing.",
    },
    ParamSpec {
        name: "strip_ansi",
        py_type: "bool",
        required: false,
        default_repr: "True",
        doc: "Strip ANSI escapes before rule matching (default True).",
    },
    ParamSpec {
        name: "receipt_head_lines",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "First lines shown in quiet receipt head (0..=20).",
    },
    ParamSpec {
        name: "receipt_tail_lines",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "Last lines shown in quiet receipt tail (0..=50, default 5).",
    },
    ParamSpec {
        name: "target_id",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional registered remote federation target_id.",
    },
    ParamSpec {
        name: "limits",
        py_type: "dict | None",
        required: false,
        default_repr: "None",
        doc: "Optional {'memory': '24GiB'|'40%'|'none', 'priority': 'idle'|'below_normal'|'normal'}.",
    },
];

const RUN_AND_WATCH_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "argv",
        py_type: "list[str]",
        required: true,
        default_repr: "",
        doc: "Non-empty argv array, e.g. ['cargo', 'test'].",
    },
    ParamSpec {
        name: "cwd",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional working directory.",
    },
    ParamSpec {
        name: "env",
        py_type: "list[dict[str, str]]",
        required: false,
        default_repr: "[]",
        doc: "Array of {'key': 'NAME', 'value': 'VAL'} objects.",
    },
    ParamSpec {
        name: "grace_ms",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "Grace window (ms) between graceful and forced stop.",
    },
    ParamSpec {
        name: "bucket_config_json",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional JSON string {'max_events': N, 'ttl': seconds}.",
    },
    ParamSpec {
        name: "rules",
        py_type: "list[dict] | None",
        required: false,
        default_repr: "None",
        doc: "Inline comb rules, minimal [{'pattern': 'ERROR'}].",
    },
    ParamSpec {
        name: "rules_json",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Deprecated JSON-array string form of rules; prefer rules=[...].",
    },
    ParamSpec {
        name: "wait_ms",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "Max wait budget in ms (default 5000, cap 60000; clamped to remaining script wall time). NOT timeout_ms.",
    },
    ParamSpec {
        name: "max_signals",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "Max signals returned (default 50, cap 500).",
    },
    ParamSpec {
        name: "tag",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional per-bucket tag for subscription routing.",
    },
    ParamSpec {
        name: "strip_ansi",
        py_type: "bool",
        required: false,
        default_repr: "True",
        doc: "Strip ANSI escapes before rule matching (default True).",
    },
    ParamSpec {
        name: "receipt_head_lines",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "First lines shown in quiet receipt head (0..=20).",
    },
    ParamSpec {
        name: "receipt_tail_lines",
        py_type: "int | None",
        required: false,
        default_repr: "None",
        doc: "Last lines shown in quiet receipt tail (0..=50, default 5).",
    },
    ParamSpec {
        name: "compact",
        py_type: "bool",
        required: false,
        default_repr: "False",
        doc: "When True, project signals to {'summary', 'stream', 'seq', 'severity'}.",
    },
    ParamSpec {
        name: "wait_until",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Pass 'exit' to keep waiting until terminal state even after max_signals.",
    },
    ParamSpec {
        name: "target_id",
        py_type: "str | None",
        required: false,
        default_repr: "None",
        doc: "Optional registered remote federation target_id.",
    },
    ParamSpec {
        name: "limits",
        py_type: "dict | None",
        required: false,
        default_repr: "None",
        doc: "Optional {'memory': '24GiB'|'40%'|'none', 'priority': 'idle'|'below_normal'|'normal'}.",
    },
];

/// All 60 canonical operations exposed in Terminal Commander Code Mode.
pub const TC_CODE_OPS: &[CodeOpDescriptor] = &[
    // -----------------------------------------------------------------
    // Domain: runtime_audit_federation (10)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::SystemDiscover,
        name: "system_discover",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Discover OS, terminal, shell/WSL/tool probes, ranked access routes, and policy availability.",
        params: &[ParamSpec {
            name: "detail",
            py_type: "str",
            required: false,
            default_repr: "\"summary\"",
            doc: "'summary' (default) or 'full'.",
        }],
        return_type: "SystemDiscoverOut",
        typed_dicts: "class SystemDiscoverOut(TypedDict):\n    version: str\n    mcp_spec: str\n    policy_profile: str\n    methods: list[str]\n    environment: dict\n    capabilities: list[dict]",
        keywords: &[
            "discover",
            "system",
            "os",
            "shell",
            "wsl",
            "routes",
            "tools",
            "environment",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::Health,
        name: "health",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Daemon liveness ping returning uptime seconds, version, and engine identity.",
        params: &[],
        return_type: "HealthOut",
        typed_dicts: "class HealthOut(TypedDict):\n    ok: bool\n    uptime_secs: int\n    version: str\n    identity: dict | None",
        keywords: &["health", "ping", "uptime", "alive", "liveness", "version"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::PolicyStatus,
        name: "policy_status",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Report active policy profile, resolved capability flags, and bounded per-call caps.",
        params: &[],
        return_type: "PolicyStatusOut",
        typed_dicts: "class PolicyStatusOut(TypedDict):\n    profile: str\n    commands_deny_count: int\n    default_deny_path_suffix_count: int\n    file_window_bytes: int\n    bucket_read_limit: int\n    llm_can_activate_recipes: bool\n    bucket_wait_max_ms: int\n    run_and_watch_max_wait_ms: int\n    caps: dict[str, bool]\n    config_warnings: list[str]\n    governor: dict | None",
        keywords: &[
            "policy",
            "caps",
            "allow_shell",
            "allow_session",
            "allow_remote",
            "profile",
            "governor",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::SelfCheck,
        name: "self_check",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Re-run the daemon self-check and return failure count plus bounded text report.",
        params: &[],
        return_type: "SelfCheckOut",
        typed_dicts: "class SelfCheckOut(TypedDict):\n    failures: int\n    report: str",
        keywords: &["self_check", "diagnostics", "check", "report", "verify"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::AuditSince,
        name: "audit_since",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Cursor-paged read of persistent daemon audit rows after cursor.",
        params: &[
            ParamSpec {
                name: "cursor",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Return rows with audit_id > cursor (None or 0 starts from beginning).",
            },
            ParamSpec {
                name: "action_filter",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Exact action name filter, e.g. 'file_write' or 'command_stop'.",
            },
            ParamSpec {
                name: "decision_filter",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Exact decision filter, e.g. 'allow', 'deny', 'info', 'error'.",
            },
            ParamSpec {
                name: "limit",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Maximum rows returned.",
            },
        ],
        return_type: "AuditSinceOut",
        typed_dicts: "class AuditRow(TypedDict):\n    audit_id: int\n    timestamp: str\n    action: str\n    subject: str\n    decision: str\n    profile: str | None\n    reason: str | None\n\nclass AuditSinceOut(TypedDict):\n    cursor_in: int\n    next_cursor: int\n    rows: list[AuditRow]",
        keywords: &["audit", "log", "history", "security", "decisions", "trail"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RuntimeState,
        name: "runtime_state",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Bounded aggregate runtime snapshot of probes, buckets, and active rule scopes.",
        params: &[ParamSpec {
            name: "limit",
            py_type: "int | None",
            required: false,
            default_repr: "None",
            doc: "Max rows per list (pass 0 for counts-only snapshot).",
        }],
        return_type: "RuntimeStateOut",
        typed_dicts: "class RuntimeStateOut(TypedDict):\n    command_jobs: int\n    pty_jobs: int\n    file_watches: int\n    bucket_count: int\n    active_rules_count: int\n    probes: list[dict]\n    buckets: list[dict]\n    active_rules: list[dict]\n    probes_truncated: bool\n    buckets_truncated: bool\n    active_rules_truncated: bool",
        keywords: &[
            "runtime", "state", "snapshot", "jobs", "buckets", "overview",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ProbeList,
        name: "probe_list",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Flat list of live probes across command, PTY, and file-watch runtimes.",
        params: &[ParamSpec {
            name: "limit",
            py_type: "int | None",
            required: false,
            default_repr: "None",
            doc: "Max probes returned.",
        }],
        return_type: "ProbeListOut",
        typed_dicts: "class ProbeEntry(TypedDict):\n    kind: str              # command|file_watch|pty\n    job_id: str\n    bucket_id: str\n    probe_id: str\n    frames_total: int\n    events_emitted: int\n    liveness: dict\n\nclass ProbeListOut(TypedDict):\n    probes: list[ProbeEntry]\n    truncated: bool",
        keywords: &["probe", "probes", "list", "live", "running"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ProbeStatus,
        name: "probe_status",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Bounded per-probe lookup by probe_id.",
        params: &[ParamSpec {
            name: "probe_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque probe_id (prb_<32hex>).",
        }],
        return_type: "ProbeStatusOut",
        typed_dicts: "class ProbeStatusOut(TypedDict):\n    probe: dict",
        keywords: &["probe", "status", "liveness", "metrics"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::TargetList,
        name: "target_list",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "List registered remote-federation targets and local-forward reachability.",
        params: &[],
        return_type: "TargetListOut",
        typed_dicts: "class TargetEntry(TypedDict):\n    target_id: str\n    host: str\n    transport: str\n    local_forward_socket: str\n    reachable: bool\n\nclass TargetListOut(TypedDict):\n    targets: list[TargetEntry]",
        keywords: &["target", "targets", "remote", "federation", "ssh"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::TargetProbe,
        name: "target_probe",
        domain: "runtime_audit_federation",
        op_class: OpClass::Read,
        summary: "Probe one registered remote target's health over its local forward socket.",
        params: &[ParamSpec {
            name: "target_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Registered target_id from target_list().",
        }],
        return_type: "TargetProbeOut",
        typed_dicts: "class TargetProbeOut(TypedDict):\n    target_id: str\n    reachable: bool\n    daemon_version: str | None",
        keywords: &["target", "probe", "remote", "health", "version"],
    },
    // -----------------------------------------------------------------
    // Domain: command (6)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::CommandStartCombed,
        name: "command_start_combed",
        domain: "command",
        op_class: OpClass::Mutate,
        summary: "Start a background non-PTY argv command with combed output; returns job_id + bucket_id + cursor immediately.",
        params: COMMAND_START_PARAMS,
        return_type: "CommandStartOut",
        typed_dicts: "class CommandStartOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    probe_id: str\n    cursor: int",
        keywords: &[
            "command",
            "start",
            "spawn",
            "background",
            "combed",
            "argv",
            "job",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RunAndWatch,
        name: "run_and_watch",
        domain: "command",
        op_class: OpClass::Mutate,
        summary: "Start an argv command, wait up to wait_ms for rule signals and exit, and return both in one call.",
        params: RUN_AND_WATCH_PARAMS,
        return_type: "RunAndWatchOut",
        typed_dicts: "class CommandReceipt(TypedDict):\n    exit_code: int | None\n    lines_suppressed: int\n    head: list[str] | None\n    tail: list[str]\n    lines_omitted: int | None\n    tail_incomplete: bool\n\nclass RunAndWatchOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    state: str             # running|exited|failed|cancelled|unknown\n    exit_code: int | None\n    pipeline_exit_masked: bool\n    signals: list[dict]\n    signal_count: int\n    signals_capped: bool\n    compact: bool\n    receipt: CommandReceipt | None\n    complete: bool\n    wait_exhausted: bool\n    wait_cap_ms: int\n    poll_hint_ms: int | None\n    cursor: int            # pass to bucket_wait(bucket_id=..., cursor=...)\n    degraded: bool\n    recover_hint: str | None\n    outcome_trust: str | None",
        keywords: &[
            "run",
            "watch",
            "command",
            "exec",
            "argv",
            "test",
            "build",
            "signals",
            "exit_code",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::CommandStatus,
        name: "command_status",
        domain: "command",
        op_class: OpClass::Read,
        summary: "Lookup lifecycle state, counters, exit_code, receipt, and outcome_trust for a job_id.",
        params: &[
            ParamSpec {
                name: "job_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque job_id (job_<32hex>).",
            },
            ParamSpec {
                name: "target_id",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional remote target_id owning the job.",
            },
        ],
        return_type: "CommandStatusOut",
        typed_dicts: "class CommandStatusOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    probe_id: str\n    state: str             # starting|running|exited|failed|cancelled\n    frames_total: int\n    frames_stdout: int\n    frames_stderr: int\n    bytes_total: int\n    events_emitted: int\n    frames_suppressed: int\n    exit_code: int | None\n    signal: str | None\n    duration_ms: int | None\n    receipt: dict | None\n    restarted: bool\n    outcome_trust: str     # observed|reconstructed|abandoned\n    pipeline_exit_masked: bool",
        keywords: &[
            "command",
            "status",
            "exit_code",
            "state",
            "job",
            "receipt",
            "poll",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::CommandStop,
        name: "command_stop",
        domain: "command",
        op_class: OpClass::Mutate,
        summary: "Force-kill a running combed (non-PTY) command by job_id and return final counters.",
        params: &[
            ParamSpec {
                name: "job_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque job_id (job_<32hex>).",
            },
            ParamSpec {
                name: "target_id",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional remote target_id owning the job.",
            },
        ],
        return_type: "CommandStopOut",
        typed_dicts: "class CommandStopOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    frames_total: int\n    events_emitted: int\n    bytes_total: int",
        keywords: &["command", "stop", "kill", "cancel", "terminate", "job"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ShellExec,
        name: "shell_exec",
        domain: "command",
        op_class: OpClass::Mutate,
        summary: "Start ONE shell line (pipelines/compounds/redirects) via allow_shell; returns job_id + bucket_id + cursor.",
        params: &[
            ParamSpec {
                name: "shell_line",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Shell pipeline/compound line to execute.",
            },
            ParamSpec {
                name: "shell",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional interpreter override, e.g. '/bin/bash'.",
            },
            ParamSpec {
                name: "cwd",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional working directory.",
            },
            ParamSpec {
                name: "env",
                py_type: "list[dict[str, str]]",
                required: false,
                default_repr: "[]",
                doc: "Array of {'key': 'NAME', 'value': 'VAL'} objects.",
            },
            ParamSpec {
                name: "rules",
                py_type: "list[dict] | None",
                required: false,
                default_repr: "None",
                doc: "Inline comb rules, minimal [{'pattern': 'ERROR'}].",
            },
            ParamSpec {
                name: "rules_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Deprecated JSON-array string form of rules.",
            },
            ParamSpec {
                name: "bucket_config_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional JSON string {'max_events': N, 'ttl': seconds}.",
            },
            ParamSpec {
                name: "tag",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional per-bucket tag.",
            },
            ParamSpec {
                name: "receipt_head_lines",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "First lines shown in quiet receipt head (0..=20).",
            },
            ParamSpec {
                name: "receipt_tail_lines",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Last lines shown in quiet receipt tail (0..=50).",
            },
            ParamSpec {
                name: "wait_ms",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Reserved forward-parity wait parameter (shell_exec starts immediately; poll with bucket_wait/command_status).",
            },
            ParamSpec {
                name: "limits",
                py_type: "dict | None",
                required: false,
                default_repr: "None",
                doc: "Optional {'memory': '24GiB'|'40%'|'none', 'priority': 'idle'|'below_normal'|'normal'}.",
            },
        ],
        return_type: "ShellExecOut",
        typed_dicts: "class ShellExecOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    probe_id: str\n    cursor: int",
        keywords: &[
            "shell", "pipeline", "redirect", "bash", "sh", "pwsh", "exec",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::CommandOutputTail,
        name: "command_output_tail",
        domain: "command",
        op_class: OpClass::Read,
        summary: "Read the last N lines of a job's captured output without a rule (bounded 200 lines / 64 KiB).",
        params: &[
            ParamSpec {
                name: "job_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque job_id (job_<32hex>).",
            },
            ParamSpec {
                name: "max_lines",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max lines to return (default 50, cap 200).",
            },
            ParamSpec {
                name: "max_bytes",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max bytes to return (default/cap 65536).",
            },
            ParamSpec {
                name: "strip_ansi",
                py_type: "bool",
                required: false,
                default_repr: "False",
                doc: "Strip ANSI escapes from returned lines.",
            },
        ],
        return_type: "CommandOutputTailOut",
        typed_dicts: "class CommandOutputTailOut(TypedDict):\n    job_id: str\n    lines: list[str]\n    returned_lines: int\n    truncated_lines: bool\n    truncated_bytes: bool\n    evicted_frames: int",
        keywords: &["tail", "output", "lines", "stdout", "stderr", "raw", "last"],
    },
    // -----------------------------------------------------------------
    // Domain: bucket_events (4)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::BucketEventsSince,
        name: "bucket_events_since",
        domain: "bucket_events",
        op_class: OpClass::Read,
        summary: "Non-blocking cursor read of signal events in a bucket with severity/kind filtering.",
        params: &[
            ParamSpec {
                name: "bucket_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque bucket_id (bkt_<32hex>).",
            },
            ParamSpec {
                name: "cursor",
                py_type: "int",
                required: true,
                default_repr: "",
                doc: "Pagination cursor (0 on first call, then next_cursor).",
            },
            ParamSpec {
                name: "severity_min",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "trace|debug|info|low|medium|high|critical (aliases error->high, warn->medium, fatal->critical).",
            },
            ParamSpec {
                name: "kind_filter",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Exact event kind filter.",
            },
            ParamSpec {
                name: "limit",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max events to return.",
            },
            ParamSpec {
                name: "compact",
                py_type: "bool",
                required: false,
                default_repr: "False",
                doc: "Project events to {'summary', 'stream', 'seq', 'severity'}.",
            },
        ],
        return_type: "BucketEventsOut",
        typed_dicts: "class BucketEventsOut(TypedDict):\n    bucket_id: str\n    cursor_in: int\n    next_cursor: int\n    has_more: bool\n    dropped_count: int\n    events: list[SignalEvent]",
        keywords: &[
            "bucket", "events", "signals", "cursor", "since", "read", "severity",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::BucketWait,
        name: "bucket_wait",
        domain: "bucket_events",
        op_class: OpClass::Wait,
        summary: "Blocking realtime wait on a bucket; returns matching events or a timeout heartbeat.",
        params: &[
            ParamSpec {
                name: "bucket_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque bucket_id (bkt_<32hex>). NOT job_id.",
            },
            ParamSpec {
                name: "cursor",
                py_type: "int",
                required: true,
                default_repr: "",
                doc: "Pagination cursor (0 or prior next_cursor / cursor).",
            },
            ParamSpec {
                name: "severity_min",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "trace|debug|info|low|medium|high|critical.",
            },
            ParamSpec {
                name: "kind_filter",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Exact event kind filter.",
            },
            ParamSpec {
                name: "limit",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max events to return.",
            },
            ParamSpec {
                name: "max_signals",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Signal cap (stricter of limit and max_signals wins).",
            },
            ParamSpec {
                name: "timeout_ms",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Wait timeout in ms (clamped to remaining script wall time). NOT wait_ms.",
            },
            ParamSpec {
                name: "wake_mode",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "'any' (default: wake on first append) or 'match' (wait for matching event, job exit, or timeout).",
            },
            ParamSpec {
                name: "compact",
                py_type: "bool",
                required: false,
                default_repr: "False",
                doc: "Project events to {'summary', 'stream', 'seq', 'severity'}.",
            },
        ],
        return_type: "BucketWaitOut",
        typed_dicts: "class BucketWaitOut(TypedDict):\n    bucket_id: str\n    cursor_in: int\n    next_cursor: int\n    heartbeat: bool\n    wake_reason: str\n    dropped_count: int\n    events: list[SignalEvent]",
        keywords: &[
            "bucket",
            "wait",
            "poll",
            "block",
            "signals",
            "events",
            "heartbeat",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::BucketSummary,
        name: "bucket_summary",
        domain: "bucket_events",
        op_class: OpClass::Read,
        summary: "Return bucket sequence bounds, total/dropped counts, and severity histogram.",
        params: &[ParamSpec {
            name: "bucket_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque bucket_id (bkt_<32hex>).",
        }],
        return_type: "BucketSummaryOut",
        typed_dicts: "class BucketSummaryOut(TypedDict):\n    bucket_id: str\n    head_seq: int\n    tail_seq: int\n    event_count: int\n    dropped_count: int\n    by_severity: dict[str, int]",
        keywords: &["bucket", "summary", "histogram", "counts", "severity"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::EventContext,
        name: "event_context",
        domain: "bucket_events",
        op_class: OpClass::Read,
        summary: "Fetch bounded surrounding output frames around a signal event_id.",
        params: &[
            ParamSpec {
                name: "event_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque event_id (evt_<32hex>) from SignalEvent['event_id'].",
            },
            ParamSpec {
                name: "bucket_id",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional owning bucket_id (omitted resolves from event_id).",
            },
            ParamSpec {
                name: "before",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Context frames before anchor.",
            },
            ParamSpec {
                name: "after",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Context frames after anchor.",
            },
            ParamSpec {
                name: "max_bytes",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max context payload bytes.",
            },
        ],
        return_type: "EventContextOut",
        typed_dicts: "class ContextFrame(TypedDict):\n    probe_id: str\n    frame_id: str\n    stream: str\n    line: int | None\n    text: str\n\nclass EventContextOut(TypedDict):\n    bucket_id: str\n    event_id: str\n    anchor_missing: bool\n    unavailable_reason: str | None\n    pointer_unavailable_reason: str | None\n    frames: list[ContextFrame]\n    total_bytes: int\n    truncated: bool",
        keywords: &[
            "event", "context", "frames", "lines", "before", "after", "pointer",
        ],
    },
    // -----------------------------------------------------------------
    // Domain: registry (9)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::RegistrySearch,
        name: "registry_search",
        domain: "registry",
        op_class: OpClass::Read,
        summary: "FTS search over persisted comb rule definitions.",
        params: &[
            ParamSpec {
                name: "query",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "FTS search query.",
            },
            ParamSpec {
                name: "limit",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Result cap.",
            },
        ],
        return_type: "RegistrySearchOut",
        typed_dicts: "class RegistrySearchHit(TypedDict):\n    rule_id: str\n    version: int\n    event_kind: str\n    summary_template: str\n    tags: list[str]\n    severity: str\n    status: str\n\nclass RegistrySearchOut(TypedDict):\n    hits: list[RegistrySearchHit]",
        keywords: &["registry", "rule", "rules", "search", "fts"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryGet,
        name: "registry_get",
        domain: "registry",
        op_class: OpClass::Read,
        summary: "Fetch full comb rule definition by rule_id and optional version.",
        params: &[
            ParamSpec {
                name: "rule_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Rule identifier.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional version (omit for latest).",
            },
        ],
        return_type: "RegistryGetOut",
        typed_dicts: "class RegistryGetOut(TypedDict):\n    definition: dict",
        keywords: &["registry", "rule", "get", "definition"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryUpsert,
        name: "registry_upsert",
        domain: "registry",
        op_class: OpClass::Mutate,
        summary: "Insert a new immutable (rule_id, version+1) comb rule from a JSON RuleDefinition string.",
        params: &[ParamSpec {
            name: "definition_json",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "JSON-encoded RuleDefinition string (id, version, kind, severity, event_kind, summary_template, pattern/keywords).",
        }],
        return_type: "RegistryUpsertOut",
        typed_dicts: "class RegistryUpsertOut(TypedDict):\n    rule_id: str\n    version: int",
        keywords: &["registry", "rule", "upsert", "create", "save"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryTest,
        name: "registry_test",
        domain: "registry",
        op_class: OpClass::Read,
        summary: "Dry-run a stored comb rule against sample texts and its own examples.",
        params: &[
            ParamSpec {
                name: "rule_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Stored rule_id from registry_upsert / registry_search.",
            },
            ParamSpec {
                name: "samples",
                py_type: "list[dict]",
                required: true,
                default_repr: "",
                doc: "List of {'text': str, 'stream'?: 'stdout'|'stderr'|'meta'}.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional rule version (omit for latest).",
            },
        ],
        return_type: "RegistryTestOut",
        typed_dicts: "class RegistryTestOut(TypedDict):\n    matches: list[dict]\n    truncated_bytes: int\n    examples_evaluated: int\n    example_results: list[dict]\n    stream_mismatches: list[int]",
        keywords: &["registry", "rule", "test", "dry_run", "samples", "matches"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryActivate,
        name: "registry_activate",
        domain: "registry",
        op_class: OpClass::Mutate,
        summary: "Activate a comb rule under an explicit scope ({'kind': 'global'} | bucket | job | probe).",
        params: &[
            ParamSpec {
                name: "rule_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Rule identifier.",
            },
            ParamSpec {
                name: "scope",
                py_type: "dict",
                required: true,
                default_repr: "",
                doc: "Required scope object, e.g. {'kind': 'global'} or {'kind': 'job', 'job_id': '...'}.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional version (omit for latest).",
            },
        ],
        return_type: "RegistryActivateOut",
        typed_dicts: "class RegistryActivateOut(TypedDict):\n    rule_id: str\n    version: int\n    was_already_active: bool\n    scope: dict\n    jobs_rebound: int\n    superseded_versions: list[int]",
        keywords: &["registry", "rule", "activate", "enable", "scope"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryImportPack,
        name: "registry_import_pack",
        domain: "registry",
        op_class: OpClass::Mutate,
        summary: "Import one of the 25 curated comb rule packs (cargo, pytest, npm, gcc, docker, git, etc.).",
        params: &[
            ParamSpec {
                name: "pack",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Pack name, e.g. 'cargo', 'pytest', 'npm', 'gcc', 'generic.terminal', 'cleanup'.",
            },
            ParamSpec {
                name: "activate",
                py_type: "bool",
                required: false,
                default_repr: "False",
                doc: "Promote and activate imported rules immediately (requires scope when True).",
            },
            ParamSpec {
                name: "scope",
                py_type: "dict | None",
                required: false,
                default_repr: "None",
                doc: "Required when activate=True, e.g. {'kind': 'global'}.",
            },
        ],
        return_type: "RegistryImportPackOut",
        typed_dicts: "class RegistryImportPackOut(TypedDict):\n    pack: str\n    imported: list[str]\n    skipped: list[str]\n    activated: list[str]\n    failed: list[dict]",
        keywords: &[
            "registry", "pack", "import", "cargo", "pytest", "npm", "rules",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryDeactivate,
        name: "registry_deactivate",
        domain: "registry",
        op_class: OpClass::Mutate,
        summary: "Deactivate comb rules under a scope by rule_id, rule_ids, or pack (provide exactly one selector).",
        params: &[
            ParamSpec {
                name: "scope",
                py_type: "dict",
                required: true,
                default_repr: "",
                doc: "Required scope matching activation, e.g. {'kind': 'global'}.",
            },
            ParamSpec {
                name: "rule_id",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Single rule_id selector.",
            },
            ParamSpec {
                name: "rule_ids",
                py_type: "list[str] | None",
                required: false,
                default_repr: "None",
                doc: "Bulk list of rule_ids selector.",
            },
            ParamSpec {
                name: "pack",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Seed pack name selector.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional version (valid only with single rule_id; omit for latest).",
            },
        ],
        return_type: "RegistryDeactivateOut",
        typed_dicts: "class RegistryDeactivateOut(TypedDict, total=False):\n    # Single rule_id response:\n    rule_id: str\n    version: int\n    was_deactivated: bool\n    scope: dict\n    jobs_rebound: int\n    # Bulk rule_ids / pack response:\n    outcomes: list[dict]",
        keywords: &["registry", "rule", "deactivate", "disable", "pack"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistryListActive,
        name: "registry_list_active",
        domain: "registry",
        op_class: OpClass::Read,
        summary: "Snapshot of currently-active comb rules across scopes.",
        params: &[ParamSpec {
            name: "limit",
            py_type: "int | None",
            required: false,
            default_repr: "None",
            doc: "Max entries returned.",
        }],
        return_type: "RegistryListActiveOut",
        typed_dicts: "class RegistryListActiveOut(TypedDict):\n    entries: list[dict]\n    truncated: bool",
        keywords: &["registry", "active", "list", "rules"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RegistrySuggestFromSamples,
        name: "registry_suggest_from_samples",
        domain: "registry",
        op_class: OpClass::Read,
        summary: "Suggest DRAFT comb RuleDefinitions from raw output lines using deterministic heuristics.",
        params: &[
            ParamSpec {
                name: "sample_lines",
                py_type: "list[str]",
                required: true,
                default_repr: "",
                doc: "Raw output lines to analyze (alias 'samples' is also accepted at runtime).",
            },
            ParamSpec {
                name: "intent",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional advisory tool/intent hint.",
            },
            ParamSpec {
                name: "max_rules",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Cap on proposed rules (max 8).",
            },
        ],
        return_type: "RegistrySuggestOut",
        typed_dicts: "class RuleDef(TypedDict):\n    id: str\n    version: int\n    kind: str              # regex|keyword\n    status: str            # draft|active\n    severity: str\n    event_kind: str\n    pattern: str | None\n    keywords: list[str] | None\n    summary_template: str\n\nclass RegistrySuggestOut(TypedDict):\n    proposed_rules: list[RuleDef]  # NOTE: key is 'proposed_rules' (RuleDefinition dicts)\n    confidence: str\n    next_steps: list[str]\n    explanation: str",
        keywords: &[
            "registry",
            "suggest",
            "heuristic",
            "samples",
            "draft",
            "propose",
            "rules",
        ],
    },
    // -----------------------------------------------------------------
    // Domain: recipe (8)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::RecipeSearch,
        name: "recipe_search",
        domain: "recipe",
        op_class: OpClass::Read,
        summary: "FTS search over persisted argv recipes.",
        params: &[
            ParamSpec {
                name: "query",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "FTS query over argv recipes.",
            },
            ParamSpec {
                name: "limit",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Result cap.",
            },
        ],
        return_type: "RecipeSearchOut",
        typed_dicts: "class RecipeSearchOut(TypedDict):\n    hits: list[dict]",
        keywords: &["recipe", "recipes", "search", "argv", "template"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeGet,
        name: "recipe_get",
        domain: "recipe",
        op_class: OpClass::Read,
        summary: "Fetch an argv recipe definition and tombstoned status by recipe_id.",
        params: &[
            ParamSpec {
                name: "recipe_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Recipe identifier.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional version (omit for latest).",
            },
        ],
        return_type: "RecipeGetOut",
        typed_dicts: "class RecipeGetOut(TypedDict):\n    definition: dict\n    tombstoned: bool",
        keywords: &["recipe", "get", "definition", "argv"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeUpsert,
        name: "recipe_upsert",
        domain: "recipe",
        op_class: OpClass::Mutate,
        summary: "Insert a new immutable argv recipe version from a JSON definition string (does not activate).",
        params: &[ParamSpec {
            name: "definition_json",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "JSON-encoded RecipeDefinition string.",
        }],
        return_type: "RecipeUpsertOut",
        typed_dicts: "class RecipeUpsertOut(TypedDict):\n    recipe_id: str\n    version: int\n    activated: bool",
        keywords: &["recipe", "upsert", "create", "save"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeTest,
        name: "recipe_test",
        domain: "recipe",
        op_class: OpClass::Read,
        summary: "Dry-run an argv recipe with placeholder fills; validates argv without spawning.",
        params: &[
            ParamSpec {
                name: "definition_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Inline JSON RecipeDefinition (mutually exclusive with recipe_id).",
            },
            ParamSpec {
                name: "recipe_id",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Stored recipe_id (mutually exclusive with definition_json).",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional stored recipe version.",
            },
            ParamSpec {
                name: "fills",
                py_type: "dict[str, str] | None",
                required: false,
                default_repr: "None",
                doc: "Placeholder fills map.",
            },
            ParamSpec {
                name: "expect_argv0",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional expected argv[0].",
            },
        ],
        return_type: "RecipeTestOut",
        typed_dicts: "class RecipeTestOut(TypedDict):\n    ok: bool\n    activated: bool\n    argv: list[str]\n    expects_met: bool\n    notes: list[str]",
        keywords: &["recipe", "test", "dry_run", "validate", "fills"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeActivate,
        name: "recipe_activate",
        domain: "recipe",
        op_class: OpClass::Mutate,
        summary: "Activate an argv recipe in global scope (requires llm_can_activate_recipes under hardened profiles).",
        params: &[
            ParamSpec {
                name: "recipe_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Recipe identifier.",
            },
            ParamSpec {
                name: "scope",
                py_type: "dict",
                required: true,
                default_repr: "",
                doc: "Global scope {'kind': 'global'}.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional recipe version.",
            },
        ],
        return_type: "RecipeActivateOut",
        typed_dicts: "class RecipeActivateOut(TypedDict):\n    recipe_id: str\n    version: int\n    was_already_active: bool\n    scope: dict",
        keywords: &["recipe", "activate", "enable"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeDeactivate,
        name: "recipe_deactivate",
        domain: "recipe",
        op_class: OpClass::Mutate,
        summary: "Deactivate an argv recipe for a scope.",
        params: &[
            ParamSpec {
                name: "recipe_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Recipe identifier.",
            },
            ParamSpec {
                name: "scope",
                py_type: "dict",
                required: true,
                default_repr: "",
                doc: "Scope object {'kind': 'global'}.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional version (omit to close open activation).",
            },
        ],
        return_type: "RecipeDeactivateOut",
        typed_dicts: "class RecipeDeactivateOut(TypedDict):\n    recipe_id: str\n    version: int\n    was_deactivated: bool\n    scope: dict",
        keywords: &["recipe", "deactivate", "disable"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeListActive,
        name: "recipe_list_active",
        domain: "recipe",
        op_class: OpClass::Read,
        summary: "Snapshot of currently-active argv recipes.",
        params: &[ParamSpec {
            name: "limit",
            py_type: "int | None",
            required: false,
            default_repr: "None",
            doc: "Max entries returned.",
        }],
        return_type: "RecipeListActiveOut",
        typed_dicts: "class RecipeListActiveOut(TypedDict):\n    entries: list[dict]\n    truncated: bool",
        keywords: &["recipe", "list", "active"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::RecipeRun,
        name: "recipe_run",
        domain: "recipe",
        op_class: OpClass::Mutate,
        summary: "Run an activated argv recipe; performs bounded watch automatically when recipe defines timeout_ms or rule_pack_ids.",
        params: &[
            ParamSpec {
                name: "recipe_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Activated recipe_id.",
            },
            ParamSpec {
                name: "scope",
                py_type: "dict",
                required: true,
                default_repr: "",
                doc: "Scope object {'kind': 'global'}.",
            },
            ParamSpec {
                name: "version",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional recipe version.",
            },
            ParamSpec {
                name: "fills",
                py_type: "dict[str, str] | None",
                required: false,
                default_repr: "None",
                doc: "Placeholder fills map.",
            },
            ParamSpec {
                name: "limits",
                py_type: "dict | None",
                required: false,
                default_repr: "None",
                doc: "Optional {'memory': '...', 'priority': '...'}.",
            },
        ],
        return_type: "RecipeRunOut",
        typed_dicts: "class RecipeRunOut(TypedDict):\n    recipe_id: str\n    version: int\n    argv: list[str]\n    lane: str\n    watched: bool\n    wait_ms: int\n    job_id: str\n    bucket_id: str\n    probe_id: str\n    cursor: int\n    state: str | None\n    exit_code: int | None\n    complete: bool\n    signals: list[dict]\n    signal_count: int\n    degraded: bool\n    recover_hint: str | None",
        keywords: &["recipe", "run", "execute", "argv"],
    },
    // -----------------------------------------------------------------
    // Domain: files (6)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::FileReadWindow,
        name: "file_read_window",
        domain: "files",
        op_class: OpClass::Read,
        summary: "Bounded line/byte window read of an absolute file path (policy-gated).",
        params: &[
            ParamSpec {
                name: "path",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Absolute path to regular file.",
            },
            ParamSpec {
                name: "start_line",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "1-based start line (default 1).",
            },
            ParamSpec {
                name: "max_lines",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max lines returned (default 200, cap 2000).",
            },
            ParamSpec {
                name: "max_bytes",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max bytes returned (cap 64 KiB).",
            },
        ],
        return_type: "FileReadWindowOut",
        typed_dicts: "class FileLine(TypedDict):\n    line: int\n    byte_offset: int\n    text: str\n\nclass FileReadWindowOut(TypedDict):\n    path: str\n    lines: list[FileLine]\n    file_bytes: int\n    truncated: bool\n    next_byte_offset: int",
        keywords: &["file", "read", "window", "lines", "cat", "view"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::FileSearch,
        name: "file_search",
        domain: "files",
        op_class: OpClass::Read,
        summary: "Bounded literal substring search over an absolute file or directory tree.",
        params: &[
            ParamSpec {
                name: "path",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Absolute file or directory path.",
            },
            ParamSpec {
                name: "query",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Literal substring query (not regex).",
            },
            ParamSpec {
                name: "case_insensitive",
                py_type: "bool | None",
                required: false,
                default_repr: "None",
                doc: "Fold ASCII case when True.",
            },
            ParamSpec {
                name: "max_matches",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max matches returned (default 100, cap 500).",
            },
            ParamSpec {
                name: "max_snippet_bytes",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max snippet bytes per match (default 240, cap 512).",
            },
        ],
        return_type: "FileSearchOut",
        typed_dicts: "class FileSearchMatch(TypedDict):\n    path: str | None\n    line: int\n    byte_offset: int\n    snippet: str\n\nclass FileSearchOut(TypedDict):\n    path: str\n    matches: list[FileSearchMatch]\n    truncated: bool\n    bytes_scanned: int",
        keywords: &["file", "search", "grep", "find", "substring", "directory"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::FileWrite,
        name: "file_write",
        domain: "files",
        op_class: OpClass::Mutate,
        summary: "Atomic UTF-8 file write or append to an absolute path (policy-gated by paths.write_allow, audited).",
        params: &[
            ParamSpec {
                name: "path",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Absolute target file path.",
            },
            ParamSpec {
                name: "content",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "UTF-8 content to write (cap 192 KiB per call).",
            },
            ParamSpec {
                name: "create_dirs",
                py_type: "bool | None",
                required: false,
                default_repr: "None",
                doc: "Create missing parent directories within allowed path.",
            },
            ParamSpec {
                name: "append",
                py_type: "bool | None",
                required: false,
                default_repr: "None",
                doc: "Append instead of atomic replace.",
            },
        ],
        return_type: "FileWriteOut",
        typed_dicts: "class FileWriteOut(TypedDict):\n    path: str\n    bytes_written: int",
        keywords: &["file", "write", "save", "append", "create"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::FileWatchStart,
        name: "file_watch_start",
        domain: "files",
        op_class: OpClass::Mutate,
        summary: "Start a daemon-owned file watch bound to a bucket; emits combed events on appended lines.",
        params: &[
            ParamSpec {
                name: "path",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Absolute path to regular file.",
            },
            ParamSpec {
                name: "follow_from_beginning",
                py_type: "bool | None",
                required: false,
                default_repr: "None",
                doc: "True to read from start; False/None to tail from end.",
            },
            ParamSpec {
                name: "bucket_config_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional JSON bucket config.",
            },
            ParamSpec {
                name: "rules",
                py_type: "list[dict] | None",
                required: false,
                default_repr: "None",
                doc: "Inline comb rules, minimal [{'pattern': 'ERROR'}].",
            },
            ParamSpec {
                name: "rules_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Deprecated JSON-array string form of rules.",
            },
            ParamSpec {
                name: "tag",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional per-bucket tag.",
            },
        ],
        return_type: "FileWatchStartOut",
        typed_dicts: "class FileWatchStartOut(TypedDict):\n    watch_id: str\n    bucket_id: str\n    probe_id: str\n    cursor: int",
        keywords: &["file", "watch", "tail", "follow", "log", "start"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::FileWatchStop,
        name: "file_watch_stop",
        domain: "files",
        op_class: OpClass::Mutate,
        summary: "Stop a live file watch by watch_id and return final counters.",
        params: &[ParamSpec {
            name: "watch_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque watch_id (job_<32hex>) from file_watch_start().",
        }],
        return_type: "FileWatchStopOut",
        typed_dicts: "class FileWatchStopOut(TypedDict):\n    watch_id: str\n    bucket_id: str\n    frames_total: int\n    events_emitted: int\n    bytes_total: int",
        keywords: &["file", "watch", "stop", "close"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::FileWatchList,
        name: "file_watch_list",
        domain: "files",
        op_class: OpClass::Read,
        summary: "Snapshot of currently-live file watches.",
        params: &[],
        return_type: "FileWatchListOut",
        typed_dicts: "class FileWatchListOut(TypedDict):\n    entries: list[dict]",
        keywords: &["file", "watch", "list", "live"],
    },
    // -----------------------------------------------------------------
    // Domain: pty_session (12: 5 PTY/credential + 7 shell session/snapshot)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::PtyCommandStart,
        name: "pty_command_start",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Start an interactive argv command attached to a PTY; returns job_id + bucket_id + cursor.",
        params: &[
            ParamSpec {
                name: "argv",
                py_type: "list[str]",
                required: true,
                default_repr: "",
                doc: "Non-empty argv array.",
            },
            ParamSpec {
                name: "cwd",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional working directory.",
            },
            ParamSpec {
                name: "env",
                py_type: "list[dict[str, str]]",
                required: false,
                default_repr: "[]",
                doc: "Array of {'key': 'NAME', 'value': 'VAL'} objects.",
            },
            ParamSpec {
                name: "rows",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Terminal rows.",
            },
            ParamSpec {
                name: "cols",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Terminal columns.",
            },
            ParamSpec {
                name: "bucket_config_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional JSON bucket config.",
            },
            ParamSpec {
                name: "rules",
                py_type: "list[dict] | None",
                required: false,
                default_repr: "None",
                doc: "Inline comb rules.",
            },
            ParamSpec {
                name: "rules_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Deprecated JSON-array string form of rules.",
            },
            ParamSpec {
                name: "tag",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional per-bucket tag.",
            },
            ParamSpec {
                name: "limits",
                py_type: "dict | None",
                required: false,
                default_repr: "None",
                doc: "Optional {'memory': '...', 'priority': '...'}.",
            },
        ],
        return_type: "PtyCommandStartOut",
        typed_dicts: "class PtyCommandStartOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    probe_id: str\n    cursor: int",
        keywords: &["pty", "interactive", "tty", "start", "command"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::PtyCommandWriteStdin,
        name: "pty_command_write_stdin",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Write UTF-8 stdin bytes to a PTY job (refused with SecretInputDenied on password prompts; use credential_request).",
        params: &[
            ParamSpec {
                name: "job_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque PTY job_id (job_<32hex>).",
            },
            ParamSpec {
                name: "bytes",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "UTF-8 stdin text to write (cap 4096 bytes).",
            },
            ParamSpec {
                name: "cursor",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Bucket cursor for optional wait_ms settle window.",
            },
            ParamSpec {
                name: "wait_ms",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Optional settle wait (ms) to return combed events in the same call.",
            },
        ],
        return_type: "PtyWriteStdinOut",
        typed_dicts: "class PtyWriteStdinOut(TypedDict, total=False):\n    job_id: str\n    bytes_written: int\n    secret_prompt_active: bool\n    awaiting_credential: dict | None\n    cursor_in: int\n    next_cursor: int\n    has_more: bool\n    dropped_count: int\n    events: list[SignalEvent]",
        keywords: &["pty", "stdin", "write", "input", "interactive"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::PtyCommandStop,
        name: "pty_command_stop",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Stop a running PTY job by job_id and return final counters.",
        params: &[ParamSpec {
            name: "job_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque PTY job_id (job_<32hex>).",
        }],
        return_type: "PtyCommandStopOut",
        typed_dicts: "class PtyCommandStopOut(TypedDict):\n    job_id: str\n    bucket_id: str\n    frames_total: int\n    events_emitted: int\n    bytes_total: int\n    stdin_bytes_written: int\n    secret_prompts_total: int",
        keywords: &["pty", "stop", "kill", "close"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::PtyCommandList,
        name: "pty_command_list",
        domain: "pty_session",
        op_class: OpClass::Read,
        summary: "Snapshot of live PTY jobs including secret_prompt_active and awaiting_credential.",
        params: &[],
        return_type: "PtyCommandListOut",
        typed_dicts: "class PtyCommandListOut(TypedDict):\n    entries: list[dict]",
        keywords: &["pty", "list", "live", "awaiting_credential"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::CredentialRequest,
        name: "credential_request",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Ask the owner directly for the password a PTY job is waiting on (never passes secrets through the model).",
        params: &[ParamSpec {
            name: "job_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque PTY job_id with awaiting_credential.",
        }],
        return_type: "CredentialRequestOut",
        typed_dicts: "class CredentialRequestOut(TypedDict):\n    job_id: str\n    status: str            # provided|declined|owner_action_required|timeout|pending|not_awaiting\n    command: str | None",
        keywords: &[
            "credential",
            "password",
            "sudo",
            "ssh",
            "prompt",
            "owner",
            "secret",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ShellSessionStart,
        name: "shell_session_start",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Start a persistent shell session with sticky cwd/env (unix-only; requires allow_session).",
        params: &[
            ParamSpec {
                name: "shell",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional interpreter override, e.g. '/bin/bash'.",
            },
            ParamSpec {
                name: "cwd",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Initial working directory.",
            },
            ParamSpec {
                name: "env",
                py_type: "list[dict[str, str]]",
                required: false,
                default_repr: "[]",
                doc: "Array of {'key': 'NAME', 'value': 'VAL'} objects.",
            },
            ParamSpec {
                name: "rules",
                py_type: "list[dict] | None",
                required: false,
                default_repr: "None",
                doc: "Inline comb rules bound to the session bucket.",
            },
            ParamSpec {
                name: "rules_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Deprecated JSON-array string form of rules.",
            },
            ParamSpec {
                name: "bucket_config_json",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional JSON bucket config.",
            },
            ParamSpec {
                name: "tag",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional per-bucket tag.",
            },
        ],
        return_type: "ShellSessionStartOut",
        typed_dicts: "class ShellSessionStartOut(TypedDict):\n    session_id: str\n    bucket_id: str\n    state: str",
        keywords: &["session", "shell", "persistent", "sticky", "cwd", "start"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ShellSessionExec,
        name: "shell_session_exec",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Execute one line inside a persistent shell session and return combed events + next_cursor (unix-only).",
        params: &[
            ParamSpec {
                name: "session_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque session_id (ses_<32hex>).",
            },
            ParamSpec {
                name: "line",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Shell line to execute (no trailing newline).",
            },
            ParamSpec {
                name: "cursor",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Session bucket cursor (0/None or prior next_cursor).",
            },
            ParamSpec {
                name: "wait_ms",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Bounded settle wait in ms.",
            },
        ],
        return_type: "ShellSessionExecOut",
        typed_dicts: "class ShellSessionExecOut(TypedDict):\n    session_id: str\n    bucket_id: str\n    bytes_written: int\n    cursor_in: int\n    next_cursor: int\n    has_more: bool\n    dropped_count: int\n    events: list[SignalEvent]",
        keywords: &["session", "shell", "exec", "line", "sticky"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ShellSessionStatus,
        name: "shell_session_status",
        domain: "pty_session",
        op_class: OpClass::Read,
        summary: "Report a persistent shell session's state, cwd, and bounded env snapshot (unix-only).",
        params: &[ParamSpec {
            name: "session_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque session_id (ses_<32hex>).",
        }],
        return_type: "ShellSessionStatusOut",
        typed_dicts: "class ShellSessionStatusOut(TypedDict):\n    session_id: str\n    bucket_id: str\n    state: str\n    cwd: str | None\n    env_snapshot: list[list[str]]\n    last_active_at: int",
        keywords: &["session", "status", "cwd", "env"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ShellSessionStop,
        name: "shell_session_stop",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Terminate a persistent shell session and report terminal state (unix-only).",
        params: &[ParamSpec {
            name: "session_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque session_id (ses_<32hex>).",
        }],
        return_type: "ShellSessionStopOut",
        typed_dicts: "class ShellSessionStopOut(TypedDict):\n    session_id: str\n    state: str\n    terminal_reason: str",
        keywords: &["session", "stop", "close", "terminate"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::ShellSessionList,
        name: "shell_session_list",
        domain: "pty_session",
        op_class: OpClass::Read,
        summary: "Snapshot of currently-live persistent shell sessions (unix-only).",
        params: &[],
        return_type: "ShellSessionListOut",
        typed_dicts: "class ShellSessionListOut(TypedDict):\n    sessions: list[dict]",
        keywords: &["session", "list", "live"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::WorkspaceSnapshotCreate,
        name: "workspace_snapshot_create",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Save a shell session's cwd and bounded env as a restorable snapshot (unix-only).",
        params: &[
            ParamSpec {
                name: "session_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque session_id (ses_<32hex>).",
            },
            ParamSpec {
                name: "name",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional human-friendly snapshot label.",
            },
        ],
        return_type: "WorkspaceSnapshotCreateOut",
        typed_dicts: "class WorkspaceSnapshotCreateOut(TypedDict):\n    snapshot_id: str",
        keywords: &["workspace", "snapshot", "create", "save", "session"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::WorkspaceSnapshotApply,
        name: "workspace_snapshot_apply",
        domain: "pty_session",
        op_class: OpClass::Mutate,
        summary: "Restore a saved workspace snapshot's cwd and env into a live shell session (unix-only).",
        params: &[
            ParamSpec {
                name: "snapshot_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque snapshot_id from workspace_snapshot_create().",
            },
            ParamSpec {
                name: "session_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Target live session_id (ses_<32hex>).",
            },
        ],
        return_type: "WorkspaceSnapshotApplyOut",
        typed_dicts: "class WorkspaceSnapshotApplyOut(TypedDict):\n    applied: bool\n    session_id: str\n    cwd: str | None",
        keywords: &["workspace", "snapshot", "apply", "restore", "session"],
    },
    // -----------------------------------------------------------------
    // Domain: subscriptions (5)
    // -----------------------------------------------------------------
    CodeOpDescriptor {
        op: TcCodeOp::SubscriptionOpen,
        name: "subscription_open",
        domain: "subscriptions",
        op_class: OpClass::Wait,
        summary: "Open one multiplexed predicate-routed subscription across buckets/jobs/probes.",
        params: &[
            ParamSpec {
                name: "severity_min",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Minimum event severity: trace|debug|info|low|medium|high|critical.",
            },
            ParamSpec {
                name: "kind",
                py_type: "list[str] | None",
                required: false,
                default_repr: "None",
                doc: "Event-kind allowlist.",
            },
            ParamSpec {
                name: "sources",
                py_type: "dict | None",
                required: false,
                default_repr: "None",
                doc: "{'kind': 'all'} (default) | {'kind': 'jobs', 'jobs': [...]} | {'kind': 'buckets', 'buckets': [...]} | {'kind': 'probes', 'probes': [...]}.",
            },
            ParamSpec {
                name: "tag",
                py_type: "str | None",
                required: false,
                default_repr: "None",
                doc: "Optional per-bucket tag filter.",
            },
        ],
        return_type: "SubscriptionOpenOut",
        typed_dicts: "class SubscriptionOpenOut(TypedDict):\n    sub_id: str\n    boot_id: str\n    predicate_hash: str\n    created_at_ms: int\n    matched_sources: int",
        keywords: &[
            "subscription",
            "open",
            "multiplex",
            "subscribe",
            "predicate",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::SubscriptionPull,
        name: "subscription_pull",
        domain: "subscriptions",
        op_class: OpClass::Wait,
        summary: "Blocking multiplexed pull of matched events and liveness delta from sub_id.",
        params: &[
            ParamSpec {
                name: "sub_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque sub_id from subscription_open().",
            },
            ParamSpec {
                name: "max",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Max events to return (default/cap 50).",
            },
            ParamSpec {
                name: "timeout_ms",
                py_type: "int | None",
                required: false,
                default_repr: "None",
                doc: "Blocking timeout in ms (default 5000, cap 8000; clamped to remaining script wall time).",
            },
        ],
        return_type: "SubscriptionPullOut",
        typed_dicts: "class SubscriptionEventItem(TypedDict):\n    bucket_id: str\n    job_id: str | None\n    seq: int\n    event: SignalEvent\n\nclass SubscriptionPullOut(TypedDict, total=False):\n    events: list[SubscriptionEventItem]\n    liveness: list[dict]\n    lagged: bool\n    truncated: bool",
        keywords: &[
            "subscription",
            "pull",
            "events",
            "multiplex",
            "stream",
            "wait",
        ],
    },
    CodeOpDescriptor {
        op: TcCodeOp::SubscriptionList,
        name: "subscription_list",
        domain: "subscriptions",
        op_class: OpClass::Read,
        summary: "Snapshot of open subscriptions.",
        params: &[ParamSpec {
            name: "limit",
            py_type: "int | None",
            required: false,
            default_repr: "None",
            doc: "Max rows returned (default/cap 64).",
        }],
        return_type: "SubscriptionListOut",
        typed_dicts: "class SubscriptionListOut(TypedDict):\n    subscriptions: list[dict]\n    truncated: bool",
        keywords: &["subscription", "list", "open"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::SubscriptionClose,
        name: "subscription_close",
        domain: "subscriptions",
        op_class: OpClass::Wait,
        summary: "Close a subscription by sub_id and free its slot (idempotent).",
        params: &[ParamSpec {
            name: "sub_id",
            py_type: "str",
            required: true,
            default_repr: "",
            doc: "Opaque sub_id to close.",
        }],
        return_type: "SubscriptionCloseOut",
        typed_dicts: "class SubscriptionCloseOut(TypedDict):\n    closed: bool",
        keywords: &["subscription", "close", "unsubscribe"],
    },
    CodeOpDescriptor {
        op: TcCodeOp::SubscriptionSeek,
        name: "subscription_seek",
        domain: "subscriptions",
        op_class: OpClass::Wait,
        summary: "Reposition a subscription's offset for one bucket (clamped to live range).",
        params: &[
            ParamSpec {
                name: "sub_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Opaque sub_id from subscription_open().",
            },
            ParamSpec {
                name: "bucket_id",
                py_type: "str",
                required: true,
                default_repr: "",
                doc: "Bucket id (bkt_<32hex>) to reposition.",
            },
            ParamSpec {
                name: "seq",
                py_type: "int",
                required: true,
                default_repr: "",
                doc: "Requested re-read sequence number.",
            },
        ],
        return_type: "SubscriptionSeekOut",
        typed_dicts: "class SubscriptionSeekOut(TypedDict):\n    clamped_seq: int\n    lagged: bool",
        keywords: &["subscription", "seek", "rewind", "cursor", "offset"],
    },
];

const DOMAIN_ORDER: &[(&str, &str)] = &[
    ("command", "Commands & Shell"),
    ("bucket_events", "Buckets & Event Context"),
    ("files", "Files & File Watches"),
    ("registry", "Comb Rule Registry"),
    ("recipe", "Argv Recipes"),
    ("pty_session", "PTY, Credentials & Persistent Sessions"),
    ("subscriptions", "Multiplexed Subscriptions"),
    (
        "runtime_audit_federation",
        "Status, Discovery, Audit, Runtime & Federation",
    ),
];

fn render_signature_line(desc: &CodeOpDescriptor) -> String {
    let mut params = String::new();
    for (idx, p) in desc.params.iter().enumerate() {
        if idx > 0 {
            params.push_str(", ");
        }
        params.push_str(p.name);
        params.push_str(": ");
        params.push_str(p.py_type);
        if !p.required {
            params.push_str(" = ");
            params.push_str(p.default_repr);
        }
    }
    if params.is_empty() {
        format!("def {}() -> {}", desc.name, desc.return_type)
    } else {
        format!("def {}(*, {}) -> {}", desc.name, params, desc.return_type)
    }
}

fn render_full_stub(desc: &CodeOpDescriptor) -> String {
    let mut out = String::new();
    if desc.typed_dicts.contains("SignalEvent") {
        out.push_str(SIGNAL_TYPED_DICTS);
        out.push_str("\n\n");
    }
    out.push_str(desc.typed_dicts);
    out.push_str("\n\n");
    out.push_str(&render_signature_line(desc));
    out.push_str(":\n    \"\"\"[");
    out.push_str(match desc.op_class {
        OpClass::Read => "read",
        OpClass::Wait => "wait",
        OpClass::Mutate => "mutate",
    });
    out.push_str("] ");
    out.push_str(desc.summary);
    out.push('\n');
    for p in desc.params {
        out.push_str("    - ");
        out.push_str(p.name);
        out.push_str(": ");
        out.push_str(p.doc);
        out.push('\n');
    }
    out.push_str("    \"\"\"\n    ...");
    out
}

/// Renders Python stubs for `code_search`.
///
/// - When `query` is `None` or blank: returns a compact domain-grouped 1-line signature
///   index of all 60 functions.
/// - When `query` is non-empty: ranks matching functions by name/domain/keyword/param/summary
///   match and renders their full `TypedDict` definitions + keyword-only `def` stubs.
#[must_use]
pub fn render_stubs(query: Option<&str>, limit: Option<usize>) -> String {
    let trimmed = query.map_or("", str::trim);
    if trimmed.is_empty() {
        let mut out = String::from(
            "# Terminal Commander Code Mode -- 60 keyword-only Python functions\n\
             # Pass query='<topic or fn_name>' to inspect TypedDict return fields.\n",
        );
        for &(domain_key, domain_title) in DOMAIN_ORDER {
            out.push_str("\n## ");
            out.push_str(domain_title);
            out.push_str(" (");
            out.push_str(domain_key);
            out.push_str(")\n");
            for desc in TC_CODE_OPS.iter().filter(|d| d.domain == domain_key) {
                out.push_str(&render_signature_line(desc));
                out.push('\n');
            }
        }
        return out;
    }

    let terms: Vec<String> = trimmed
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();

    let mut scored: Vec<(u32, &CodeOpDescriptor)> = TC_CODE_OPS
        .iter()
        .filter_map(|desc| {
            let mut score = 0u32;
            let name_lower = desc.name.to_ascii_lowercase();
            let summary_lower = desc.summary.to_ascii_lowercase();
            let domain_lower = desc.domain.to_ascii_lowercase();
            for term in &terms {
                if name_lower == *term {
                    score += 100;
                } else if name_lower.contains(term) {
                    score += 50;
                }
                if domain_lower == *term || domain_lower.contains(term) {
                    score += 25;
                }
                if desc.keywords.iter().any(|k| *k == term) {
                    score += 35;
                } else if desc.keywords.iter().any(|k| k.contains(term.as_str())) {
                    score += 15;
                }
                if summary_lower.contains(term) {
                    score += 10;
                }
                if desc
                    .params
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case(term))
                {
                    score += 10;
                }
            }
            (score > 0).then_some((score, desc))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(b.1.name)));
    let cap = limit.unwrap_or(5).clamp(1, TC_CODE_OPS.len());

    if scored.is_empty() {
        let names: Vec<&str> = TC_CODE_OPS.iter().map(|d| d.name).collect();
        return format!(
            "# No Code Mode stub matched {trimmed:?}. Available functions: {}\n# Pass an empty query to list all 60 one-line signatures.\n",
            names.join(", ")
        );
    }

    let mut out = String::from("from typing import TypedDict\n\n");
    for (idx, (_, desc)) in scored.into_iter().take(cap).enumerate() {
        if idx > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&render_full_stub(desc));
    }
    out.push('\n');
    out
}

/// Renders the self-contained agent manual for Terminal Commander Code Mode.
#[must_use]
pub fn render_idioms() -> String {
    let mut fn_list = String::new();
    for desc in TC_CODE_OPS {
        fn_list.push_str("- `");
        fn_list.push_str(&render_signature_line(desc));
        fn_list.push_str("` -- ");
        fn_list.push_str(desc.summary);
        fn_list.push('\n');
    }

    format!(
        "# Terminal Commander Code Mode Idioms (Pure-Rust Monty Python 3.14 Subset)\n\
\n\
## Sandbox rules & resource caps\n\
1. **Keyword arguments ONLY**: Every host function requires `fn(key=value)` -- positional arguments or unknown kwargs raise `TypeError` with the exact signature.\n\
2. **No imports needed for TC tools**: All 60 functions (`run_and_watch`, `bucket_wait`, `event_context`, `file_read_window`, etc.) are pre-bound globals.\n\
3. **Final expression = JSON `value`**: End your script with the dict/list you want returned (`print(...)` goes to `stdout`). Intermediate bucket events stay in Rust RAM.\n\
4. **Pure-Rust Monty subset**: Plain `def`, list/dict comprehensions, `for`/`if`/`while`/`try`/`except`, slicing, `json`, `re`, `math` work normally. Avoid `yield`, `async`/`await`, `class`, `del`, `open()`, `os.*`, `subprocess`, or network imports (`PermissionError` / `ModuleNotFoundError`).\n\
5. **Dual budgets & timers**:\n\
   - 10s Python CPU feed budget (30s ceiling) + 120s wall-clock budget (300s ceiling; host waits suspend CPU accounting and auto-clamp to remaining wall time).\n\
   - 64 total host calls (256 ceiling) and 8 mutating calls (`max_mutation_calls`, 32 ceiling).\n\
   - 16,000 inline UTF-8 chars (larger outputs spill losslessly to `tc-code-spill-*.json` reported in `spill_path`).\n\
\n\
## Canonical 1-Turn Patterns (verified wire keys)\n\
\n\
### Pattern 1: Run + Triage Failing Build + Read Event Context in 1 Turn\n\
```python\n\
run = run_and_watch(\n\
    argv=[\"cargo\", \"test\", \"--workspace\"],\n\
    rules=[{{\"pattern\": r\"error(\\[E\\d+\\])?:|FAILED\", \"severity\": \"high\"}}],\n\
    wait_ms=45000,\n\
)\n\
if run[\"exit_code\"] == 0:\n\
    res = {{\"ok\": True, \"exit_code\": 0, \"signals\": run[\"signal_count\"]}}\n\
else:\n\
    ev = bucket_events_since(bucket_id=run[\"bucket_id\"], cursor=0, severity_min=\"high\")\n\
    contexts = [\n\
        event_context(event_id=e[\"event_id\"])\n\
        for e in ev[\"events\"][:3]\n\
    ]\n\
    tail = command_output_tail(job_id=run[\"job_id\"], max_lines=25, strip_ansi=True)\n\
    res = {{\n\
        \"ok\": False,\n\
        \"exit_code\": run[\"exit_code\"],\n\
        \"error_count\": len(ev[\"events\"]),\n\
        \"top_contexts\": contexts,\n\
        \"tail\": tail[\"lines\"],\n\
    }}\n\
res\n\
```\n\
\n\
### Pattern 2: Bounded Long-Job Poll + Summary Without Context Flood\n\
```python\n\
start = command_start_combed(\n\
    argv=[\"cargo\", \"build\", \"--release\"],\n\
    rules=[{{\"pattern\": \"error|warning\"}}],\n\
)\n\
st = command_status(job_id=start[\"job_id\"])\n\
cursor = start[\"cursor\"]\n\
if st[\"state\"] in (\"starting\", \"running\"):\n\
    w = bucket_wait(bucket_id=start[\"bucket_id\"], cursor=cursor, timeout_ms=8000)\n\
    cursor = w[\"next_cursor\"]\n\
    st = command_status(job_id=start[\"job_id\"])\n\
summary = bucket_summary(bucket_id=st[\"bucket_id\"])\n\
{{\"state\": st[\"state\"], \"exit_code\": st[\"exit_code\"], \"next_cursor\": cursor, \"summary\": summary}}\n\
```\n\
\n\
### Pattern 3: Suggest + Upsert + Test + Activate a Comb Rule in 1 Turn\n\
```python\n\
import json\n\
tail = command_output_tail(job_id=\"job_00000000000000000000000000000001\", max_lines=60, strip_ansi=True)\n\
drafts = registry_suggest_from_samples(sample_lines=tail[\"lines\"])\n\
sample_objs = [{{\"text\": line}} for line in tail[\"lines\"][:20]]\n\
verified = []\n\
for rule_def in drafts[\"proposed_rules\"][:2]:\n\
    rule_def[\"status\"] = \"active\"\n\
    saved = registry_upsert(definition_json=json.dumps(rule_def))\n\
    check = registry_test(rule_id=saved[\"rule_id\"], version=saved[\"version\"], samples=sample_objs)\n\
    if len(check[\"matches\"]) > 0:\n\
        act = registry_activate(rule_id=saved[\"rule_id\"], version=saved[\"version\"], scope={{\"kind\": \"global\"}})\n\
        verified.append({{\"rule_id\": saved[\"rule_id\"], \"version\": saved[\"version\"], \"matches\": len(check[\"matches\"]), \"active\": act}})\n\
verified\n\
```\n\
\n\
## Registered functions ({count})\n\
{fn_list}",
        count = TC_CODE_OPS.len(),
    )
}
