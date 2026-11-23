// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Tool-surface selection.
//!
//! `TC_SURFACE=compact` advertises the verb-dispatched facade tools;
//! `TC_SURFACE=code` advertises the 3-tool Code Mode surface (`code_search`,
//! `code_execute`, `status`) and is the default; only an explicit `full`
//! gives the granular 60-tool surface. Read once per `tools/list` / `tools/call` so an operator can flip
//! it without rebuild.

/// Which MCP tool surface to advertise + admit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The granular legacy tools (explicit `TC_SURFACE=full` only).
    Full,
    /// The verb-dispatched facade tools.
    Compact,
    /// The 3-tool Python 3.14-subset Code Mode surface (`code_search`, `code_execute`, `status`; default).
    Code,
}

impl Surface {
    /// Parse a raw `TC_SURFACE` value. `None` or any unrecognized value => `Code`.
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("compact") => Self::Compact,
            Some(v) if v.eq_ignore_ascii_case("full") => Self::Full,
            _ => Self::Code,
        }
    }
}

/// Resolve the active surface from the live `TC_SURFACE` env var.
#[must_use]
pub fn surface_from_env() -> Surface {
    Surface::parse(std::env::var("TC_SURFACE").ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_surface_defaults_and_values() {
        assert_eq!(Surface::parse(None), Surface::Code); // unset -> Code
        assert_eq!(Surface::parse(Some("full")), Surface::Full);
        assert_eq!(Surface::parse(Some("compact")), Surface::Compact);
        assert_eq!(Surface::parse(Some("COMPACT")), Surface::Compact); // case-insensitive
        assert_eq!(Surface::parse(Some("code")), Surface::Code);
        assert_eq!(Surface::parse(Some("CODE")), Surface::Code);
        assert_eq!(Surface::parse(Some(" code ")), Surface::Code);
        assert_eq!(Surface::parse(Some("garbage")), Surface::Code); // unknown -> Code
        assert_eq!(Surface::parse(Some(" FULL ")), Surface::Full);
    }
}
