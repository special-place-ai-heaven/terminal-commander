// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Shared test source, not a crate: test files include it with
// `#[path = "../../test_support/isolated_env.rs"] mod isolated_env;`.

use std::path::Path;
use std::process::Command;

/// Run a spawned terminal-commander process, and everything it starts, as if
/// for a user whose home is `home`, with no endpoint variable inherited from
/// the developer's shell. Call it first and set the test's own `TC_*` after.
///
/// Without it a login shell anywhere in the tree reads the developer's real
/// profile, and the installed Linux autostart there starts a second daemon
/// from the inherited `TC_DATA`/`TC_SOCKET` -- onto the test's socket.
// `redundant_pub_crate` wants `pub` here; `unreachable_pub` rejects that.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn isolate<'a>(cmd: &'a mut Command, home: &Path) -> &'a mut Command {
    #[cfg(unix)]
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"));
    #[cfg(not(unix))]
    let _ = home;
    cmd.env_remove("TC_SOCKET")
        .env_remove("TC_DATA")
        .env_remove("TC_SESSION")
}
