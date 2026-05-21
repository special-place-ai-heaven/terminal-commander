# Policy-decision enum (closed set)

| Decision | Meaning |
|---|---|
| `allow` | Action proceeds. Audit record emitted before the action runs. |
| `deny` | Action refused. Audit record emitted; caller sees a policy error. |
| `allow_with_audit` | Action proceeds AND a high-severity audit record is emitted. Used for default-deny overrides per `POLICY.md` section 5. |
| `error` | Policy evaluation itself failed (invalid profile, missing version). Action refused; emits an error-tagged audit record. |

The set is CLOSED. A new decision value requires amending
`POLICY.md` section 6 first.
