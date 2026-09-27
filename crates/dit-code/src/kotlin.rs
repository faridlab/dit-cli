//! Kotlin extraction. Imports name packages and declarations, not files, so
//! the file's own `package` is read too: resolution needs it to find which
//! file an import means, and which files share a package without importing.

use dit_model::{
    path_shaped, CodeCall, CodeConst, CodeImport, CodeRelation, CodeString, CodeSymbol, FileFacts,
    RelationKind, SymbolKind,
};
use tree_sitter::{Node, Parser};

pub(crate) fn extract(text: &str) -> FileFacts {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .is_err()
    {
        return FileFacts::default();
    }
    let Some(tree) = parser.parse(text, None) else {
        return FileFacts::default();
    };
    let src = text.as_bytes();
    let mut facts = FileFacts::default();
    let root = tree.root_node();
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        top_level(node, src, &mut facts);
    }
    collect_calls(root, src, &mut facts.calls);
    collect_strings(root, src, &mut facts.strings);
    collect_type_refs(root, src, &mut facts.type_refs);
    facts
}

fn collect_type_refs(node: Node<'_>, src: &[u8], out: &mut Vec<CodeCall>) {
    if node.kind() == "user_type" {
        if let Some(head) = node.named_child(0).filter(|h| h.kind() == "identifier") {
            let name = text(head, src);
            if !out.iter().any(|c| c.callee == name) {
                out.push(CodeCall {
                    callee: name.to_owned(),
                    line: line(node),
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_type_refs(child, src, out);
    }
}

fn text<'a>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or_default()
}

fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

/// Kotlin is public by default; only `private` hides a declaration from
/// other files (`internal` is still visible across the module).
fn is_exported(node: Node<'_>, src: &[u8]) -> bool {
    let mut cursor = node.walk();
    let private = node
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "modifiers")
        .any(|m| {
            let mut c2 = m.walk();
            let hidden = m
                .named_children(&mut c2)
                .any(|v| v.kind() == "visibility_modifier" && text(v, src) == "private");
            hidden
        });
    !private
}

fn has_modifier(node: Node<'_>, src: &[u8], word: &str) -> bool {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "modifiers")
        .any(|m| text(m, src).split_whitespace().any(|w| w == word));
    found
}

fn has_token(node: Node<'_>, kind: &str) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == kind);
    found
}

fn name_of<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name("name").map(|n| text(n, src))
}

fn top_level(node: Node<'_>, src: &[u8], facts: &mut FileFacts) {
    match node.kind() {
        "package_header" => {
            let mut cursor = node.walk();
            let qualified = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "qualified_identifier" || c.kind() == "identifier");
            facts.package = qualified.map(|q| text(q, src).to_owned());
        }
        "import" => import(node, src, facts),
        "class_declaration" | "object_declaration" => class(node, src, facts),
        "function_declaration" => {
            if let Some(name) = name_of(node, src) {
                facts.symbols.push(CodeSymbol {
                    name: name.to_owned(),
                    kind: SymbolKind::Function,
                    line: line(node),
                    exported: is_exported(node, src),
                });
            }
        }
        "property_declaration" => property(node, src, facts, None),
        "type_alias" => {
            let mut cursor = node.walk();
            let name = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "identifier");
            if let Some(name) = name {
                facts.symbols.push(CodeSymbol {
                    name: text(name, src).to_owned(),
                    kind: SymbolKind::Type,
                    line: line(node),
                    exported: is_exported(node, src),
                });
            }
        }
        _ => {}
    }
}

/// `import a.b.C`, `import a.b.C as D`, `import a.b.*`. The specifier is the
/// qualified name as written; the name taken is its last segment — an alias
/// renames it locally, but users are found by what the target defines.
fn import(node: Node<'_>, src: &[u8], facts: &mut FileFacts) {
    let mut cursor = node.walk();
    let Some(qualified) = node
        .named_children(&mut cursor)
        .find(|c| c.kind() == "qualified_identifier" || c.kind() == "identifier")
    else {
        return;
    };
    let path = text(qualified, src).to_owned();
    let wildcard = text(node, src).trim_end().ends_with('*');
    let (specifier, name) = if wildcard {
        (format!("{path}.*"), "*".to_owned())
    } else {
        let last = path.rsplit('.').next().unwrap_or(&path).to_owned();
        (path, last)
    };
    facts.imports.push(CodeImport {
        specifier,
        names: vec![name],
        reexport: false,
        line: line(node),
    });
}

/// A type name without its package or generic arguments.
fn type_name(node: Node<'_>, src: &[u8]) -> String {
    let t = text(node, src);
    let t = t.split('<').next().unwrap_or(t);
    t.rsplit('.').next().unwrap_or(t).trim().to_owned()
}

fn class(node: Node<'_>, src: &[u8], facts: &mut FileFacts) {
    let Some(name) = name_of(node, src) else {
        return;
    };
    let interface = has_token(node, "interface");
    let kind = if interface {
        SymbolKind::Interface
    } else if has_modifier(node, src, "enum") {
        SymbolKind::Enum
    } else {
        SymbolKind::Class
    };
    let exported = is_exported(node, src);
    facts.symbols.push(CodeSymbol {
        name: name.to_owned(),
        kind,
        line: line(node),
        exported,
    });
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "delegation_specifiers" => {
                let mut c2 = child.walk();
                for spec in child.named_children(&mut c2) {
                    let mut c3 = spec.walk();
                    let Some(inner) = spec.named_children(&mut c3).next() else {
                        continue;
                    };
                    // `: Base(…)` calls a superclass constructor; a bare type
                    // is an interface — except on an interface, which can
                    // only extend other interfaces.
                    let (target, relation) = match inner.kind() {
                        "constructor_invocation" => {
                            let mut c4 = inner.walk();
                            let ty = inner
                                .named_children(&mut c4)
                                .find(|c| c.kind() == "user_type");
                            (ty, RelationKind::Inherits)
                        }
                        "user_type" if interface => (Some(inner), RelationKind::Inherits),
                        "user_type" => (Some(inner), RelationKind::Implements),
                        "explicit_delegation" => {
                            let mut c4 = inner.walk();
                            let ty = inner
                                .named_children(&mut c4)
                                .find(|c| c.kind() == "user_type");
                            (ty, RelationKind::Implements)
                        }
                        _ => (None, RelationKind::Implements),
                    };
                    if let Some(ty) = target {
                        facts.relations.push(CodeRelation {
                            from: name.to_owned(),
                            to: type_name(ty, src),
                            kind: relation,
                        });
                    }
                }
            }
            "class_body" | "enum_class_body" => members(child, src, name, exported, facts),
            _ => {}
        }
    }
}

/// Methods as `Type::method`, like Rust's; a companion object's members
/// belong to the type that holds it.
fn members(body: Node<'_>, src: &[u8], owner: &str, exported: bool, facts: &mut FileFacts) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "function_declaration" => {
                if let Some(name) = name_of(member, src) {
                    facts.symbols.push(CodeSymbol {
                        name: format!("{owner}::{name}"),
                        kind: SymbolKind::Function,
                        line: line(member),
                        exported: exported && is_exported(member, src),
                    });
                }
            }
            "companion_object" => {
                let mut c2 = member.walk();
                let inner = member
                    .named_children(&mut c2)
                    .find(|c| c.kind() == "class_body");
                if let Some(inner) = inner {
                    members(inner, src, owner, exported, facts);
                }
            }
            "property_declaration" => property(member, src, facts, Some(owner)),
            _ => {}
        }
    }
}

/// A property. Top-level ones are symbols; a string value at any level is
/// kept as a constant, so `"$BASE/users"` can be read whole.
fn property(node: Node<'_>, src: &[u8], facts: &mut FileFacts, owner: Option<&str>) {
    let mut cursor = node.walk();
    let mut name = None;
    let mut value = None;
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "variable_declaration" => {
                let mut c2 = child.walk();
                name = child
                    .named_children(&mut c2)
                    .find(|c| c.kind() == "identifier")
                    .map(|n| text(n, src).to_owned());
            }
            "string_literal" => value = Some(child),
            _ => {}
        }
    }
    let Some(name) = name else {
        return;
    };
    if owner.is_none() {
        facts.symbols.push(CodeSymbol {
            name: name.clone(),
            kind: SymbolKind::Const,
            line: line(node),
            exported: is_exported(node, src),
        });
    }
    if let Some(value) = value.and_then(|v| template(v, src)) {
        facts.consts.push(CodeConst { name, value });
    }
}

fn collect_calls(node: Node<'_>, src: &[u8], out: &mut Vec<CodeCall>) {
    if node.kind() == "call_expression" {
        let callee = node.named_child(0);
        if let Some(callee) = callee {
            if let Some(name) = callee_name(callee, src) {
                out.push(CodeCall {
                    callee: name,
                    line: line(node),
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_calls(child, src, out);
    }
}

/// `helper` → `helper`; `Other.make` → `Other.make`; anything chained through
/// another call (`a.b().c`) → its last name.
fn callee_name(node: Node<'_>, src: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, src).to_owned()),
        "navigation_expression" => {
            let t = text(node, src);
            if t.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                Some(t.to_owned())
            } else {
                let mut cursor = node.walk();
                let last = node
                    .named_children(&mut cursor)
                    .filter(|c| c.kind() == "identifier")
                    .last();
                last.map(|l| text(l, src).to_owned())
            }
        }
        _ => None,
    }
}

fn collect_strings(node: Node<'_>, src: &[u8], out: &mut Vec<CodeString>) {
    if node.kind() == "string_literal" {
        if let Some(t) = template(node, src).filter(|t| path_shaped(t)) {
            out.push(CodeString {
                text: t,
                line: line(node),
            });
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_strings(child, src, out);
    }
}

/// A one-line string literal in the `${name}` form; `None` for a raw
/// (`"""`) string, which is prose or a query, not a path.
fn template(node: Node<'_>, src: &[u8]) -> Option<String> {
    let raw = text(node, src);
    if raw.starts_with("\"\"\"") {
        return None;
    }
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    Some(normalise_template(inner))
}

/// Rewrite Kotlin's `$name` and `${expr}` into `${name}` / `${}`.
pub(crate) fn normalise_template(inner: &str) -> String {
    let chars: Vec<char> = inner.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '$' if chars.get(i + 1) == Some(&'{') => {
                let mut depth = 0;
                let mut j = i + 1;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let expr: String = chars[i + 2..j.min(chars.len())].iter().collect();
                out.push_str(&placeholder(expr.trim()));
                i = j + 1;
            }
            '$' if chars
                .get(i + 1)
                .is_some_and(|c| c.is_alphabetic() || *c == '_') =>
            {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();
                out.push_str(&placeholder(&name));
                i = j;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// `${name}` for a plain or dotted identifier (`BASE`, `Routes.BASE`) — a
/// constant may stand behind it — and `${}` for anything else.
pub(crate) fn placeholder(expr: &str) -> String {
    let plain = !expr.is_empty()
        && expr
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        && expr
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !expr.ends_with('.')
        && !expr.contains("..");
    if plain {
        format!("${{{expr}}}")
    } else {
        "${}".to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"package com.acme.app.ui

import com.acme.app.data.Repo
import com.acme.app.data.*
import kotlinx.coroutines.launch as go

private const val BASE = "api/v1/users"

typealias Id = String

interface Screen : Base2 {
    fun render()
}

open class Base(val x: Int)

data class UserScreen(private val repo: Repo) : Base(1), Screen {
    override fun render() {
        val u = repo.load("$BASE/${id}/profile")
        go { helper(u) }
        Other.make()
    }

    private fun hidden() {}

    companion object {
        fun create() = UserScreen(Repo())
    }
}

object Registry {
    val all = listOf<Screen>()
}

enum class Kind { A, B }

fun helper(x: Any) = println(x)

fun String.ext(): Int = 1
"#;

    #[test]
    fn package_imports_and_declarations_are_read() {
        let f = extract(SAMPLE);
        assert_eq!(f.package.as_deref(), Some("com.acme.app.ui"));
        let imports: Vec<(&str, &str)> = f
            .imports
            .iter()
            .map(|i| (i.specifier.as_str(), i.names[0].as_str()))
            .collect();
        assert_eq!(
            imports,
            [
                ("com.acme.app.data.Repo", "Repo"),
                ("com.acme.app.data.*", "*"),
                ("kotlinx.coroutines.launch", "launch"),
            ]
        );
        let sym = |n: &str| f.symbols.iter().find(|s| s.name == n);
        assert_eq!(sym("Screen").unwrap().kind, SymbolKind::Interface);
        assert_eq!(sym("Kind").unwrap().kind, SymbolKind::Enum);
        assert_eq!(sym("Registry").unwrap().kind, SymbolKind::Class);
        assert_eq!(sym("Id").unwrap().kind, SymbolKind::Type);
        assert!(!sym("BASE").unwrap().exported);
        assert!(sym("UserScreen::render").is_some());
        assert!(sym("UserScreen::create").is_some(), "companion members");
        assert!(!sym("UserScreen::hidden").unwrap().exported);
        assert!(sym("ext").is_some(), "extension functions by name");
        assert!(sym("helper").unwrap().exported);
    }

    #[test]
    fn heritage_calls_and_strings_are_read() {
        let f = extract(SAMPLE);
        let rel: Vec<(&str, &str, RelationKind)> = f
            .relations
            .iter()
            .map(|r| (r.from.as_str(), r.to.as_str(), r.kind))
            .collect();
        assert!(rel.contains(&("UserScreen", "Base", RelationKind::Inherits)));
        assert!(rel.contains(&("UserScreen", "Screen", RelationKind::Implements)));
        assert!(rel.contains(&("Screen", "Base2", RelationKind::Inherits)));
        let calls: Vec<&str> = f.calls.iter().map(|c| c.callee.as_str()).collect();
        for c in ["repo.load", "helper", "Other.make", "UserScreen", "Repo"] {
            assert!(calls.contains(&c), "missing call {c}: {calls:?}");
        }
        let strings: Vec<&str> = f.strings.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(strings, ["api/v1/users", "${BASE}/${id}/profile"]);
        let types: Vec<&str> = f.type_refs.iter().map(|c| c.callee.as_str()).collect();
        for t in ["Repo", "String", "Screen", "Any"] {
            assert!(types.contains(&t), "missing type {t}: {types:?}");
        }
        assert_eq!(
            f.consts,
            [CodeConst {
                name: "BASE".into(),
                value: "api/v1/users".into()
            }]
        );
    }

    #[test]
    fn templates_are_normalised() {
        assert_eq!(normalise_template("$a/b/${c.d}/${e}"), "${a}/b/${c.d}/${e}");
        assert_eq!(normalise_template("${f(x)}/${a + b}"), "${}/${}");
        assert_eq!(normalise_template("\\$x/y"), "$x/y");
        assert_eq!(normalise_template("cost $5"), "cost $5");
    }
}
