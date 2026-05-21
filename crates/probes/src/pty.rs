// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Terminal / PTY normalization + prompt detection (TC19).
//!
//! Two portable pieces ship here:
//!
//! 1. [`AnsiNormalizer`]: feeds bytes through a `vte::Parser` and
//!    emits only printable text (ANSI escapes stripped, CR-overwrite
//!    collapsed into a single logical line). Used by the process
//!    probe upstream of the sifter.
//!
//! 2. [`PromptDetector`]: matches a small set of canonical prompts
//!    (sudo password, ssh password, basic shell `$`/`#`) against a
//!    normalized line and returns a `PromptKind`.
//!
//! The actual pty-process spawn path is deferred to a POSIX harness;
//! these portable normalizers are the parts the sifter runtime needs
//! today.
//!
//! Source-status: live (TC19) for normalization + prompt detection.
//! Full PTY spawn deferred (see goal-file decision lock).

use vte::Parser;

/// Reset the buffer when the parser emits a Carriage Return.
const CR_COLLAPSE: bool = true;

/// Stateful ANSI/CR-aware normalizer. Feed bytes via `feed`,
/// pull complete lines via `take_lines`.
#[derive(Default)]
pub struct AnsiNormalizer {
    parser: Parser,
    line: String,
    pending: Vec<String>,
}

#[allow(clippy::missing_fields_in_debug)]
impl core::fmt::Debug for AnsiNormalizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // vte::Parser does not implement Debug; we surface a summary.
        f.debug_struct("AnsiNormalizer")
            .field("line", &self.line)
            .field("pending_count", &self.pending.len())
            .finish()
    }
}

impl AnsiNormalizer {
    /// Construct an empty normalizer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes. Internal state may complete one or more lines.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut sink = Sink {
            line: &mut self.line,
            pending: &mut self.pending,
        };
        self.parser.advance(&mut sink, bytes);
    }

    /// Drain completed lines (without newline terminators).
    pub fn take_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }

    /// Flush whatever partial line is pending as a final line.
    pub fn flush(&mut self) -> Option<String> {
        if self.line.is_empty() {
            None
        } else {
            let l = std::mem::take(&mut self.line);
            Some(l)
        }
    }
}

struct Sink<'a> {
    line: &'a mut String,
    pending: &'a mut Vec<String>,
}

impl vte::Perform for Sink<'_> {
    fn print(&mut self, c: char) {
        self.line.push(c);
    }
    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.pending.push(std::mem::take(self.line));
            }
            b'\r' if CR_COLLAPSE => {
                // Carriage return: overwrite from start of line.
                self.line.clear();
            }
            b'\t' => self.line.push('\t'),
            _ => {}
        }
    }
    // ESC sequences, CSI, OSC etc. are intentionally ignored
    // (they're how ANSI color/cursor codes are dispatched).
}

/// Canonical prompt kinds we detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptKind {
    SudoPassword,
    SshPassword,
    GenericPassword,
    Shell,
    YesNo,
    None,
}

/// Detector for canonical prompts.
pub struct PromptDetector;

impl PromptDetector {
    /// Classify a normalized line as a prompt.
    #[must_use]
    pub fn classify(line: &str) -> PromptKind {
        // sudo and ssh password prompts (case insensitive).
        let lower = line.to_lowercase();
        let lower_trim = lower.trim_end();
        if lower_trim.contains("[sudo] password") {
            return PromptKind::SudoPassword;
        }
        if lower_trim.contains("password:") && lower_trim.contains('@') {
            return PromptKind::SshPassword;
        }
        if lower_trim.ends_with("password:") {
            return PromptKind::GenericPassword;
        }
        if lower_trim.ends_with("(y/n)")
            || lower_trim.ends_with("(yes/no)")
            || lower_trim.ends_with("[y/n]")
        {
            return PromptKind::YesNo;
        }
        // Bare shell prompts (heuristic): line ends with `$` or `#`
        // optionally followed by trailing whitespace.
        if lower_trim.ends_with('$') || lower_trim.ends_with('#') {
            return PromptKind::Shell;
        }
        PromptKind::None
    }

    /// Whether the detected prompt is a password / secret prompt.
    #[must_use]
    pub const fn is_secret(kind: PromptKind) -> bool {
        matches!(
            kind,
            PromptKind::SudoPassword | PromptKind::SshPassword | PromptKind::GenericPassword,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_strips_color_escapes() {
        let mut n = AnsiNormalizer::new();
        n.feed(b"\x1b[31merror:\x1b[0m something broke\n");
        let lines = n.take_lines();
        assert_eq!(lines, vec!["error: something broke".to_owned()]);
    }

    #[test]
    fn ansi_cr_collapses_progress_lines() {
        let mut n = AnsiNormalizer::new();
        // Progress: "10%\r25%\r100%\n" should yield one line "100%".
        n.feed(b"10%\r25%\r100%\n");
        let lines = n.take_lines();
        assert_eq!(lines, vec!["100%".to_owned()]);
    }

    #[test]
    fn ansi_multiline_breakdown() {
        let mut n = AnsiNormalizer::new();
        n.feed(b"first line\nsecond line\nthird");
        let lines = n.take_lines();
        assert_eq!(lines, vec!["first line", "second line"]);
        assert_eq!(n.flush().unwrap(), "third");
    }

    #[test]
    fn prompt_sudo_detected() {
        assert_eq!(
            PromptDetector::classify("[sudo] password for dev: "),
            PromptKind::SudoPassword
        );
        assert!(PromptDetector::is_secret(PromptKind::SudoPassword));
    }

    #[test]
    fn prompt_ssh_password_detected() {
        assert_eq!(
            PromptDetector::classify("dev@host-a's password:"),
            PromptKind::SshPassword
        );
    }

    #[test]
    fn prompt_shell_detected() {
        assert_eq!(PromptDetector::classify("dev@host:~$ "), PromptKind::Shell);
        assert_eq!(PromptDetector::classify("root@host:~# "), PromptKind::Shell);
    }

    #[test]
    fn prompt_yes_no_detected() {
        assert_eq!(
            PromptDetector::classify("Continue? [y/n]"),
            PromptKind::YesNo
        );
        assert_eq!(
            PromptDetector::classify("Are you sure? (yes/no)"),
            PromptKind::YesNo
        );
    }

    #[test]
    fn prompt_non_match_returns_none() {
        assert_eq!(
            PromptDetector::classify("just a regular log line"),
            PromptKind::None
        );
        assert!(!PromptDetector::is_secret(PromptKind::None));
    }
}
