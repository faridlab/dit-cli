//! Frontmatter documents with surgical, round-trip-preserving edits.
//!
//! No mature Rust YAML library preserves a file byte-for-byte, and the merge
//! driver's correctness depends on that property. So this module never
//! re-serializes a document from a typed struct: the original lines ARE the
//! storage, and an edit rewrites only the lines of the keys that actually
//! changed. Unknown fields, comments, blank lines and key order survive
//! untouched.
//!
//! Scope is deliberately narrow so it can be audited: top-level `key: value`
//! entries with scalars, flow sequences (`[a, b]`), block sequences and one
//! level of block mapping — everything DIT itself writes, plus verbatim
//! preservation of anything it does not understand. Anchors and aliases are
//! not supported on purpose: they enable exponential-expansion inputs.

/// A parse or validation failure. Every variant says what to do about it.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FrontmatterError {
    #[error(
        "frontmatter must open with a `---` line; found `{0}` — fix the delimiters or drop them"
    )]
    MissingOpeningDelimiter(String),
    #[error(
        "frontmatter opened but never closed with a second `---` line — add the closing delimiter"
    )]
    Unclosed,
    #[error(
        "line {line}: `{text}` is not a `key: value` entry, comment or blank — fix the indentation"
    )]
    BadEntry { line: usize, text: String },
    #[error("key `{key}` has an unterminated quote — close the quote")]
    UnterminatedQuote { key: String },
    #[error("key `{key}` has an unterminated flow sequence `[` — close the bracket")]
    UnterminatedFlow { key: String },
}

#[derive(Debug, Clone, PartialEq)]
enum Entry {
    /// A top-level entry: the `key:` line plus any continuation lines (block
    /// sequence items, nested mappings). `raw` holds the original bytes.
    Pair { key: String, raw: Vec<String> },
    /// A comment or blank line, preserved in place.
    Loose(String),
}

/// One file: frontmatter (surgically editable) + markdown body.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    entries: Vec<Entry>,
    body: String,
}

impl std::fmt::Display for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.serialize())
    }
}

/// The parsed value of a frontmatter key, for inspection and merging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Scalar(Option<String>),
    Seq(Vec<String>),
    /// Nested block mapping — kept verbatim; DIT's own schema never emits one
    /// inside issue frontmatter, but older/newer clients might.
    Map(Vec<String>),
}

impl Document {
    /// Parse a full file. A file with no frontmatter delimiters at all is
    /// legal (body-only documents exist); an opening delimiter without a
    /// closing one is not.
    pub fn parse(input: &str) -> Result<Document, FrontmatterError> {
        let text = input.strip_suffix('\n').unwrap_or(input);
        let mut lines = text.split('\n');

        let first = lines.next().unwrap_or("");
        if first.trim_end() != "---" {
            // Body-only document.
            return Ok(Document {
                entries: vec![],
                body: input.to_owned(),
            });
        }
        if first != "---" {
            return Err(FrontmatterError::MissingOpeningDelimiter(first.to_owned()));
        }

        let mut fm_lines: Vec<String> = vec![];
        let mut fm_len = first.len(); // bytes of the opening delimiter line
        let mut closed = false;
        for line in lines {
            fm_len += 1 + line.len(); // newline + line
            if line.trim_end() == "---" || line.trim_end() == "..." {
                closed = true;
                break;
            }
            fm_lines.push(line.to_owned());
        }
        if !closed {
            return Err(FrontmatterError::Unclosed);
        }
        // Everything after the closing delimiter's newline is the body — even
        // when the body itself contains `---` lines.
        let body = input.get(fm_len + 1..).unwrap_or_default().to_owned();

        let entries = parse_entries(&fm_lines)?;
        Ok(Document { entries, body })
    }

    /// Serialize. Byte-identical to the input for an unmodified document.
    pub fn serialize(&self) -> String {
        if self.entries.is_empty() {
            return self.body.clone();
        }
        let mut out = String::from("---\n");
        for e in &self.entries {
            match e {
                Entry::Pair { raw, .. } => {
                    for l in raw {
                        out.push_str(l);
                        out.push('\n');
                    }
                }
                Entry::Loose(l) => {
                    out.push_str(l);
                    out.push('\n');
                }
            }
        }
        out.push_str("---\n");
        if !self.body.is_empty() {
            out.push_str(&self.body);
        }
        out
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn set_body(&mut self, body: String) {
        self.body = body;
    }

    /// All top-level keys, in file order.
    pub fn keys(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Pair { key, .. } => Some(key.as_str()),
                Entry::Loose(_) => None,
            })
            .collect()
    }

    /// The parsed value of a key.
    pub fn get(&self, key: &str) -> Option<Value> {
        let Entry::Pair { raw, .. } = self
            .entries
            .iter()
            .find(|e| matches!(e, Entry::Pair { key: k, .. } if k == key))?
        else {
            return None;
        };
        let first = raw.first()?;
        let (_, value_part) = split_key_line(first)?;
        Some(parse_value(value_part, &raw[1..]))
    }

    /// Scalar accessor: `None` when the key is absent; `Scalar(None)` when it
    /// is present but empty.
    pub fn get_str(&self, key: &str) -> Option<Option<String>> {
        match self.get(key)? {
            Value::Scalar(v) => Some(v),
            _ => None,
        }
    }

    pub fn get_list(&self, key: &str) -> Option<Vec<String>> {
        match self.get(key)? {
            Value::Seq(items) => Some(items),
            _ => None,
        }
    }

    /// Set (or insert) a key with an already-serialized value. Only this
    /// entry's line changes; a trailing comment on the replaced line is kept.
    pub fn set_raw(&mut self, key: &str, serialized: &str) {
        let new_line = format!("{key}: {serialized}");
        for e in &mut self.entries {
            if let Entry::Pair { key: k, raw } = e {
                if k != key {
                    continue;
                }
                let merged = raw.first().map(|old| keep_trailing_comment(old, &new_line));
                raw.clear();
                raw.push(merged.unwrap_or(new_line.clone()));
                return;
            }
        }
        // New key: insert after the last Pair so trailing comments stay last.
        let idx = self
            .entries
            .iter()
            .rposition(|e| matches!(e, Entry::Pair { .. }));
        let entry = Entry::Pair {
            key: key.to_owned(),
            raw: vec![new_line],
        };
        match idx {
            Some(i) => self.entries.insert(i + 1, entry),
            None => self.entries.insert(0, entry),
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.entries
            .retain(|e| !matches!(e, Entry::Pair { key: k, .. } if k == key));
    }

    /// True when both documents carry the same parsed value for `key`.
    pub fn same_value(&self, other: &Document, key: &str) -> bool {
        self.get(key) == other.get(key)
    }
}

/// Split `key: rest` into its parts. Returns None when the line has no key.
fn split_key_line(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(':')?;
    let key = &line[..idx];
    if key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, line[idx + 1..].trim()))
}

/// Preserve a trailing `# comment` when DIT rewrites an entry's value.
fn keep_trailing_comment(old: &str, new_line: &str) -> String {
    let Some((_, comment)) = split_trailing_comment(old) else {
        return new_line.to_owned();
    };
    format!("{new_line} {comment}")
}

/// Split a value from an unquoted trailing comment. Quoted `#` stays in value.
fn split_trailing_comment(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && (i == 0 || bytes[i - 1] == b' ') => {
                let comment = &value[i..];
                let value_part = value[..i].trim_end();
                return Some((value_part, comment));
            }
            _ => {}
        }
    }
    None
}

fn parse_entries(lines: &[String]) -> Result<Vec<Entry>, FrontmatterError> {
    let mut entries = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            entries.push(Entry::Loose(line.clone()));
            i += 1;
            continue;
        }
        if line.starts_with(' ') || line.starts_with('-') {
            return Err(FrontmatterError::BadEntry {
                line: i + 1,
                text: line.clone(),
            });
        }
        let Some((key, value_part)) = split_key_line(line) else {
            return Err(FrontmatterError::BadEntry {
                line: i + 1,
                text: line.clone(),
            });
        };
        // Continuation lines: indented block sequences / nested maps.
        let mut raw = vec![line.clone()];
        if value_part.is_empty() {
            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with(' ') || lines[j].starts_with('-')) {
                raw.push(lines[j].clone());
                j += 1;
            }
            i = j;
        } else {
            i += 1;
        }
        validate_value(key, value_part)?;
        entries.push(Entry::Pair {
            key: key.to_owned(),
            raw,
        });
    }
    Ok(entries)
}

/// Reject unterminated quotes/brackets on the key line so the merge driver
/// never merges against a half-written file.
fn validate_value(key: &str, value: &str) -> Result<(), FrontmatterError> {
    let v = split_trailing_comment(value)
        .map(|(v, _)| v)
        .unwrap_or(value);
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0i32;
    for ch in v.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '[' if !in_single && !in_double => depth += 1,
            ']' if !in_single && !in_double => depth -= 1,
            _ => {}
        }
    }
    if in_single || in_double {
        return Err(FrontmatterError::UnterminatedQuote {
            key: key.to_owned(),
        });
    }
    if depth != 0 {
        return Err(FrontmatterError::UnterminatedFlow {
            key: key.to_owned(),
        });
    }
    Ok(())
}

/// Parse the value of a key line, consulting continuation lines for block
/// sequences and nested maps.
fn parse_value(value_part: &str, continuation: &[String]) -> Value {
    let bare = split_trailing_comment(value_part)
        .map(|(v, _)| v)
        .unwrap_or(value_part)
        .trim();
    if !bare.is_empty() {
        if bare.starts_with('[') {
            return Value::Seq(parse_flow_seq(bare));
        }
        return Value::Scalar(Some(unquote(bare)));
    }
    // Empty on the key line: block sequence / nested map / null.
    if continuation.is_empty() {
        return Value::Scalar(None);
    }
    if continuation[0].trim_start().starts_with("- ") || continuation[0].trim_end() == "-" {
        let items = continuation
            .iter()
            .map(|l| l.trim_start().trim_start_matches('-').trim())
            .filter(|l| !l.is_empty())
            .map(unquote)
            .collect();
        return Value::Seq(items);
    }
    Value::Map(continuation.to_vec())
}

/// Parse `[a, b, "c d"]` — the only flow form DIT serializes.
fn parse_flow_seq(s: &str) -> Vec<String> {
    let inner = s
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(s);
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in inner.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                cur.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                cur.push(ch);
            }
            ',' if !in_single && !in_double => {
                items.push(unquote(cur.trim()));
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        items.push(unquote(cur.trim()));
    }
    items
}

/// Strip one layer of matching quotes, if present.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[s.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
        {
            return s[1..s.len() - 1].to_owned();
        }
    }
    s.to_owned()
}

/// Serialize a scalar the canonical way: bare whenever it round-trips as a
/// plain YAML scalar, quoted when it would not. Plain scalars may contain
/// spaces — titles stay readable — but not indicator characters in the wrong
/// place, `": "`, a trailing `:`, `" #"`, or a boolean/number/null lookalike
/// (other YAML tools would coerce those to a different type).
pub fn serialize_scalar(s: &str) -> String {
    let ambiguous_lookalike = matches!(
        s,
        "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off" | "" | "-"
    ) || s.parse::<f64>().is_ok();
    let first_ok = s.chars().next().is_some_and(|c| {
        !c.is_whitespace()
            && !matches!(
                c,
                '-' | '?'
                    | ':'
                    | ','
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '#'
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '\''
                    | '"'
                    | '%'
                    | '@'
                    | '`'
            )
    });
    let safe = !s.is_empty()
        && first_ok
        && !s.ends_with(char::is_whitespace)
        && !s.ends_with(':')
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.contains(['\n', '\r', '\t'])
        && !ambiguous_lookalike;
    if safe {
        s.to_owned()
    } else {
        format!("{s:?}")
    }
}

/// Serialize a list as a flow sequence — canonical DIT form.
pub fn serialize_seq(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let joined: Vec<String> = items.iter().map(|i| serialize_scalar(i)).collect();
    format!("[{}]", joined.join(", "))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const ISSUE: &str = "---\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\nschema: 1\ntitle: Login timeout\n# touched by budi\nstatus: in_progress\nlabels: [auth, frontend]\nestimate: 3\nnotes: \"keep me verbatim\"\n---\n\nBody text.\n";

    #[test]
    fn unmodified_document_roundtrips_byte_exact() {
        let doc = Document::parse(ISSUE).unwrap();
        assert_eq!(doc.to_string(), ISSUE);
    }

    #[test]
    fn unknown_fields_and_comments_survive_a_patch() {
        let mut doc = Document::parse(ISSUE).unwrap();
        doc.set_raw("status", "review");
        let out = doc.to_string();
        // Unknown field untouched, byte-exact.
        assert!(out.contains("notes: \"keep me verbatim\"\n"));
        // Comment untouched, in place.
        assert!(out.contains("# touched by budi\n"));
        assert!(out.contains("status: review"));
        // And the patch itself round-trips.
        assert_eq!(Document::parse(&out).unwrap().to_string(), out);
    }

    #[test]
    fn trailing_comment_on_a_rewritten_entry_is_kept() {
        let input = "---\ntitle: old # budi wrote this\nstatus: todo\n---\nx\n";
        let mut doc = Document::parse(input).unwrap();
        doc.set_raw("title", "new");
        assert!(doc.to_string().contains("title: new # budi wrote this"));
    }

    #[test]
    fn reads_scalars_and_lists() {
        let doc = Document::parse(ISSUE).unwrap();
        assert_eq!(
            doc.get_str("id").unwrap().unwrap(),
            "01K3M9ZXQ2R7VN8P4TDBCEFGHJ"
        );
        assert_eq!(doc.get_list("labels").unwrap(), vec!["auth", "frontend"]);
        assert_eq!(doc.get_str("missing"), None);
    }

    #[test]
    fn block_sequences_are_read_and_preserved() {
        let input = "---\nassignees:\n  - farid\n  - budi\n---\nx\n";
        let mut doc = Document::parse(input).unwrap();
        assert_eq!(doc.get_list("assignees").unwrap(), vec!["farid", "budi"]);
        doc.set_raw("status", "todo");
        assert!(doc.to_string().contains("  - farid\n  - budi"));
    }

    #[test]
    fn quoted_values_with_commas_and_hashes() {
        let input = "---\na: \"x, y # not a comment\"\nb: [\"p q\", r]\n---\nx\n";
        let doc = Document::parse(input).unwrap();
        assert_eq!(doc.get_str("a").unwrap().unwrap(), "x, y # not a comment");
        assert_eq!(doc.get_list("b").unwrap(), vec!["p q", "r"]);
    }

    #[test]
    fn body_only_documents_are_legal() {
        let doc = Document::parse("Just some markdown.\n").unwrap();
        assert_eq!(doc.to_string(), "Just some markdown.\n");
        assert!(doc.keys().is_empty());
    }

    #[test]
    fn unterminated_quote_is_rejected() {
        assert!(Document::parse("a: \"unterminated\n").is_ok());
        // ^ body-only (no delimiters) — quotes only matter inside frontmatter.
        let err = Document::parse("---\na: \"unterminated\n---\n").unwrap_err();
        assert!(matches!(err, FrontmatterError::UnterminatedQuote { .. }));
    }

    #[test]
    fn unclosed_frontmatter_is_an_error() {
        assert!(matches!(
            Document::parse("---\na: 1\n").unwrap_err(),
            FrontmatterError::Unclosed
        ));
    }

    #[test]
    fn serialization_quotes_when_needed() {
        // Spaces alone do not force quotes — titles stay readable.
        assert_eq!(serialize_scalar("Fix login timeout"), "Fix login timeout");
        assert_eq!(serialize_scalar("in_progress"), "in_progress");
        assert_eq!(
            serialize_scalar("2026-08-16T09:12:00Z"),
            "2026-08-16T09:12:00Z"
        );
        assert_eq!(serialize_scalar("2026-09-01"), "2026-09-01");
        // These would change meaning as plain scalars.
        assert_eq!(serialize_scalar(""), "\"\"");
        assert_eq!(serialize_scalar("true"), "\"true\"");
        assert_eq!(serialize_scalar("123"), "\"123\"");
        assert_eq!(
            serialize_scalar("title: with colon"),
            "\"title: with colon\""
        );
        assert_eq!(serialize_scalar("has # hash"), "\"has # hash\"");
        assert_eq!(serialize_scalar("- dash first"), "\"- dash first\"");
        assert_eq!(serialize_seq(&[]), "[]");
        assert_eq!(
            serialize_seq(&["auth".into(), "auth backend".into()]),
            "[auth, auth backend]"
        );
        // Round-trip: everything we serialize parses back to the same string.
        for s in [
            "Fix login timeout",
            "true",
            "123",
            "title: with colon",
            "has # hash",
            "",
        ] {
            let doc = Document::parse(&format!("---\nk: {}\n---\n", serialize_scalar(s))).unwrap();
            assert_eq!(doc.get_str("k").unwrap().unwrap(), s, "round-trip of {s:?}");
        }
    }
}
