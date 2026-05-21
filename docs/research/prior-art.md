# Prior Art - Terminal Commander Landscape Survey

Author: research agent R2-delta
Date: 2026-05-21
Scope: TC01 blind-spot H1 (prior art survey)

Plain ASCII. Every project carries a URL. Each entry ends with a
"TC differs because..." line that names the specific delta.

---

## 1. Agent-aware consumer terminals

### 1.1 Warp.dev (Warp Terminal / Warp Agent / Oz)

- URL: https://www.warp.dev/
- Type: Agentic Development Environment (ADE) centered on a custom
  terminal binary.
- What it solves: gives developers a modern terminal with a built-in
  agent layer that can read the user's commands and outputs, drive
  multi-agent workflows, and integrate model providers (Claude,
  OpenAI, Google).
- Shape: end-user GUI terminal binary plus an "Oz" cloud agent
  platform with observability, governance, and credit management.
- Audience: individual developers and enterprises.
- Programmatic: an API documentation portal exists at
  https://docs.warp.dev/ but the home page does not advertise a
  headless signal-extraction surface that a third-party LLM harness
  can call.
- TC differs because: Terminal Commander is a headless, provider-
  neutral MCP layer, not a terminal binary. It does not present a UI,
  does not own the user's shell, and emits structured signal events
  that any MCP-capable agent (Claude Code, Codex CLI, IDE agents)
  consumes. Warp owns the prompt; TC owns the signal extraction
  pipeline behind whatever shell the user already runs.

### 1.2 Cursor terminal agent (Cursor 3, Cursor CLI)

- URL: https://cursor.com/docs/agent/tools/terminal
- URL: https://cursor.com/cli
- What it solves: Cursor's agent runs shell commands as a tool inside
  an IDE-driven loop. Cursor 3 (April 2026) rebuilt the IDE around
  agent orchestration. Cursor CLI lets agent runs start from a
  terminal, integrate with CI, and be configured via sandbox.json
  for network and filesystem policies.
- Quoted behavior from docs: "Commands execute automatically while
  staying confined to your workspace."
- Output handling: Cursor docs focus on approval flows and
  sandboxing; the docs do not advertise a public "wait for signal"
  primitive or a structured event stream surface for the running
  command. The agent inside Cursor reads command output the
  traditional way (read tool result, parse in-LLM).
- TC differs because: TC moves the "read large output, decide what
  matters" burden OUT of the LLM and into a local streaming parser.
  TC exposes `bucket_wait` so an agent blocks on classified events
  with cursors and severity filters instead of tailing raw output
  inside a tool turn. TC is also harness-neutral - Cursor's terminal
  tool is Cursor-only.

### 1.3 Fig / Amazon Q Developer CLI

- URL: https://github.com/withfig/autocomplete
- URL: https://github.com/aws/amazon-q-developer-cli
- What it was/is: Fig was a terminal autocomplete and AI helper that
  used the macOS Accessibility API plus shell integration to read
  what the user typed and overlay completions. It was acquired by
  Amazon and rebranded as Amazon Q Developer CLI.
- Observation surface: per Fig FAQ, it "uses the Accessibility API
  on Mac to position the window, and integrates with your shell to
  read what you've typed" - primarily input observation, not passive
  output classification.
- TC differs because: TC is not a typing-side helper. TC watches
  long-running output (stdout, stderr, files, directories, future
  artifact streams) and classifies what came back, not what is being
  typed in. The signal direction is opposite.

### 1.4 Claude Code internal Bash tool (and background monitor)

- URL: https://docs.claude.com/en/docs/agents-and-tools/tool-use/bash-tool
- URL: https://github.com/Piebald-AI/claude-code-system-prompts
- What it is: the current state-of-art for "agent runs a command and
  reads the output." Claude Code's Bash tool runs in a persistent
  bash session. A complementary background monitor pattern streams
  stdout lines as chat notifications - "each stdout line is an
  event" with 200ms batching - and guidance instructs the user to
  pre-filter at the script level using `grep --line-buffered`, awk,
  or wrappers that emit exactly the success/failure signals.
- Key insight from Claude Code patterns (cited by community
  documentation): "Instead of running status checks on a timer,
  Claude waits for a process to emit a signal (a file write, a log
  entry, or a stdout stream match) and acts only when that signal
  arrives."
- TC differs because: Claude Code's pattern pushes the filtering
  responsibility back onto the USER (write a better grep, write a
  better wrapper). TC builds that capability INTO a reusable
  daemon with a versioned rule registry, so the same `cargo` or
  `apt` or `pytest` sifter pack works for every agent and every
  project. TC also provides bounded context lookup by event pointer
  - Claude Code's background monitor does not surface "show me 3
  lines before and 5 lines after event X" as a first-class tool.
  TC is also provider-neutral via MCP, while Claude Code's pattern
  is Claude-Code-only.

---

## 2. Containerized agent execution layers

### 2.1 container-use (Dagger-era project)

- URL: https://github.com/dagger/container-use
- What it solves: lets multiple coding agents run in parallel in
  isolated containerized environments without interfering with one
  another. Each agent gets its own container in a separate git
  branch.
- Architecture: an MCP server (stdio) that abstracts terminal and
  command execution into containers. Users can drop into any
  agent's terminal interactively.
- Observability claim: "full visibility into command history and
  logs showing exactly what agents executed, rather than relying on
  agent-provided summaries."
- TC differs because: container-use is about WHERE commands run
  (isolation, parallelism, git branches). TC is about HOW their
  output is read (continuous streaming sifters, signal buckets,
  context pointers). The two are orthogonal and composable - TC
  could observe output of a command being run inside a
  container-use environment. container-use observability is "see
  the logs"; TC observability is "classify, deduplicate, and
  surface only relevant lines as structured events with severity."

### 2.2 Dagger

- URL: https://dagger.io/
- What it solves: programmable build/test/deploy pipelines as code,
  with SDKs in eight languages, a runtime, system API, and an
  interactive REPL. Built-in tracing, logs, and metrics.
- Architecture: container-based execution model with typed inputs,
  intelligent caching, and a consistent execution interface across
  laptop, CI, and cloud.
- TC differs because: Dagger is a build orchestration platform that
  REPLACES shell scripts and YAML with typed pipeline code. TC does
  not replace anything - it observes whatever already runs (raw
  shell, just, make, cargo, docker, npm, dagger itself). TC sits
  one layer down: regardless of how the command was launched, TC
  watches the output. Dagger's observability is built into its own
  runtime; TC works for tools that have no telemetry of their own.

---

## 3. Programmable shell and command adjacencies

### 3.1 just

- URL: https://github.com/casey/just
- What it is: a command runner described as "a handy way to save
  and run project-specific commands." Cross-platform, recipe-based,
  no build-system semantics.
- Quote: "just is a command runner, not a build system, so it avoids
  much of make's complexity and idiosyncrasies."
- TC differs because: just is the LAUNCHER. TC is the OBSERVER. A
  user runs `just test`; TC's process probe streams the output and
  emits typed pytest-fail / test-pass / coverage events. There is
  no overlap - the two compose.

### 3.2 cargo-watch (and successor bacon/watchexec)

- URL: https://github.com/watchexec/cargo-watch
- Status: archived, recommended successor Bacon
  (https://dystroy.org/bacon/) or Watchexec
  (https://github.com/watchexec/watchexec).
- What it does: watches the filesystem for source changes and
  re-runs cargo commands. Pure file-event triggering, no stdout
  parsing.
- TC differs because: cargo-watch's loop is "file changed -> rerun."
  TC's loop is "output produced -> classify -> emit signal." Both
  are "react to a stream," but the stream is different (filesystem
  events vs command output). They could compose: cargo-watch
  triggers `cargo build`, TC observes the build output and emits
  compiler-error events.

### 3.3 direnv

- URL: https://github.com/direnv/direnv
- What it does: loads/unloads environment variables based on the
  current directory by sourcing a `.envrc` file before each shell
  prompt.
- TC differs because: direnv operates on shell entry, not on
  command execution or output. It is purely a config-loader. No
  signal extraction. Listed only because the planner's blind-spot
  ledger named it as a process-orchestration adjacency; the
  conceptual overlap is essentially zero.

---

## 4. Recording vs. real-time signal sifting

### 4.1 asciinema (with tmux pipelines)

- URL: https://github.com/asciinema/asciinema
- What it does: records terminal sessions into a compact
  `.cast` file format (asciicast v1/v2/v3). Supports playback,
  live streaming, conversion to text, and concatenation. Written
  in Rust, GPLv3.
- Real-time aspect: it supports live streaming to viewers, but the
  output is the raw cast format intended for replay - not
  classified signal events.
- TC differs because: asciinema is a PASSIVE archival recorder.
  Nothing in asciinema classifies "this line is a build error" or
  "this line is a sudo prompt." A user could post-process a cast
  file with regex tools, but that is a polling pattern, not a live
  classified signal stream. TC's bucket model (severity, kind,
  pointer) does not exist in asciinema. asciinema would be a
  reasonable upstream INPUT FORMAT for a future TC archival
  feature, but not a replacement.

---

## 5. Peer MCP server implementations (design references)

### 5.1 Official filesystem MCP server (TypeScript)

- URL: https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem
- Language: TypeScript / Node.js.
- Tools: 14 (read_text_file, read_media_file, read_multiple_files,
  list_directory, list_directory_with_sizes, directory_tree,
  search_files, get_file_info, list_allowed_directories,
  write_file, edit_file, create_directory, move_file).
- Architecture: single-process. Directory sandboxing via CLI args or
  the MCP Roots protocol. Dry-run previews for edits.
- TC differs because: filesystem MCP server is request/response over
  static files. TC streams - a probe lives for the duration of a
  watched command/file/directory, and events accumulate into a
  bucket the agent reads by cursor. TC has a daemon for streaming
  work; the official filesystem server has no daemon, just an MCP
  process per invocation.

### 5.2 github-mcp-server (Go)

- URL: https://github.com/github/github-mcp-server
- Language: Go.
- Tools: repository ops, issue / PR management, GitHub Actions,
  security alerts, discussions, notifications, projects, gists,
  team management. Selectable toolsets.
- Architecture: single-process. Deployable locally via Docker or
  remotely as GitHub-hosted.
- TC differs because: GitHub MCP server is a thin wrapper over an
  external REST/GraphQL API - GitHub.com is doing all the heavy
  lifting. TC has no remote API to wrap; the daemon IS the engine.
  GitHub MCP server doesn't need streaming because GitHub itself
  exposes events. TC must produce events itself by classifying
  noisy local streams.

### 5.3 container-use (covered above 2.1)

Listed again here for completeness as a peer MCP server in Go,
exposing containerized command execution as MCP tools. Single
process. Stdio transport.

### 5.4 fetch MCP server (Python)

- URL: https://github.com/modelcontextprotocol/servers/tree/main/src/fetch
  (note: WebFetch returned 404 against the directory; canonical link
  is the servers monorepo)
- Language: Python.
- Tools: a single `fetch` tool with `url`, `max_length`, `start_index`,
  `raw` parameters. Respects robots.txt for model-initiated requests.
- TC differs because: fetch is a one-shot HTTP-to-markdown call. It
  has no concept of duration, no buckets, no cursor. The closest TC
  analog is `file_read_window` - both bound the output size, but
  TC's surface is a streaming pipeline, not a single fetch.

### 5.5 rust-mcp-stack ecosystem

- URL: https://github.com/rust-mcp-stack
- Repos: rust-mcp-sdk, rust-mcp-filesystem, rust-mcp-schema,
  mcp-discovery, oauth2-test-server, mcp-discovery-action.
- Scope: an alternate Rust ecosystem for MCP servers, MIT-licensed.
- TC posture: prior research selected the OFFICIAL rmcp crate
  (modelcontextprotocol/rust-sdk) as the MCP SDK, per
  `mcp-rust-sdk.md` in this folder. rust-mcp-stack is a viable
  alternative if the official SDK ever blocks; recording it here so
  the existence of an alternative is part of the documented
  landscape.
- TC differs because: rust-mcp-stack is a TOOLKIT (SDK + schema +
  discovery + sample filesystem server). TC is a PRODUCT that
  happens to use an MCP SDK as a dependency. rust-mcp-filesystem
  occupies the same niche as the official filesystem server; both
  are static-file-oriented and lack the streaming/sifter/bucket
  surface TC is designed for.

---

## 6. Structured signal extraction from text streams (cross-domain analog)

### 6.1 Honeycomb honeytail

- URL: https://github.com/honeycombio/honeytail
- URL: https://docs.honeycomb.io/send-data/logs/structured/honeytail
- URL: https://www.honeycomb.io/blog/new-custom-regex-log-ingestion
- What it is: Honeycomb's agent for ingesting log file data and
  extracting structured events. Quote: "Contains various parsers
  for extracting structured data out of common log files."
- Parser stack: JSON, regex (with named capture groups), logfmt,
  nginx, MySQL, PostgreSQL, MongoDB, CSV, syslog, ArangoDB.
- Daemon behavior: "designed to run as a daemon so that it can
  continuously consume new content as it appears in the log files
  as well as detect when a log file rotates" and resume from
  saved progress after interrupts.
- Output: structured events shipped to Honeycomb for analysis.
- TC differs because: this is the closest direct analog. The
  architectural patterns OVERLAP substantially - daemon, pluggable
  parsers, regex named groups, file rotation, resume-after-restart.
  The DIFFERENCES are:
  1. Consumer: honeytail ships events to a remote SaaS for query;
     TC keeps everything local and exposes events back to the agent
     synchronously through MCP.
  2. Output shape: honeytail produces flat structured records;
     TC produces typed events with severity, kind enum, source
     pointer, and captures.
  3. Source set: honeytail watches log FILES. TC watches files
     plus live command output plus directories plus future
     artifacts (JUnit XML, coverage JSON). Honeytail covers a
     subset of TC's probe types.
  4. Bounded context: honeytail emits an event and moves on; TC
     keeps a context spool so the agent can request "3 lines
     before, 5 lines after." That round-trip primitive is not in
     honeytail.
  5. Dynamic registry: TC lets the LLM search, create, test, and
     activate sifter rules at runtime. honeytail rules are static
     config.

### 6.2 Datadog agent log collection

- URL: https://docs.datadoghq.com/logs/log_collection/
- What it does: Datadog agent on the host tails log files or
  listens on UDP/TCP. Logs flow through parsers and processors
  (integration pipelines) that extract structured fields before
  shipment.
- Quote-equivalent: agents can "tail log files or listen for logs
  sent over UDP/TCP," with "Processors" available to transform and
  enrich during ingestion. Default integration pipelines remap
  logging-library parameters to standard attributes and "extract
  the `error.message` and `error.kind`."
- TC differs because: Datadog's pipeline is local agent + remote
  SaaS, like Honeycomb. TC is local-only. Datadog's processor
  catalog is enterprise-wide and tuned for production logs (web
  servers, databases, cloud platforms). TC's sifter catalog is
  tuned for developer-loop output (compilers, package managers,
  test frameworks, interactive prompts). The closest TC concept is
  the rule pack - generic.terminal.json, apt.json, cargo.json,
  npm.json, pytest.json, gcc.json - shipped as defaults per
  README's planned `rules/` directory. The agent-to-pipeline shape
  is genuinely similar; the consumer side is different (LLM via MCP
  vs. operator via web UI).

---

## 7. Position statement: where Terminal Commander sits

Terminal Commander is best described as the intersection of three
adjacent product categories:

1. From honeytail / Datadog: continuous, daemon-backed, pluggable-
   parser ingestion of noisy text into structured events.
2. From Claude Code's background-monitor pattern: agent-facing
   "wait for relevant signal" instead of "tail raw output."
3. From MCP server practice (filesystem, github-mcp-server,
   container-use): provider-neutral tool surface usable by any
   MCP-capable harness.

What no surveyed product does:

- No project combines all three of: local daemon, MCP surface for
  any harness, AND LLM-runtime-mutable rule registry with structured
  events plus bounded context pointers.
- No project markets "structured event bucket with cursor and
  severity filter" as a first-class primitive for coding agents.
- No project ships a curated, versionable sifter catalog tuned for
  developer-loop tools (cargo, apt, pytest, gcc, npm) and exposes
  that catalog through MCP for an agent to search, edit, test, and
  activate at runtime.

What is novel:

- The signal-bucket-by-cursor model. Other tools either push every
  parsed event downstream (Datadog/Honeycomb) or expose nothing
  (cargo-watch). TC's bucket is a persistent, replayable,
  cursor-addressable stream that an agent reads only the delta of.
- The LLM-runtime sifter registry. honeytail rules are config-time;
  TC rules are runtime, searchable, testable, and activatable from
  an MCP call. This is genuinely new and aligns with the README's
  "registry_search / registry_create / registry_test /
  registry_activate" tool surface.
- The combination of probe types (process + terminal-PTY + file +
  directory + planned journal + planned artifact) under one
  pointer/context model. honeytail covers file probes only;
  container-use covers process execution but not signal classifying;
  asciinema records but does not classify.

What is NOT novel:

- The daemon + parser-stack + structured-event shape (Honeycomb
  and Datadog ship this for years).
- The MCP server shape (multiple references already exist).
- The Rust + tokio + SQLite stack (prior research files in this
  folder document the precedent).

The novelty is the COMBINATION and the AGENT-OPERATED-REGISTRY
slant. Position TC as "the streaming-signal MCP server tuned for
coding-agent loops, the way Datadog's agent is tuned for production
ops loops."

---

## Sources cited

- https://www.warp.dev/
- https://cursor.com/docs/agent/tools/terminal
- https://cursor.com/cli
- https://github.com/withfig/autocomplete
- https://github.com/aws/amazon-q-developer-cli
- https://docs.claude.com/en/docs/agents-and-tools/tool-use/bash-tool
- https://github.com/Piebald-AI/claude-code-system-prompts
- https://github.com/dagger/container-use
- https://dagger.io/
- https://github.com/casey/just
- https://github.com/watchexec/cargo-watch
- https://github.com/direnv/direnv
- https://github.com/asciinema/asciinema
- https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem
- https://github.com/github/github-mcp-server
- https://github.com/modelcontextprotocol/servers/tree/main/src/fetch
- https://github.com/rust-mcp-stack
- https://github.com/honeycombio/honeytail
- https://docs.honeycomb.io/send-data/logs/structured/honeytail
- https://www.honeycomb.io/blog/new-custom-regex-log-ingestion
- https://docs.datadoghq.com/logs/log_collection/
