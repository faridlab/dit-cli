//! DQL lexer. Hand-rolled — the grammar is tiny and a dependency-free lexer
//! keeps this crate usable from WASM with no extra cost.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Bare word: field names, unquoted values, keywords. Keywords are
    /// recognized by the parser (case-insensitively), not the lexer.
    Ident(String),
    Str(String),
    Num(f64),
    /// `-7d`, `+2w`, `-1m`, `-3y`, normalized to days.
    Rel(i64),
    /// `@me`
    Me,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Tilde,
    LParen,
    RParen,
    Comma,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LexError {
    #[error("position {pos}: unexpected character `{ch}`")]
    UnexpectedChar { pos: usize, ch: char },
    #[error("position {pos}: unterminated string — close the quote")]
    UnterminatedString { pos: usize },
    #[error("position {pos}: `~` only works as `field ~ \"text\"` (full-text match)")]
    BareTilde { pos: usize },
}

pub fn lex(input: &str) -> Result<Vec<Tok>, LexError> {
    let mut toks = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            '=' => {
                toks.push(Tok::Eq);
                i += 1;
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                toks.push(Tok::Ne);
                i += 2;
            }
            '>' => {
                let ge = chars.get(i + 1) == Some(&'=');
                toks.push(if ge { Tok::Ge } else { Tok::Gt });
                i += if ge { 2 } else { 1 };
            }
            '<' => {
                let le = chars.get(i + 1) == Some(&'=');
                toks.push(if le { Tok::Le } else { Tok::Lt });
                i += if le { 2 } else { 1 };
            }
            '"' => {
                let mut s = String::new();
                let mut j = i + 1;
                loop {
                    match chars.get(j) {
                        None => return Err(LexError::UnterminatedString { pos: i }),
                        Some('"') => break,
                        Some(&ch) => {
                            s.push(ch);
                            j += 1;
                        }
                    }
                }
                toks.push(Tok::Str(s));
                i = j + 1;
            }
            '@' if chars.get(i + 1) == Some(&'m') && chars.get(i + 2) == Some(&'e') => {
                // `@me` — only when not immediately followed by a word char.
                if chars
                    .get(i + 3)
                    .is_none_or(|c| !c.is_alphanumeric() && *c != '_')
                {
                    toks.push(Tok::Me);
                    i += 3;
                } else {
                    return Err(LexError::UnexpectedChar { pos: i, ch: '@' });
                }
            }
            '-' | '+' if starts_relative(&chars, i) => {
                let mut j = i + 1;
                let mut digits = String::new();
                while chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
                    digits.push(chars[j]);
                    j += 1;
                }
                let unit = chars
                    .get(j)
                    .copied()
                    .ok_or(LexError::UnexpectedChar { pos: i, ch: c })?;
                let n: i64 = digits
                    .parse()
                    .map_err(|_| LexError::UnexpectedChar { pos: i, ch: c })?;
                let signed = if c == '-' { -n } else { n };
                let days = match unit {
                    'd' => signed,
                    'w' => signed * 7,
                    'm' => signed * 30,
                    'y' => signed * 365,
                    _ => return Err(LexError::UnexpectedChar { pos: j, ch: unit }),
                };
                toks.push(Tok::Rel(days));
                i = j + 1;
            }
            '~' => {
                toks.push(Tok::Tilde);
                i += 1;
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while chars
                    .get(j)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '.')
                {
                    j += 1;
                }
                // If word characters continue past the digits (`01K3M9…`,
                // `2026-W33`), the whole thing is a word, not a number.
                if chars.get(j).is_some_and(|c| {
                    c.is_alphabetic() || matches!(c, '_' | '-' | '/' | ':' | '@' | '\'')
                }) {
                    let mut k = j;
                    while chars.get(k).is_some_and(|c| {
                        c.is_alphanumeric()
                            || matches!(c, '_' | '-' | '.' | '/' | ':' | '#' | '\'' | '@')
                    }) {
                        k += 1;
                    }
                    let word: String = chars[i..k].iter().collect();
                    toks.push(Tok::Ident(word));
                    i = k;
                } else {
                    let text: String = chars[i..j].iter().collect();
                    let n = text
                        .parse()
                        .map_err(|_| LexError::UnexpectedChar { pos: i, ch: c })?;
                    toks.push(Tok::Num(n));
                    i = j;
                }
            }
            c if c.is_alphanumeric()
                || c == '_'
                || c == '-'
                || c == '.'
                || c == '/'
                || c == ':'
                || c == '#'
                || c == '@'
                || c == '\'' =>
            {
                let mut j = i;
                while chars.get(j).is_some_and(|c| {
                    c.is_alphanumeric()
                        || matches!(c, '_' | '-' | '.' | '/' | ':' | '#' | '\'' | '@')
                }) {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                toks.push(Tok::Ident(word));
                i = j;
            }
            ch => return Err(LexError::UnexpectedChar { pos: i, ch }),
        }
    }
    Ok(toks)
}

/// A `-`/`+` starts a relative date only when digits and a unit follow.
fn starts_relative(chars: &[char], i: usize) -> bool {
    let mut j = i + 1;
    if !chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    while chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
        j += 1;
    }
    matches!(chars.get(j), Some('d') | Some('w') | Some('m') | Some('y'))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn lexes_the_reference_query() {
        let toks =
            lex("status != done AND assignee = @me AND label IN (auth, api) AND updated > -7d")
                .unwrap();
        assert_eq!(
            toks,
            vec![
                Tok::Ident("status".into()),
                Tok::Ne,
                Tok::Ident("done".into()),
                Tok::Ident("AND".into()),
                Tok::Ident("assignee".into()),
                Tok::Eq,
                Tok::Me,
                Tok::Ident("AND".into()),
                Tok::Ident("label".into()),
                Tok::Ident("IN".into()),
                Tok::LParen,
                Tok::Ident("auth".into()),
                Tok::Comma,
                Tok::Ident("api".into()),
                Tok::RParen,
                Tok::Ident("AND".into()),
                Tok::Ident("updated".into()),
                Tok::Gt,
                Tok::Rel(-7),
            ]
        );
    }

    #[test]
    fn relative_dates_normalize_to_days() {
        assert_eq!(lex("-2w").unwrap(), vec![Tok::Rel(-14)]);
        assert_eq!(lex("-1m").unwrap(), vec![Tok::Rel(-30)]);
        assert_eq!(lex("+3y").unwrap(), vec![Tok::Rel(1095)]);
        assert_eq!(lex("-3d").unwrap(), vec![Tok::Rel(-3)]);
    }

    #[test]
    fn minus_alone_is_not_a_relative_date() {
        // `estimate > -1` is a plain number; `-x` a bare word.
        assert_eq!(
            lex("x = -todo").unwrap(),
            vec![Tok::Ident("x".into()), Tok::Eq, Tok::Ident("-todo".into()),]
        );
    }

    #[test]
    fn quoted_strings_keep_spaces() {
        assert_eq!(
            lex("title ~ \"login timeout\"").unwrap(),
            vec![
                Tok::Ident("title".into()),
                Tok::Tilde,
                Tok::Str("login timeout".into()),
            ]
        );
    }

    #[test]
    fn unterminated_string_is_an_error() {
        assert!(lex("title = \"oops").is_err());
    }

    #[test]
    fn values_with_punctuation_stay_identifiers() {
        assert_eq!(
            lex("id = 01K3M9ZXQ2-R7VN").unwrap(),
            vec![
                Tok::Ident("id".into()),
                Tok::Eq,
                Tok::Ident("01K3M9ZXQ2-R7VN".into()),
            ]
        );
        assert_eq!(
            lex("sprint = 2026-W33").unwrap(),
            vec![
                Tok::Ident("sprint".into()),
                Tok::Eq,
                Tok::Ident("2026-W33".into()),
            ]
        );
    }
}
