# Source Map - terminal-commander-mvp

## User-provided source material

- Repository URL: `https://github.com/special-place-administrator/terminal-commander.git`
- Product name candidates and chosen working name: Terminal Commander, Live Signal Comber, signal buckets, probes, sifters, registry.
- User requirement: LLMs should interact with an MCP-operated abstraction layer rather than directly tailing, grepping, or reading large terminal output.
- User requirement: every terminal line or file update must be processed continuously by local probes, not by periodic LLM polling.
- User requirement: dynamic registry of regex/keyword/condition rules where an LLM can search, create, test, edit, and activate rules by ID.
- User requirement: signal buckets must expose timestamped, pointer-backed, vetted events and bounded context windows.
- User requirement: system should support terminal streams, command execution, files, directories/artifacts, parallel probes, router/proxy behavior, scale, flexibility, and provider-neutral MCP operation.
- User requirement: branch-safe, evidence-driven, numbered `/goal` files that can be run linearly.

## Verified project state before goal generation

- User reports the GitHub repository has been created and the initial README.md has been added.
- No local clone or repository contents were inspected while generating this goal chain.

## Inferences to verify in TC01

- Rust is the preferred implementation stack for the serious product.
- SQLite or an embedded store is appropriate for registry and event store persistence.
- MCP server can be implemented as a local user-mode process communicating with a daemon through an internal API.
- Linux and WSL can share most probe/runtime behavior but require documented service/startup differences.
