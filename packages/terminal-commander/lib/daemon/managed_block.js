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

/**
 * Line-exact view of the managed block in `content`. "malformed" means any
 * marker layout other than exactly one BEGIN line followed by one END line;
 * callers must leave such a file alone rather than guess.
 *
 * @returns {{state:"absent"|"malformed"}|{state:"single", body:string}}
 */
function managedBlockState(content, label) {
  const { begin, end } = blockMarkers(label);
  const lines = content.split("\n");
  const bare = (l) => l.replace(/\r$/, "");
  const begins = [];
  const ends = [];
  lines.forEach((l, i) => {
    if (bare(l) === begin) begins.push(i);
    else if (bare(l) === end) ends.push(i);
  });
  if (begins.length === 0 && ends.length === 0) return { state: "absent" };
  if (begins.length !== 1 || ends.length !== 1 || begins[0] > ends[0]) {
    return { state: "malformed" };
  }
  return {
    state: "single",
    body: lines.slice(begins[0] + 1, ends[0]).map(bare).join("\n"),
    lines,
    beginIdx: begins[0],
    endIdx: ends[0],
  };
}

/**
 * Replace the body of a well-formed managed block in place. Every byte outside
 * the block (including the marker lines and the trailing newline state) is
 * kept; body lines reuse the BEGIN line's CRLF/LF ending.
 */
function replaceManagedBlockBody(content, label, blockBody) {
  const s = managedBlockState(content, label);
  if (s.state !== "single") throw new Error(`managed block is ${s.state}`);
  const cr = s.lines[s.beginIdx].endsWith("\r") ? "\r" : "";
  return [
    ...s.lines.slice(0, s.beginIdx + 1),
    ...blockBody.trimEnd().split("\n").map((l) => `${l}${cr}`),
    ...s.lines.slice(s.endIdx),
  ].join("\n");
}

function escapeRe(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

module.exports = {
  blockMarkers,
  applyManagedBlock,
  extractManagedBlock,
  hasManagedBlock,
  managedBlockState,
  replaceManagedBlockBody,
};
