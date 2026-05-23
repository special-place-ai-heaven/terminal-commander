# @terminal-commander/linux-arm64

Internal platform package for **linux/arm64** (aarch64). Consumed by
the `terminal-commander` root wrapper via `optionalDependencies`; do
not depend on this package directly.

This tarball carries the three prebuilt Rust release binaries:

- `terminal-commanderd`
- `terminal-commander-mcp`
- `terminal-commander`

CI builds them inline on `ubuntu-24.04-arm` and stages them into `bin/`
before `npm publish --tag beta`.

## Release marker (first npm beta)

- First public beta publish is driven by release-please on `main`.
- Supported host: **linux arm64** only (`os: linux`, `cpu: arm64`).
- No Windows or macOS binaries in this package.
- No musl / Alpine variant.

## License

Apache-2.0. See `LICENSE`.
