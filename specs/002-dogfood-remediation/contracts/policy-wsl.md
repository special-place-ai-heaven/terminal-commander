# Contract: WSL Nested-Shell Classification (US8)

**Layer**: daemon policy/spawn seam — `crates/daemon/src/command.rs` (argv
lane) and `crates/daemon/src/pty_command.rs` (PTY lane). Constitutional
basis: Principle II — no argv smuggling; `SHELL_INTERPRETERS_DENY` intact.
The stance: **the shell capability follows the shell, whichever side of the
WSL boundary it runs on.** WSL is this host's boundary, not a remote
machine; `allow_remote` is not implicated.

## Classifier contract

One function, both lanes: full argv in, classification out. Inspection is
**argv-only** — file contents are never read; the existing
`SHELL_INTERPRETERS_DENY` list is the ONLY interpreter authority (no second
list to drift).

### Step 1 — carrier detection

argv[0] is a WSL carrier iff its file-name component matches `wsl` or
`wsl.exe` case-insensitively (bare name, relative, or absolute path — all
identical). Not a carrier -> `NotWsl` -> the existing argv[0] interpreter
check applies unchanged.

### Step 2 — management commands (allow, as today)

If the first argument is a recognized management flag, the invocation
manages WSL itself and carries no command payload -> `Management`:

```text
--list  -l   --status   --version   --help   --shutdown   --terminate  -t
--set-default  -s   --set-version   --export   --import   --import-in-place
--mount   --unmount   --update   --install   --uninstall   --manage
--set-default-user   --unregister   --distribution-id   --system-distro-info
```

### Step 3 — payload location

Skip recognized selectors/introducers, in any valid order, to find the
first payload token:

```text
-d <distro>   --distribution <distro>
-u <user>     --user <user>
--cd <dir>
--system
-e            --exec
--
--shell-type <standard|login|none>
```

### Step 4 — payload classification

| Payload | Class | Rationale |
|---|---|---|
| first token's basename in `SHELL_INTERPRETERS_DENY` (e.g. `bash`, `sh`, `zsh`, `busybox`) | `NestedShell{interpreter}` | a shell line smuggled through argv |
| EMPTY (bare `wsl.exe`, or selectors with no command) | `NestedShell{interpreter: "default shell"}` | wsl with no command launches the distro's default interactive shell |
| any other program (`cargo`, `uname`, `ls`, ...) | `NonShellPayload` | runs exactly as today |
| unrecognized flag in payload position | `UnknownConstruction` | **FAIL CLOSED** — treated as potentially carrying a payload |

## Enforcement contract

| Class | `allow_shell=false` | `allow_shell=true` |
|---|---|---|
| `NotWsl` / `Management` / `NonShellPayload` | unchanged | unchanged |
| `NestedShell` | **DENY** | run + audit tag |
| `UnknownConstruction` | **DENY** (fail closed) | run + audit tag |

- **Deny error**: wire code `IpcErrorCode::ShellInterpreterDenied` (reused —
  agents already know this teaching shape), message extended to name (a)
  the nested interpreter, (b) the `wsl` carrier, and (c) the `allow_shell`
  gate + the `shell_exec` remedy. Example:

  ```text
  shell interpreter 'bash' denied inside a 'wsl.exe' invocation; the argv
  lane is not a shell bridge on either side of the WSL boundary. Remedy:
  invoke the Linux program directly (wsl.exe -e <program> ...); for
  pipelines/redirects use the shell_exec tool, gated by the allow_shell
  policy cap.
  ```

  Denials emit the existing `command_rejected` audit row (decision `deny`)
  before returning, as the argv-interpreter guard does today.
- **Allow audit tag**: no audit schema change. The command-start audit row's
  `metadata_json` gains `"nested_shell": "<interpreter>"` (or
  `"wsl_construction": "unknown"` for `UnknownConstruction`), and the
  `reason` text notes the classification.

## Both lanes, one truth

The classifier is applied at BOTH argv-lane sites: the command guard block
(today `shell_interpreter_basename(&req.argv[0])`, `command.rs:732-746`)
and the PTY lane (`pty_command.rs:267`). A payload denied on one lane must
be denied on the other — divergence between lanes is a defect.

## Compatibility guarantees (from the dogfood transcript — must keep working)

```text
wsl.exe -e cargo build            -> NonShellPayload -> runs
wsl.exe -d Ubuntu -e uname -a     -> NonShellPayload -> runs
wsl.exe --list --verbose          -> Management      -> runs
wsl.exe --status                  -> Management      -> runs
```

## Denials under allow_shell=false (all spellings classified identically)

```text
wsl.exe -e bash -lc "..."             -> NestedShell(bash)
wsl bash                              -> NestedShell(bash)
wsl.exe -- sh -c "..."                -> NestedShell(sh)
wsl.exe -d Ubuntu -e zsh              -> NestedShell(zsh)
C:\Windows\System32\wsl.exe -e bash   -> NestedShell(bash)
wsl.exe --exec busybox sh             -> NestedShell(busybox)
wsl.exe                               -> NestedShell(default shell)
wsl.exe --some-future-flag x          -> UnknownConstruction (fail closed)
```

## Documentation obligation (FR-061)

`docs/security/POLICY.md`, alongside the existing `SHELL_INTERPRETERS_DENY`
documentation, gains the stance: what is inspected (argv only), the
fail-closed rule for unknown constructions, the enforcement matrix above,
and the rationale (shell capability follows the shell across the WSL
boundary; WSL is the host's boundary, not a remote target).
