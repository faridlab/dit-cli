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
