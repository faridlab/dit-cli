//! The code map's facts (ADR 0025): what one source file defines, imports,
//! re-exports, calls and extends. Derived from the source at reindex, stored in
//! the index, never written to a file (I5).

/// A language the extractor has a grammar for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeLang {
    TypeScript,
    /// TSX; also used for `.js` / `.jsx`, which it parses as a superset.
    Tsx,
    Rust,
    Kotlin,
}

impl CodeLang {
    /// The language a path is written in, by extension; `None` for a file the
    /// extractor has no grammar for (indexed as a file only).
    pub fn of(path: &str) -> Option<CodeLang> {
        let ext = path.rsplit_once('.').map(|(_, e)| e)?;
        match ext {
            "ts" | "mts" | "cts" => Some(CodeLang::TypeScript),
            "tsx" | "js" | "jsx" | "mjs" | "cjs" => Some(CodeLang::Tsx),
            "rs" => Some(CodeLang::Rust),
            "kt" | "kts" => Some(CodeLang::Kotlin),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CodeLang::TypeScript => "typescript",
            CodeLang::Tsx => "tsx",
            CodeLang::Rust => "rust",
            CodeLang::Kotlin => "kotlin",
        }
    }

    pub fn parse(s: &str) -> Option<CodeLang> {
        Some(match s {
            "typescript" => CodeLang::TypeScript,
            "tsx" => CodeLang::Tsx,
            "rust" => CodeLang::Rust,
            "kotlin" => CodeLang::Kotlin,
            _ => return None,
        })
    }
}

/// What a defined symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    Type,
    Enum,
    Const,
    Struct,
    Trait,
    Module,
    Macro,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Type => "type",
            SymbolKind::Enum => "enum",
            SymbolKind::Const => "const",
            SymbolKind::Struct => "struct",
            SymbolKind::Trait => "trait",
            SymbolKind::Module => "module",
            SymbolKind::Macro => "macro",
        }
    }

    pub fn parse(s: &str) -> Option<SymbolKind> {
        Some(match s {
            "function" => SymbolKind::Function,
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "type" => SymbolKind::Type,
            "enum" => SymbolKind::Enum,
            "const" => SymbolKind::Const,
            "struct" => SymbolKind::Struct,
            "trait" => SymbolKind::Trait,
            "module" => SymbolKind::Module,
            "macro" => SymbolKind::Macro,
            _ => return None,
        })
    }
}

/// A top-level symbol a file defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSymbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based.
    pub line: u32,
    /// Exported (`export`, `pub`) — visible to other files.
    pub exported: bool,
}

/// One import — or re-export — as written: the module it names and the
/// symbols it takes. `names` is empty for a side-effect import or a module
/// declaration; `*` stands for a namespace or glob import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeImport {
    /// The specifier as written: `./hooks`, `@/crud/hooks`, `crate::agent`.
    pub specifier: String,
    pub names: Vec<String>,
    /// `export … from` / `pub use`: passes the names through to importers.
    pub reexport: bool,
    pub line: u32,
}

/// A call (or JSX use) of a named function or component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeCall {
    /// The name called: `useResourceList`, `api.get`, `Repo::open`.
    pub callee: String,
    pub line: u32,
}

/// A type relation: `from` extends or implements `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRelation {
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationKind {
    Inherits,
    Implements,
}

/// A string literal shaped like a path — the raw material of the seam link
/// (ADR 0025 milestone 3). Interpolations are written `${name}` when they
/// name a plain identifier, `${}` otherwise, so a constant can be put back
/// in at query time; whether it is an API call is decided against the
/// registered specs then, never here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeString {
    pub text: String,
    pub line: u32,
}

/// A string constant a file defines (`const P = "api/v1/x"`, `const val
/// BASE = "…"`), in the same `${name}` form, so a literal built from it can
/// be read whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeConst {
    pub name: String,
    pub value: String,
}

/// Everything the extractor reads out of one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFacts {
    pub symbols: Vec<CodeSymbol>,
    pub imports: Vec<CodeImport>,
    pub calls: Vec<CodeCall>,
    pub relations: Vec<CodeRelation>,
    /// The package a Kotlin file declares; imports name packages, not paths.
    pub package: Option<String>,
    pub strings: Vec<CodeString>,
    pub consts: Vec<CodeConst>,
    /// Type names a Kotlin file mentions (`x: Account`, `List<Order>`), each
    /// once with its first line: a same-package dependency is often only a
    /// type, never called.
    pub type_refs: Vec<CodeCall>,
}

/// Whether a literal is worth keeping as a possible path: it has a `/`, no
/// whitespace, is not a URL with a scheme, a relative module specifier or a
/// file name, and has at least one segment of letters — or a named
/// placeholder, which a constant may stand behind (`${P}/${id}`).
pub fn path_shaped(text: &str) -> bool {
    text.contains('/')
        && text.len() <= 300
        && !text.chars().any(char::is_whitespace)
        && !text.contains("://")
        && !text.starts_with('.')
        && !text.starts_with('@')
        && !text.starts_with("//")
        && text.split('/').any(|s| {
            let word = s.len() > 1
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            let named = s.len() > 3 && s.starts_with("${") && s.ends_with('}');
            word || named
        })
        && !text
            .rsplit('/')
            .next()
            .and_then(|last| last.rsplit_once('.'))
            .is_some_and(|(_, ext)| {
                matches!(
                    ext,
                    "ts" | "tsx"
                        | "js"
                        | "jsx"
                        | "css"
                        | "scss"
                        | "svg"
                        | "png"
                        | "jpg"
                        | "json"
                        | "md"
                        | "html"
                        | "kt"
                        | "rs"
                        | "yaml"
                        | "yml"
                        | "woff2"
                        | "webp"
                )
            })
}

/// One segment of an API path, from either side of the seam link: a literal
/// in the code or a path in a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiSegment {
    Lit(String),
    /// `{id}` in a spec, `${…}` in code — any one segment.
    Any,
}

/// Split a path into segments: query and fragment dropped, leading and
/// trailing `/` ignored, and any segment holding `${` or `{` a wildcard.
pub fn api_segments(path: &str) -> Vec<ApiSegment> {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.contains('{') {
                ApiSegment::Any
            } else {
                ApiSegment::Lit(s.to_owned())
            }
        })
        .collect()
}

/// How a literal from the code relates to one spec path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFit {
    /// Every segment lines up: this literal calls that operation. `guessed`
    /// counts the segments the spec names outright that the literal only
    /// fills with a value — `${id}` against `/bulk`. The fewer, the closer.
    Calls {
        guessed: usize,
    },
    /// The literal is the leading part of the path — a base that longer
    /// literals are built from, not a call.
    Prefix,
    None,
}

pub fn api_fit(literal: &[ApiSegment], spec: &[ApiSegment]) -> ApiFit {
    if literal.len() > spec.len() {
        return ApiFit::None;
    }
    let lines_up = literal.iter().zip(spec).all(|(a, b)| match (a, b) {
        (ApiSegment::Any, _) | (_, ApiSegment::Any) => true,
        (ApiSegment::Lit(x), ApiSegment::Lit(y)) => x == y,
    });
    let guessed = literal
        .iter()
        .zip(spec)
        .filter(|(a, b)| matches!((a, b), (ApiSegment::Any, ApiSegment::Lit(_))))
        .count();
    match (lines_up, literal.len() == spec.len()) {
        (false, _) => ApiFit::None,
        (true, true) => ApiFit::Calls { guessed },
        (true, false) => ApiFit::Prefix,
    }
}

/// A path in a code map entry: a registered code root and a glob inside it,
/// written `<root>:<glob>` (ADR 0025).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPath {
    pub root: String,
    pub glob: String,
}

impl MapPath {
    /// `webapp:src/resources/*/index.ts` → root `webapp`, glob after the colon.
    pub fn parse(text: &str) -> Option<MapPath> {
        let (root, glob) = text.split_once(':')?;
        let (root, glob) = (root.trim(), glob.trim());
        (!root.is_empty() && !glob.is_empty()).then(|| MapPath {
            root: root.to_owned(),
            glob: glob.to_owned(),
        })
    }

    pub fn written(&self) -> String {
        format!("{}:{}", self.root, self.glob)
    }
}

/// One authored piece of intent: for this task, change here, copy that,
/// never touch those — and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    pub task: String,
    pub change: Vec<MapPath>,
    pub example: Option<MapPath>,
    pub never: Vec<MapPath>,
    pub why: Option<String>,
}

/// A `dit-map` fence: named entries, and the commit of each root they were
/// last confirmed against — the pin `dit code map confirm` moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeMap {
    pub map: String,
    pub entries: Vec<MapEntry>,
    /// `(root, commit)`: confirmed against these commits.
    pub confirmed: Vec<(String, String)>,
}

/// A path glob: `**` spans any number of directories (including none), `*`
/// any run of characters inside one segment, `?` one character. Paths use `/`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn seg(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg(&p[1..], s) || (!s.is_empty() && seg(p, &s[1..])),
            (Some(b'?'), Some(_)) => seg(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => seg(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn parts(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => parts(&p[1..], s) || (!s.is_empty() && parts(p, &s[1..])),
            (Some(pp), Some(ss)) => seg(pp.as_bytes(), ss.as_bytes()) && parts(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.trim_end_matches('/').split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    // A pattern naming a directory (`src/desks/`) covers everything under it.
    if pattern.ends_with('/') {
        return s.len() >= p.len() && parts(&p, &s[..p.len()]);
    }
    parts(&p, &s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn globs_span_directories_segments_and_characters() {
        assert!(glob_match("src/**", "src/a/b/c.ts"));
        assert!(glob_match("src/**/*.ts", "src/c.ts"));
        assert!(glob_match("src/**/*.ts", "src/a/b/c.ts"));
        assert!(!glob_match("src/**/*.ts", "src/a/b/c.tsx"));
        assert!(glob_match(
            "src/resources/*/index.ts",
            "src/resources/product/index.ts"
        ));
        assert!(!glob_match(
            "src/resources/*/index.ts",
            "src/resources/a/b/index.ts"
        ));
        assert!(glob_match("src/generated/**", "src/generated/x/y.ts"));
        assert!(!glob_match("src/generated/**", "src/gen/x.ts"));
        assert!(glob_match(
            "src/desks/",
            "src/desks/people/PayrollRunsPage.tsx"
        ));
        assert!(glob_match(
            "crates/*/src/lib.rs",
            "crates/dit-core/src/lib.rs"
        ));
        assert!(glob_match("a?c.rs", "abc.rs"));
    }

    #[test]
    fn the_language_follows_the_extension() {
        assert_eq!(CodeLang::of("src/a.tsx"), Some(CodeLang::Tsx));
        assert_eq!(CodeLang::of("src/a.js"), Some(CodeLang::Tsx));
        assert_eq!(CodeLang::of("src/a.ts"), Some(CodeLang::TypeScript));
        assert_eq!(CodeLang::of("src/lib.rs"), Some(CodeLang::Rust));
        assert_eq!(CodeLang::of("README.md"), None);
    }

    #[test]
    fn api_literals_line_up_with_spec_paths() {
        let spec = api_segments("/api/v1/performance/cycles/{id}/open");
        assert_eq!(
            api_fit(&api_segments("api/v1/performance/cycles/${id}/open"), &spec),
            ApiFit::Calls { guessed: 0 }
        );
        assert_eq!(
            api_fit(
                &api_segments("/api/v1/performance/cycles/7/open?x=1"),
                &spec
            ),
            ApiFit::Calls { guessed: 0 },
            "a concrete id fills a parameter; the query is not the path"
        );
        assert_eq!(
            api_fit(&api_segments("api/v1/performance"), &spec),
            ApiFit::Prefix
        );
        assert_eq!(
            api_fit(
                &api_segments("api/v1/performance/cycles/${id}/close"),
                &spec
            ),
            ApiFit::None
        );
        assert_eq!(
            api_fit(
                &api_segments("api/v1/performance/cycles/${id}/open/x"),
                &spec
            ),
            ApiFit::None
        );
        assert_eq!(
            api_fit(
                &api_segments("api/v1/items/${id}"),
                &api_segments("/api/v1/items/bulk")
            ),
            ApiFit::Calls { guessed: 1 },
            "a value where the spec names the segment is a weaker fit"
        );
    }

    #[test]
    fn path_shaped_keeps_paths_and_drops_the_rest() {
        for yes in [
            "api/v1/users",
            "/api/v1/x/${id}",
            "${P}/cycles",
            "${P}/${id}",
        ] {
            assert!(path_shaped(yes), "{yes}");
        }
        for no in [
            "./hooks",
            "@/crud/hooks",
            "https://example.com/a/b",
            "dd/MM/yyyy hh",
            "/img/logo.svg",
            "a/b",
            "${}/${}",
            "plain",
        ] {
            assert!(!path_shaped(no), "{no}");
        }
    }
}
