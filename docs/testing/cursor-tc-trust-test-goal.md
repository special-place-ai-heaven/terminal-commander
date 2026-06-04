# MISSION — Empirical Tool-Surface Test of Terminal Commander (for Cursor)

You are **Cursor** (OpenAI's agentic CLI). The `terminal-commander` MCP server is
configured in your environment and exposes a set of tools. Your job is to run an
**empirical, hands-on trust test** of those tools by actually calling them on a
real project, and to write a single structured report of what worked, what hit a
format/schema wall, and where the error did (or did not) teach you the fix.

This file is your complete brief. It embeds every prompt and every target you
need. You do **not** need any other document. Do **not** read Terminal
Commander's own Rust source to figure out tool argument formats — that defeats
the entire test (see CONSTRAINTS). Behave like a naive MCP client: read the
**live tool schemas** the MCP server advertises, construct calls from those, and
record what happened.

---

## THE GOAL (one line)

`/goal empirically test every terminal-commander MCP tool by actually calling it on a real project, in order P1->P11 then O1/O2 then Debrief, until a single structured report exists at outputs/cursor-tc-test-report.md with a per-prompt table, per-trust-breaker rollup, a first-try trust ratio, the Debrief self-assessment, and ranked findings — without simulating any call, without reverse-engineering arg formats from TC source, and without destructive shell ops.`

---

## CONTEXT (what you're testing and why)

- **Terminal Commander (TC)** is a Rust daemon + MCP server. Its pitch: an LLM can
  run shell commands, **comb** their output down to just the signal (matching
  lines + a bounded exit receipt), watch files, read/search files, manage
  detection "rules" and "rule packs", and (on supported platforms) drive PTY
  sessions — all through structured MCP tools instead of raw shell.
- The thing under test is **trust**: when a real LLM (you) tries to use these
  tools cold, does each call succeed **on the first try** because the tool's
  schema/description made the input format obvious — or do you hit a wall
  (missing field, wrong shape, "scope required", "rule not active", unsupported
  platform) and have to learn the format from the error?
- This run is the **baseline**. The repo is at TC **0.1.38** (the version this
  test kit was written against). You are capturing the walls as they exist today.

### Tool surface you should expect (names only — read the live schema for args)

These 17 tools are what TC advertises. Treat this list as a map, not a spec —
**get each tool's real input schema from the MCP server, not from this file**:

- Command run/comb: `run_and_watch`, `command_start_combed`, `command_status`,
  `command_output_tail`, `bucket_events_since`
- Rules / packs: `registry_upsert`, `registry_activate`, `registry_deactivate`,
  `registry_import_pack`
- Files: `file_read_window`, `file_search`, `file_watch_start`
- Interactive: `pty_command_start`
- Discovery / state: `system_discover`, `runtime_state`, `probe_list`

---

## SUCCESS CRITERIA (the mission is DONE only when ALL are true)

1. You have confirmed the `terminal-commander` MCP tools are actually available
   to you (you can list them and see their schemas), and you have recorded the
   installed TC version (run `terminal-commander --version`; if that binary is
   not on PATH, record how the MCP server identifies its version, or record
   "version unknown — could not determine" with the reason).
2. You have executed **every** prompt P1 through P11, then O1 and O2, **in that
   exact order** (P4 must run before P11 — P4 creates the rule that P11
   deactivates), by **actually invoking** the relevant terminal-commander MCP
   tools and capturing the **real** response or error **verbatim** for each.
3. You have run the **Debrief** reflection last and recorded honest answers.
4. A single report file exists at **`outputs/cursor-tc-test-report.md`**
   (relative to the Terminal Commander repo root, i.e.
   `E:\project\terminal-commander\outputs\cursor-tc-test-report.md`) containing
   all six required sections listed under "REQUIRED REPORT FORMAT" below.
5. Every "OK" you claim in the report is backed by a quoted real tool response —
   no outcome is asserted from reading code or from what you *expected* to
   happen.

---

## CONSTRAINTS (rules you MUST follow)

- `[invoke]` **Actually call the tools.** Evidence = the real MCP responses and
  errors. Narrating what a tool "would" return, or simulating a call, is a
  failed prompt — mark it fail and say you did not really call it. No fabricated
  responses.
- `[naive-first]` **Construct every call from the tool's live schema first**, as
  a first-time client would. You are FORBIDDEN from reading Terminal Commander's
  Rust source (anything under `crates/`, `src/`, the SPEC, etc.) to discover an
  argument's shape before calling. If a call fails and you only then figure out
  the correct format **from the error message**, that is a key datapoint — record
  it as "learned from error", not as a clean first-try success.
- `[honesty]` **A wall is a wall. A guess that failed is a failure.** Report the
  rough edges; do not paper over them, do not retroactively call a 3-retry
  success a "first try", do not soften an error into "minor". Honesty over
  optimism: mock is mock, blocked is blocked, unverified is unverified.
- `[platform-datapoint]` If a tool is genuinely unavailable on this platform
  (e.g. PTY/`pty_command_start` on Windows), **record that as the result** (it is
  exactly what TB-1 probes) — do not silently skip it. Note whether you learned
  it was unavailable **up front** from `system_discover` (good) or only by trying
  and hitting an `UnsupportedPlatform`-style error mid-plan (bad).
- `[no-destruct]` **No destructive shell operations.** Use the safe, bounded test
  commands suggested in each prompt (build/test, `mkdir -p`, `printf`, `node -e`,
  `df -h`, reading a logfile, etc.). Do not `rm -rf`, do not delete repo files,
  do not modify tracked source, do not push or commit anything. The only file you
  create is the report (and any tiny throwaway test artifact a prompt explicitly
  needs, e.g. a temp dir or a temp logfile, placed under a temp/scratch path).
- `[real-project]` Work in a **real project directory** with a buildable
  Rust/Node project, at least one source file, and a logfile (or one you create
  for the test). The Terminal Commander repo itself
  (`E:\project\terminal-commander`) is a fine choice: it builds with
  `cargo build`, has source files, and you can generate a logfile. Whatever you
  pick, **record the exact working dir and the real commands/paths you
  substituted** for each prompt's `<...>` placeholder.
- `[order]` Run P1..P11 strictly in order, then O1, O2, then Debrief. Do not
  reorder. P4 before P11 is mandatory.
- `[verbatim]` Capture real responses/errors **verbatim** (quote them). Truncate
  only obviously huge bodies, and say "[truncated]" when you do.

---

## STOP CONDITIONS

- **MCP server cannot initialize / tools not available:** if you cannot list the
  terminal-commander tools or the server fails to start/handshake, **stop**.
  Write the report with this as the single top finding (include the exact
  initialization error verbatim), fill the table rows with "blocked — server
  unavailable", and end. Do not fake the rest of the run.
- If a single prompt is blocked but the server is otherwise healthy, **do not
  stop** — mark that prompt fail/blocked with the real error and continue to the
  next prompt. One bad prompt does not abort the mission.

---

## SETUP (do this first, before P1)

1. **List tools + confirm availability.** Enumerate the terminal-commander MCP
   tools and confirm you can see their input schemas. If you cannot -> STOP
   CONDITION above.
2. **Record TC version.** Run `terminal-commander --version`. Capture the exact
   string. If unavailable, note how you determined the version (or that you
   couldn't).
3. **Record harness identity.** Note that the executor is **Cursor** and which
   model/harness you are running as (model name/version as known to you).
4. **Pick + record the working dir** and the concrete real values you'll
   substitute for each prompt's `<...>` placeholders (build command, a quiet
   command, an exploratory command, a token to search, a source file, a logfile
   path). Default working dir: `E:\project\terminal-commander`. Suggested
   substitutions on this repo:
   - `<build-or-test command>` -> `cargo build` (noisy enough to comb)
   - `<a command that succeeds with little/no output>` -> `mkdir -p` of a temp dir
   - `<an exploratory command>` -> `df -h` (or `cargo --version`)
   - `<a command that prints a line with PANIC>` -> `printf 'ok\nPANIC: boom\n'`
   - `<cargo|npm|pytest|apt|gcc|make|cleanup|generic.terminal>` -> `cargo`
   - `<a logfile path>` -> create a small temp logfile and append `ERROR` lines,
     OR point at a real build log you capture
   - `<a token>` -> `fn main`; `<a source file or dir>` -> a real `.rs` file in
     the repo
   - `<python3|sudo ...>` -> `python3` (interactive session)
   - `<a noisy build/test command>` (O1) -> `cargo build`
   Adjust if you choose a different project — just record what you actually used.

---

## THE PROMPTS — run each, in order, by actually calling the tools

For each prompt: (a) read it as the user request, (b) construct the
terminal-commander call(s) from the **live schema**, (c) call them for real,
(d) capture the verbatim response/error, (e) note whether it worked first try or
you had to learn the format from an error, (f) fill the corresponding table row.
The **Target** block under each prompt tells you what *should* happen and what
trust-breaker (TB) it probes — use it to judge the outcome, but the verdict comes
from the real result, not the target.

### P1 — Run + comb (core value)
> Use your terminal-commander tools to run `<build-or-test command>` and show me
> only the error/failure lines plus the exit code. I don't want the full output.

**Target:** `run_and_watch` or `command_start_combed` (+ inline `rules` or an
imported pack). Success = matching lines + exit code, first try, no raw-shell
fallback. Probes: core product value + `run_and_watch` ergonomics; `argv`/`rules`
shape (**TB-11**).

### P2 — Quiet command (no-fake-success)
> Use terminal-commander to run `<a command that succeeds with little/no output>`
> and tell me whether it passed.

**Target:** `run_and_watch`/`command_status` returns a bounded exit **receipt**
even with 0 rule matches. Success = you report pass/exit cleanly; you do NOT
treat a quiet command as an error or bounce to raw shell. Probes: the "quiet
command never looks broken" promise.

### P3 — Tail a one-off command
> Use terminal-commander to run `<an exploratory command whose format you don't
> know>` and show me its output. I don't have a rule for it.

**Target:** `command_output_tail` (start then tail), bounded. Success = readable
tail, first try. Probes: tail ergonomics; do you know tail exists for rule-less
reads.

### P4 — Author + activate a CUSTOM rule (the hard path) — RUN BEFORE P11
> Use terminal-commander to create a rule that flags any line containing `PANIC`
> as critical severity, activate it, then run `<a command that prints a line with
> PANIC, e.g. printf 'ok\nPANIC: boom\n'>` and show me what it caught.

**Target:** `registry_upsert` (a full RuleDefinition) -> `registry_activate`
(with scope) -> run. Success = authored + activated + matched, **first try**.
Probes the worst cluster: **TB-3** (does the schema show the RuleDefinition
shape, or 5 "missing field" round-trips?), **TB-4** (do you know to set
`status:active`, or hit RuleNotActive?), **TB-5** (do you pass `scope`, or hit
"scope required"?). NOTE: remember the rule's identity (id/name/version/scope) —
you will deactivate it in P11.

### P5 — Import a rule pack
> Use terminal-commander to set up expert error detection for
> `<cargo|npm|pytest|apt|gcc|make|cleanup|generic.terminal>` commands, then run
> the matching command and show me only the signal.

**Target:** `registry_import_pack` (activate=true, scope) -> run. Success = pack
imported + activated + matched, first try. Probes: **TB-5** (scope on import),
**TB-9** (is `cleanup` discoverable as a pack?).

### P6 — Set an environment variable (the brutal one)
> Use terminal-commander to run
> `node -e "console.log('FOO=' + process.env.FOO)"` with the environment variable
> `FOO` set to `bar`, and show me the output.

**Target:** `run_and_watch`/`command_start_combed` with `env`. Success = `FOO=bar`
printed, command does NOT crash, first try. Probes: **TB-2** — do you pass `env`
as the array-of-`{key,value}` (not a map)? does the program still have PATH
(env replace-vs-overlay)? Watch for exit 134 / crash = the replace-semantics
wall — if it crashes, that's the datapoint.

### P7 — Watch a log file
> Use terminal-commander to watch `<a logfile path>` for any line containing
> `ERROR` and show me the matching events.

**Target:** `file_watch_start` -> `bucket_events_since` (cursor). Success = watch
started + events read, first try. Probes: **TB-8** (do you know the first
`cursor` is 0 / where it comes from?), opaque-id flow. (You may need to append an
`ERROR` line to the file to generate an event — do so via a safe `printf`/append,
not a destructive op.)

### P8 — Read + search a file
> Use terminal-commander to find where `<a token, e.g. fn main>` appears in
> `<a source file or dir>` and show me the surrounding lines.

**Target:** `file_search` -> `file_read_window`. Success = search hit + window,
first try. Probes: **TB-10** (do `path`/`query` descriptions make the call
obvious?), line-window params.

### P9 — Interactive / PTY session
> Use terminal-commander to start an interactive `<python3|sudo ...>` session and
> send it one command.

**Target:** `pty_command_start` (ideally after `system_discover`). Success on
**Windows** = you learn PTY is **unavailable up front** (from discovery) and say
so / route around it, instead of trying and hitting `UnsupportedPlatform`
mid-plan. On **Linux/mac** = PTY works. Probes: **TB-1** (does discovery tell the
truth about PTY on this platform?). Record which platform you're on and which of
these two outcomes occurred.

### P10 — Discover capabilities + inspect
> Use terminal-commander to tell me what it can do, and what commands/probes are
> currently running.

**Target:** `system_discover` + `runtime_state`/`probe_list`. Success = accurate
capability list + running state. Probes: **TB-1** (no false "PTY available" on
Windows), discovery sufficiency (can you self-configure from discover alone, or
do you need to guess schemas?).

### P11 — Deactivate (RUN AFTER P4)
> Use terminal-commander to deactivate the PANIC rule you created earlier.

**Target:** `registry_deactivate`. Success = deactivated, OR a **teaching error**
if you pass a wrong version/scope. Probes: **TB-6** — if you mismatch, do you get
`ok:true` silently (bad) or a clear "no active row..." error (good)? Record which.

### O1 — Organic (no "use terminal-commander" steer)
> Run `<a noisy build/test command>` and just tell me what failed.

**Target:** Do you REACH FOR terminal-commander on your own, or default to raw
shell? Record which you chose and **why**. (Higher-order trust signal.)

### O2 — Organic (no steer)
> Something keeps erroring in `<logfile>`. Find the errors for me.

**Target:** Same as O1 — natural preference. Record tool-vs-shell choice + why.

---

## DEBRIEF (run LAST, after O2 — answer honestly about the run you just did)

1. Which terminal-commander tools did you call?
2. Did any call fail on your **FIRST** attempt? For each: what was the error, and
   did the tool's description/schema tell you the correct input format **before**
   you called it, or did you only learn it from the error?
3. Were there moments you considered using the **raw shell** instead of
   terminal-commander? Which, and why?
4. For each tool you used, rate **1-5**: "I knew exactly how to call it from its
   description alone." List the low scores and what was missing.

Be candid — this is hunting for rough edges.

---

## REQUIRED REPORT FORMAT (write to `outputs/cursor-tc-test-report.md`)

The report MUST contain all six sections, in this order:

### 1. Header
- TC version tested (exact `terminal-commander --version` string, or how
  determined / why unknown)
- Date of run
- Executor = Cursor; model/harness identity
- Platform/OS
- Working dir used + the concrete substitutions for every `<...>` placeholder

### 2. Per-prompt table
One row per prompt (P1..P11, O1, O2). Columns exactly:

| Prompt | Outcome (OK/partial/fail) | Tools called | First try? (retries) | Format/schema wall? (which) | Did the error teach the fix? | Fell back to raw shell? | Notes (quote the real response/error) |
|--------|---------------------------|--------------|----------------------|-----------------------------|------------------------------|-------------------------|----------------------------------------|

- **Outcome:** OK / partial / fail — judged from the real result.
- **Tools called:** the actual MCP tool names you invoked for that prompt.
- **First try? (retries):** "yes" or "no — N retries"; a success after retries is
  NOT a first-try success.
- **Format/schema wall?:** name the specific wall (missing field, wrong env
  shape, scope required, rule not active, unknown cursor, unsupported platform,
  etc.) or "none".
- **Did the error teach the fix?:** yes/no/n-a — did the error message contain
  enough to fix the call without guessing?
- **Fell back to raw shell?:** yes/no — and if yes, why.
- **Notes:** quote the real response or error verbatim (truncate huge bodies with
  "[truncated]").

### 3. Per-trust-breaker rollup (TB-1 .. TB-11)
For each trust-breaker, "hit" or "clean", with the evidence (which prompt, what
happened). Use this mapping (derived from the Targets above):

- **TB-1** — Discovery honesty about PTY / platform (P9, P10)
- **TB-2** — `env` shape (array-of-{key,value} vs map) + PATH replace-vs-overlay /
  crash (P6)
- **TB-3** — RuleDefinition shape discoverable from schema vs missing-field
  cascade (P4)
- **TB-4** — knowing to set `status:active` vs RuleNotActive (P4)
- **TB-5** — `scope` required on activate/import (P4, P5)
- **TB-6** — mismatched deactivate: clear teaching error vs silent false-success
  (P11)
- **TB-8** — first `cursor` value / where it comes from (P7)
- **TB-9** — `cleanup` discoverable as a pack (P5)
- **TB-10** — `path`/`query` descriptions make file_search obvious (P8)
- **TB-11** — `argv`/`rules` shape on run_and_watch (P1)
- **TB-7** — any additional wall you hit that doesn't map to the above (record it
  here so the rollup stays complete; if none, say "clean — no extra wall").

For each: state hit/clean + cite the prompt and quote the deciding evidence.

### 4. Trust ratio
`first-try successful calls / total calls` — count every real MCP call you made
(across all prompts), and report the fraction + the percentage. Show the numerator
and denominator and briefly how you counted.

### 5. Debrief self-assessment
The four Debrief answers above, written out honestly (tools used; first-try
failures + whether schema or error taught the format; raw-shell temptations;
per-tool 1-5 clarity ratings with the low scores explained).

### 6. Ranked findings + recommendations
Ranked list (most to least severe) of where TC's **schema / tool description /
error message** failed to make the correct call format clear **up front**. For
each finding: the symptom (with prompt + quoted evidence), the trust-breaker it
maps to, and a concrete recommendation (what the schema/description/error should
say so a naive client gets it right first try). End with the single highest-
leverage fix.

---

## WHAT "GOOD" WOULD LOOK LIKE (reference, not a requirement to fake)

This is the bar a *hardened* TC would clear — use it to calibrate severity, NOT
to invent passing results:
- P4/P5: rule authoring + pack activation succeed FIRST try (no missing-field
  cascade, no scope error, no Draft/inactive error).
- P6: `env` set right first try; program keeps PATH; no crash.
- P7: cursor "just works" (0 on first call).
- P9/P10: discovery is HONEST about PTY on this platform; you never plan around a
  tool that will fail.
- P11: a mismatched deactivate ERRORS clearly instead of false-success.
- Debrief: few/no "learned it from the error"; high "knew from the description";
  rare/no shell-fallback temptation.

If reality falls short of any of these, that gap IS the finding. Report it
plainly.

---

## FINISH

When `outputs/cursor-tc-test-report.md` exists with all six sections, every prompt
row filled from real calls, the trust ratio computed, and findings ranked — the
mission is complete. State the report path and a 3-line summary (trust ratio,
worst trust-breaker hit, top recommendation). Do not push or commit. Do not claim
any "OK" you cannot back with a quoted real response.
