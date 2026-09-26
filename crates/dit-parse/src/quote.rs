//! YAML quote scanning shared by the frontmatter, fence and config parsers.

/// Where a character sits relative to YAML quoting, scanning left to right.
///
/// A quote opens a quoted scalar only where a scalar begins — at the start of
/// the value, after `[`, `{` or `,` inside a flow collection, after a mapping
/// key's `: `, or after a sequence item's `- `. Anywhere else
/// it is an ordinary character: `Work plan's lane` is a plain scalar with an
/// apostrophe in it, not the start of a string that never closes. Inside a
/// single-quoted scalar `''` is an escaped quote; inside a double-quoted one a
/// backslash escapes the next character.
#[derive(Default)]
pub(crate) struct QuoteScan {
    single: bool,
    double: bool,
    escaped: bool,
    /// The next non-space character begins a scalar.
    at_scalar_start: bool,
}

impl QuoteScan {
    pub(crate) fn new() -> Self {
        Self {
            at_scalar_start: true,
            ..Self::default()
        }
    }

    pub(crate) fn quoted(&self) -> bool {
        self.single || self.double
    }

    /// Feed one character (with the one after it, for `''`). Returns whether
    /// that character is structural — outside every quoted scalar, and not a
    /// quote that opens or closes one.
    pub(crate) fn step(&mut self, ch: char, next: Option<char>) -> bool {
        if self.double {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == '"' {
                self.double = false;
            }
            return false;
        }
        if self.single {
            if self.escaped {
                // second half of a `''` escape
                self.escaped = false;
            } else if ch == '\'' {
                if next == Some('\'') {
                    self.escaped = true;
                } else {
                    self.single = false;
                }
            }
            return false;
        }
        if self.at_scalar_start && (ch == '\'' || ch == '"') {
            self.single = ch == '\'';
            self.double = ch == '"';
            self.at_scalar_start = false;
            return false;
        }
        let before_space = next.is_none_or(char::is_whitespace);
        if matches!(ch, '[' | '{' | ',') || (ch == ':' && before_space) {
            // a flow item, or the value after a mapping key, begins next
            self.at_scalar_start = true;
        } else if ch == '-' && self.at_scalar_start && before_space {
            // `- item`: the item begins after the dash
        } else if !ch.is_whitespace() {
            self.at_scalar_start = false;
        }
        true
    }
}
