# Run order — terminal-commander-windows-wsl-bridge

Status: WWS01 Completed (commit `6220eb2`); WWS02 Completed (commit `1da40f3`); WWS03 Completed (commit `ec8441e`); WWS04 Completed (commit `d86e73f`); WWS05 Completed (commit `ae37878`); WWS06 Completed (commit `4936904`); WWS07..WWS09 Pending.
Branch: `main`.

Execute goals strictly in this order. Each goal MUST pass its
verification gates AND a prep amendment (where appropriate) BEFORE
the next goal starts. Same precedent as the TC43–TC48 runtime
chain + NPM01–NPM10 distribution chain.

| Order | Goal   | Depends on | Stop the chain if blocked |
|-------|--------|------------|---------------------------|
| 1     | WWS01  | —          | YES — UX contract gates the chain |
| 2     | WWS02  | WWS01      | YES — root package contract gates the shim |
| 3     | WWS03  | WWS02      | YES — WSL doctor must work before bridge shim |
| 4     | WWS04  | WWS03      | YES — bridge shim is the Cursor entrypoint |
| 5     | WWS05  | WWS04      | YES — Cursor config writer needs the shim path |
| 6     | WWS06  | WWS05      | NO — install / pairing flow may ship as docs first if automation is blocked |
| 7     | WWS07  | WWS06      | YES — end-to-end smoke is the final gate |
| 8     | WWS08  | WWS07      | NO — README + release contract update is docs-only |
| 9     | WWS09  | WWS08      | YES — pre-publish readiness review closes the chain |

## Chain rules

- Each goal commits in two parts: verified work commit + goal
  status commit (TC43+ / NPM precedent).
- Prep amendments land in a separate `chore(goals): ...` commit
  BEFORE implementation when scope drift is found.
- `crates/**` and runtime behavior remain off-limits across the
  whole chain. The bridge / shim is JS only.
- The NPM10 `npm-bootstrap-publish.yml` workflow MUST NOT be
  dispatched during this chain. The first publish stays gated on
  the corrected package contract (WWS02) + WWS09 readiness review.
- `release-please.yml` and `npm-binary-build.yml` workflow files
  are off-limits unless WWS02 prep amendment explicitly widens
  scope.
- All long-lived tokens (`NPM_TOKEN_TC`, `CARGO_REGISTRY_TOKEN_TC`,
  `RELEASE_PLEASE_TOKEN_TC`) stay unused.
- Branch is `main`. Prohibited branches: `master`,
  `feature/terminal-commander-mvp`,
  `feature/terminal-commander-runtime`, `production`, `release`.
- Provider smoke promotion to `Go` is still gated on at least one
  provider live smoke transcript. WWS07 captures the Cursor smoke
  if the operator is reachable; otherwise records `Not Run`
  honestly.

## Exit criteria for the chain

- `npm install -g terminal-commander` on a clean Windows 11 host
  installs the root package; `terminal-commander setup cursor-wsl`
  detects WSL, picks a distro, ensures the runtime is installed
  or prints the exact one-line install command, writes the Cursor
  MCP config (or shows the operator what to paste), and prints a
  ready banner.
- `npm install -g terminal-commander` inside a WSL distro
  installs the root package + the matching platform package via
  `optionalDependencies` (existing NPM02–NPM05 contract).
- Cursor launched on Windows after setup discovers the
  `terminal-commander` MCP server and surfaces the 29-tool TC45
  catalogue.
- A real Cursor session executes `command_start_combed` →
  `bucket_wait` → `command_status` against `echo hello` end-to-end
  through the Windows bridge into WSL, with a captured transcript
  attached as evidence. If not feasible on the verification host,
  WWS07 marks the smoke `Not Run` with the exact blocker.
- README and `RELEASE_CHECKLIST.md` updated to reflect the
  Windows-bridge + WSL-runtime split.
- `BACKLOG.md` follow-ups for any work deferred during the chain.
- Beta posture in `RELEASE_CHECKLIST.md` either:
  - stays `Conditional Go` because at least one provider live
    smoke remains `Not Run`, OR
  - promotes to `Go` if the Cursor live smoke transcript lands
    AND every other gate from NPM09 §7 / WWS09 is green.
- Workflow `npm-bootstrap-publish.yml` is documented in WWS08 as
  the one-time bootstrap path; the disable / rotate follow-up
  (BACKLOG P1.5b) remains operator-driven and is NOT executed
  inside this chain.
