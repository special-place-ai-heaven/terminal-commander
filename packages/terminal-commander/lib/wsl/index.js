// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS03 WSL helper barrel.

"use strict";

const detect = require("./detect.js");
const doctor = require("./doctor.js");
const distroName = require("./distro-name.js");
const bridge = require("./spawn.js");

module.exports = {
  ...detect,
  ...doctor,
  ...distroName,
  ...bridge,
};
