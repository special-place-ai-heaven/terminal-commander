// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 CLI barrel.

"use strict";

module.exports = {
  ...require("./parser.js"),
  ...require("./run.js"),
  ...require("./doctor.js"),
  ...require("./setup_cursor_wsl.js"),
  ...require("./pair_create.js"),
  ...require("./pair_accept.js"),
  ...require("./setup_state.js"),
};
