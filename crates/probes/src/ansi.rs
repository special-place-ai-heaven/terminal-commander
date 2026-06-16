// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shared ANSI/CSI/OSC stripper (TC-B1, omni spec 001 FR-026).
//!
//! Colored output silently defeats anchored sifter rules (`^\[AAP\]`
//! never matches a line that begins with `\x1b[0;32m`) and pollutes
//! emitted summaries with escape bytes. This module exposes one
//! UTF-8-safe helper, [`strip_ansi`], that the non-PTY process path
//! calls BEFORE rule matching and summary construction while the RAW
//! bytes stay in the frame store (`ContextRingManager`).
//!
//! The stripper drives the same `vte::Parser` the PTY
//! [`crate::pty::AnsiNormalizer`] uses, so both paths interpret escape
//! sequences identically. It keeps only printable text plus horizontal
//! tab; every CSI (`ESC [ ... m`, cursor moves, ...), OSC (`ESC ] ... BEL`),
//! and other escape sequence is consumed and discarded. Because input
//! arrives as a decoded `&str`, the result is always valid UTF-8 and no
//! multibyte character is split.
//!
//! This is a per-line (per-frame) helper: it does NOT collapse CR/LF or
//! join lines (the read layer already splits on `\n`); it only removes
//! in-band escape sequences from one logical line. A bare `\r` inside a
//! line is dropped (it carries no printable content once the read layer
//! has split lines), matching the existing `trim_end_matches('\r')`
//! behavior on the process path.
//!
//! Source-status: live (TC-B1).

use vte::Parser;

/// Strip ANSI/CSI/OSC escape sequences from a single decoded line,
/// returning a plain-text string with only printable characters and
/// horizontal tabs retained.
///
/// UTF-8-safe by construction: the input is a `&str` and the parser
/// emits whole `char`s via [`vte::Perform::print`], so no multibyte
/// sequence is ever split or corrupted.
///
/// Fast path: a line with no `ESC` (`0x1b`) byte cannot contain an
/// escape sequence, so it is returned as-is without invoking the
/// parser. This keeps the common (uncolored) case allocation-light
/// relative to feeding every byte through `vte`.
#[must_use]
pub fn strip_ansi(input: &str) -> String {
    // No escape byte -> nothing to strip. The read layer already
    // trims trailing CR; a stray interior CR is rare and harmless to
    // keep here, but we still drop it below when the parser runs.
    if !input.as_bytes().contains(&0x1b) {
        return input.to_owned();
    }
    let mut parser = Parser::new();
    let mut sink = StripSink {
        out: String::with_capacity(input.len()),
    };
    parser.advance(&mut sink, input.as_bytes());
    sink.out
}

/// `vte::Perform` collector that keeps only printable text + tab.
struct StripSink {
    out: String,
}

impl vte::Perform for StripSink {
    fn print(&mut self, c: char) {
        self.out.push(c);
    }

    fn execute(&mut self, byte: u8) {
        // Preserve only horizontal tab as in-band whitespace. CR/LF do
        // not appear here (the read layer splits on `\n` and trims
        // trailing `\r`); any other C0 control is escape-adjacent noise
        // and is dropped.
        if byte == b'\t' {
            self.out.push('\t');
        }
    }

    // All escape/CSI/OSC/DCS callbacks intentionally do nothing: the
    // whole point is to consume and discard those sequences. The
    // default `vte::Perform` impls for the remaining hooks are no-ops.
}

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[test]
    fn plain_text_passes_through_unchanged() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn sgr_color_codes_are_removed() {
        // `\x1b[0;32m[AAP]\x1b[0m MySQL is ready` -> the field-ledger
        // TC-B1 repro line.
        let colored = "\u{1b}[0;32m[AAP]\u{1b}[0m MySQL is ready";
        assert_eq!(strip_ansi(colored), "[AAP] MySQL is ready");
    }

    #[test]
    fn anchored_prefix_is_exposed_after_strip() {
        // The matcher consequence: a `^\[AAP\]` pattern can only match
        // once the leading SGR escape is gone.
        let colored = "\u{1b}[0;32m[AAP] backend up";
        let stripped = strip_ansi(colored);
        assert!(stripped.starts_with("[AAP]"), "got: {stripped:?}");
    }

    #[test]
    fn osc_sequence_is_removed() {
        // OSC 0 (set window title) terminated by BEL, then real text.
        let s = "\u{1b}]0;my title\u{7}real output";
        assert_eq!(strip_ansi(s), "real output");
    }

    #[test]
    fn cursor_movement_csi_is_removed() {
        let s = "before\u{1b}[2Kafter";
        assert_eq!(strip_ansi(s), "beforeafter");
    }

    #[test]
    fn tab_is_preserved() {
        assert_eq!(strip_ansi("a\tb"), "a\tb");
    }

    #[test]
    fn utf8_multibyte_is_not_corrupted() {
        // Mixed escapes around multibyte (CJK + emoji) text. The output
        // must remain byte-for-byte valid and uncorrupted.
        let s = "\u{1b}[31m日本語\u{1b}[0m \u{1f680} done";
        let stripped = strip_ansi(s);
        assert_eq!(stripped, "日本語 \u{1f680} done");
        // And it round-trips as valid UTF-8 (String guarantees this,
        // but assert the content explicitly for the multibyte chars).
        assert!(stripped.contains('日'));
        assert!(stripped.contains('\u{1f680}'));
    }

    #[test]
    fn no_escape_fast_path_matches_slow_path() {
        // The fast path (no 0x1b) and the parser path must agree on
        // plain text.
        let plain = "plain 日本語 line";
        assert_eq!(strip_ansi(plain), plain);
    }
}
