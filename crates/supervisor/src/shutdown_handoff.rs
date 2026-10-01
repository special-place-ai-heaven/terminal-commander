// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Name and security descriptor for the Windows daemon shutdown event.
//
// `terminal-commander update` has to stop a running `terminal-commanderd.exe`
// before npm can replace the image. `TerminateProcess` from a normal
// (medium integrity) token fails with Access Denied when that daemon is
// elevated. The command pipe cannot be the workaround: lowering its
// integrity label would let a medium process drive an elevated daemon's
// command API. This event is shutdown-only.

/// Kernel object name the daemon creates and `update-locks` signals.
///
/// `Local\` is the interactive session namespace, shared by the elevated and
/// filtered tokens of the same logon. The pid keeps two daemons from sharing
/// one event.
#[must_use]
pub fn event_name(pid: u32) -> String {
    format!(r"Local\terminal-commanderd-shutdown-{pid}")
}

/// SDDL for that event.
///
/// Owner is the daemon user. The only allow ACE is `EVENT_MODIFY_STATE`
/// (`0x0002`) for that same user, so a caller can signal shutdown and nothing
/// else. `S:(ML;;NW;;;ME)` labels the object Medium: a non-elevated updater
/// can `SetEvent` even when the daemon process is high integrity, and a low
/// integrity process cannot write up. No Everyone (`WD`) ace.
///
/// # Errors
///
/// Returns an error when `user_sid` is not a canonical SID string. The value
/// is interpolated into SDDL, so anything else is refused.
pub fn event_sddl(user_sid: &str) -> Result<String, &'static str> {
    // Canonical SIDs are `S-` plus digits and hyphens. That alphabet cannot
    // close an ACE or add a trustee, which is the whole check.
    if user_sid.len() > 184
        || !user_sid.starts_with("S-")
        || !user_sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
    {
        return Err("refusing user sid that is not a canonical SID string");
    }
    Ok(format!(
        "O:{user_sid}D:(A;;0x0002;;;{user_sid})S:(ML;;NW;;;ME)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_is_session_local_and_pid_scoped() {
        assert_eq!(
            event_name(53132),
            r"Local\terminal-commanderd-shutdown-53132"
        );
    }

    #[test]
    fn event_sddl_is_same_user_signal_only_at_medium_integrity() {
        let sid = "S-1-5-21-1-2-3-1001";
        let sddl = event_sddl(sid).expect("canonical sid");
        assert!(sddl.contains("S:(ML;;NW;;;ME)"), "{sddl}");
        assert!(sddl.contains(&format!("O:{sid}")), "{sddl}");
        assert!(sddl.contains(&format!("(A;;0x0002;;;{sid})")), "{sddl}");
        assert!(!sddl.contains("WD"), "no Everyone ace: {sddl}");
        assert!(!sddl.contains("GA"), "no generic-all: {sddl}");
    }

    #[test]
    fn event_sddl_rejects_ace_injection() {
        assert!(event_sddl(r"S-1-5-21);(A;;GA;;;WD").is_err());
        assert!(event_sddl("").is_err());
        assert!(event_sddl("admin").is_err());
        assert!(event_sddl("S-1-5-21-1\n").is_err());
    }
}
