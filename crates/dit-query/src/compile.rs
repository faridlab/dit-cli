//! DQL → SQL compilation for the `issues` table in the index.
//!
//! Every value becomes a bound parameter — never string-interpolated — so a
//! hostile query (`title = "x\"; DROP TABLE issues"`) is just a weird string.
//! Field names are validated against the fixed vocabulary at parse time and
//! mapped here to a closed set of column expressions; there is no path from
//! user text to SQL structure.
//!
//! This crate compiles; only `dit-index` executes. There is deliberately no
//! in-memory evaluator (one semantics, one engine).

use std::fmt::Write as _;

use time::OffsetDateTime;

use crate::ast::{Dir, Expr, Field, Op, Query, Val};

/// A bound parameter value. Mirrors what rusqlite accepts, without depending
/// on rusqlite (this crate must compile to wasm32).
#[derive(Debug, Clone, PartialEq)]
pub enum SqlVal {
    Text(String),
    Real(f64),
    Int(i64),
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CompileError {
    #[error(
        "`@me` needs to know who you are — pass --me or set your alias in people/<alias>.yaml"
    )]
    WhoIsMe,
    #[error("`{field}` only compares with {expected} — e.g. `{example}`")]
    TypeMismatch {
        field: &'static str,
        expected: &'static str,
        example: &'static str,
    },
}

/// A compiled query: a WHERE fragment over the `issues` table plus the
/// parameters to bind, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub where_sql: String,
    pub params: Vec<SqlVal>,
    pub order_sql: String,
    pub limit: Option<u32>,
}

/// Compile with a known user and clock. Both are injected: the crate is pure.
pub fn compile(
    query: &Query,
    me: Option<&str>,
    now: OffsetDateTime,
) -> Result<Compiled, CompileError> {
    let mut c = Compiler {
        params: Vec::new(),
        me,
        now,
    };
    let where_sql = match &query.filter {
        None => "1=1".to_owned(),
        Some(e) => c.expr(e)?,
    };
    let mut order_sql = String::new();
    for (i, (field, dir)) in query.order.iter().enumerate() {
        if i > 0 {
            order_sql.push_str(", ");
        }
        let dir_sql = match dir {
            Dir::Asc => "ASC",
            Dir::Desc => "DESC",
        };
        let _ = write!(order_sql, "{} {}", column(field), dir_sql);
    }
    Ok(Compiled {
        where_sql,
        params: c.params,
        order_sql,
        limit: query.limit,
    })
}

struct Compiler<'a> {
    params: Vec<SqlVal>,
    me: Option<&'a str>,
    now: OffsetDateTime,
}

impl<'a> Compiler<'a> {
    fn expr(&mut self, e: &Expr) -> Result<String, CompileError> {
        match e {
            Expr::And(l, r) => {
                let ls = self.expr(l)?;
                let rs = self.expr(r)?;
                Ok(format!("({ls} AND {rs})"))
            }
            Expr::Or(l, r) => {
                let ls = self.expr(l)?;
                let rs = self.expr(r)?;
                Ok(format!("({ls} OR {rs})"))
            }
            Expr::Cmp { field, op, value } => self.cmp(*field, *op, value),
        }
    }

    fn cmp(&mut self, field: Field, op: Op, value: &Val) -> Result<String, CompileError> {
        // Full-text match: FTS over title+body, regardless of which of the
        // two the user named — searching "login" should find it anywhere.
        if op == Op::Match {
            let text = match value {
                Val::Str(s) => s.clone(),
                _ => {
                    return Err(CompileError::TypeMismatch {
                        field: field.name(),
                        expected: "text",
                        example: "title ~ \"login\"",
                    })
                }
            };
            let idx = self.push_param(SqlVal::Text(fts_phrase(&text)));
            return Ok(format!(
                "issues.rowid IN (SELECT rowid FROM issues_fts WHERE issues_fts MATCH ?{idx})"
            ));
        }
        match field {
            Field::Assignee => return self.set_membership("issue_assignees", "alias", op, value),
            Field::Label => return self.set_membership("issue_labels", "label", op, value),
            Field::Created | Field::Updated | Field::Due => return self.date_cmp(field, op, value),
            _ => {}
        }
        // Plain column comparison.
        let col = column(&field);
        match (op, value) {
            (Op::In | Op::NotIn, Val::List(items)) => {
                if items.is_empty() {
                    // `IN ()` is a syntax error in SQLite; an empty list
                    // matches nothing, its negation matches everything
                    // (modulo NULLs).
                    return Ok(if matches!(op, Op::In) {
                        "0".to_owned()
                    } else {
                        "1".to_owned()
                    });
                }
                let mut parts = Vec::new();
                for item in items {
                    let v = self.scalar(item)?;
                    let idx = self.push_param(v);
                    parts.push(format!("?{idx}"));
                }
                let not = if matches!(op, Op::NotIn) { "NOT " } else { "" };
                Ok(format!("{col} {not}IN ({})", parts.join(", ")))
            }
            (_, Val::List(_)) => Err(CompileError::TypeMismatch {
                field: field.name(),
                expected: "a single value or IN (...)",
                example: "status = todo",
            }),
            (_, v) => {
                let sv = self.scalar(v)?;
                let idx = self.push_param(sv);
                Ok(format!("{col} {} ?{idx}", sql_op(op)))
            }
        }
    }

    /// `assignee`/`label` live in side tables: compile to EXISTS subqueries.
    fn set_membership(
        &mut self,
        table: &str,
        col: &str,
        op: Op,
        value: &Val,
    ) -> Result<String, CompileError> {
        // Rendered as SQL text, never as a boolean — a bool interpolated into
        // the format string prints `false`/`true` and corrupts the SQL.
        let not = if matches!(op, Op::NotIn | Op::Ne) {
            "NOT "
        } else {
            ""
        };
        match value {
            Val::List(items) if matches!(op, Op::In | Op::NotIn) => {
                let mut parts = Vec::new();
                for item in items {
                    let v = self.scalar(item)?;
                    let idx = self.push_param(v);
                    parts.push(format!("?{idx}"));
                }
                Ok(format!(
                    "{not}EXISTS (SELECT 1 FROM {table} t WHERE t.issue_id = issues.id AND t.{col} IN ({}))",
                    parts.join(", ")
                ))
            }
            Val::List(_) => Err(CompileError::TypeMismatch {
                field: "assignee/label",
                expected: "a single value or IN (...)",
                example: "assignee = @me",
            }),
            v => {
                let sv = self.scalar(v)?;
                let idx = self.push_param(sv);
                Ok(format!(
                    "{not}EXISTS (SELECT 1 FROM {table} t WHERE t.issue_id = issues.id AND t.{col} = ?{idx})"
                ))
            }
        }
    }

    /// created/updated/due: relative dates resolve against the injected clock
    /// and compare as canonical RFC3339 text. Lexicographic order equals
    /// chronological order because DIT writes timestamps in one fixed
    /// UTC second-precision format.
    fn date_cmp(&mut self, field: Field, op: Op, value: &Val) -> Result<String, CompileError> {
        let days = match value {
            Val::RelDays(days) => *days,
            _ => {
                return Err(CompileError::TypeMismatch {
                    field: field.name(),
                    expected: "a relative date like -7d",
                    example: "updated > -7d",
                })
            }
        };
        let cutoff = self.now + time::Duration::days(days);
        let text = dit_model::format_rfc3339(cutoff);
        let col = column(&field);
        let idx = self.push_param(SqlVal::Text(text));
        Ok(format!("{col} {} ?{idx}", sql_op(op)))
    }

    fn scalar(&mut self, v: &Val) -> Result<SqlVal, CompileError> {
        match v {
            Val::Str(s) => Ok(SqlVal::Text(s.clone())),
            Val::Num(n) => Ok(SqlVal::Real(*n)),
            Val::RelDays(days) => {
                let t = self.now + time::Duration::days(*days);
                Ok(SqlVal::Text(dit_model::format_rfc3339(t)))
            }
            Val::Me => match self.me {
                Some(m) => Ok(SqlVal::Text(m.to_owned())),
                None => Err(CompileError::WhoIsMe),
            },
            Val::List(_) => Err(CompileError::TypeMismatch {
                field: "this one",
                expected: "a single value",
                example: "status = todo",
            }),
        }
    }

    /// Parameters are positional; the returned index is 1-based, appended in
    /// binding order.
    fn push_param(&mut self, v: SqlVal) -> usize {
        self.params.push(v);
        self.params.len()
    }
}

fn sql_op(op: Op) -> &'static str {
    match op {
        Op::Eq => "=",
        Op::Ne => "!=",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::In | Op::NotIn => "IN",
        Op::Match => "MATCH",
    }
}

/// Map a field to its column expression in the `issues` table.
fn column(field: &Field) -> &'static str {
    match field {
        Field::Id => "issues.id",
        Field::ShortRef => "issues.short_ref",
        Field::Title => "issues.title",
        Field::Kind => "issues.type",
        Field::Status => "issues.status",
        Field::Priority => "issues.priority",
        Field::Reporter => "issues.reporter",
        Field::Assignee | Field::Label => "issues.id", // handled via EXISTS
        Field::Epic => "issues.epic",
        Field::Estimate => "issues.estimate",
        Field::Sprint => "issues.sprint",
        Field::Created => "issues.created",
        Field::Updated => "issues.updated",
        Field::Due => "issues.due",
        Field::Body => "issues.body",
    }
}

/// Make an FTS5-safe phrase: wrap in double quotes and escape internal
/// quotes, so `"` or `NEAR` in user text can never become FTS syntax.
fn fts_phrase(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse(
            "2026-08-17T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    }

    fn compile_str(dql: &str) -> Compiled {
        let q = parse(dql).unwrap();
        compile(&q, Some("farid"), now()).unwrap()
    }

    #[test]
    fn values_are_bound_never_interpolated() {
        let c = compile_str("title = \"x'; DROP TABLE issues; --\"");
        assert_eq!(
            c.where_sql, "issues.title = ?1",
            "structure must not depend on the value"
        );
        assert_eq!(c.params.len(), 1);
    }

    #[test]
    fn reference_query_compiles() {
        let c = compile_str(
            "status != done AND assignee = @me AND label IN (auth, api) AND updated > -7d \
             ORDER BY priority DESC, updated DESC LIMIT 20",
        );
        assert_eq!(c.limit, Some(20));
        assert!(c.where_sql.contains("issues.status != ?1"));
        assert!(c
            .where_sql
            .contains("EXISTS (SELECT 1 FROM issue_assignees"));
        assert!(c.where_sql.contains("EXISTS (SELECT 1 FROM issue_labels"));
        // -7d from 2026-08-17 → 2026-08-10.
        assert!(
            c.params
                .contains(&SqlVal::Text("2026-08-10T12:00:00Z".into())),
            "{:?}",
            c.params
        );
        assert_eq!(c.order_sql, "issues.priority DESC, issues.updated DESC");
    }

    #[test]
    fn set_membership_renders_valid_sql_for_both_polarities() {
        // Both lines execute against a real SQLite in dit-index; here the
        // exact spelling is pinned so a bool-to-string regression (`false`
        // interpolated into SQL) cannot pass a `.contains` check again.
        let positive = compile_str("assignee = budi");
        assert_eq!(
            positive.where_sql,
            "EXISTS (SELECT 1 FROM issue_assignees t WHERE t.issue_id = issues.id AND t.alias = ?1)"
        );
        let negative = compile_str("label != auth");
        assert_eq!(
            negative.where_sql,
            "NOT EXISTS (SELECT 1 FROM issue_labels t WHERE t.issue_id = issues.id AND t.label = ?1)"
        );
    }

    #[test]
    fn at_me_without_a_user_is_an_actionable_error() {
        let q = parse("assignee = @me").unwrap();
        let err = compile(&q, None, now()).unwrap_err();
        assert!(err.to_string().contains("who you are"), "{err}");
    }

    #[test]
    fn empty_query_is_trivially_true() {
        let c = compile_str("");
        assert_eq!(c.where_sql, "1=1");
        assert!(c.params.is_empty());
    }

    #[test]
    fn fts_match_wraps_in_quotes() {
        let c = compile_str("title ~ login");
        assert!(
            c.where_sql.contains("issues_fts MATCH ?1"),
            "{}",
            c.where_sql
        );
        assert_eq!(c.params, vec![SqlVal::Text("\"login\"".into())]);
    }

    #[test]
    fn fts_match_joins_on_rowid_not_id() {
        // The FTS table is external-content keyed by the issues table's
        // rowid. `id` is a ULID string, so joining on it would silently match
        // nothing.
        let c = compile_str("title ~ login");
        assert_eq!(
            c.where_sql,
            "issues.rowid IN (SELECT rowid FROM issues_fts WHERE issues_fts MATCH ?1)"
        );
    }

    #[test]
    fn fts_user_text_cannot_become_fts_syntax() {
        // The phrase contains FTS operators; quoting must neutralize them.
        let c = compile_str("title ~ \"login OR NEAR(a b) AND *\"");
        assert_eq!(
            c.params,
            vec![SqlVal::Text("\"login OR NEAR(a b) AND *\"".into())]
        );
    }

    #[test]
    fn in_list_binds_each_element() {
        let c = compile_str("status IN (todo, in_progress)");
        assert!(c.where_sql.contains("issues.status IN (?1, ?2)"));
        assert_eq!(
            c.params,
            vec![
                SqlVal::Text("todo".into()),
                SqlVal::Text("in_progress".into())
            ]
        );
    }

    #[test]
    fn empty_in_list_matches_nothing() {
        let c = compile_str("status IN ()");
        assert_eq!(c.where_sql, "0");
    }

    #[test]
    fn numbers_bind_for_estimate() {
        let c = compile_str("estimate >= 3");
        assert_eq!(c.where_sql, "issues.estimate >= ?1");
        assert_eq!(c.params, vec![SqlVal::Real(3.0)]);
    }

    #[test]
    fn and_or_compile_with_parentheses() {
        let c = compile_str("status = done OR status = cancelled AND type = bug");
        assert_eq!(
            c.where_sql,
            "(issues.status = ?1 OR (issues.status = ?2 AND issues.type = ?3))"
        );
    }

    #[test]
    fn dates_only_accept_relative_values() {
        let err = {
            let q = parse("updated > 2026-01-01").unwrap();
            compile(&q, Some("farid"), now()).unwrap_err()
        };
        assert!(err.to_string().contains("relative date"), "{err}");
    }
}
