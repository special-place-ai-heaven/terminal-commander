# Goal Chain Index — terminal-commander-npm-distribution

Status: reopened for NPM10 bootstrap exception (2026-05-23).
Branch: `main`.
Successor of: `terminal-commander-runtime` (TC48 = Conditional Go on `main` at `e42e7e4`).
Chain outcome (NPM09): **Conditional Go** preserved. Local runtime + npm layout + CI matrix + release-please + trusted-publishing workflow + Cursor docs + README all `live`. First live npm publish + Cursor provider live smoke remain Pending operator-driven steps.
NPM10 (added 2026-05-23): one-time `NPM_TOKEN_TC` bootstrap publish workflow, `workflow_dispatch` only, two-gate confirm. Explicit policy exception to NPM07's OIDC-only contract because npmjs.com may require a package page to exist before trusted publisher can be configured. After the first publish succeeds, NPM11 (TBD) disables the bootstrap workflow and rotates `NPM_TOKEN_TC`.

This chain stands up npm distribution + Cursor MCP install path for
Terminal Commander. It is a release / distribution lifecycle chain,
NOT a runtime feature chain. The TC33–TC48 runtime surface is the
input contract; this chain does not change runtime behavior.

Language: ASCII only.

## Chain summary

| Goal  | Title                                                          | Status   |
|-------|----------------------------------------------------------------|----------|
| NPM01 | Memory and Symforge release audit                              | Completed (5dcbaa4) |
| NPM02 | npm binary packaging contract                                  | Completed (81f4ea1) |
| NPM03 | Wrapper package and platform package layout                    | Completed (00f3ddb) |
| NPM04 | Local `npm pack` and global install smoke                      | Completed (970db2c) |
| NPM05 | GitHub Actions build matrix                                    | Completed (2bbf2fd) |
| NPM06 | release-please manifest config                                 | Completed (e81eb3f) |
| NPM07 | npm trusted publishing workflow                                | Completed (1b4267e) |
| NPM08 | Cursor MCP install config smoke                                | Completed (6ab2343) |
| NPM08b| README project normative overhaul                              | Completed (cacfef5) |
| NPM09 | Release dry-run and beta publish review                        | Completed (b2770e1) |
| NPM10 | Bootstrap first npm publish with NPM_TOKEN_TC (exception)      | Completed (9353fb4) |

## Target user experience (assumption — locked at NPM02)

```bash
# Inside the supported platform (Linux x64 / Linux arm64, or WSL2)
npm install -g terminal-commander
```

Then on PATH:

- `terminal-commanderd`
- `terminal-commander-mcp`
- `terminal-commander`

Cursor MCP config (project `.cursor/mcp.json` or global
`~/.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp"
    }
  }
}
```

Windows operators run Cursor on Windows and invoke through WSL:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "wsl",
      "args": ["-d", "Ubuntu-24.04", "bash", "-lc", "terminal-commander-mcp"]
    }
  }
}
```

## Locked assumptions (must be honored or amended in a prep PR)

- Root npm wrapper package: `terminal-commander`.
- Platform binary packages:
  - `@terminal-commander/linux-x64`
  - `@terminal-commander/linux-arm64`
- Root `bin` field exposes the three commands as Node shims that
  resolve the platform binary via `optionalDependencies`.
- Initial platforms: Linux x64 + Linux arm64 only. No macOS, no
  Windows-native package until the runtime supports them
  (`crates/probes/src/pty.rs` is `cfg(unix)`; the daemon UDS is
  Unix-only; TC44 explicitly defers Windows ConPTY).
- WSL is supported by installing inside WSL OR by Cursor invoking
  `wsl ... terminal-commander-mcp`.
- No `postinstall` binary download. Platform packages ship their
  binaries directly and are pulled via `optionalDependencies`.
- Prefer npm trusted publishing / OIDC via GitHub Actions. No
  long-lived `NPM_TOKEN` unless trusted publishing is impossible
  AND explicitly approved in a goal final report.
- Provenance enabled.
- Cursor MCP config is the primary provider smoke target for this
  chain.

## Out of chain (deferred)

- macOS arm64 / x64 packages — pending runtime macOS validation.
- Windows-native binary package — pending Windows runtime support
  (currently out of scope per TC44 `non_goals`).
- crates.io publishing — out of TC31 + TC48 scope.
- Static linking / musl variants — opportunistic.
- Cursor-extension automatic install — opportunistic.
- Continue, Cline, Codex, Claude Code config templates — covered by
  the runtime chain (TC46) and the operator beta phase, not this
  chain.

## Cross-chain invariants (inherited from terminal-commander-runtime)

- MCP must not spawn commands directly. (`rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` clean.)
- MCP must not read files directly. (`rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` clean.)
- No network listener.
- No raw stream endpoint.
- No shell-execution expansion.
- No privileged helper or system service install.
- No secrets, tokens, private usernames, private absolute paths, or
  machine-specific paths in committed artifacts.
- `Not Run` evidence MUST NOT be promoted to PASS.
- Beta posture ceiling remains `Conditional Go` until at least one
  provider live smoke transcript exists.
