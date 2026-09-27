//! Rust extraction.

use dit_model::{
    CodeCall, CodeImport, CodeRelation, CodeSymbol, FileFacts, RelationKind, SymbolKind,
};
use tree_sitter::{Node, Parser};

pub(crate) fn extract(text: &str) -> FileFacts {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
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
        item(node, src, &mut facts);
    }
    collect_calls(root, src, &mut facts.calls);
    facts
}

fn text<'a>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or_default()
}

fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

fn is_pub(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|c| c.kind() == "visibility_modifier");
    found
}

fn named(node: Node<'_>, src: &[u8], kind: SymbolKind, facts: &mut FileFacts) {
    if let Some(name) = node.child_by_field_name("name") {
        facts.symbols.push(CodeSymbol {
            name: text(name, src).to_owned(),
            kind,
            line: line(node),
            exported: is_pub(node),
        });
    }
}

fn item(node: Node<'_>, src: &[u8], facts: &mut FileFacts) {
    match node.kind() {
        "use_declaration" => {
            if let Some(arg) = node.child_by_field_name("argument") {
                let mut pairs = Vec::new();
                flatten_use(arg, src, "", &mut pairs);
                let reexport = is_pub(node);
                let mut grouped: Vec<CodeImport> = Vec::new();
                for (module, name) in pairs {
                    match grouped.iter_mut().find(|i| i.specifier == module) {
                        Some(existing) => {
                            if let Some(n) = name {
                                existing.names.push(n);
                            }
                        }
                        None => grouped.push(CodeImport {
                            specifier: module,
                            names: name.into_iter().collect(),
                            reexport,
                            line: line(node),
                        }),
                    }
                }
                facts.imports.extend(grouped);
            }
        }
        "mod_item" => {
            named(node, src, SymbolKind::Module, facts);
            // `mod x;` names another file; `mod x { … }` is inline.
            if node.child_by_field_name("body").is_none() {
                if let Some(name) = node.child_by_field_name("name") {
                    facts.imports.push(CodeImport {
                        specifier: format!("self::{}", text(name, src)),
                        names: Vec::new(),
                        reexport: false,
                        line: line(node),
                    });
                }
            }
        }
        "function_item" => named(node, src, SymbolKind::Function, facts),
        "struct_item" | "union_item" => named(node, src, SymbolKind::Struct, facts),
        "enum_item" => named(node, src, SymbolKind::Enum, facts),
        "trait_item" => named(node, src, SymbolKind::Trait, facts),
        "const_item" | "static_item" => named(node, src, SymbolKind::Const, facts),
        "type_item" => named(node, src, SymbolKind::Type, facts),
        "macro_definition" => named(node, src, SymbolKind::Macro, facts),
        "impl_item" => impl_item(node, src, facts),
        _ => {}
    }
}

/// The type name an `impl` is for, without generics or path.
fn type_name(node: Node<'_>, src: &[u8]) -> String {
    let t = text(node, src);
    let t = t.split('<').next().unwrap_or(t);
    t.rsplit("::").next().unwrap_or(t).trim().to_owned()
}

fn impl_item(node: Node<'_>, src: &[u8], facts: &mut FileFacts) {
    let Some(ty) = node.child_by_field_name("type") else {
        return;
    };
    let ty = type_name(ty, src);
    if let Some(tr) = node.child_by_field_name("trait") {
        facts.relations.push(CodeRelation {
            from: ty.clone(),
            to: type_name(tr, src),
            kind: RelationKind::Implements,
        });
    }
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for f in body.named_children(&mut cursor) {
        if f.kind() != "function_item" {
            continue;
        }
        if let Some(name) = f.child_by_field_name("name") {
            facts.symbols.push(CodeSymbol {
                name: format!("{ty}::{}", text(name, src)),
                kind: SymbolKind::Function,
                line: line(f),
                exported: is_pub(f),
            });
        }
    }
}

/// A use tree as `(module, name)` pairs: `crate::a::{B, c::D}` →
/// `(crate::a, B)`, `(crate::a::c, D)`; a glob is `(module, *)`; `use x;` is
/// `(x, None)`.
fn flatten_use(node: Node<'_>, src: &[u8], prefix: &str, out: &mut Vec<(String, Option<String>)>) {
    let join = |p: &str, s: &str| {
        if p.is_empty() {
            s.to_owned()
        } else if s.is_empty() {
            p.to_owned()
        } else {
            format!("{p}::{s}")
        }
    };
    match node.kind() {
        "scoped_identifier" => {
            let path = node
                .child_by_field_name("path")
                .map(|p| text(p, src))
                .unwrap_or_default();
            let name = node
                .child_by_field_name("name")
                .map(|n| text(n, src).to_owned());
            out.push((join(prefix, path), name));
        }
        "identifier" | "self" | "crate" | "super" => {
            if prefix.is_empty() {
                out.push((text(node, src).to_owned(), None));
            } else {
                out.push((prefix.to_owned(), Some(text(node, src).to_owned())));
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                flatten_use(path, src, prefix, out);
            }
        }
        "use_wildcard" => {
            let path = text(node, src)
                .trim_end_matches("::*")
                .trim_end_matches('*');
            out.push((
                join(prefix, path.trim_end_matches("::")),
                Some("*".to_owned()),
            ));
        }
        "scoped_use_list" => {
            let path = node
                .child_by_field_name("path")
                .map(|p| text(p, src))
                .unwrap_or_default();
            let next = join(prefix, path);
            if let Some(list) = node.child_by_field_name("list") {
                flatten_use(list, src, &next, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                flatten_use(child, src, prefix, out);
            }
        }
        _ => {}
    }
}

fn collect_calls(node: Node<'_>, src: &[u8], out: &mut Vec<CodeCall>) {
    if node.kind() == "call_expression" {
        if let Some(f) = node.child_by_field_name("function") {
            let f = if f.kind() == "generic_function" {
                f.child_by_field_name("function").unwrap_or(f)
            } else {
                f
            };
            if matches!(f.kind(), "identifier" | "scoped_identifier") {
                out.push(CodeCall {
                    callee: text(f, src).to_owned(),
                    line: line(node),
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(child, src, out);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use dit_model::CodeLang;

    const LIB: &str = r#"//! A crate.
mod agent;
pub mod morse;
use std::path::Path;
use crate::agent::{AgentContext, RuleFile, tools::{a, b as bee}};
use dit_model::*;
pub use morse::{MorseReport, ProofView};

pub struct Dit { root: String }
enum Mode { A }
pub trait Shown {}
pub const LIMIT: usize = 3;
type Alias = u8;
macro_rules! m { () => {} }

impl Shown for Dit {}
impl Dit {
    pub fn open(path: &Path) -> Self {
        let repo = Repo::open(path);
        helper();
        repo.head();
        Self { root: String::new() }
    }
}

fn helper() {}
"#;

    fn facts() -> FileFacts {
        crate::extract(CodeLang::Rust, LIB)
    }

    #[test]
    fn a_use_tree_splits_into_one_import_per_module() {
        let f = facts();
        let find = |spec: &str| f.imports.iter().find(|i| i.specifier == spec).unwrap();
        assert_eq!(find("std::path").names, vec!["Path"]);
        assert_eq!(find("crate::agent").names, vec!["AgentContext", "RuleFile"]);
        assert_eq!(find("crate::agent::tools").names, vec!["a", "b"]);
        assert_eq!(find("dit_model").names, vec!["*"]);
        let reexport = find("morse");
        assert!(reexport.reexport);
        assert_eq!(reexport.names, vec!["MorseReport", "ProofView"]);
    }

    #[test]
    fn a_module_declaration_points_at_another_file() {
        let f = facts();
        let agent = f
            .imports
            .iter()
            .find(|i| i.specifier == "self::agent")
            .unwrap();
        assert!(agent.names.is_empty());
        assert!(f.imports.iter().any(|i| i.specifier == "self::morse"));
    }

    #[test]
    fn items_are_symbols_marked_pub_or_not() {
        let f = facts();
        let sym = |name: &str| f.symbols.iter().find(|s| s.name == name).unwrap();
        assert_eq!(sym("Dit").kind, SymbolKind::Struct);
        assert!(sym("Dit").exported);
        assert_eq!(sym("Mode").kind, SymbolKind::Enum);
        assert!(!sym("Mode").exported);
        assert_eq!(sym("Shown").kind, SymbolKind::Trait);
        assert_eq!(sym("LIMIT").kind, SymbolKind::Const);
        assert_eq!(sym("Alias").kind, SymbolKind::Type);
        assert_eq!(sym("m").kind, SymbolKind::Macro);
        assert_eq!(sym("morse").kind, SymbolKind::Module);
        assert_eq!(sym("helper").kind, SymbolKind::Function);
        // Methods are reached through their type, not listed as file symbols.
        assert!(f
            .symbols
            .iter()
            .any(|s| s.name == "Dit::open" && s.exported));
    }

    #[test]
    fn a_trait_impl_is_a_relation() {
        assert!(facts().relations.contains(&CodeRelation {
            from: "Dit".into(),
            to: "Shown".into(),
            kind: RelationKind::Implements,
        }));
    }

    #[test]
    fn calls_are_named_by_path() {
        let f = facts();
        let callees: Vec<&str> = f.calls.iter().map(|c| c.callee.as_str()).collect();
        for want in ["Repo::open", "helper", "String::new"] {
            assert!(callees.contains(&want), "missing {want}: {callees:?}");
        }
    }
}
