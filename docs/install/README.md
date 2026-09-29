# Install / Startup - Terminal Commander

Status: current install contract as of 2026-05-25.

This document captures the supported install and startup behavior for
Terminal Commander. Per `docs/security/PRIVILEGE_MODEL.md`, installers
never auto-elevate and never install a privileged helper.

Language: ASCII only.

## 1. Supported package targets

The npm wrapper resolves one optional native platform package:

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`
- `@terminal-commander/windows-x64`
- `@terminal-commander/mac-x64`
- `@terminal-commander/mac-arm64`

Windows uses the native Windows package by default. The legacy
Windows-to-WSL bridge is opt-in with `TC_USE_LEGACY_WSL_BRIDGE=1`.

## 2. Install model

Primary user install:

```bash
npm install -g terminal-commander@latest
```

npm installs outside CI run a guarded `postinstall` setup for detected
harnesses. CI and `TC_NO_AUTO_SETUP=1` skip it. Run the explicit command below
to configure or repair a harness whenever auto-setup was skipped.

Development builds can still run the Rust crates directly:

```bash
cargo build --release -p terminal-commanderd -p terminal-commander-mcp -p terminal-commander-cli
```

## 3. Explicit setup

To configure or repair detected harnesses explicitly:

```bash
terminal-commander setup harness
```

To target one provider:

```bash
terminal-commander setup harness --provider cursor
terminal-commander setup harness --provider codex-cli
terminal-commander setup harness --provider omp
terminal-commander setup harness --provider claude-code
terminal-commander setup harness --provider claude-desktop
```

The setup command refreshes only Terminal Commander's entry and backs up an
existing config before writing.

## 4. Startup model

The LLM harness launches `terminal-commander-mcp` as a stdio MCP
adapter. The adapter talks to the local daemon over local IPC:

- Unix-like systems use Unix domain sockets.
- Windows native uses named pipes.

Daemon startup is owned by the installed adapter/supervisor path when
the harness invokes `terminal-commander-mcp`, not by npm lifecycle
scripts. The daemon is local-only; it does not open a network listener
(the owner's one-shot password page for `credential_request` binds
`127.0.0.1` only, and only while a PTY password prompt is pending).

Linux operators may still use the user-level systemd example at
`config/terminal-commanderd.service.example`. The example unit does not
install itself.

## 5. Legacy WSL setup

WSL setup is explicit and legacy-scoped. It is not run by npm install.
Use WSL-specific setup or doctor commands only when intentionally using
the legacy Windows-to-WSL bridge.

Filesystem placement for WSL remains strict: the daemon SQLite database
must live on a native Linux filesystem, never `/mnt/c` drvfs. See
`config/terminal-commanderd.example.toml` for the recommended
`data_dir = "$HOME/.local/share/terminal-commanderd"`.

## 6. Configuration

`config/terminal-commanderd.example.toml` is the operator-tunable
configuration. It sets the policy profile, retention limits, and data
directory. It is safe to commit because it contains no secrets.

## 7. Operator checklist

1. Install with `npm install -g terminal-commander@latest`.
2. Run `terminal-commander setup harness`.
3. Run `terminal-commander doctor harness` and
   `terminal-commander doctor daemon`.
4. Start the LLM harness and verify `system_discover` reports the live
   tool catalogue.

## 8. What install never adds

- No setuid binary.
- No polkit rule.
- No system-level systemd unit.
- No privileged helper.
- No network-listening service (loopback-only credential page aside).
- No hidden-window helper.
- No automatic WSL runtime install.

## 9. Source-status

| Component | Status |
|---|---|
| npm wrapper package | live |
| Native platform packages | live for Linux, Windows, and macOS targets listed above |
| rmcp stdio adapter | live |
| Daemon IPC | live via UDS on Unix-like systems and named pipes on Windows |
| Explicit harness setup | live |
| Explicit WSL legacy bridge | live only when opted in |
| Privileged helper installer | never |
