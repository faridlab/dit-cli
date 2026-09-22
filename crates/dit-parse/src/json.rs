//! JSON into the same [`Yaml`] tree the schema parser produces.
//!
//! OpenAPI documents are published as `.yaml` and `.json` in roughly equal
//! measure — most server frameworks emit JSON — and everything downstream of
//! reading one only wants a typed tree. Rather than teach the YAML subset to
//! swallow multi-line flow collections, which would loosen a parser that is
//! deliberately strict about the files DIT itself writes, JSON gets its own
//! reader. Its grammar is small and closed, so this stays short.
//!
//! Numbers and booleans become `Yaml::Str`, exactly as the YAML side leaves
//! them: `as_u32` and `as_bool` do the interpreting, and a single
//! representation means callers never branch on which file format they came
//! from. Pure, no I/O, wasm-clean.

use crate::yaml::Yaml;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum JsonError {
    #[error("offset {at}: expected {expected}, found `{found}`")]
    Expected {
        at: usize,
        expected: &'static str,
        found: String,
    },
    #[error("offset {at}: the document ends in the middle of a value")]
    Truncated { at: usize },
    #[error("offset {at}: trailing text after the document")]
    Trailing { at: usize },
    #[error("nested more than {limit} deep — refused before it becomes a stack overflow")]
    TooDeep { limit: usize },
}

/// How far a document may nest. Hostile input arrives by pull request, and a
/// few thousand opening brackets is a stack overflow, not a parse error.
const MAX_DEPTH: usize = 64;

pub fn parse(text: &str) -> Result<Yaml, JsonError> {
    let bytes: Vec<char> = text.chars().collect();
    let mut p = Parser {
        src: &bytes,
        at: 0,
        depth: 0,
    };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.at < p.src.len() {
        return Err(JsonError::Trailing { at: p.at });
    }
    Ok(value)
}

struct Parser<'a> {
    src: &'a [char],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.src.get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, want: char, expected: &'static str) -> Result<(), JsonError> {
        match self.peek() {
            Some(c) if c == want => {
                self.at += 1;
                Ok(())
            }
            Some(c) => Err(JsonError::Expected {
                at: self.at,
                expected,
                found: c.to_string(),
            }),
            None => Err(JsonError::Truncated { at: self.at }),
        }
    }

    fn value(&mut self) -> Result<Yaml, JsonError> {
        // Depth is charged per collection, not per call, so the limit means
        // what it says and a flat document of any size is unaffected.
        match self.peek().ok_or(JsonError::Truncated { at: self.at })? {
            '{' => self.object(),
            '[' => self.array(),
            '"' => Ok(Yaml::Str(self.string()?)),
            _ => self.bare(),
        }
    }

    fn enter(&mut self) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(JsonError::TooDeep { limit: MAX_DEPTH });
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Yaml, JsonError> {
        self.enter()?;
        self.at += 1; // `{`
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Yaml::Map(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(':', "`:` after an object key")?;
            self.skip_ws();
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(',') => self.at += 1,
                Some('}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Yaml::Map(entries));
                }
                Some(c) => {
                    return Err(JsonError::Expected {
                        at: self.at,
                        expected: "`,` or `}`",
                        found: c.to_string(),
                    })
                }
                None => return Err(JsonError::Truncated { at: self.at }),
            }
        }
    }

    fn array(&mut self) -> Result<Yaml, JsonError> {
        self.enter()?;
        self.at += 1; // `[`
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Yaml::Seq(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(',') => self.at += 1,
                Some(']') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Yaml::Seq(items));
                }
                Some(c) => {
                    return Err(JsonError::Expected {
                        at: self.at,
                        expected: "`,` or `]`",
                        found: c.to_string(),
                    })
                }
                None => return Err(JsonError::Truncated { at: self.at }),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect('"', "a quoted string")?;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or(JsonError::Truncated { at: self.at })?;
            self.at += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let esc = self.peek().ok_or(JsonError::Truncated { at: self.at })?;
                    self.at += 1;
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => out.push(self.unicode_escape()?),
                        other => {
                            return Err(JsonError::Expected {
                                at: self.at,
                                expected: "a known escape",
                                found: other.to_string(),
                            })
                        }
                    }
                }
                other => out.push(other),
            }
        }
    }

    /// `\uXXXX`, including a surrogate pair. A lone surrogate is not a
    /// character; it becomes the replacement rather than failing the whole
    /// document, because one bad description should not hide an API.
    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let first = self.hex4()?;
        if (0xD800..0xDC00).contains(&first) {
            let saved = self.at;
            if self.peek() == Some('\\') {
                self.at += 1;
                if self.peek() == Some('u') {
                    self.at += 1;
                    let second = self.hex4()?;
                    if (0xDC00..0xE000).contains(&second) {
                        let combined = 0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        return Ok(char::from_u32(combined).unwrap_or('\u{fffd}'));
                    }
                }
            }
            self.at = saved;
            return Ok('\u{fffd}');
        }
        Ok(char::from_u32(first).unwrap_or('\u{fffd}'))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.peek().ok_or(JsonError::Truncated { at: self.at })?;
            let d = c.to_digit(16).ok_or(JsonError::Expected {
                at: self.at,
                expected: "four hex digits",
                found: c.to_string(),
            })?;
            v = v * 16 + d;
            self.at += 1;
        }
        Ok(v)
    }

    /// `true`, `false`, `null`, and numbers. Kept as written, because the
    /// YAML side keeps its scalars as written too and the accessors are what
    /// interpret them.
    fn bare(&mut self) -> Result<Yaml, JsonError> {
        let start = self.at;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.') {
                self.at += 1;
            } else {
                break;
            }
        }
        if start == self.at {
            let found = self.peek().map(|c| c.to_string());
            return match found {
                Some(found) => Err(JsonError::Expected {
                    at: self.at,
                    expected: "a value",
                    found,
                }),
                None => Err(JsonError::Truncated { at: self.at }),
            };
        }
        let raw: String = self.src[start..self.at].iter().collect();
        Ok(if raw == "null" {
            Yaml::Null
        } else {
            Yaml::Str(raw)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SPEC: &str = r#"{
  "openapi": "3.0.3",
  "info": { "title": "Acme", "version": "1.4.0" },
  "servers": [
    { "url": "http://localhost:3000", "description": "local" }
  ],
  "paths": {
    "/users": {
      "post": { "operationId": "createUser", "summary": "Register a user" }
    }
  }
}"#;

    #[test]
    fn a_json_spec_reads_as_the_same_tree_a_yaml_one_would() {
        let doc = parse(SPEC).unwrap();
        assert_eq!(
            doc.get("openapi").and_then(Yaml::as_str),
            Some("3.0.3"),
            "a value spanning several lines is still one document"
        );
        let op = doc
            .get("paths")
            .and_then(|p| p.get("/users"))
            .and_then(|p| p.get("post"))
            .and_then(|p| p.get("operationId"))
            .and_then(Yaml::as_str);
        assert_eq!(op, Some("createUser"));
        let servers = doc.get("servers").and_then(Yaml::as_seq).unwrap();
        assert_eq!(
            servers[0].get("url").and_then(Yaml::as_str),
            Some("http://localhost:3000")
        );
    }

    #[test]
    fn numbers_booleans_and_null_land_where_the_yaml_side_leaves_them() {
        let doc = parse(r#"{"n": 3, "big": -1.5e3, "yes": true, "gone": null}"#).unwrap();
        assert_eq!(doc.get("n").and_then(Yaml::as_u32), Some(3));
        assert_eq!(doc.get("big").and_then(Yaml::as_str), Some("-1.5e3"));
        assert_eq!(doc.get("yes").and_then(Yaml::as_bool), Some(true));
        assert_eq!(doc.get("gone"), Some(&Yaml::Null));
    }

    #[test]
    fn escapes_inside_strings_survive() {
        let doc = parse(r#"{"a": "line\nbreak \"quoted\" é \\"}"#).unwrap();
        assert_eq!(
            doc.get("a").and_then(Yaml::as_str),
            Some("line\nbreak \"quoted\" é \\")
        );
    }

    #[test]
    fn empty_collections_are_kept_rather_than_dropped() {
        let doc = parse(r#"{"a": {}, "b": []}"#).unwrap();
        assert_eq!(doc.get("a"), Some(&Yaml::Map(vec![])));
        assert_eq!(doc.get("b"), Some(&Yaml::Seq(vec![])));
    }

    #[test]
    fn malformed_input_names_where_it_gave_up() {
        assert!(matches!(
            parse(r#"{"a": 1,}"#),
            Err(JsonError::Expected { .. })
        ));
        assert!(matches!(
            parse(r#"{"a": "#),
            Err(JsonError::Truncated { .. })
        ));
        assert!(matches!(
            parse(r#"{"a": 1} junk"#),
            Err(JsonError::Trailing { .. })
        ));
    }

    #[test]
    fn nesting_is_refused_before_it_can_overflow_the_stack() {
        let bomb = "[".repeat(5_000);
        assert_eq!(parse(&bomb), Err(JsonError::TooDeep { limit: MAX_DEPTH }));
    }
}
