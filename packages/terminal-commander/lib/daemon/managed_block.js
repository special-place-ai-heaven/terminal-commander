// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const BEGIN_SUFFIX = " BEGIN";
const END_SUFFIX = " END";

function blockMarkers(label) {
  const tag = String(label || "managed");
  return {
    begin: `# terminal-commander ${tag}${BEGIN_SUFFIX}`,
    end: `# terminal-commander ${tag}${END_SUFFIX}`,
  };
}

/**
 * Replace or append a marked block in `content`.
 *
 * @param {string} content
 * @param {string} label
 * @param {string} blockBody  lines inside the markers (no markers)
 * @returns {string}
 */
function applyManagedBlock(content, label, blockBody) {
  const { begin, end } = blockMarkers(label);
  const inner = `${begin}\n${blockBody.trimEnd()}\n${end}\n`;
  const existing = extractManagedBlock(content, label);
  if (existing != null) {
    const re = new RegExp(
      `${escapeRe(begin)}[\\s\\S]*?${escapeRe(end)}\\n?`,
      "m",
    );
    return content.replace(re, inner);
  }
  const base = content.length === 0 || content.endsWith("\n") ? content : `${content}\n`;
  return `${base}\n${inner}`;
}

function extractManagedBlock(content, label) {
  const { begin, end } = blockMarkers(label);
  const re = new RegExp(`${escapeRe(begin)}\\n([\\s\\S]*?)\\n${escapeRe(end)}`, "m");
  const m = content.match(re);
  return m ? m[1] : null;
}

function hasManagedBlock(content, label) {
  return extractManagedBlock(content, label) != null;
}

function escapeRe(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

module.exports = {
  blockMarkers,
  applyManagedBlock,
  extractManagedBlock,
  hasManagedBlock,
};
