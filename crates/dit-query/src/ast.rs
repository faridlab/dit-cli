//! The DQL AST and the field vocabulary.

/// A query field. Singular is canonical (`assignee`, `label`); the plural
/// forms are accepted as aliases because users type both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Id,
    ShortRef,
    Number,
    Title,
    Kind,
    Status,
    Priority,
    Reporter,
    Assignee,
    Label,
    Epic,
    Estimate,
    Sprint,
    Created,
    Updated,
    Due,
    Body,
}

impl Field {
    /// Resolve a user-typed field name. Case-insensitive.
    pub fn resolve(name: &str) -> Option<Field> {
        let f = match name.to_ascii_lowercase().as_str() {
            "id" => Field::Id,
            "short_ref" | "short" | "key" => Field::ShortRef,
            "number" => Field::Number,
            "title" => Field::Title,
            "type" | "kind" => Field::Kind,
            "status" => Field::Status,
            "priority" => Field::Priority,
            "reporter" => Field::Reporter,
            "assignee" | "assignees" => Field::Assignee,
            "label" | "labels" => Field::Label,
            "epic" => Field::Epic,
            "estimate" => Field::Estimate,
            "sprint" => Field::Sprint,
            "created" => Field::Created,
            "updated" => Field::Updated,
            "due" => Field::Due,
            "body" | "text" => Field::Body,
            _ => return None,
        };
        Some(f)
    }

    pub fn name(self) -> &'static str {
        match self {
            Field::Id => "id",
            Field::ShortRef => "short_ref",
            Field::Number => "number",
            Field::Title => "title",
            Field::Kind => "type",
            Field::Status => "status",
            Field::Priority => "priority",
            Field::Reporter => "reporter",
            Field::Assignee => "assignee",
            Field::Label => "label",
            Field::Epic => "epic",
            Field::Estimate => "estimate",
            Field::Sprint => "sprint",
            Field::Created => "created",
            Field::Updated => "updated",
            Field::Due => "due",
            Field::Body => "body",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    In,
    NotIn,
    /// Full-text match (`~`).
    Match,
}

impl Op {
    pub fn symbol(self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::In => "IN",
            Op::NotIn => "NOT IN",
            Op::Match => "~",
        }
    }
}

/// A literal value. Relative dates stay unresolved in the AST — the clock is
/// injected at compile time, keeping this crate pure.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Str(String),
    Num(f64),
    /// `@me` — the current user's alias, resolved at compile time.
    Me,
    /// A relative date like `-7d`: this many days (negative = past) from now.
    /// Weeks/months/years are normalized to days at parse time.
    RelDays(i64),
    List(Vec<Val>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Cmp { field: Field, op: Op, value: Val },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

/// A parsed query: filter, ordering, limit.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub filter: Option<Expr>,
    pub order: Vec<(Field, Dir)>,
    pub limit: Option<u32>,
}

impl Query {
    /// The empty query matches everything.
    pub fn match_all() -> Query {
        Query {
            filter: None,
            order: vec![],
            limit: None,
        }
    }
}
