# Run order — terminal-commander-npm-distribution

Status: skeleton (NPM00 init).
Branch: `main`.

Execute goals strictly in this order. Each goal MUST pass its
verification gates AND a prep amendment (where appropriate) BEFORE
the next goal starts. Same precedent as the TC43–TC48 runtime chain.

| Order | Goal  | Depends on | Stop the chain if blocked |
|-------|-------|------------|---------------------------|
| 1     | NPM01 | —          | YES — audit gates the chain |
| 2     | NPM02 | NPM01      | YES — contract gates layout |
| 3     | NPM03 | NPM02      | YES — layout gates everything |
| 4     | NPM04 | NPM03      | YES — local install must work |
| 5     | NPM05 | NPM04      | YES — CI cannot ship without local proof |
| 6     | NPM06 | NPM05      | YES — release-please needs the matrix |
| 7     | NPM07 | NPM06      | YES — publishing needs release-please |
| 8     | NPM08 | NPM07      | NO — Cursor smoke is operator-driven, may run after a candidate ships |
| 9     | NPM08b| NPM08      | NO — README overhaul is documentation-only; chain ships if NPM08b slips |
| 10    | NPM09 | NPM08b     | YES — dry-run is the final gate |

## Chain rules

- Each goal commits in two parts: verified work commit + goal
  status commit (TC43+ precedent).
- Prep amendments land in a separate `chore(goals): ...` commit
  BEFORE implementation when scope drift is found.
- Provider smoke (NPM08 Cursor) is operator-driven; if the operator
  host lacks Cursor, the goal is marked `Not Run` with the exact
  blocker — never promoted to PASS.
- `crates/**` and runtime behavior remain off-limits across the
  whole chain. Any product-code change must be a tiny compatibility
  fix tied to a recorded issue, otherwise stop and report.
- Branch is `main`. Prohibited branches: `master`,
  `feature/terminal-commander-mvp`,
  `feature/terminal-commander-runtime`, `production`, `release`.

## Exit criteria for the chain

- `npm install -g terminal-commander` works on a clean Linux x64
  host and yields `terminal-commanderd`, `terminal-commander-mcp`,
  `terminal-commander` on PATH.
- Cursor MCP stdio config from `docs/integrations/cursor.md` (added
  by NPM08) successfully discovers the 29-tool TC45 catalogue and
  executes the minimal `command_start_combed` → `bucket_wait` →
  `command_status` flow.
- GitHub Actions builds Linux x64 + Linux arm64 platform binaries
  reproducibly.
- release-please manifest mode produces a release PR on
  Conventional Commits.
- npm trusted publishing via GitHub Actions OIDC publishes the root
  + platform packages on tag.
- Beta posture in `RELEASE_CHECKLIST.md` either:
  - stays `Conditional Go` because provider live smoke remains
    `Not Run`, OR
  - promotes to `Go` if at least one Cursor MCP live smoke
    transcript is attached.
