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
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CodeLang::TypeScript => "typescript",
            CodeLang::Tsx => "tsx",
            CodeLang::Rust => "rust",
        }
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

/// Everything the extractor reads out of one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFacts {
    pub symbols: Vec<CodeSymbol>,
    pub imports: Vec<CodeImport>,
    pub calls: Vec<CodeCall>,
    pub relations: Vec<CodeRelation>,
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
}
