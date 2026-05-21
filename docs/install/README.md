# Install / Startup - Terminal Commander

Status: TC26 baseline.

This document captures the locked install + startup story for the
Terminal Commander MVP. Per `docs/security/PRIVILEGE_MODEL.md` the
installer NEVER auto-elevates and NEVER installs a privileged helper.

Language: ASCII only.

## 1. MVP target platforms

- Linux native (x86_64, aarch64)
- WSL2 (Linux distribution, NOT Windows native)

macOS and Windows-native are deferred (see ROADMAP).

## 2. Install model: cargo install + manual wire-up

MVP ships as a Cargo workspace. There is no setuid binary, no
distribution package, and no systemd unit at MVP. Operators install
via:

```bash
cargo install --path crates/daemon       # terminal-commanderd
cargo install --path crates/mcp          # terminal-commander-mcp
cargo install --path crates/cli          # terminal-commander
```

Binaries land in `$CARGO_HOME/bin` (default `~/.cargo/bin`).

## 3. Startup

### 3.1 Linux native (with systemd)

A user-level systemd unit example lives in
`config/terminal-commanderd.service.example`. It runs the daemon under
the operator's UID. Operators copy it to
`~/.config/systemd/user/terminal-commanderd.service` and
`systemctl --user enable --now terminal-commanderd`.

The example unit does NOT install. It is a starting point.

### 3.2 WSL2 (no systemd)

Per `docs/research/wsl-boundary.md`, WSL2 distros do NOT have
systemd by default. The daemon is launched manually from the
operator's shell rc file or wrapper script:

```bash
# In ~/.profile or ~/.bashrc, after the cargo bin path is set:
if [ -z "$TERMINAL_COMMANDERD_PID" ]; then
    terminal-commanderd >>"$HOME/.cache/tcmd.log" 2>&1 &
    export TERMINAL_COMMANDERD_PID=$!
fi
```

Filesystem placement: the daemon SQLite database MUST live on a
native Linux filesystem (ext4 / btrfs / xfs on WSL2), NEVER on
`/mnt/c` (drvfs 9P). The TC12 store rejects 9P-backed paths at
writer open per `EVENT_STORE.md` section 3. See
`config/terminal-commanderd.example.toml` for the recommended
`data_dir = "$HOME/.local/share/terminal-commanderd"`.

## 4. Config files

`config/terminal-commanderd.example.toml` is the operator-tunable
configuration. It sets the policy profile (default
`developer_local`), retention limits, and data directory. It is
SAFE TO COMMIT (no secrets).

## 5. Operator checklist

1. Install the three binaries via `cargo install --path ...`.
2. Copy `config/terminal-commanderd.example.toml` to
   `~/.config/terminal-commanderd/terminal-commanderd.toml` and edit.
3. Verify the data directory is on a native filesystem (NOT
   /mnt/c on WSL2).
4. Run `terminal-commander doctor` (TC25 CLI) and check every
   line says `ok`.
5. Start the daemon (systemd-user on bare Linux; shell rc on WSL2).
6. Verify MCP server attaches by running it through your LLM
   harness (see TC27 examples).

## 6. What MVP does NOT install

- No setuid binary.
- No polkit rule.
- No system-level systemd unit (only user-level example).
- No privileged helper.
- No network-listening service.

These are deliberate gaps. See `docs/security/PRIVILEGE_MODEL.md`
sections 5, 6, 9.

## 7. Source-status

| Component | Status |
|---|---|
| cargo install paths | live (TC26) |
| systemd user-unit example | live (TC26) |
| WSL2 startup snippet | live (TC26) |
| Distribution packages (deb/rpm/aur) | deferred (post-MVP) |
| Privileged helper installer | NEVER (PRIVILEGE_MODEL.md) |
