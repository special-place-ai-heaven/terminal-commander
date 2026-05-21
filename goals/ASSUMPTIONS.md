# Assumptions Register - terminal-commander-mvp

## Confirmed by user

- Target repository: `https://github.com/special-place-administrator/terminal-commander.git`
- Repository is new/empty except the initial README.md already added by the user.
- Target branch policy is accepted as:
  - target_branch: `feature/terminal-commander-mvp`
  - prohibited_branches: `["main", "master"]`
- Goal files should be run linearly with `/goal`.
- Work mode: mixed planning and implementation goals.

## Architect assumptions to verify during goals

- Stack: Rust workspace for daemon, probes, storage, CLI, and MCP server.
- Storage: SQLite or equivalent embedded local store for registry and event data; exact crate/backend must be selected and documented during implementation.
- MCP transport: provider-neutral local MCP server, with stdio or local transport selected and documented during implementation.
- Security: no privileged helper or sudo/root execution until policy and explicit design goals allow it.
- Destructive changes: no destructive migrations, filesystem deletions, or system install actions unless a goal explicitly permits and verifies dry-run behavior first.
- Platform: Linux and WSL are the primary targets; macOS/Windows-native behavior is out of MVP unless discovered and documented.
- Raw output: no unbounded terminal/file output is returned by default.

## Correction protocol

If any assumption is wrong, the running agent must stop the current goal, mark it `Blocked`, record the corrected fact, and avoid guessing.
