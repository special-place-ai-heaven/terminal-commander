// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 CLI argv parser. Dependency-free. Pure function over argv.
//
// Supported commands (locked at WWS06 prep amendment):
//
//   terminal-commander                                  -> --help
//   terminal-commander --help
//   terminal-commander doctor
//   terminal-commander doctor wsl [--distro <name>] [--probe-runtime] [--help]
//   terminal-commander setup [harness] [flags] [--help]
//   terminal-commander setup harness [flags] [--help]
//   terminal-commander setup cursor-wsl [flags] [--help]  (deprecated)
//   terminal-commander doctor harness
//   terminal-commander doctor daemon [--distro <name>]
//   terminal-commander setup daemon-autostart [--distro <name>] [--dry-run]
//   terminal-commander pair --help
//   terminal-commander pair create [--distro <name>] [--help]
//   terminal-commander pair accept <code> [--help]
//   terminal-commander restart [--distro <name>] [--force] [--help]
//
// Flags for `setup cursor-wsl`:
//   --distro <name>        (priority 1 over TC_WSL_DISTRO env)
//   --global               (default; mutually exclusive with --project)
//   --project <path>       (mutually exclusive with --global)
//   --force                (refuse-existing override; WWS05 contract)
//   --clobber-backup       (allow .bak overwrite; WWS05 contract)
//   --print-config         (print stanza JSON and exit 0; no writes)
//   --dry-run              (print plan and exit 0; no writes, no spawn)
//   --install-wsl-runtime  (single constant `npm install -g terminal-commander`
//                           invocation via lib/wsl/spawn.js; NO sudo, NO
//                           credentials)
//
// All unknown flags exit non-zero with a bounded usage panel.

"use strict";

const BOOLEAN_FLAGS = new Set([
  "--help",
  "--global",
  "--force",
  "--clobber-backup",
  "--print-config",
  "--dry-run",
  "--install-wsl-runtime",
  "--probe-runtime",
  "--uninstall",
]);

const STRING_FLAGS = new Set(["--distro", "--project", "--provider", "--surface"]);

/** Legal values for `--surface` (mirrors the Rust MCP server's TC_SURFACE gate). */
const SURFACE_VALUES = new Set(["compact", "full"]);

const HARNESS_SETUP_FLAGS = new Set([
  "distro",
  "global",
  "project",
  "force",
  "clobber-backup",
  "print-config",
  "dry-run",
  "install-wsl-runtime",
  "provider",
  "surface",
  "uninstall",
]);

function makeError(message, usage) {
  return { ok: false, error: message, usage };
}

/**
 * Parse `terminal-commander` argv into a structured command + flags
 * shape. Pure function — does not touch the filesystem, env, or any
 * other process state beyond what's passed in.
 *
 * @param {string[]} argv  argv slice (no `node`, no shim path).
 * @returns {{
 *   ok: true,
 *   command: string,
 *   subcommand?: string,
 *   flags: object,
 *   positional: string[],
 *   help: boolean
 * } | { ok: false, error: string, usage?: string }}
 */
function parseArgv(argv) {
  if (!Array.isArray(argv)) {
    return makeError("internal error: argv must be an array");
  }

  const tokens = argv.slice();

  // Collect leading positional args (commands / subcommands /
  // positional values), interleaved with flags.
  const flags = {};
  const positional = [];
  let help = false;

  while (tokens.length > 0) {
    const tok = tokens.shift();
    if (tok === "--help" || tok === "-h") {
      help = true;
      continue;
    }
    if (tok.startsWith("--")) {
      if (BOOLEAN_FLAGS.has(tok)) {
        flags[tok.slice(2)] = true;
        continue;
      }
      if (STRING_FLAGS.has(tok)) {
        const v = tokens.shift();
        if (v == null || v.startsWith("--")) {
          return makeError(`flag ${tok} requires a value`);
        }
        flags[tok.slice(2)] = v;
        continue;
      }
      return makeError(`unknown flag: ${tok}`);
    }
    positional.push(tok);
  }

  const cmd = positional.shift();
  if (cmd == null) {
    return { ok: true, command: "help", flags, positional, help: true };
  }

  switch (cmd) {
    case "doctor": {
      const sub = positional.shift();
      if (sub != null && sub !== "wsl" && sub !== "harness" && sub !== "daemon") {
        return makeError(`unknown doctor subcommand: ${sub}`);
      }
      for (const f of Object.keys(flags)) {
        if (!["distro", "probe-runtime"].includes(f)) {
          return makeError(`flag --${f} is not valid for 'doctor'`);
        }
      }
      return {
        ok: true,
        command: "doctor",
        subcommand: sub || null,
        flags,
        positional,
        help,
      };
    }
    case "setup": {
      let sub = positional.shift();
      if (sub == null) {
        return {
          ok: true,
          command: "setup",
          subcommand: "harness",
          flags,
          positional,
          help,
        };
      }
      if (sub === "harness") {
        sub = "harness";
      } else if (sub === "cursor-wsl") {
        sub = "cursor-wsl";
      } else if (sub === "daemon-autostart") {
        sub = "daemon-autostart";
      } else if (sub === "daemon-logon") {
        sub = "daemon-logon";
      } else {
        return makeError(`unknown setup subcommand: ${sub}`);
      }
      for (const f of Object.keys(flags)) {
        if (!HARNESS_SETUP_FLAGS.has(f)) {
          return makeError(`flag --${f} is not valid for 'setup ${sub}'`);
        }
      }
      if (flags.global === true && flags.project != null) {
        return makeError("--global and --project are mutually exclusive");
      }
      if (flags.surface != null && !SURFACE_VALUES.has(flags.surface)) {
        return makeError(
          `flag --surface must be 'compact' or 'full' (got '${flags.surface}')`,
        );
      }
      return {
        ok: true,
        command: "setup",
        subcommand: sub,
        flags,
        positional,
        help,
      };
    }
    case "pair": {
      const sub = positional.shift();
      if (sub == null) {
        return { ok: true, command: "pair", subcommand: null, flags, positional, help: true };
      }
      if (sub !== "create" && sub !== "accept") {
        return makeError(`unknown pair subcommand: ${sub}`);
      }
      if (sub === "create") {
        for (const f of Object.keys(flags)) {
          if (!["distro"].includes(f)) {
            return makeError(`flag --${f} is not valid for 'pair create'`);
          }
        }
        return {
          ok: true,
          command: "pair",
          subcommand: "create",
          flags,
          positional,
          help,
        };
      }
      // pair accept <code>
      const code = positional.shift();
      if (code == null && !help) {
        return makeError("'pair accept' requires a <code> argument");
      }
      for (const f of Object.keys(flags)) {
        return makeError(`flag --${f} is not valid for 'pair accept'`);
      }
      return {
        ok: true,
        command: "pair",
        subcommand: "accept",
        flags,
        positional: code != null ? [code] : [],
        help,
      };
    }
    case "restart": {
      for (const f of Object.keys(flags)) {
        if (!["distro", "force"].includes(f)) {
          return makeError(`flag --${f} is not valid for 'restart'`);
        }
      }
      return {
        ok: true,
        command: "restart",
        subcommand: null,
        flags,
        positional,
        help,
      };
    }
    case "help": {
      return { ok: true, command: "help", flags, positional, help: true };
    }
    default:
      return makeError(`unknown command: ${cmd}`);
  }
}

const USAGE = `terminal-commander — Windows control plane for the WSL runtime

USAGE
  terminal-commander <command> [subcommand] [flags]

COMMANDS
  doctor                              Print Windows host diagnostics.
  doctor wsl                          Run the read-only WSL discovery probe.
  doctor harness                      List detected vs configured MCP harnesses.
  doctor daemon                       Daemon socket + autostart status (WSL probe on Windows).
  setup                               Bootstrap all detected harnesses (default).
  setup daemon-autostart              Install systemd user unit or profile autostart in WSL/Linux.
  setup harness                       Same as setup (explicit).
  setup cursor-wsl                    Deprecated; use setup harness.
  pair create                         Generate a 6-digit pairing code (optional).
  pair accept <code>                  Validate a pairing code (deferred handshake).
  restart                             Replace the running daemon with the
                                      installed binary (terminal-commanderd
                                      update --force). Use after an upgrade.

GLOBAL FLAGS
  --help, -h                          Show command help.

EXAMPLES
  terminal-commander doctor wsl --probe-runtime
  terminal-commander setup cursor-wsl --print-config
  terminal-commander setup cursor-wsl --distro Ubuntu-24.04 --force
  terminal-commander pair create
  terminal-commander restart

NOTE
  To UPGRADE the npm package itself, use your package manager
  (e.g. 'npm update -g terminal-commander'), then run
  'terminal-commander restart' to swap the running daemon.
`;

const RESTART_USAGE = `terminal-commander restart [--distro <name>] [--force]

Replace the running terminal-commander daemon with the installed binary by
invoking 'terminal-commanderd update --force'. Windows uses the native daemon
by default. WSL is selected only when --distro, TC_WSL_DISTRO, or
TC_USE_LEGACY_WSL_BRIDGE=1 explicitly requests it.

This does NOT upgrade the npm package. Upgrade with your package manager
('npm update -g terminal-commander'), THEN run 'terminal-commander restart'.

FLAGS
  --distro <name>                     Explicitly restart the daemon in this WSL
                                      distro instead of native Windows.
  --force                             Always passed to the daemon; documented
                                      here for symmetry. A same-version daemon
                                      is replaced anyway (that is the point).
  --help, -h                          Show this panel.
`;

const DOCTOR_USAGE = `terminal-commander doctor [wsl|harness] [--distro <name>] [--probe-runtime]

  doctor                              Print Windows host diagnostics.
  doctor wsl                          Run the read-only WSL probe (detect +
                                      optional runtime presence check).
  doctor harness                      Show harness detection / config status.

FLAGS
  --distro <name>                     Operator-supplied WSL distro to probe.
  --probe-runtime                     Probe whether terminal-commander-mcp is
                                      installed inside the chosen distro.
                                      Default OFF (read-only enumeration only).
  --help, -h                          Show this panel.
`;

const SETUP_USAGE = `terminal-commander setup [harness] [flags]

Bootstrap: ensure WSL runtime (Windows) and merge MCP config for every
detected harness (Cursor, Codex CLI, Claude, ...).

Subcommands:
  setup                               Full harness bootstrap (default).
  setup harness                       Same as setup.
  setup daemon-autostart              Install daemon autostart (Linux / WSL).
  setup daemon-logon                  OPT-IN, no-admin per-user logon Scheduled
                                      Task that pre-starts the daemon at logon
                                      (Windows). Off unless you run this.
                                      Remove with --uninstall.
  setup cursor-wsl                    Deprecated alias (Cursor-focused).

Legacy cursor-wsl flags still apply to harness / cursor-wsl:

DISTRO SELECTION PRIORITY
  1. --distro <name>            (operator override)
  2. TC_WSL_DISTRO env          (operator override)
  3. detectWsl().default_distro (asterisk row in 'wsl -l -v')
  4. bounded refusal            (no_default_distro_ambiguous)

FLAGS
  --distro <name>                     Pin the WSL distro for the generated
                                      stanza. Validated via safety whitelist
                                      + live 'wsl -l -v' membership.
  --global                            Write ~/.cursor/mcp.json or
                                      %USERPROFILE%\\.cursor\\mcp.json (default).
  --project <path>                    Write <path>/.cursor/mcp.json. Mutually
                                      exclusive with --global.
  --force                              Overwrite an existing terminal-commander
                                      entry. Always creates <mcp.json>.bak
                                      before overwrite.
  --clobber-backup                    Allow overwriting an existing .bak.
  --print-config                      Print the planned stanza JSON and exit 0.
                                      Writes nothing.
  --dry-run                           Print the planned actions and exit 0.
                                      Writes nothing. Performs no spawn.
  --install-wsl-runtime               Alias for default WSL ensure (always on
                                      for bootstrap unless TC_SKIP_BOOTSTRAP=1).
  --provider <id>                     Configure only one harness (cursor,
                                      codex-cli, claude-code, claude-desktop,
                                      gemini, kimi).
  --surface <compact|full>            MCP tool surface the configured server
                                      advertises. Writes env.TC_SURFACE into
                                      each harness stanza (cursor, claude-code,
                                      claude-desktop). Omit to leave it unset
                                      (server default). 'compact' = 5 verb
                                      facades; 'full' = all tools.
  --uninstall                         (daemon-logon only) Remove the per-user
                                      logon Scheduled Task.
  --help, -h                          Show this panel.

SAFETY
  - Refuses existing terminal-commander entry without --force.
  - Always creates <mcp.json>.bak before overwrite.
  - Atomic write via same-directory tmp file + rename (WWS05 writer).
  - Never writes secrets / tokens / credentials.
  - Never invokes sudo. Never asks for passwords.
`;

const PAIR_USAGE = `terminal-commander pair <subcommand>

  pair create [--distro <name>]       Generate a 6-digit code and persist
                                      bounded pair.json under
                                      %LOCALAPPDATA%\\terminal-commander\\.
                                      Pair codes are operator confirmation,
                                      NOT a security secret.
  pair accept <code>                  Validate a 6-digit code. Returns
                                      pair_accepted if it matches the
                                      persisted record, else pair_deferred.
                                      The full WSL-side handshake is deferred
                                      to a future enhancement.

FLAGS
  --distro <name>                     Annotate pair.json with the distro
                                      (pair create only).
  --help, -h                          Show this panel.
`;

module.exports = {
  parseArgv,
  USAGE,
  DOCTOR_USAGE,
  SETUP_USAGE,
  PAIR_USAGE,
  RESTART_USAGE,
  BOOLEAN_FLAGS,
  STRING_FLAGS,
};
