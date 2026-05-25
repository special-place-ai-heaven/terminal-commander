# Changelog

## [0.1.10](https://github.com/special-place-ai-heaven/terminal-commander/compare/v0.1.9...v0.1.10) (2026-05-25)


### Bug Fixes

* make terminal commander setup and update native-first ([35f0ac9](https://github.com/special-place-ai-heaven/terminal-commander/commit/35f0ac9a372a00b5b7a7d1cb1941660791485da0))

## [0.1.9](https://github.com/special-place-ai-heaven/terminal-commander/compare/v0.1.8...v0.1.9) (2026-05-25)


### Bug Fixes

* make npm distribution av-safe ([af24679](https://github.com/special-place-ai-heaven/terminal-commander/commit/af246791160b0b84fbf0a2501fb9c780e044d83b))

## [0.1.8](https://github.com/special-place-administrator/terminal-commander/compare/v0.1.7...v0.1.8) (2026-05-25)


### Features

* **bootstrap:** zero-touch npm install with lazy MCP repair ([b7b2e9f](https://github.com/special-place-administrator/terminal-commander/commit/b7b2e9fb093274dfe8d6f6a9205b1dd13a6736e1))
* **daemon:** autostart on WSL boot and before MCP bridge ([499c093](https://github.com/special-place-administrator/terminal-commander/commit/499c093ccdb72b8a23f01b305f430946213042de))
* **install:** zero-touch Windows bootstrap and harness auto-config ([4eba830](https://github.com/special-place-administrator/terminal-commander/commit/4eba83014c2c4faf25f42705d1b3ba92de34e5d4))
* **release:** prepare first npm beta publish ([ab4b87e](https://github.com/special-place-administrator/terminal-commander/commit/ab4b87ee952f208391f101cf8cc1f536f0df6106))
* **resolver:** support darwin/x64 + darwin/arm64 platform targets ([44eadd6](https://github.com/special-place-administrator/terminal-commander/commit/44eadd6be09de599efe2c1b32a32efa0fb774634))


### Bug Fixes

* **mcp+supervisor.js:** close 3 codex follow-up issues from ae2de3c ([d64bb88](https://github.com/special-place-administrator/terminal-commander/commit/d64bb883f140401fc296caa7d27c4108e55d91b0))
* **packages:** pin windows-x64 + mac-x64 + mac-arm64 optionalDependencies by version ([25860e3](https://github.com/special-place-administrator/terminal-commander/commit/25860e34a795e52edaada43eb2be95e0bfb394fe))
* **supervisor.js:** await daemon readiness before spawning MCP (cold-start race) ([f436f48](https://github.com/special-place-administrator/terminal-commander/commit/f436f48ae5a57c609303c24ecec452c0692081f3))
* **supervisor.js:** close parent fd copy after daemon spawn ([1c29109](https://github.com/special-place-administrator/terminal-commander/commit/1c29109d67a397bed2c3d036a67650121b160474))
* **supervisor.js:** early-return on signal during cold-start wait ([62ce224](https://github.com/special-place-administrator/terminal-commander/commit/62ce224237379de70e736005a3e76a55d62defd0))
* **supervisor.js:** stop deleting session state on MCP exit ([56fb0cb](https://github.com/special-place-administrator/terminal-commander/commit/56fb0cb27de0ebe6ed1ac6eb13feced8f4eb44dc))
* **windows:** reliable WSL MCP bridge and harness config paths ([13cae8e](https://github.com/special-place-administrator/terminal-commander/commit/13cae8e194eedae06f26f21c798cc298dbd57fbf))

## [0.1.5](https://github.com/special-place-administrator/terminal-commander/compare/v0.1.4...v0.1.5) (2026-05-25)


### Features

* **resolver:** support darwin/x64 + darwin/arm64 platform targets ([44eadd6](https://github.com/special-place-administrator/terminal-commander/commit/44eadd6be09de599efe2c1b32a32efa0fb774634))


### Bug Fixes

* **packages:** pin windows-x64 + mac-x64 + mac-arm64 optionalDependencies by version ([25860e3](https://github.com/special-place-administrator/terminal-commander/commit/25860e34a795e52edaada43eb2be95e0bfb394fe))

## [0.1.4](https://github.com/special-place-administrator/terminal-commander/compare/v0.1.3...v0.1.4) (2026-05-23)


### Features

* **bootstrap:** zero-touch npm install with lazy MCP repair on first connect ([b7b2e9f](https://github.com/special-place-administrator/terminal-commander/commit/b7b2e9fb093274dfe8d6f6a9205b1dd13a6736e1))
* **daemon:** autostart on WSL boot (systemd or profile) and before MCP bridge ([499c093](https://github.com/special-place-administrator/terminal-commander/commit/499c093ccdb72b8a23f01b305f430946213042de))


### Bug Fixes

* **windows:** WSL MCP bridge uses Linux-first PATH and re-execs native MCP when the Windows npm shim is invoked under /mnt/c ([13cae8e](https://github.com/special-place-administrator/terminal-commander/commit/13cae8e194eedae06f26f21c798cc298dbd57fbf))
* **harness:** Claude Code MCP writes target ~/.claude.json ([13cae8e](https://github.com/special-place-administrator/terminal-commander/commit/13cae8e194eedae06f26f21c798cc298dbd57fbf))

## [0.1.3](https://github.com/special-place-administrator/terminal-commander/compare/v0.1.2...v0.1.3) (2026-05-23)


### Features

* **install:** zero-touch Windows bootstrap and harness auto-config ([4eba830](https://github.com/special-place-administrator/terminal-commander/commit/4eba83014c2c4faf25f42705d1b3ba92de34e5d4))
* **release:** prepare first npm beta publish ([ab4b87e](https://github.com/special-place-administrator/terminal-commander/commit/ab4b87ee952f208391f101cf8cc1f536f0df6106))

## [0.1.2](https://github.com/special-place-administrator/terminal-commander/compare/v0.1.1...v0.1.2) (2026-05-23)


### Features

* **install:** zero-touch Windows bootstrap and harness auto-config ([4eba830](https://github.com/special-place-administrator/terminal-commander/commit/4eba83014c2c4faf25f42705d1b3ba92de34e5d4))
* **release:** prepare first npm beta publish ([ab4b87e](https://github.com/special-place-administrator/terminal-commander/commit/ab4b87ee952f208391f101cf8cc1f536f0df6106))

## [0.1.1-beta.1](https://github.com/special-place-administrator/terminal-commander/compare/v0.1.0-beta.1...v0.1.1-beta.1) (2026-05-23)


### Features

* **release:** prepare first npm beta publish ([ab4b87e](https://github.com/special-place-administrator/terminal-commander/commit/ab4b87ee952f208391f101cf8cc1f536f0df6106))
