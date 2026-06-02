# Repository instructions

## Cursor Cloud specific instructions

- Standard development commands live in `README.md`, `CONTRIBUTING.md`, and `TESTING.md`; prefer those sources for routine lint, test, and build commands.
- The product is local-only: `terminal-commanderd` is the daemon, `terminal-commander-mcp` is the stdio MCP adapter, and `terminal-commander` is the admin CLI. No external database, cache, Docker service, or network listener is required for core development.
- When starting a manual daemon in Cursor Cloud, use an explicit data directory and pass the matching socket to clients, for example set `TC_SOCKET=/tmp/terminal-commander-dev-data/terminal-commanderd.sock` for CLI or MCP checks. Set `TC_IDLE_TTL_SECS=0` only for long-lived manual dev sessions; the documented default self-reaps idle daemons.
- The npm wrapper package has a stale committed lockfile shape relative to `package.json`. For dependency refreshes, use the approved startup update script rather than a plain `npm install`, which can dirty `packages/terminal-commander/package-lock.json`.
- When a change touches `cfg(unix)` / `cfg(windows)` / `target_os` code or any test, run the OS gates before pushing as described in `CONTRIBUTING.md` section 6.1 ("OS-specific code").
