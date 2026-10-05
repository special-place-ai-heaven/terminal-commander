// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! The daemon runs any command for whoever reaches its socket, and its store
//! holds command output, so its data directory, socket and store must be
//! owner-only whatever the umask. These tests bootstrap under umask 000 (the
//! most permissive) and check the modes the daemon leaves behind.

#![cfg(unix)]
#![allow(unsafe_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use terminal_commanderd::ipc::protocol::{IpcRequest, IpcResponse};
use terminal_commanderd::{DaemonClient, DaemonConfig, DaemonState, IpcServer};

fn tmp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("tc-modes-{tag}-{}-{nanos}", std::process::id()))
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

/// Bootstrap and serve with the process umask at 000.
fn serve_permissive(data: &Path) -> (Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    // SAFETY: umask only swaps the process file-creation mask.
    unsafe { libc::umask(0) };
    let state = Arc::new(DaemonState::bootstrap(DaemonConfig::defaults_in(data)).unwrap());
    let handle = IpcServer::new(Arc::clone(&state), state.config.socket_path())
        .spawn()
        .unwrap();
    (state, handle)
}

#[test]
fn a_permissive_umask_still_leaves_the_data_dir_socket_and_store_owner_only() {
    rt().block_on(async {
        let root = tmp_root("umask");
        // A missing parent too: every directory created on the way is private.
        let data = root.join("nested").join("data");
        let (state, handle) = serve_permissive(&data);
        assert_eq!(mode(&data), 0o700, "data dir");
        assert_eq!(mode(&root.join("nested")), 0o700, "created parent");
        assert_eq!(mode(&state.config.socket_path()), 0o600, "socket");
        let db = state.config.db_path();
        assert_eq!(mode(&db), 0o600, "store");
        for suffix in ["-wal", "-shm"] {
            let side = PathBuf::from(format!("{}{suffix}", db.display()));
            if side.exists() {
                assert_eq!(mode(&side), 0o600, "{}", side.display());
            }
        }
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&root);
    });
}

#[test]
fn a_world_readable_data_dir_is_tightened_and_named_by_self_check() {
    rt().block_on(async {
        let data = tmp_root("loose");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (state, handle) = serve_permissive(&data);
        assert_eq!(mode(&data), 0o700);
        let client =
            DaemonClient::new(state.config.socket_path()).with_timeout(Duration::from_secs(30));
        let Ok(IpcResponse::SelfCheck(sc)) = client.call(1, IpcRequest::SelfCheck).await else {
            panic!("self_check failed");
        };
        assert!(
            sc.report.contains(&format!(
                "tightened {} from mode 755 to 700",
                data.display()
            )),
            "{}",
            sc.report
        );
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

/// The daemon serves its own user only. Another uid cannot be produced
/// without root, so this checks the decision itself; the refusal path is
/// the same branch any foreign peer takes.
#[test]
fn only_the_daemons_own_uid_is_served() {
    // SAFETY: geteuid takes no arguments and cannot fail.
    let own = unsafe { libc::geteuid() };
    assert!(terminal_commanderd::ipc::peer::same_user(own));
    assert!(!terminal_commanderd::ipc::peer::same_user(
        own.wrapping_add(1)
    ));
    // The refusal names both identities and the fix.
    let msg = terminal_commanderd::ipc::peer::foreign_peer_message(0);
    assert!(msg.contains(&format!("daemon runs as uid {own}")), "{msg}");
    assert!(msg.contains("client runs as uid 0 (root)"), "{msg}");
    assert!(
        msg.contains("Run the client as the user that owns the daemon"),
        "{msg}"
    );
}
