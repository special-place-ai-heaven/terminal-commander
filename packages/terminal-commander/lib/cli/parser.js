// SPDX-License-Identifier: Apache-2.0
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
//   terminal-commander setup --help
//   terminal-commander setup cursor-wsl [flags] [--help]
//   terminal-commander pair --help
//   terminal-commander pair create [--distro <name>] [--help]
//   terminal-commander pair accept <code> [--help]
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
]);

const STRING_FLAGS = new Set(["--distro", "--project"]);

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
      if (sub != null && sub !== "wsl") {
        return makeError(`unknown doctor subcommand: ${sub}`);
      }
      // Allowed flags: --distro, --probe-runtime, --help.
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
      const sub = positional.shift();
      if (sub == null) {
        // `terminal-commander setup` alone -> setup help.
        return { ok: true, command: "setup", subcommand: null, flags, positional, help: true };
      }
      if (sub !== "cursor-wsl") {
        return makeError(`unknown setup subcommand: ${sub}`);
      }
      const allowed = [
        "distro",
        "global",
        "project",
        "force",
        "clobber-backup",
        "print-config",
        "dry-run",
        "install-wsl-runtime",
      ];
      for (const f of Object.keys(flags)) {
        if (!allowed.includes(f)) {
          return makeError(`flag --${f} is not valid for 'setup cursor-wsl'`);
        }
      }
      if (flags.global === true && flags.project != null) {
        return makeError("--global and --project are mutually exclusive");
      }
      return {
        ok: true,
        command: "setup",
        subcommand: "cursor-wsl",
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
  setup cursor-wsl                    Set up Cursor MCP config to launch the
                                      WWS04 bridge into a WSL distro.
  pair create                         Generate a 6-digit pairing code (optional).
  pair accept <code>                  Validate a pairing code (deferred handshake).

GLOBAL FLAGS
  --help, -h                          Show command help.

EXAMPLES
  terminal-commander doctor wsl --probe-runtime
  terminal-commander setup cursor-wsl --print-config
  terminal-commander setup cursor-wsl --distro Ubuntu-24.04 --force
  terminal-commander pair create
`;

const DOCTOR_USAGE = `terminal-commander doctor [wsl] [--distro <name>] [--probe-runtime]

  doctor                              Print Windows host diagnostics.
  doctor wsl                          Run the read-only WSL probe (detect +
                                      optional runtime presence check).

FLAGS
  --distro <name>                     Operator-supplied WSL distro to probe.
  --probe-runtime                     Probe whether terminal-commander-mcp is
                                      installed inside the chosen distro.
                                      Default OFF (read-only enumeration only).
  --help, -h                          Show this panel.
`;

const SETUP_USAGE = `terminal-commander setup cursor-wsl [flags]

Write or merge a Cursor MCP config that points at the WWS04 bridge shim.

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
  --install-wsl-runtime               Attempt one bounded inside-WSL install
                                      via 'npm install -g terminal-commander'.
                                      NO sudo. NO password. Returns
                                      install_permission_required or
                                      npm_package_unpublished honestly on
                                      failure. Until NPM07 publish lands,
                                      expect npm_package_unpublished.
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
  BOOLEAN_FLAGS,
  STRING_FLAGS,
};
