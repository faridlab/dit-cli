//! A small YAML subset parser for the committed schema files
//! (`workflow.yaml`, `config.yaml`).
//!
//! Issue frontmatter does NOT go through this — it uses the surgical
//! `Document`, which preserves bytes. These files are read-only for DIT
//! (written once by `dit init`, then edited by humans), so a typed tree is
//! enough. Supported: block mappings, block sequences (including `- key: v`
//! entries continued on the next lines), flow sequences `[a, b]`, flow
//! mappings `{k: v}`, quoted and bare scalars, comments. Not supported, on
//! purpose: anchors/aliases (exponential-expansion inputs), multi-document
//! streams, block scalars (`|`, `>`).

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum YamlError {
    #[error("line {line}: expected `key: value`, a `- item`, or a comment — found `{text}`")]
    BadLine { line: usize, text: String },
    #[error("line {line}: unterminated quote or bracket")]
    Unterminated { line: usize },
    #[error("line {line}: inconsistent indentation — expected {expected} spaces, found {found}")]
    Indent {
        line: usize,
        expected: usize,
        found: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Null,
    Str(String),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

impl Yaml {
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Yaml::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Yaml]> {
        match self {
            Yaml::Seq(items) => Some(items),
            _ => None,
        }
    }

    /// Scalar as u32 (wip_limit, estimate, schema_version).
    pub fn as_u32(&self) -> Option<u32> {
        self.as_str().and_then(|s| s.parse().ok())
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.as_str()? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

struct Line {
    indent: usize,
    text: String, // comment-stripped, trimmed
    no: usize,    // 1-based source line number
    /// The already-gathered value of a `key: |` or `key: >` block. Resolved
    /// during the line scan, because the continuation lines are deeper than
    /// their key and every structural rule below would read them as a nested
    /// block instead of as text.
    block: Option<String>,
}

pub fn parse(text: &str) -> Result<Yaml, YamlError> {
    let mut lines = Vec::new();
    let raw_lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    while i < raw_lines.len() {
        let raw = raw_lines[i];
        // A block scalar's own lines are literal text: comments are not
        // stripped from them and `#` is just a character, so the header has
        // to be recognised before anything else touches the line.
        if let Some((key, style, indent)) = block_header(raw) {
            let (value, next) = gather_block(&raw_lines, i + 1, indent, style);
            lines.push(Line {
                indent,
                text: format!("{key}:"),
                no: i + 1,
                block: Some(value),
            });
            i = next;
            continue;
        }
        let stripped = strip_comment(raw);
        if stripped.trim().is_empty() {
            i += 1;
            continue;
        }
        let indent = stripped.len() - stripped.trim_start().len();
        lines.push(Line {
            indent,
            text: stripped.trim().to_owned(),
            no: i + 1,
            block: None,
        });
        i += 1;
    }
    if lines.is_empty() {
        return Ok(Yaml::Null);
    }
    let mut idx = 0;
    parse_block(&lines, &mut idx, lines[0].indent)
}

/// How a block scalar joins its lines and what it does with the trailing
/// newline. Only the four forms real documents use are accepted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockStyle {
    /// `|` — lines kept as written.
    Literal,
    /// `>` — lines folded into one, joined with spaces.
    Folded,
}

/// `key: |`, `key: >`, with an optional `-` or `+` chomping indicator.
/// Returns the key, the style, and the key's own indentation.
fn block_header(raw: &str) -> Option<(String, BlockStyle, usize)> {
    let indent = raw.len() - raw.trim_start().len();
    let text = raw.trim();
    let (key, value) = split_entry(text)?;
    let mut chars = value.chars();
    let style = match chars.next()? {
        '|' => BlockStyle::Literal,
        '>' => BlockStyle::Folded,
        _ => return None,
    };
    // Everything after the marker must be a chomping indicator and nothing
    // else — `a: |x` is a scalar that happens to start with a pipe.
    if !chars.all(|c| c == '-' || c == '+') {
        return None;
    }
    Some((key, style, indent))
}

/// Consume the lines belonging to a block scalar: those indented deeper than
/// the key, plus blank lines between them. Returns the value and the index
/// of the first line that is not part of it.
fn gather_block(
    raw_lines: &[&str],
    start: usize,
    key_indent: usize,
    style: BlockStyle,
) -> (String, usize) {
    let mut body: Vec<&str> = Vec::new();
    let mut i = start;
    while i < raw_lines.len() {
        let line = raw_lines[i];
        let deeper = line.len() - line.trim_start().len() > key_indent;
        if line.trim().is_empty() || deeper {
            body.push(line);
            i += 1;
        } else {
            break;
        }
    }
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    // The block's own indentation is set by its first non-blank line and
    // stripped from every line, so nesting the document deeper does not
    // change the text.
    let strip = body
        .iter()
        .find(|l| !l.trim().is_empty())
        .map_or(0, |l| l.len() - l.trim_start().len());
    let cut = |l: &&str| -> String {
        if l.len() >= strip {
            l[strip..].trim_end().to_owned()
        } else {
            l.trim().to_owned()
        }
    };
    let value = match style {
        BlockStyle::Literal => body.iter().map(cut).collect::<Vec<_>>().join("\n"),
        BlockStyle::Folded => body
            .iter()
            .map(cut)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    };
    (value, i)
}

/// Strip a trailing `# comment` that is outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double && (i == 0 || line[..i].ends_with(' ')) => {
                return &line[..i];
            }
            _ => {}
        }
    }
    line
}

fn parse_block(lines: &[Line], idx: &mut usize, indent: usize) -> Result<Yaml, YamlError> {
    if is_seq_item(&lines[*idx].text) {
        parse_seq(lines, idx, indent)
    } else {
        parse_map(lines, idx, indent)
    }
}

fn is_seq_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

/// Split a `key: value` (or `key:`) line. `key:value` without the space is a
/// scalar in YAML, not a mapping — hence None.
fn split_entry(text: &str) -> Option<(String, String)> {
    let colon = text.find(':')?;
    let key = text[..colon].trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    match text[colon + 1..].chars().next() {
        None => Some((key.to_owned(), String::new())),
        Some(' ') => Some((key.to_owned(), text[colon + 1..].trim().to_owned())),
        _ => None,
    }
}

/// Parse the value that follows a consumed `key:` line when it is empty on
/// the key line: a deeper block, a same-indent sequence, or null.
fn parse_child(lines: &[Line], idx: &mut usize, key_indent: usize) -> Result<Yaml, YamlError> {
    if *idx < lines.len() && lines[*idx].indent > key_indent {
        let child_indent = lines[*idx].indent;
        parse_block(lines, idx, child_indent)
    } else if *idx < lines.len()
        && lines[*idx].indent == key_indent
        && is_seq_item(&lines[*idx].text)
    {
        // `key:` followed by `- a` at the same indent: the sequence belongs
        // to the key. Humans write this; our emitter always indents.
        parse_seq(lines, idx, key_indent)
    } else {
        Ok(Yaml::Null)
    }
}

fn parse_map(lines: &[Line], idx: &mut usize, indent: usize) -> Result<Yaml, YamlError> {
    let mut entries: Vec<(String, Yaml)> = Vec::new();
    while *idx < lines.len() {
        let (line_no, text, cur_indent, block) = {
            let l = &lines[*idx];
            (l.no, l.text.clone(), l.indent, l.block.clone())
        };
        if cur_indent < indent {
            break;
        }
        if cur_indent > indent {
            return Err(YamlError::Indent {
                line: line_no,
                expected: indent,
                found: cur_indent,
            });
        }
        if is_seq_item(&text) {
            break;
        }
        let Some((key, value_part)) = split_entry(&text) else {
            return Err(YamlError::BadLine {
                line: line_no,
                text,
            });
        };
        *idx += 1; // the key line is consumed in every branch below
        let value = if let Some(text) = block {
            Yaml::Str(text)
        } else if !value_part.is_empty() {
            parse_scalar(&value_part, line_no)?
        } else {
            parse_child(lines, idx, indent)?
        };
        entries.push((key, value));
    }
    Ok(Yaml::Map(entries))
}

fn parse_seq(lines: &[Line], idx: &mut usize, indent: usize) -> Result<Yaml, YamlError> {
    let mut items = Vec::new();
    while *idx < lines.len() {
        let (line_no, text, cur_indent) = {
            let l = &lines[*idx];
            (l.no, l.text.clone(), l.indent)
        };
        if cur_indent < indent || !is_seq_item(&text) {
            break;
        }
        if cur_indent > indent {
            return Err(YamlError::Indent {
                line: line_no,
                expected: indent,
                found: cur_indent,
            });
        }
        let rest = if text == "-" {
            String::new()
        } else {
            text[2..].trim().to_owned()
        };
        if rest.is_empty() {
            // `-` alone: the item is the block indented under it, or null.
            *idx += 1;
            if *idx < lines.len() && lines[*idx].indent > indent {
                let child_indent = lines[*idx].indent;
                items.push(parse_block(lines, idx, child_indent)?);
            } else {
                items.push(Yaml::Null);
            }
        } else if split_entry(&rest).is_some() {
            // `- key: v` — a map whose first entry sits on the dash line;
            // the remaining entries are indented past the dash.
            let (key, value_part) = split_entry(&rest).ok_or(YamlError::BadLine {
                line: line_no,
                text: rest.clone(),
            })?;
            let mut entries = Vec::new();
            if value_part.is_empty() {
                return Err(YamlError::BadLine {
                    line: line_no,
                    text: rest,
                });
            }
            entries.push((key, parse_scalar(&value_part, line_no)?));
            *idx += 1;
            let entry_indent = indent + 2;
            while *idx < lines.len() && lines[*idx].indent >= entry_indent {
                let (cont_no, cont_text, cont_indent) = {
                    let l = &lines[*idx];
                    (l.no, l.text.clone(), l.indent)
                };
                let cont_block = lines[*idx].block.clone();
                let Some((k, vp)) = split_entry(&cont_text) else {
                    return Err(YamlError::BadLine {
                        line: cont_no,
                        text: cont_text,
                    });
                };
                *idx += 1;
                let v = if let Some(text) = cont_block {
                    Yaml::Str(text)
                } else if !vp.is_empty() {
                    parse_scalar(&vp, cont_no)?
                } else {
                    parse_child(lines, idx, cont_indent)?
                };
                entries.push((k, v));
            }
            items.push(Yaml::Map(entries));
        } else {
            items.push(parse_scalar(&rest, line_no)?);
            *idx += 1;
        }
    }
    Ok(Yaml::Seq(items))
}

fn parse_scalar(s: &str, line_no: usize) -> Result<Yaml, YamlError> {
    let s = s.trim();
    let len = s.len();
    let quoted = (s.starts_with('"') && s.ends_with('"') && len >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && len >= 2);
    if quoted {
        return Ok(Yaml::Str(unquote(s)));
    }
    if s.starts_with('[') {
        if !s.ends_with(']') {
            return Err(YamlError::Unterminated { line: line_no });
        }
        let mut items = Vec::new();
        for part in split_flow(&s[1..len - 1]) {
            let p = part.trim();
            if !p.is_empty() {
                items.push(parse_scalar(p, line_no)?);
            }
        }
        return Ok(Yaml::Seq(items));
    }
    if s.starts_with('{') {
        if !s.ends_with('}') {
            return Err(YamlError::Unterminated { line: line_no });
        }
        let mut entries = Vec::new();
        for part in split_flow(&s[1..len - 1]) {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            let colon = p.find(':').ok_or(YamlError::BadLine {
                line: line_no,
                text: p.to_owned(),
            })?;
            let key = unquote(p[..colon].trim());
            entries.push((key, parse_scalar(p[colon + 1..].trim(), line_no)?));
        }
        return Ok(Yaml::Map(entries));
    }
    if s.starts_with('"') || s.starts_with('\'') {
        return Err(YamlError::Unterminated { line: line_no });
    }
    if s == "null" || s == "~" {
        return Ok(Yaml::Null);
    }
    Ok(Yaml::Str(s.to_owned()))
}

/// Split a flow body on top-level commas.
fn split_flow(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                cur.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                cur.push(ch);
            }
            '[' | '{' if !in_single && !in_double => {
                depth += 1;
                cur.push(ch);
            }
            ']' | '}' if !in_single && !in_double => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if !in_single && !in_double && depth == 0 => {
                parts.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    parts
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'"' && b[s.len() - 1] == b'"') || (b[0] == b'\'' && b[s.len() - 1] == b'\'') {
            return s[1..s.len() - 1].to_owned();
        }
    }
    s.to_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const WORKFLOW: &str = "\
# The board's columns and rules.
statuses:
  - { id: backlog, label: Backlog, category: todo }
  - { id: in_progress, label: In Progress, category: doing, wip_limit: 3 }
  - id: done
    label: Done
    category: done
    terminal: true
transitions:
  - from: [backlog, todo]
    to: in_progress
derived:
  - on: commit_trailer
    implies: review
";

    #[test]
    fn literal_block_scalars_keep_their_lines() {
        // Real OpenAPI documents describe things, and descriptions wrap.
        // Before block scalars were understood, the indented continuation
        // lines read as inconsistent indentation and killed the whole file.
        let y = parse(
            "info:\n  title: Acme\n  description: |\n    First line.\n    Second line.\nservers:\n  - url: \"http://localhost:3000\"\n",
        )
        .unwrap();
        assert_eq!(
            y.get("info").and_then(|i| i.get("description")).unwrap(),
            &Yaml::Str("First line.\nSecond line.".into())
        );
        assert_eq!(
            y.get("info")
                .and_then(|i| i.get("title"))
                .and_then(Yaml::as_str),
            Some("Acme"),
            "the keys around a block scalar still parse"
        );
        assert!(y.get("servers").and_then(Yaml::as_seq).is_some());
    }

    #[test]
    fn folded_and_chomped_blocks_are_understood() {
        let y = parse("a: >\n  one\n  two\nb: 2\n").unwrap();
        assert_eq!(
            y.get("a"),
            Some(&Yaml::Str("one two".into())),
            "folded joins with spaces"
        );
        assert_eq!(y.get("b").and_then(Yaml::as_str), Some("2"));

        let y = parse("a: |-\n  one\n\nb: 2\n").unwrap();
        assert_eq!(y.get("a"), Some(&Yaml::Str("one".into())));
        assert_eq!(y.get("b").and_then(Yaml::as_str), Some("2"));
    }

    #[test]
    fn a_hash_inside_a_block_scalar_is_not_a_comment() {
        let y = parse("a: |\n  # not a comment\n  text\nb: 2\n").unwrap();
        assert_eq!(y.get("a"), Some(&Yaml::Str("# not a comment\ntext".into())));
        assert_eq!(y.get("b").and_then(Yaml::as_str), Some("2"));
    }

    #[test]
    fn parses_the_workflow_shape() {
        let y = parse(WORKFLOW).unwrap();
        let statuses = y.get("statuses").unwrap().as_seq().unwrap();
        assert_eq!(statuses.len(), 3);
        assert_eq!(statuses[0].get("id").unwrap().as_str().unwrap(), "backlog");
        assert_eq!(statuses[1].get("wip_limit").unwrap().as_u32(), Some(3));
        assert_eq!(statuses[2].get("terminal").unwrap().as_bool(), Some(true));
        let transitions = y.get("transitions").unwrap().as_seq().unwrap();
        assert_eq!(
            transitions[0].get("from").unwrap().as_seq().unwrap().len(),
            2
        );
        let derived = y.get("derived").unwrap().as_seq().unwrap();
        assert_eq!(
            derived[0].get("on").unwrap().as_str().unwrap(),
            "commit_trailer"
        );
    }

    #[test]
    fn block_maps_one_level_deep() {
        let y = parse("outer:\n  a: 1\n  b: two\n").unwrap();
        let outer = y.get("outer").unwrap();
        assert_eq!(outer.get("a").unwrap().as_str().unwrap(), "1");
        assert_eq!(outer.get("b").unwrap().as_str().unwrap(), "two");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let y = parse("# lead\n\na: 1  # trailing\n\n# tail\n").unwrap();
        assert_eq!(y.get("a").unwrap().as_str().unwrap(), "1");
    }

    #[test]
    fn empty_input_is_null() {
        assert_eq!(parse("").unwrap(), Yaml::Null);
        assert_eq!(parse("# only a comment\n").unwrap(), Yaml::Null);
    }

    #[test]
    fn flow_sequences_and_maps() {
        let y = parse("a: [x, y]\nb: {k: v, n: 2}\nc: []\n").unwrap();
        assert_eq!(y.get("a").unwrap().as_seq().unwrap().len(), 2);
        assert_eq!(y.get("b").unwrap().get("k").unwrap().as_str().unwrap(), "v");
        assert_eq!(y.get("c").unwrap().as_seq().unwrap().len(), 0);
    }

    #[test]
    fn quoted_strings_keep_commas_and_hashes() {
        let y = parse("a: \"x, y # z\"\nb: 'plain'\n").unwrap();
        assert_eq!(y.get("a").unwrap().as_str().unwrap(), "x, y # z");
        assert_eq!(y.get("b").unwrap().as_str().unwrap(), "plain");
    }

    #[test]
    fn inconsistent_indent_is_an_error_with_a_line_number() {
        let err = parse("a:\n  b: 1\n   c: 2\n").unwrap_err();
        assert!(matches!(err, YamlError::Indent { line: 3, .. }), "{err:?}");
    }

    #[test]
    fn unterminated_flow_is_an_error() {
        assert!(parse("a: [x, y\n").is_err());
        assert!(parse("a: \"oops\n").is_err());
    }

    #[test]
    fn a_sequence_at_the_parent_indent_belongs_to_its_key() {
        let y = parse("key:\n- a\n- b\n").unwrap();
        assert_eq!(y.get("key").unwrap().as_seq().unwrap().len(), 2);
    }

    #[test]
    fn trailing_whitespace_is_tolerated() {
        let y = parse("a: 1   \nb:   x y  \n").unwrap();
        assert_eq!(y.get("a").unwrap().as_str().unwrap(), "1");
        assert_eq!(y.get("b").unwrap().as_str().unwrap(), "x y");
    }
}
