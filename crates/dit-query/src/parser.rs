//! DQL parser: tokens → AST. Precedence is `AND` over `OR`; parentheses
//! group; `ORDER BY` and `LIMIT` are suffix clauses.

use crate::ast::{Dir, Expr, Field, Op, Query, Val};
use crate::lexer::{lex, Tok};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParseError {
    #[error("DQL could not be read: {0}")]
    Lex(#[from] crate::lexer::LexError),
    #[error("unknown field `{0}` — the available fields are id, short_ref, title, type, status, priority, reporter, assignee, label, epic, estimate, sprint, created, updated, due, body")]
    UnknownField(String),
    #[error("`{op}` needs a value on its right — e.g. `status {op} todo`")]
    MissingValue { op: String },
    #[error("`{op}` does not take a list — drop the parentheses or use IN")]
    UnexpectedList { op: String },
    #[error("IN needs a parenthesized list — e.g. `status IN (todo, in_progress)`")]
    InNeedsList,
    #[error("expected a comparison like `status = todo`, found `{0}`")]
    ExpectedComparison(String),
    #[error("`{0}` cannot appear here — expected a field name or `(`")]
    UnexpectedToken(String),
    #[error("ORDER BY needs at least one field — e.g. `ORDER BY priority DESC`")]
    EmptyOrderBy,
    #[error("LIMIT needs a whole number — e.g. `LIMIT 50`")]
    BadLimit,
    #[error("trailing input after LIMIT: `{0}` — put ORDER BY and LIMIT last, in that order")]
    TrailingAfterLimit(String),
    #[error("the query ended early — {0}")]
    UnexpectedEnd(String),
    #[error("`~` matches text — e.g. `title ~ \"login\"` or `body ~ timeout`")]
    MatchNeedsText,
}

pub fn parse(input: &str) -> Result<Query, ParseError> {
    let toks = lex(input)?;
    let mut p = Parser {
        toks,
        pos: 0,
        limit_seen: false,
    };
    p.query()
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    limit_seen: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// Case-insensitive keyword test on the current token.
    fn at_keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(kw))
    }

    fn describe(&self) -> String {
        match self.peek() {
            Some(t) => format!("{t:?}"),
            None => "end of query".to_owned(),
        }
    }

    fn query(&mut self) -> Result<Query, ParseError> {
        let filter = if self.peek().is_none() {
            None
        } else {
            Some(self.or_expr()?)
        };
        let mut order = vec![];
        let mut limit = None;
        while self.peek().is_some() {
            if self.at_keyword("ORDER") {
                if self.limit_seen {
                    return Err(ParseError::TrailingAfterLimit(self.describe()));
                }
                self.next();
                if !self.at_keyword("BY") {
                    return Err(ParseError::UnexpectedToken(self.describe()));
                }
                self.next();
                order = self.order_list()?;
            } else if self.at_keyword("LIMIT") {
                self.next();
                self.limit_seen = true;
                limit = Some(self.limit_value()?);
            } else {
                return Err(ParseError::UnexpectedToken(self.describe()));
            }
        }
        Ok(Query {
            filter,
            order,
            limit,
        })
    }

    fn order_list(&mut self) -> Result<Vec<(Field, Dir)>, ParseError> {
        let mut out = vec![];
        loop {
            let Some(Tok::Ident(name)) = self.next() else {
                return Err(ParseError::EmptyOrderBy);
            };
            let field = Field::resolve(&name).ok_or(ParseError::UnknownField(name.clone()))?;
            let dir = if self.at_keyword("ASC") {
                self.next();
                Dir::Asc
            } else if self.at_keyword("DESC") {
                self.next();
                Dir::Desc
            } else {
                Dir::Asc
            };
            out.push((field, dir));
            match self.peek() {
                Some(Tok::Comma) => {
                    self.next();
                }
                _ => break,
            }
        }
        if out.is_empty() {
            return Err(ParseError::EmptyOrderBy);
        }
        Ok(out)
    }

    fn limit_value(&mut self) -> Result<u32, ParseError> {
        match self.next() {
            Some(Tok::Num(n)) if n.fract() == 0.0 && n >= 0.0 => Ok(n as u32),
            _ => Err(ParseError::BadLimit),
        }
    }

    fn or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.and_expr()?;
        while self.at_keyword("OR") {
            self.next();
            let right = self.and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.primary()?;
        while self.at_keyword("AND") {
            self.next();
            let right = self.primary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek(), Some(Tok::LParen)) {
            self.next();
            let inner = self.or_expr()?;
            if !matches!(self.peek(), Some(Tok::RParen)) {
                return Err(ParseError::UnexpectedEnd(format!(
                    "expected `)` after the group, found {}",
                    self.describe()
                )));
            }
            self.next();
            return Ok(inner);
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, ParseError> {
        let Some(Tok::Ident(name)) = self.next() else {
            return Err(ParseError::ExpectedComparison(self.describe()));
        };
        let field = Field::resolve(&name).ok_or(ParseError::UnknownField(name.clone()))?;
        let op = match self.next() {
            Some(Tok::Eq) => Op::Eq,
            Some(Tok::Ne) => Op::Ne,
            Some(Tok::Gt) => Op::Gt,
            Some(Tok::Ge) => Op::Ge,
            Some(Tok::Lt) => Op::Lt,
            Some(Tok::Le) => Op::Le,
            Some(Tok::Tilde) => Op::Match,
            Some(Tok::Ident(kw)) if kw.eq_ignore_ascii_case("IN") => Op::In,
            Some(Tok::Ident(kw)) if kw.eq_ignore_ascii_case("NOT") => {
                // NOT must continue with IN.
                match self.next() {
                    Some(Tok::Ident(kw2)) if kw2.eq_ignore_ascii_case("IN") => Op::NotIn,
                    _ => {
                        return Err(ParseError::UnexpectedToken(
                            "NOT — only `NOT IN` exists in DQL".to_owned(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ParseError::ExpectedComparison(format!(
                    "an operator (= != > >= < <= ~ IN) after `{name}`, found {}",
                    self.describe()
                )))
            }
        };
        let value = self.value(op)?;
        Ok(Expr::Cmp { field, op, value })
    }

    fn value(&mut self, op: Op) -> Result<Val, ParseError> {
        let tok = self.next().ok_or(ParseError::UnexpectedEnd(format!(
            "a value after `{}`",
            op.symbol()
        )))?;
        match tok {
            Tok::Str(s) => Ok(Val::Str(s)),
            Tok::Num(n) => Ok(Val::Num(n)),
            Tok::Rel(days) => Ok(Val::RelDays(days)),
            Tok::Me => Ok(Val::Me),
            Tok::Ident(s) => {
                // A bare word is a string value. Keywords can never be values.
                if matches!(
                    s.to_ascii_uppercase().as_str(),
                    "AND" | "OR" | "IN" | "NOT" | "ORDER" | "BY" | "LIMIT" | "ASC" | "DESC"
                ) {
                    return Err(ParseError::MissingValue {
                        op: op.symbol().into(),
                    });
                }
                Ok(Val::Str(s))
            }
            Tok::LParen => {
                // A list — only valid for IN / NOT IN.
                if !matches!(op, Op::In | Op::NotIn) {
                    return Err(ParseError::UnexpectedList {
                        op: op.symbol().into(),
                    });
                }
                let mut items = Vec::new();
                loop {
                    match self.peek() {
                        Some(Tok::RParen) => {
                            self.next();
                            break;
                        }
                        Some(_) => {
                            // Parse one non-list value.
                            let v = self.value(Op::In)?;
                            items.push(v);
                            match self.peek() {
                                Some(Tok::Comma) => {
                                    self.next();
                                }
                                Some(Tok::RParen) => {}
                                _ => {
                                    return Err(ParseError::UnexpectedEnd(
                                        "expected `,` or `)` in the list".into(),
                                    ))
                                }
                            }
                        }
                        None => {
                            return Err(ParseError::UnexpectedEnd(
                                "the list is missing its `)`".into(),
                            ))
                        }
                    }
                }
                Ok(Val::List(items))
            }
            Tok::Tilde => Err(ParseError::MatchNeedsText),
            other => Err(ParseError::ExpectedComparison(format!(
                "`{}` followed by `{other:?}` — that is not a value",
                op.symbol()
            ))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ast::{Field, Op};

    #[test]
    fn parses_the_reference_query() {
        let q = parse(
            "status != done AND assignee = @me AND label IN (auth, api) AND updated > -7d \
             ORDER BY priority DESC, updated DESC LIMIT 20",
        )
        .unwrap();
        assert_eq!(
            q.order,
            vec![(Field::Priority, Dir::Desc), (Field::Updated, Dir::Desc)]
        );
        assert_eq!(q.limit, Some(20));
        // Structure: everything ANDed together.
        let Expr::And(a, b) = q.filter.unwrap() else {
            panic!("expected And at the top");
        };
        let _ = (a, b);
    }

    #[test]
    fn precedence_and_binds_tighter_than_or() {
        let q = parse("status = done OR status = cancelled AND type = bug").unwrap();
        // Must parse as done OR (cancelled AND bug).
        let Expr::Or(left, right) = q.filter.unwrap() else {
            panic!("expected Or at the top");
        };
        assert!(matches!(*left, Expr::Cmp { .. }));
        assert!(matches!(*right, Expr::And(_, _)));
    }

    #[test]
    fn parentheses_override_precedence() {
        let q = parse("(status = done OR status = cancelled) AND type = bug").unwrap();
        let Expr::And(left, _) = q.filter.unwrap() else {
            panic!("expected And at the top");
        };
        assert!(matches!(*left, Expr::Or(_, _)));
    }

    #[test]
    fn empty_query_matches_all() {
        let q = parse("").unwrap();
        assert!(q.filter.is_none());
        assert!(q.order.is_empty());
        assert_eq!(q.limit, None);
    }

    #[test]
    fn not_in_works() {
        let q = parse("status NOT IN (done, cancelled)").unwrap();
        let Expr::Cmp { op, value, .. } = q.filter.unwrap() else {
            panic!()
        };
        assert_eq!(op, Op::NotIn);
        assert_eq!(
            value,
            Val::List(vec![Val::Str("done".into()), Val::Str("cancelled".into())])
        );
    }

    #[test]
    fn quoted_values_with_spaces() {
        let q = parse("title ~ \"login timeout\"").unwrap();
        let Expr::Cmp { op, value, .. } = q.filter.unwrap() else {
            panic!()
        };
        assert_eq!(op, Op::Match);
        assert_eq!(value, Val::Str("login timeout".into()));
    }

    #[test]
    fn unknown_fields_are_rejected_with_the_list() {
        let err = parse("colour = red").unwrap_err();
        assert!(err.to_string().contains("unknown field `colour`"), "{err}");
    }

    #[test]
    fn keywords_cannot_be_values() {
        assert!(parse("status = AND").is_err());
        assert!(parse("status = LIMIT").is_err());
    }

    #[test]
    fn limit_must_be_a_whole_number() {
        assert!(parse("status = todo LIMIT 10.5").is_err());
        assert!(parse("status = todo LIMIT -3").is_err());
    }

    #[test]
    fn ulid_like_values_lex_as_one_token() {
        let q = parse("id = 01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap();
        let Expr::Cmp { value, .. } = q.filter.unwrap() else {
            panic!()
        };
        assert_eq!(value, Val::Str("01K3M9ZXQ2R7VN8P4TDBCEFGHJ".into()));
    }

    #[test]
    fn order_by_defaults_to_asc() {
        let q = parse("status = todo ORDER BY created").unwrap();
        assert_eq!(q.order, vec![(Field::Created, Dir::Asc)]);
    }

    #[test]
    fn dangling_operator_is_a_clear_error() {
        let err = parse("status =").unwrap_err();
        assert!(err.to_string().contains("ended early"), "{err}");
    }
}
