# Recipe registry

Status: operator guide for the live argv recipe registry.
Audience: an LLM harness, and the human who activates recipes.
Language: ASCII only.

A recipe is a versioned argv task. A rule combs command output into
signals. Beachhead discovery names an execution route. Those are three
products. This page is how to use the recipe one.

Authoritative tool contract:
[`docs/mcp/TOOL_CONTROL_SURFACE.md`](../mcp/TOOL_CONTROL_SURFACE.md).
Lane selection for everything else:
[`docs/mcp/OMNI_PLAYBOOK.md`](../mcp/OMNI_PLAYBOOK.md).

## Recipes, rules, and beachhead

| Product | Question it answers | Names |
|---|---|---|
| Recipe registry | What argv task should run? | Full: `recipe_search`, `recipe_get`, `recipe_upsert`, `recipe_test`, `recipe_activate`, `recipe_deactivate`, `recipe_list_active`, `recipe_run`. Compact: `recipe`. |
| Rule registry | How should output be combed into signals? | Full: `registry_*`. Compact: `registry`. |
| Beachhead | Which execution route is available on this host? | `system_discover` `access_routes` and `beachhead` (`argv_template`). |

Do not send recipe actions to the `registry` facade. Compact `recipe`
actions are `search`, `get`, `upsert`, `test`, `activate`, `deactivate`,
`list_active`, and `run`. The harness provider registry
(`lib/harness/registry.js`) is a different name and is not this product.

CAP01 (capability registry / tentacles) is out of this surface. Recipe
definitions carry no secrets and no secret env values. `argv[0]` is a
program, not a shell interpreter (`powershell`, `pwsh`, `cmd`, `bash`,
`sh`, `zsh`, `fish`). There is no `shell_line` and no PowerShell or
shell script body.

## Happy path

Search, then an operator activates, then the model runs.

1. Search the store.

```text
recipe action=search query="git status"
recipe_search { "query": "git status" }
```

2. Read the hit if you need the argv and title.

```text
recipe action=get recipe_id="git.status"
```

3. Operator activates. MCP `recipe_activate` and `recipe_deactivate` are
   denied while `[policy] llm_can_activate_recipes` is false (the
   default). The deny text is `recipe_activate_requires_admin`. The
   operator commands are:

```text
terminal-commander recipes import [--activate]
terminal-commander recipes activate <recipe_id> [--version N]
terminal-commander recipes deactivate <recipe_id> [--version N]
terminal-commander recipes tombstone <recipe_id>
```

   `recipes activate` and `recipes import --activate` open a global
   activation only. Job, bucket, and probe scopes are refused, so a
   recipe cannot stay runnable after that job exits. `recipes deactivate`
   with no `--version` closes the active version in global scope, not
   the latest stored version. `recipes tombstone` retires the id and
   closes every open activation. `recipe_upsert` and `recipe_test` do
   not activate. `recipe_test` does not start a job. A tombstoned seed
   id is reported and skipped on import; the rest of the bank still
   imports, and a retry is not stuck on that id.

4. Run only an activated recipe. `recipe_run` (compact
   `recipe action=run`) refuses a recipe that is not active for the
   scope.

```text
recipe action=run recipe_id="git.status" scope={"kind":"global"}
recipe_run { "recipe_id": "git.status", "scope": { "kind": "global" } }
```

`recipe_run` uses the argv command lane. When the recipe has
`timeout_ms` or `rule_pack_ids`, the run follows `run_and_watch`.
Otherwise it follows `command_start_combed`. It does not call
`shell_exec`. `allow_shell` stays off.

If the recipe is not active, list what is:

```text
recipe action=list_active
```

Then wait for an operator to activate. Do not ask to enable shell.

## Teach: retry_with_recipe

A shell-misuse deny is MCP `-32602` with `kind` = `policy_denied`.
The daemon `ShellTeach` chooses the steer:

| Daemon | Envelope |
|---|---|
| `recipe_id` is set (matching activated recipe) | `recover_hint` = `retry_with_recipe`, `intended_tool` = `recipe_run`, `intended_example` = `{"recipe_id":"git.status"}` |
| no matching recipe | `recover_hint` = `retry_with_argv`, `intended_tool` = `run_and_watch`, `intended_example` = `{"argv":["git","status"]}` |

`alternatives` still lists argv tools (`run_and_watch`,
`command_start_combed`, file tools, PTY). `shell_exec` stays last and
tagged `operator_opt_in`. Follow `recover_hint`. Do not ask to enable
shell.

Discover's `shell_exec` catalogue row, while `allow_shell` is off, keeps
the argv steer (`retry_with_argv` / `run_and_watch`). The recipe steer
is on the deny envelope when the daemon sets `recipe_id`.

## Prefer argv and recipe_run over shell

Default order:

1. An activated recipe: `recipe_run` or `recipe action=run`.
2. A direct argv call: `run_and_watch` or `command_start_combed` (compact
   `command action=run_and_watch` / `command action=run`).
3. File tools for reading and searching files.
4. `shell_exec` only when the operator has set `allow_shell` and the
   line is still not an argv or a recipe. Default is off. A deny is not
   a request to flip that cap.

Beachhead `direct_argv` / `wsl_argv` templates end in `{args...}`. Use
them to place the program. The recipe still owns the concrete argv.

## Example recipe ids

These ids are the curated argv examples. Call them once they are stored
and activated. This page does not import them.

| `recipe_id` | argv |
|---|---|
| `git.status` | `["git", "status", "--short"]` |
| `git.diff` | `["git", "diff"]` |
| `git.log` | `["git", "log", "--oneline", "-n", "20"]` |
| `cargo.check` | `["cargo", "check"]` |
| `cargo.test` | `["cargo", "test"]` |
| `npm.test` | `["npm", "test"]` |
| `node.version` | `["node", "--version"]` |
| `git.ls-files` | `["git", "ls-files"]` |

`git.ls-files` is the listing probe. There is no `rg.files` seed in this
set. `npm` and `cargo` and `git` and `node` are argv0 programs, not
shell wrappers.

## Protocol floor

Harness setup is unchanged. Tip Terminal Commander accepts MCP protocol
**2026-07-28** only. Codex still needs its opt-in. OMP that initializes
with `2025-11-25` stays out of support. See
[MCP protocol floor](README.md#mcp-protocol-floor). This surface does
not reopen dual-protocol or OMP client work.

## See also

- [`docs/mcp/TOOL_CONTROL_SURFACE.md`](../mcp/TOOL_CONTROL_SURFACE.md)
  -- live catalogue (59 tools; six compact facades, including `recipe`).
- [`docs/mcp/OMNI_PLAYBOOK.md`](../mcp/OMNI_PLAYBOOK.md) -- lane choice
  when no recipe matches.
- [`POLICY.md`](../../POLICY.md) -- `llm_can_activate_recipes` and
  `allow_shell`.
- [`docs/integrations/README.md`](README.md) -- provider configs and the
  protocol floor.
