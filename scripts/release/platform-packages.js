// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Single source of truth for the list of platform packages whose
// versions are pinned in root's optionalDependencies.
"use strict";

const PLATFORM_PACKAGES = Object.freeze([
  "@terminal-commander/linux-x64",
  "@terminal-commander/linux-arm64",
  "@terminal-commander/windows-x64",
  "@terminal-commander/mac-x64",
  "@terminal-commander/mac-arm64",
]);

module.exports = { PLATFORM_PACKAGES };
