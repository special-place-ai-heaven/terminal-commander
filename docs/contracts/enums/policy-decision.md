# Policy-decision enum (closed set)

Note (2026-10-05): audit rows also accept `info` (`ALLOWED_AUDIT_DECISIONS`,
`crates/store/src/audit.rs`); the `PolicyDecision` enum itself has the four
values below.

| Decision | Meaning |
|---|---|
| `allow` | Action proceeds. Audit record emitted before the action runs. |
| `deny` | Action refused. Audit record emitted; caller sees a policy error. |
| `allow_with_audit` | Action proceeds AND a high-severity audit record is emitted. In practice used for audited capability use (shell, session lanes); the default-deny override mechanism of `POLICY.md` section 5 is not implemented. |
| `error` | Policy evaluation itself failed (invalid profile, missing version). Action refused; emits an error-tagged audit record. |

The set is CLOSED. A new decision value requires amending
`POLICY.md` section 6 first.
