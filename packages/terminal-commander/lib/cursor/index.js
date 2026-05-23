// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS05 Cursor config writer barrel.

"use strict";

const config = require("./config.js");
const write = require("./write.js");

module.exports = {
  ...config,
  ...write,
};
