# terminal-commander

Local MCP-operated terminal/file signal channel. Raw output in,
vetted signal out; context by pointer. Linux + WSL2 only.

This npm package is the **root wrapper** for Terminal Commander. It
ships only JS bin shims; the actual Rust binaries arrive via the
matching platform `optionalDependency`:

- `@terminal-commander/linux-x64`
- `@terminal-commander/linux-arm64`

## Install

```sh
npm install -g terminal-commander
```

`npm install` pulls only the platform package matching your host
(`process.platform` + `process.arch`). On a host without any
matching platform package, the shims exit `64` with a one-line
stderr message naming the supported targets.

## Commands

| Command | Purpose |
|---------|---------|
| `terminal-commanderd` | The daemon. Holds the UDS, the bucket manager, the sifter runtime, the audit sink. |
| `terminal-commander-mcp` | The rmcp stdio MCP adapter. Forwards every tool call through the daemon UDS. |
| `terminal-commander` | Admin CLI. |

## Safety model

- The shims `require()` only the bundled resolver and
  `child_process.spawn` the resolved Rust binary with `shell: false`
  and `stdio: 'inherit'`.
- No postinstall script.
- No network calls at install time.
- No file reads beyond resolving the platform package via
  `require.resolve()`.
- No environment-variable echo.
- The MCP crate inside the Rust binaries refuses to spawn child
  commands, open network listeners, or read files — verified at
  every release by the project guard greps.

## Documentation

The runtime documentation, MCP tool catalogue, integration guides
(Codex CLI, Claude Code, Cursor), security model, and beta
release checklist all live in the upstream repository:

<https://github.com/special-place-administrator/terminal-commander>

## License

Apache-2.0. See `LICENSE`.
