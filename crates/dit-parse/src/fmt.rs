//! `dit fmt` — canonical markdown formatting.
//!
//! gofmt-style: one canonical form, so that a Vim edit and a DIT edit of the
//! same body produce identical bytes. Frontmatter is **never** reformatted —
//! it is already canonical by construction (surgical writes), and reflowing
//! it would destroy unknown fields.
//!
//! Formatting a file that still contains diff3 conflict markers is refused,
//! not attempted anyway. comrak reads `=======` as a setext underline and
//! `>>>>>>>` as a blockquote, so formatting a conflicted file silently
//! destroys the markers — and with them the user's only chance to resolve
//! the conflict by hand. Markers inside fenced code blocks are legitimate
//! content and are left alone.

use comrak::{format_commonmark, parse_document, Arena, Options};

use crate::frontmatter::Document;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FmtError {
    #[error("file contains git conflict markers — resolve them (edit the file, keep one side) before formatting; `dit fmt` will not touch it")]
    ConflictMarkers,
    #[error("formatting failed: {0}")]
    Write(#[from] std::fmt::Error),
}

/// Format a whole file: parse (frontmatter kept verbatim), format the body,
/// re-serialize. Idempotent: `fmt(fmt(x)) == fmt(x)`.
pub fn fmt(input: &str) -> Result<String, FmtError> {
    let mut doc = Document::parse(input).map_err(|_| FmtError::ConflictMarkers)?;
    // ^ A frontmatter that cannot even be parsed is treated the same as a
    //   conflicted file: refuse to rewrite rather than guess. Half-merged
    //   frontmatter is exactly the shape this catches.
    format_document(&mut doc)?;
    Ok(doc.to_string())
}

/// Format only the body of an already-parsed document, in place.
pub fn format_document(doc: &mut Document) -> Result<(), FmtError> {
    let body = doc.body().to_owned();
    let formatted = format_body(&body)?;
    doc.set_body(formatted);
    Ok(())
}

/// Format a markdown body (no frontmatter involved).
pub fn format_body(body: &str) -> Result<String, FmtError> {
    if has_conflict_markers(body) {
        return Err(FmtError::ConflictMarkers);
    }
    let options = dit_options();
    let arena = Arena::new();
    let root = parse_document(&arena, body, &options);
    let mut out = String::new();
    format_commonmark(root, &options, &mut out)?;
    Ok(post_process(&out))
}

/// True when the text contains diff3 conflict markers outside fenced code
/// blocks. `=======` (7+) and `<<<<<<< `/`>>>>>>> `/`||||||| ` at line start.
pub fn has_conflict_markers(text: &str) -> bool {
    let mut fence: Option<(char, usize)> = None; // (fence char, length)
    let fence_len = |t: &str, ch: char| t.chars().take_while(|&c| c == ch).count();
    for line in text.split('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        // A fence opens/closes only with ≥3 backticks/tildes at ≤3 indent.
        let (ch, len) = if indent <= 3 && fence_len(trimmed, '`') >= 3 {
            ('`', fence_len(trimmed, '`'))
        } else if indent <= 3 && fence_len(trimmed, '~') >= 3 {
            ('~', fence_len(trimmed, '~'))
        } else {
            ('\0', 0)
        };
        match fence {
            // Closing fence: same char, at least as long, info string empty
            // (a closing fence carries no info).
            Some((open_ch, open_len))
                if ch == open_ch
                    && len >= open_len
                    && trimmed.chars().skip(len).all(|c| c.is_whitespace()) =>
            {
                fence = None;
                continue;
            }
            // Inside a code block: content, not markers.
            _ if fence.is_some() => continue,
            None if len >= 3 => {
                fence = Some((ch, len));
                continue;
            }
            _ => {}
        }
        let is_marker = trimmed.starts_with("<<<<<<< ")
            || trimmed.starts_with(">>>>>>> ")
            || trimmed.starts_with("||||||| ")
            || (trimmed.starts_with('=')
                && trimmed.len() >= 7
                && trimmed.chars().all(|c| c == '='));
        if is_marker {
            return true;
        }
    }
    false
}

/// The comrak option set — this is the normative `dit fmt` profile. Field
/// values are pinned by the golden tests; changing anything here is a
/// formatting change for every file in every workspace.
pub fn dit_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    // Obsidian-style `[[target|title]]`. Without the flag comrak escapes the
    // brackets; with it, `[[x]]` serializes as `[[x|x]]` and the post-pass
    // below collapses it back.
    options.extension.wikilinks_title_after_pipe = true;
    // Without this comrak escapes `[[`, `#`, and `_` even where CommonMark
    // does not require it — readable text comes back full of backslashes.
    // The option is marked experimental upstream, hence the version pin.
    options.render.experimental_minimize_commonmark = true;
    options
}

/// Post-processing passes, each idempotent by construction:
/// 1. collapse `[[x|x]]` → `[[x]]` (comrak's wikilink long form);
/// 2. trim whitespace on lines that are *entirely* whitespace — never on
///    lines with text, where trailing spaces can be hard breaks.
fn post_process(text: &str) -> String {
    let collapsed = collapse_self_titled_wikilinks(text);
    trimmed_blank_lines(&collapsed)
}

fn collapse_self_titled_wikilinks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some(close) = find_closing(text, i + 2) {
                let inner = &text[i + 2..close];
                if let Some(pipe) = inner.find('|') {
                    let (target, title) = inner.split_at(pipe);
                    let title = &title[1..];
                    if target == title {
                        // `[[x|x]]` → `[[x]]`
                        out.push_str("[[");
                        out.push_str(target);
                        out.push_str("]]");
                        i = close + 2;
                        continue;
                    }
                }
                out.push_str("[[");
                i += 2;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Find the `]]` that closes a wikilink opened at depth, honoring nesting.
fn find_closing(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 2;
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            depth += 2;
            i += 2;
            continue;
        }
        if bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b']' {
            depth -= 2;
            if depth == 0 {
                return Some(i);
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

fn trimmed_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut iter = text.split('\n').peekable();
    while let Some(line) = iter.next() {
        if iter.peek().is_some() && !line.is_empty() && line.chars().all(char::is_whitespace) {
            out.push('\n');
        } else {
            out.push_str(line);
            if iter.peek().is_some() {
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn fmt_is_idempotent() {
        let input = "---\ntitle: x\n---\n\nSome  *bold*   text\n\n\n- a\n- b\n";
        let once = fmt(input).unwrap();
        let twice = fmt(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn fmt_survives_a_whitespace_trimming_editor() {
        // Many editors trim trailing whitespace on save; the formatter must
        // not put back what they remove. fmt∘trim∘fmt == fmt∘fmt.
        let input = "para\n\n- a\n   \n- b\n";
        let once = format_body(input).unwrap();
        let trimmed: String = once
            .split('\n')
            .map(|l| l.trim_end_matches([' ', '\t']))
            .collect::<Vec<_>>()
            .join("\n");
        let again = format_body(&trimmed).unwrap();
        assert_eq!(once, again);
    }

    #[test]
    fn conflict_markers_are_refused_not_mangled() {
        let conflicted = "text\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\n";
        assert_eq!(fmt(conflicted).unwrap_err(), FmtError::ConflictMarkers);
        assert!(has_conflict_markers(conflicted));
        // diff3 variant with the common ancestor section.
        let diff3 = "a\n<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> f\n";
        assert!(has_conflict_markers(diff3));
    }

    #[test]
    fn marker_lookalikes_inside_code_fences_are_content() {
        let legit = "```diff\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> f\n```\nafter\n";
        assert!(!has_conflict_markers(legit));
        let formatted = format_body(legit).unwrap();
        assert!(formatted.contains("<<<<<<< HEAD"));
    }

    #[test]
    fn a_seven_equals_heading_is_still_a_marker() {
        assert!(has_conflict_markers("para\n=======\n"));
        // Six equals is a legal setext underline, not a marker.
        assert!(!has_conflict_markers("para\n======\n"));
    }

    #[test]
    fn wikilinks_round_trip_to_the_short_form() {
        let formatted = format_body("see [[docs/flows/auth-session]]\n").unwrap();
        assert!(
            formatted.contains("[[docs/flows/auth-session]]"),
            "{formatted}"
        );
        // A genuinely titled wikilink keeps its title.
        let titled = format_body("see [[docs/flows/auth-session|the auth flow]]\n").unwrap();
        assert!(
            titled.contains("[[docs/flows/auth-session|the auth flow]]"),
            "{titled}"
        );
    }

    #[test]
    fn underscores_and_hashes_are_not_escaped() {
        let formatted = format_body("status in_progress and #refs stay readable\n").unwrap();
        assert!(formatted.contains("in_progress"));
        assert!(formatted.contains("#refs"));
        assert!(!formatted.contains("\\#"), "{formatted}");
        assert!(!formatted.contains("\\_"), "{formatted}");
    }

    #[test]
    fn tables_tasklists_and_fences_are_preserved() {
        let input = "| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] todo\n- [x] done\n\n```dit:query\nstatus = \"todo\"\n```\n";
        let formatted = format_body(input).unwrap();
        assert!(formatted.contains("| a | b |"));
        assert!(formatted.contains("- [ ] todo"));
        assert!(formatted.contains("```dit:query"));
    }

    #[test]
    fn hard_breaks_survive_the_blank_line_trim() {
        // Two trailing spaces on a line *with text* are a hard break. comrak's
        // canonical form is the backslash break; either way the break must
        // survive formatting and stay a break afterwards.
        let formatted = format_body("line one  \nline two\n").unwrap();
        let is_break = formatted.contains("line one\\\n") || formatted.contains("line one  \n");
        assert!(is_break, "hard break was destroyed: {formatted:?}");
        let again = format_body(&formatted).unwrap();
        assert_eq!(again, formatted, "hard break form must be stable");
    }

    #[test]
    fn frontmatter_passes_through_verbatim() {
        let input = "---\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\n# comment kept\nlabels: [a, b]\n---\nbody *text*\n";
        let out = fmt(input).unwrap();
        assert!(out.starts_with(
            "---\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\n# comment kept\nlabels: [a, b]\n---\n"
        ));
    }
}
