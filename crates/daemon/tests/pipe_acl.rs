// SPDX-License-Identifier: Apache-2.0
// Verifies the named-pipe security descriptor restricts access to
// LocalSystem + Administrators + current user.

#![cfg(windows)]

use terminal_commanderd::ipc::pipe_acl;

#[test]
fn sddl_includes_current_user_sid_and_denies_world() {
    let sddl = pipe_acl::build_sddl_for_current_user()
        .expect("build sddl");
    // Owner: current user.
    assert!(sddl.contains("O:"));
    // No (A;;...;;;WD)  (Everyone allow) and no (A;;...;;;BU) (Users).
    assert!(!sddl.contains(";;;WD)"), "ACL must not allow Everyone (WD): {sddl}");
    assert!(!sddl.contains(";;;BU)"), "ACL must not allow Users (BU): {sddl}");
    // Allow LocalSystem (SY) and Administrators (BA).
    assert!(sddl.contains(";;;SY)"));
    assert!(sddl.contains(";;;BA)"));
}
