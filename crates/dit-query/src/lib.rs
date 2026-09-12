//! DQL — the query language for issue lists.
//!
//! Three stages, each pure and testable without a database:
//! lex → parse → compile. The output of `compile` is a WHERE fragment plus
//! bound parameters; only `dit-index` ever executes it, so there is exactly
//! one query semantics (SQLite's) instead of a second, drifting in-memory
//! evaluator. A WASM build of this crate lets the editor validate queries as
//! the user types them without shipping SQLite to the browser.

mod ast;
mod compile;
mod lexer;
mod parser;

pub use ast::{Dir, Expr, Field, Op, Query, Val};
pub use compile::{compile, CompileError, Compiled, SqlVal};
pub use lexer::{lex, LexError, Tok};
pub use parser::{parse, ParseError};

/// Parse and compile in one step — what callers that only have a string want.
pub fn compile_str(
    input: &str,
    me: Option<&str>,
    now: time::OffsetDateTime,
) -> Result<Compiled, QueryError> {
    let q = parse(input)?;
    Ok(compile(&q, me, now)?)
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Compile(#[from] CompileError),
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod start_field_tests {
    use super::*;

    #[test]
    fn start_is_a_date_field_like_due() {
        // A Gantt filter is `start <= +7d`, so the field has to resolve
        // relative dates against the injected clock the way `due` does —
        // not compare the literal text the user typed.
        let q = parse("start <= +7d").expect("start parses");
        let compiled = compile(
            &q,
            None,
            time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
        )
        .expect("start compiles");
        assert!(
            compiled.where_sql.contains("issues.start"),
            "{}",
            compiled.where_sql
        );
        match compiled.params.as_slice() {
            [SqlVal::Text(resolved)] => {
                assert!(resolved.starts_with("2027-01-22"), "resolved to {resolved}");
            }
            other => panic!("expected one resolved date, got {other:?}"),
        }
    }

    #[test]
    fn start_orders_like_a_date() {
        let q = parse("status != done ORDER BY start ASC").expect("parses");
        let compiled = compile(
            &q,
            None,
            time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
        )
        .expect("compiles");
        assert!(
            compiled.order_sql.contains("issues.start"),
            "{}",
            compiled.order_sql
        );
    }
}
