// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies the named-pipe security descriptor restricts access to the
// current user, and that the raw pipe path refuses remote clients.

#![cfg(windows)]

use terminal_commanderd::ipc::pipe_acl;

#[test]
fn sddl_includes_current_user_sid_and_denies_world() {
    let sddl = pipe_acl::build_sddl_for_current_user().expect("build sddl");
    // Owner: current user.
    assert!(sddl.contains("O:"));
    // No (A;;...;;;WD)  (Everyone allow) and no (A;;...;;;BU) (Users).
    assert!(
        !sddl.contains(";;;WD)"),
        "ACL must not allow Everyone (WD): {sddl}"
    );
    assert!(
        !sddl.contains(";;;BU)"),
        "ACL must not allow Users (BU): {sddl}"
    );
    // Exactly one allow entry: the current user (the owner).
    let owner = sddl
        .strip_prefix("O:")
        .and_then(|rest| rest.split("D:").next())
        .expect("owner");
    assert_eq!(sddl, format!("O:{owner}D:(A;;GA;;;{owner})"));
}

/// The raw `CreateNamedPipeW` path (used whenever the ACL is set) must
/// refuse remote clients and claim the first instance; tokio's builder,
/// which defaults both, is not on that path.
#[test]
fn the_acl_pipe_path_rejects_remote_clients_and_squatters() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ipc/pipe_acl.rs"),
    )
    .unwrap();
    assert!(source.contains("PIPE_REJECT_REMOTE_CLIENTS.0"));
    assert!(source.contains("open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE"));
}
