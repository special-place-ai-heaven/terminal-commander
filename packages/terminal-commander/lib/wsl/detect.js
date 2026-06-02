// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS03 WSL distro discovery helper.
//
// `detectWsl(opts)` answers:
//
//   - Is the host Windows?
//   - Is `wsl.exe` callable?
//   - Which distros are installed (parsed from `wsl.exe -l -v`)?
//   - Which distro is the WSL default (asterisk marker)?
//   - What is each distro's state (Running / Stopped / etc.) and
//     WSL version (1 / 2)?
//
// `detectWsl` runs only `wsl.exe -l -v`. It does NOT install
// anything, does NOT modify any distro, does NOT write any file, and
// does NOT contact the network. It is the read-only discovery layer
// consumed by:
//
//   - WWS04 `lib/wsl/spawn.js` (validates distro choice against the
//     live whitelist before spawning the MCP bridge).
//   - WWS06 `terminal-commander setup cursor-wsl` (persists the
//     chosen distro into `%LOCALAPPDATA%\terminal-commander\setup.json`).
//
// All `wsl.exe` invocations use argv-array `spawn(wslPath, argv, ...)`
// with `shell: false` and no window-hiding options. NO `bash -lc`.
// NO shell interpolation. NO operator input concatenated into argv.
// The only operator-supplied string this module ever receives is
// `opts.wslPath`, and only as a literal path to `wsl.exe`.
//
// Encoding: legacy `wsl.exe` writes UTF-16 LE with a BOM and NUL
// padding between every ASCII character. Modern builds may write
// UTF-8. The parser handles both by stripping NUL bytes and
// normalising CRLF before tokenising.

"use strict";

const { spawn } = require("node:child_process");

const DETECT_REASONS = Object.freeze({
  OK: "ok",
  UNSUPPORTED_HOST: "unsupported_host",
  WSL_NOT_FOUND: "wsl_not_found",
  NO_DISTROS: "no_distros",
  WSL_COMMAND_FAILED: "wsl_command_failed",
  CHECK_TIMEOUT: "check_timeout",
});

const DEFAULT_TIMEOUT_MS = 5000;

/**
 * Default executor. Runs `wsl.exe argv` with no shell and inherited PATH
 * only (no env injection beyond Node defaults).
 * Returns `{ status, signal, stdout: Buffer, stderr: Buffer, error }`.
 */
function defaultExec({ wslPath, argv, timeoutMs }) {
  return new Promise((resolve) => {
    let settled = false;
    const stdoutChunks = [];
    const stderrChunks = [];
    let child;
    try {
      child = spawn(wslPath, argv, {
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
      });
    } catch (err) {
      resolve({
        status: null,
        signal: null,
        stdout: Buffer.alloc(0),
        stderr: Buffer.alloc(0),
        error: err,
      });
      return;
    }

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      try {
        child.kill("SIGKILL");
      } catch (_e) {
        /* ignore */
      }
      const err = new Error("wsl.exe timeout");
      err.code = "CHECK_TIMEOUT";
      resolve({
        status: null,
        signal: null,
        stdout: Buffer.concat(stdoutChunks),
        stderr: Buffer.concat(stderrChunks),
        error: err,
      });
    }, timeoutMs);

    child.stdout.on("data", (chunk) => stdoutChunks.push(chunk));
    child.stderr.on("data", (chunk) => stderrChunks.push(chunk));

    child.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: null,
        signal: null,
        stdout: Buffer.concat(stdoutChunks),
        stderr: Buffer.concat(stderrChunks),
        error: err,
      });
    });

    child.on("close", (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        status: code,
        signal,
        stdout: Buffer.concat(stdoutChunks),
        stderr: Buffer.concat(stderrChunks),
        error: null,
      });
    });
  });
}

/**
 * Normalise wsl.exe output bytes into a plain UTF-8 string.
 *
 * Handles:
 *   - UTF-16 LE BOM (`0xFF 0xFE`) at the head of the buffer.
 *   - NUL-padded ASCII (legacy wsl.exe output even without BOM).
 *   - CRLF line endings.
 *
 * @param {Buffer|string} bytes
 * @returns {string}
 */
function normalizeWslOutput(bytes) {
  if (typeof bytes === "string") {
    return bytes.replace(/\u0000/g, "").replace(/\r\n/g, "\n");
  }
  if (!bytes || bytes.length === 0) return "";
  let text;
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) {
    text = bytes.slice(2).toString("utf16le");
  } else {
    text = bytes.toString("utf8");
  }
  // Strip stray NUL bytes (legacy wsl.exe output even without BOM).
  text = text.replace(/\u0000/g, "");
  // Normalise CRLF.
  text = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  return text;
}

/**
 * Parse `wsl.exe -l -v` output into a structured distro list.
 *
 * Output shape (after normalisation):
 *
 *   ```
 *     NAME            STATE           VERSION
 *   * Ubuntu-24.04    Running         2
 *     Debian          Stopped         2
 *   ```
 *
 * The default distro is marked with `*` in the leftmost column.
 *
 * @param {string} text  Normalised stdout from `wsl.exe -l -v`.
 * @returns {{ distros: Array<{name:string,state:string,wsl_version:number|null,is_default:boolean}>, default_distro: string|null, malformed: boolean }}
 */
function parseVerboseList(text) {
  const distros = [];
  let defaultDistro = null;
  let malformed = false;

  if (!text || !text.trim()) {
    return { distros, default_distro: null, malformed: false };
  }

  const rawLines = text.split("\n");
  for (const rawLine of rawLines) {
    if (!rawLine.trim()) continue;
    // Skip the header row. `wsl.exe -l -v` always starts the header
    // with the three column titles in this exact order (English
    // locale only — non-English locales are not supported by the
    // WWS01 contract, see §15 R-WWS-01).
    if (/^\s*NAME\s+STATE\s+VERSION\s*$/i.test(rawLine)) continue;

    // Asterisk marker is the first non-space character on the
    // default distro's line.
    const isDefault = /^\s*\*\s+/.test(rawLine);
    const stripped = rawLine.replace(/^\s*\*?\s*/, "");
    const cols = stripped.split(/\s+/).filter(Boolean);
    if (cols.length === 0) continue;
    if (cols.length < 2) {
      // Plausibly a `wsl.exe -l -q` line that slipped in; mark as
      // malformed so the caller can decide.
      malformed = true;
      continue;
    }

    const name = cols[0];
    const state = cols[1] || "";
    const versionRaw = cols[2];
    let wslVersion = null;
    if (versionRaw && /^\d+$/.test(versionRaw)) {
      wslVersion = parseInt(versionRaw, 10);
    }

    distros.push({
      name,
      state,
      wsl_version: wslVersion,
      is_default: isDefault,
    });
    if (isDefault) defaultDistro = name;
  }

  return { distros, default_distro: defaultDistro, malformed };
}

/**
 * Read-only WSL discovery probe.
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 *     Host platform. Anything other than `'win32'` short-circuits to
 *     `unsupported_host`.
 * @param {(args: {wslPath:string, argv:string[], timeoutMs:number}) => Promise<{status:number|null,signal:string|null,stdout:Buffer,stderr:Buffer,error:Error|null}>} [opts.exec]
 *     Injected executor. Defaults to a thin wrapper around
 *     `child_process.spawn('wsl.exe', argv, { shell: false })`.
 * @param {string} [opts.wslPath="wsl.exe"]
 *     Absolute or PATH-resolved name of the WSL CLI. Must be passed
 *     as a single argv element by the caller; the helper does not
 *     concatenate user input here.
 * @param {number} [opts.timeoutMs=5000]
 *     Maximum wall-clock time for the verbose-list probe.
 * @returns {Promise<{
 *   host_platform: string,
 *   wsl_callable: boolean,
 *   default_distro: string|null,
 *   distros: Array<{name:string,state:string,wsl_version:number|null,is_default:boolean}>,
 *   reason: string,
 *   raw_excerpt_for_debug?: string,
 * }>}
 */
async function detectWsl(opts) {
  const platform = (opts && opts.platform) || process.platform;
  const exec = (opts && opts.exec) || defaultExec;
  const wslPath = (opts && opts.wslPath) || "wsl.exe";
  const timeoutMs =
    opts && typeof opts.timeoutMs === "number" ? opts.timeoutMs : DEFAULT_TIMEOUT_MS;

  if (platform !== "win32") {
    return {
      host_platform: platform,
      wsl_callable: false,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.UNSUPPORTED_HOST,
    };
  }

  const result = await exec({
    wslPath,
    argv: ["-l", "-v"],
    timeoutMs,
  });

  if (result.error) {
    if (
      result.error.code === "ENOENT" ||
      /ENOENT|not found|cannot find/i.test(result.error.message || "")
    ) {
      return {
        host_platform: platform,
        wsl_callable: false,
        default_distro: null,
        distros: [],
        reason: DETECT_REASONS.WSL_NOT_FOUND,
      };
    }
    if (result.error.code === "CHECK_TIMEOUT") {
      return {
        host_platform: platform,
        wsl_callable: true,
        default_distro: null,
        distros: [],
        reason: DETECT_REASONS.CHECK_TIMEOUT,
      };
    }
    return {
      host_platform: platform,
      wsl_callable: true,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.WSL_COMMAND_FAILED,
    };
  }

  if (result.status !== 0) {
    const stderrText = normalizeWslOutput(result.stderr).trim();
    // `wsl.exe -l -v` prints "Windows Subsystem for Linux has no
    // installed distributions." with a non-zero exit code when no
    // distros exist. The exact wording varies across builds; match
    // generously.
    if (/no installed distributions/i.test(stderrText)) {
      return {
        host_platform: platform,
        wsl_callable: true,
        default_distro: null,
        distros: [],
        reason: DETECT_REASONS.NO_DISTROS,
      };
    }
    return {
      host_platform: platform,
      wsl_callable: true,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.WSL_COMMAND_FAILED,
      raw_excerpt_for_debug: stderrText.slice(0, 200),
    };
  }

  const text = normalizeWslOutput(result.stdout);
  const parsed = parseVerboseList(text);

  if (parsed.distros.length === 0) {
    return {
      host_platform: platform,
      wsl_callable: true,
      default_distro: null,
      distros: [],
      reason: DETECT_REASONS.NO_DISTROS,
    };
  }

  return {
    host_platform: platform,
    wsl_callable: true,
    default_distro: parsed.default_distro,
    distros: parsed.distros,
    reason: DETECT_REASONS.OK,
  };
}

module.exports = {
  detectWsl,
  parseVerboseList,
  normalizeWslOutput,
  DETECT_REASONS,
  DEFAULT_TIMEOUT_MS,
};
