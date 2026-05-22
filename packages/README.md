# `packages/` — npm distribution surface

Status: NPM03 baseline.

This directory holds the npm packages that distribute Terminal
Commander binaries. Locked layout per the
[NPM02 contract](../docs/release/npm-binary-packaging-contract.md).

| Package directory | Published name | Role |
|-------------------|----------------|------|
| `terminal-commander/` | `terminal-commander` | Root wrapper. JS bin shims + platform resolver. Pulls platform binaries via `optionalDependencies`. |
| `terminal-commander-linux-x64/` | `@terminal-commander/linux-x64` | Linux x86_64 binaries. |
| `terminal-commander-linux-arm64/` | `@terminal-commander/linux-arm64` | Linux aarch64 binaries. |

User-facing install:

```sh
npm install -g terminal-commander
```

Installs three commands on PATH:

- `terminal-commanderd`
- `terminal-commander-mcp`
- `terminal-commander`

## What this layout does NOT do

- Does NOT run a postinstall downloader.
- Does NOT compile Rust during `npm install`.
- Does NOT ship macOS or Windows-native binaries (TC44 runtime is
  Unix-only).
- Does NOT touch the filesystem or open sockets beyond resolving +
  spawning the installed Rust binary. The shims are bounded to
  `child_process.spawn` with `shell: false` and `stdio: 'inherit'`.

## Local development

Build the Rust workspace, then copy the binaries into the matching
platform package `bin/`:

```sh
cargo build --release -p terminal-commanderd -p terminal-commander-mcp -p terminal-commander
arch=$(uname -m)
case "$arch" in
  x86_64)  plat=linux-x64 ;;
  aarch64) plat=linux-arm64 ;;
  *) echo "unsupported $arch"; exit 64 ;;
esac
cp target/release/terminal-commanderd      packages/terminal-commander-$plat/bin/
cp target/release/terminal-commander-mcp   packages/terminal-commander-$plat/bin/
cp target/release/terminal-commander       packages/terminal-commander-$plat/bin/
```

NPM04 ships a `scripts/smoke/verify-npm-local-install.sh` that
automates this end-to-end.

## Tarball contents

`packages/<name>/package.json` declares a `files` whitelist; only
files in that whitelist ship inside the published tarball. The
repository's `packages/.gitignore` excludes the real Rust binaries
+ `*.tgz` artifacts; placeholders (named `*.placeholder`) survive
both the gitignore and the `files` filter so the layout is tracked
without committing the binaries themselves.

## Provenance

This layout is bound by the NPM02 contract; any divergence requires
a prep amendment to NPM03 or a successor goal.
