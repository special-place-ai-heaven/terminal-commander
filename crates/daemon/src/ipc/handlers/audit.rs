// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! `audit_since` handler (P4).
//!
//! Read-only, cursor-paged view of the persistent audit log. The
//! handler reads rows via the store actor's [`StoreClient::audit_since`]
//! and maps each in-memory `AuditRow` to the wire-stable
//! [`AuditRowWire`]. A read failure surfaces
//! [`IpcErrorCode::Internal`] — the closed error set is NOT widened
//! for this method (the P4 spec mandates reuse).
//!
//! The handler goes through the normal audited dispatch envelope: the
//! dispatcher records one `info`/`error` audit row per call, same as
//! every other method.

use std::sync::Arc;

use terminal_commander_store::{AuditReadRequest, AuditRow};
use time::format_description::well_known::Rfc3339;

use crate::ipc::protocol::{
    AuditRowWire, AuditSinceParams, AuditSinceResponse, IpcError, IpcErrorCode, IpcResponse,
    MAX_AUDIT_READ_LIMIT,
};
use crate::state::DaemonState;

/// Map one store `AuditRow` to its wire mirror. `timestamp` is encoded
/// as RFC3339 — the same encoding the store persists — so the protocol
/// crate needs no `time` dependency. A formatting failure (practically
/// unreachable for a valid `OffsetDateTime`) falls back to an empty
/// string rather than dropping the row.
pub(in crate::ipc::server) fn row_to_wire(row: AuditRow) -> AuditRowWire {
    AuditRowWire {
        audit_id: row.audit_id,
        timestamp: row.timestamp.format(&Rfc3339).unwrap_or_default(),
        action: row.action,
        subject: row.subject,
        decision: row.decision,
        profile: row.profile,
        reason: row.reason,
        actor: row.actor,
        metadata_json: row.metadata_json,
    }
}

pub(in crate::ipc::server) fn handle_audit_since(
    state: &Arc<DaemonState>,
    params: &AuditSinceParams,
) -> Result<IpcResponse, IpcError> {
    // Clamp the caller's limit at the dispatcher boundary so the
    // request side is bounded by the protocol cap. `None` passes
    // through so the store applies its own default; the store also
    // re-clamps, so this is belt-and-suspenders at the wire edge.
    let limit = params.limit.map(|n| n.min(MAX_AUDIT_READ_LIMIT));
    let request = AuditReadRequest {
        cursor: params.cursor,
        action_filter: params.action_filter.clone(),
        decision_filter: params.decision_filter.clone(),
        limit,
    };
    let rows = state.store.audit_since(&request).map_err(|e| {
        IpcError::new(
            IpcErrorCode::Internal,
            format!("audit_since read failed: {e}"),
        )
    })?;
    // `next_cursor` is the last row's id (so the client pages forward),
    // or the input cursor when the page is empty.
    let next_cursor = rows.last().map_or(params.cursor, |r| r.audit_id);
    let wire_rows: Vec<AuditRowWire> = rows.into_iter().map(row_to_wire).collect();
    Ok(IpcResponse::AuditSince(AuditSinceResponse {
        cursor_in: params.cursor,
        next_cursor,
        rows: wire_rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    /// Field-for-field mapping guard: every `AuditRow` field must reach
    /// the matching `AuditRowWire` field. Guards against drift if either
    /// struct grows a field.
    #[test]
    fn audit_row_maps_field_for_field_to_wire() {
        let ts = OffsetDateTime::from_unix_timestamp(1_780_000_000).expect("valid ts");
        let row = AuditRow {
            audit_id: 42,
            timestamp: ts,
            action: "ipc_registry_activate".to_owned(),
            subject: "uid=1000:pid=7".to_owned(),
            decision: "info".to_owned(),
            profile: Some("developer_local".to_owned()),
            reason: Some("operator intent".to_owned()),
            actor: Some("ipc".to_owned()),
            metadata_json: Some(r#"{"kind":"unix"}"#.to_owned()),
        };
        let wire = row_to_wire(row.clone());
        assert_eq!(wire.audit_id, row.audit_id);
        assert_eq!(wire.timestamp, ts.format(&Rfc3339).unwrap());
        assert_eq!(wire.action, row.action);
        assert_eq!(wire.subject, row.subject);
        assert_eq!(wire.decision, row.decision);
        assert_eq!(wire.profile, row.profile);
        assert_eq!(wire.reason, row.reason);
        assert_eq!(wire.actor, row.actor);
        assert_eq!(wire.metadata_json, row.metadata_json);
    }

    /// Optional fields map to `None` when absent on the source row.
    #[test]
    fn audit_row_optional_fields_pass_through_as_none() {
        let ts = OffsetDateTime::from_unix_timestamp(1_780_000_001).expect("valid ts");
        let row = AuditRow {
            audit_id: 1,
            timestamp: ts,
            action: "ipc_system_discover".to_owned(),
            subject: "unknown_peer".to_owned(),
            decision: "info".to_owned(),
            profile: None,
            reason: None,
            actor: None,
            metadata_json: None,
        };
        let wire = row_to_wire(row);
        assert!(wire.profile.is_none());
        assert!(wire.reason.is_none());
        assert!(wire.actor.is_none());
        assert!(wire.metadata_json.is_none());
    }
}
