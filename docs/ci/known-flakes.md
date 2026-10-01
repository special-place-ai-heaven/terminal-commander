# CI known flakes

Failures in this list are runner-harness flakes. A green product check that
then dies in a post-step is not a reason to rerun or deprecate a release.

| Class | Where | What to do |
| --- | --- | --- |
| Windows verify post-hook after a green smoke | `verify-windows-x64` in `.github/workflows/release-please.yml` | If the smoke step printed `verify-windows-x64: OK` (version + `supportedVersions`) and the job failed only on `Post Run actions/setup-node` / `Post Run actions/checkout` while the runner logged `Cleaning up orphan processes`, do not blind-rerun the release or deprecate the version. Same leftover process-tree class as a Windows `terminal-commanderd` stop returning Access denied during update-lock preflight: the probe's parent is `node` running `terminal-commander-mcp.js`, which a name-only kill of `terminal-commander-mcp` misses, and a leftover daemon/handle can deny a later stop. Product stop/update-lock path is Larry's; this row is CI-only. [#251](https://github.com/special-place-ai-heaven/terminal-commander/issues/251), run [36858930018](https://github.com/special-place-ai-heaven/terminal-commander/actions/runs/36858930018). |
