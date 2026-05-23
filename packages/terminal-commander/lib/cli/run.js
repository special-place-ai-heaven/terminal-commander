// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 CLI entry point. The bin shim's Windows branch delegates
// here; this module routes the parsed argv to the appropriate
// subcommand handler and returns a typed result the shim turns into
// stderr text + exit code.
//
// NO direct child_process invocation here. NO sudo. NO credential.

"use strict";

const { parseArgv, USAGE, DOCTOR_USAGE, SETUP_USAGE, PAIR_USAGE } = require("./parser.js");
const { runDoctor } = require("./doctor.js");
const {
  runSetupHarness,
  runSetupDefault,
  runSetupCursorWslDeprecated,
} = require("./setup_harness.js");
const { runDoctorHarness } = require("./doctor_harness.js");
const { runPairCreate } = require("./pair_create.js");
const { runPairAccept } = require("./pair_accept.js");

function helpResult(text) {
  return {
    status: "help",
    exit_code: 0,
    output: text,
  };
}

function errorResult(message) {
  return {
    status: "usage_error",
    exit_code: 64,
    output: `terminal-commander: ${message}\n\n${USAGE}`,
  };
}

async function run(opts) {
  const o = opts || {};
  const argv = Array.isArray(o.argv) ? o.argv : process.argv.slice(2);
  const platform = o.platform || process.platform;
  const env = o.env || process.env;

  const parsed = parseArgv(argv);
  if (!parsed.ok) {
    return errorResult(parsed.error);
  }
  if (parsed.help) {
    switch (parsed.command) {
      case "doctor":
        return helpResult(DOCTOR_USAGE);
      case "setup":
        return helpResult(SETUP_USAGE);
      case "pair":
        return helpResult(PAIR_USAGE);
      default:
        return helpResult(USAGE);
    }
  }
  switch (parsed.command) {
    case "doctor":
      if (parsed.subcommand === "harness") {
        return runDoctorHarness({ platform, env });
      }
      return runDoctor({
        subcommand: parsed.subcommand,
        flags: parsed.flags,
        platform,
        detect: o.detect,
        doctor: o.doctor,
      });
    case "setup":
      if (parsed.subcommand === "cursor-wsl") {
        return runSetupCursorWslDeprecated({
          flags: parsed.flags,
          platform,
          env,
          detect: o.detect,
          doctor: o.doctor,
          installExec: o.installExec,
          exec: o.installExec,
          writeConfig: o.writeConfig,
          writeState: o.writeState,
        });
      }
      if (parsed.subcommand === "harness" || parsed.subcommand == null) {
        return runSetupHarness({
          flags: parsed.flags,
          platform,
          env,
          detect: o.detect,
          doctor: o.doctor,
          installExec: o.installExec,
          ensureWslRuntime: o.ensureWslRuntime,
          writeState: o.writeState,
        });
      }
      return helpResult(SETUP_USAGE);
    case "pair":
      if (parsed.subcommand == null) {
        return helpResult(PAIR_USAGE);
      }
      if (parsed.subcommand === "create") {
        return runPairCreate({
          flags: parsed.flags,
          platform,
          env,
          writePairJson: o.writePairJson,
          stateDir: o.stateDir,
          randomInt: o.randomInt,
          randomBytes: o.randomBytes,
          now: o.now,
        });
      }
      // pair accept
      return runPairAccept({
        code: parsed.positional[0],
        platform,
        env,
        readPairJson: o.readPairJson,
        writePairJson: o.writePairJson,
        stateDir: o.stateDir,
        now: o.now,
      });
    case "help":
      return helpResult(USAGE);
    default:
      return helpResult(USAGE);
  }
}

module.exports = {
  run,
};
