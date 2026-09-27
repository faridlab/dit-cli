//! TypeScript / TSX extraction.

use dit_model::{
    path_shaped, CodeCall, CodeConst, CodeImport, CodeRelation, CodeString, CodeSymbol, FileFacts,
    RelationKind, SymbolKind,
};

use crate::kotlin::placeholder;
use tree_sitter::{Node, Parser};

pub(crate) fn extract(text: &str, tsx: bool) -> FileFacts {
    let mut parser = Parser::new();
    let language = if tsx {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    if parser.set_language(&language.into()).is_err() {
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
        top_level(node, src, false, &mut facts);
    }
    collect_calls(root, src, &mut facts.calls);
    collect_dynamic_imports(root, src, &mut facts.imports);
    collect_strings(root, src, &mut facts.strings);
    facts
}

/// A string or template literal in the `${name}` form; `None` for anything
/// else.
fn literal(node: Node<'_>, src: &[u8]) -> Option<String> {
    match node.kind() {
        "string" => Some(string_value(node, src)),
        "template_string" => {
            let (start, end) = (node.start_byte() + 1, node.end_byte().saturating_sub(1));
            if end <= start {
                return Some(String::new());
            }
            let mut out = String::new();
            let mut at = start;
            let mut cursor = node.walk();
            for sub in node.named_children(&mut cursor) {
                if sub.kind() != "template_substitution" {
                    continue;
                }
                out.push_str(std::str::from_utf8(&src[at..sub.start_byte()]).unwrap_or_default());
                let inner = text(sub, src);
                let expr = inner
                    .strip_prefix("${")
                    .and_then(|e| e.strip_suffix('}'))
                    .unwrap_or_default();
                out.push_str(&placeholder(expr.trim()));
                at = sub.end_byte();
            }
            out.push_str(std::str::from_utf8(&src[at..end]).unwrap_or_default());
            Some(out)
        }
        _ => None,
    }
}

/// Path-shaped literals anywhere in the file, except module specifiers —
/// an import names a file, not an endpoint.
fn collect_strings(node: Node<'_>, src: &[u8], out: &mut Vec<CodeString>) {
    match node.kind() {
        "import_statement" | "export_statement" if node.child_by_field_name("source").is_some() => {
            return
        }
        "call_expression" => {
            let loader = node
                .child_by_field_name("function")
                .is_some_and(|f| f.kind() == "import" || text(f, src) == "require");
            if loader {
                return;
            }
        }
        "string" | "template_string" => {
            if let Some(t) = literal(node, src).filter(|t| path_shaped(t)) {
                out.push(CodeString {
                    text: t,
                    line: line(node),
                });
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_strings(child, src, out);
    }
}

/// `import("…")` with a literal target: a dependency like any other. A target
/// built at runtime names nothing the map can follow, and is left out.
fn collect_dynamic_imports(node: Node<'_>, src: &[u8], out: &mut Vec<CodeImport>) {
    if node.kind() == "call_expression" {
        let is_import = node
            .child_by_field_name("function")
            .is_some_and(|f| f.kind() == "import");
        if is_import {
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut cursor = args.walk();
                let first = args.named_children(&mut cursor).next();
                if let Some(arg) = first {
                    let literal = match arg.kind() {
                        "string" => true,
                        // A template with no `${…}` is a literal too.
                        "template_string" => {
                            let mut c2 = arg.walk();
                            let has_sub = arg
                                .named_children(&mut c2)
                                .any(|c| c.kind() == "template_substitution");
                            !has_sub
                        }
                        _ => false,
                    };
                    if literal {
                        out.push(CodeImport {
                            specifier: string_value(arg, src),
                            names: vec!["*".to_owned()],
                            reexport: false,
                            line: line(node),
                        });
                    }
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_dynamic_imports(child, src, out);
    }
}

fn text<'a>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or_default()
}

fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

/// A string literal's contents, quotes stripped.
fn string_value(node: Node<'_>, src: &[u8]) -> String {
    text(node, src)
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_owned()
}

fn top_level(node: Node<'_>, src: &[u8], exported: bool, facts: &mut FileFacts) {
    match node.kind() {
        "import_statement" => {
            if let Some(source) = node.child_by_field_name("source") {
                facts.imports.push(CodeImport {
                    specifier: string_value(source, src),
                    names: import_names(node, src),
                    reexport: false,
                    line: line(node),
                });
            }
        }
        "export_statement" => {
            if let Some(source) = node.child_by_field_name("source") {
                let mut names = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "*" => names.push("*".to_owned()),
                        "export_clause" => {
                            let mut c2 = child.walk();
                            for spec in child.named_children(&mut c2) {
                                if let Some(name) = spec.child_by_field_name("name") {
                                    names.push(text(name, src).to_owned());
                                }
                            }
                        }
                        "namespace_export" => names.push("*".to_owned()),
                        _ => {}
                    }
                }
                facts.imports.push(CodeImport {
                    specifier: string_value(source, src),
                    names,
                    reexport: true,
                    line: line(node),
                });
            } else if let Some(decl) = node.child_by_field_name("declaration") {
                top_level(decl, src, true, facts);
            } else {
                // `export default function Foo()` / `export default class`.
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    top_level(child, src, true, facts);
                }
            }
        }
        "function_declaration" | "generator_function_declaration" => {
            symbol(node, src, SymbolKind::Function, exported, facts);
        }
        "class_declaration" | "abstract_class_declaration" => {
            symbol(node, src, SymbolKind::Class, exported, facts);
            heritage(node, src, facts);
        }
        "interface_declaration" => symbol(node, src, SymbolKind::Interface, exported, facts),
        "type_alias_declaration" => symbol(node, src, SymbolKind::Type, exported, facts),
        "enum_declaration" => symbol(node, src, SymbolKind::Enum, exported, facts),
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            for decl in node.named_children(&mut cursor) {
                if decl.kind() != "variable_declarator" {
                    continue;
                }
                if let Some(name) = decl.child_by_field_name("name") {
                    // Destructuring binds several names; only a plain
                    // identifier is a symbol worth indexing.
                    if name.kind() == "identifier" {
                        facts.symbols.push(CodeSymbol {
                            name: text(name, src).to_owned(),
                            kind: SymbolKind::Const,
                            line: line(decl),
                            exported,
                        });
                        if let Some(value) = decl
                            .child_by_field_name("value")
                            .and_then(|v| literal(v, src))
                        {
                            facts.consts.push(CodeConst {
                                name: text(name, src).to_owned(),
                                value,
                            });
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn symbol(node: Node<'_>, src: &[u8], kind: SymbolKind, exported: bool, facts: &mut FileFacts) {
    if let Some(name) = node.child_by_field_name("name") {
        facts.symbols.push(CodeSymbol {
            name: text(name, src).to_owned(),
            kind,
            line: line(node),
            exported,
        });
    }
}

/// `import A, { b, type C } from` → `default`, `b`, `C`; `* as ns` → `*`.
fn import_names(node: Node<'_>, src: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "import_clause" {
            continue;
        }
        let mut c2 = child.walk();
        for part in child.named_children(&mut c2) {
            match part.kind() {
                "identifier" => names.push("default".to_owned()),
                "namespace_import" => names.push("*".to_owned()),
                "named_imports" => {
                    let mut c3 = part.walk();
                    for spec in part.named_children(&mut c3) {
                        if let Some(name) = spec.child_by_field_name("name") {
                            names.push(text(name, src).to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    names
}

fn heritage(class: Node<'_>, src: &[u8], facts: &mut FileFacts) {
    let Some(name) = class.child_by_field_name("name") else {
        return;
    };
    let from = text(name, src).to_owned();
    let mut cursor = class.walk();
    for child in class.named_children(&mut cursor) {
        if child.kind() != "class_heritage" {
            continue;
        }
        let mut c2 = child.walk();
        for clause in child.named_children(&mut c2) {
            let kind = match clause.kind() {
                "extends_clause" => RelationKind::Inherits,
                "implements_clause" => RelationKind::Implements,
                _ => continue,
            };
            let mut c3 = clause.walk();
            for target in clause.named_children(&mut c3) {
                if matches!(target.kind(), "type_arguments" | "arguments") {
                    continue;
                }
                let to = text(target, src);
                let to = to.split('<').next().unwrap_or(to).trim();
                if !to.is_empty() {
                    facts.relations.push(CodeRelation {
                        from: from.clone(),
                        to: to.to_owned(),
                        kind,
                    });
                }
            }
        }
    }
}

/// Every call, `new`, and JSX use of a component, anywhere in the file.
fn collect_calls(node: Node<'_>, src: &[u8], out: &mut Vec<CodeCall>) {
    let callee = match node.kind() {
        "call_expression" => node.child_by_field_name("function"),
        "new_expression" => node.child_by_field_name("constructor"),
        "jsx_opening_element" | "jsx_self_closing_element" => node
            .child_by_field_name("name")
            // `<div>` is an element, not a component someone wrote.
            .filter(|n| text(*n, src).starts_with(|c: char| c.is_ascii_uppercase())),
        _ => None,
    };
    if let Some(callee) = callee {
        if matches!(
            callee.kind(),
            "identifier" | "member_expression" | "nested_identifier" | "jsx_namespace_name"
        ) {
            let name = text(callee, src);
            if !name.contains(['\n', '(']) {
                out.push(CodeCall {
                    callee: name.to_owned(),
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

    const PAGE: &str = r#"import { Text } from "@mantine/core";
import React, { useState } from "react";
import * as path from "node:path";
import "./side-effect.css";
import { RecordSurface, type RecordView } from "@/crud/RecordSurface";
import { useDeskVerb } from "./verbs/useDeskVerb";
export { state } from "./views";
export * from "./kit";

type Row = Record<string, unknown>;
const str = (v: unknown) => String(v);

export interface Props { id: string }
export enum Tone { Good, Bad }
export const LIMIT = 200;

export class Page extends Base implements Shown, Sized {}

export function PayrollRunsPage() {
  const verb = useDeskVerb({ touches: [] });
  const [x] = useState(0);
  api.get("x");
  return <RecordSurface d={verb} />;
}

function helper() { return str(1); }
"#;

    fn facts() -> FileFacts {
        crate::extract(CodeLang::Tsx, PAGE)
    }

    #[test]
    fn imports_carry_their_specifier_and_named_symbols() {
        let f = facts();
        let find = |spec: &str| f.imports.iter().find(|i| i.specifier == spec).unwrap();
        assert_eq!(find("@mantine/core").names, vec!["Text"]);
        assert_eq!(find("react").names, vec!["default", "useState"]);
        assert_eq!(find("node:path").names, vec!["*"]);
        assert!(find("./side-effect.css").names.is_empty());
        assert_eq!(
            find("@/crud/RecordSurface").names,
            vec!["RecordSurface", "RecordView"]
        );
        assert_eq!(find("./verbs/useDeskVerb").line, 6);
        assert!(!find("./verbs/useDeskVerb").reexport);
    }

    #[test]
    fn re_exports_are_imports_that_pass_names_through() {
        let f = facts();
        let views = f.imports.iter().find(|i| i.specifier == "./views").unwrap();
        assert!(views.reexport);
        assert_eq!(views.names, vec!["state"]);
        let kit = f.imports.iter().find(|i| i.specifier == "./kit").unwrap();
        assert!(kit.reexport);
        assert_eq!(kit.names, vec!["*"]);
    }

    #[test]
    fn top_level_declarations_are_symbols_marked_exported_or_not() {
        let f = facts();
        let sym = |name: &str| f.symbols.iter().find(|s| s.name == name).unwrap();
        assert_eq!(sym("Row").kind, SymbolKind::Type);
        assert!(!sym("Row").exported);
        assert_eq!(sym("str").kind, SymbolKind::Const);
        assert_eq!(sym("Props").kind, SymbolKind::Interface);
        assert_eq!(sym("Tone").kind, SymbolKind::Enum);
        assert_eq!(sym("LIMIT").kind, SymbolKind::Const);
        assert!(sym("LIMIT").exported);
        assert_eq!(sym("Page").kind, SymbolKind::Class);
        assert_eq!(sym("PayrollRunsPage").kind, SymbolKind::Function);
        assert!(sym("PayrollRunsPage").exported);
        assert!(!sym("helper").exported);
        // Locals inside a function are not file symbols.
        assert!(f.symbols.iter().all(|s| s.name != "verb"));
    }

    #[test]
    fn class_heritage_is_a_relation() {
        let f = facts();
        assert!(f.relations.contains(&CodeRelation {
            from: "Page".into(),
            to: "Base".into(),
            kind: RelationKind::Inherits
        }));
        assert!(f.relations.contains(&CodeRelation {
            from: "Page".into(),
            to: "Sized".into(),
            kind: RelationKind::Implements
        }));
    }

    // Found by the parity check against graphify: a dynamic `import()` —
    // how tests reload a module and how routes split code — is a dependency
    // too, and was missing from the map.
    #[test]
    fn a_dynamic_import_with_a_literal_is_an_import() {
        let f = crate::extract(
            CodeLang::TypeScript,
            "async function t() {\n  const mod = await import(\"@/auth/tokenStore\");\n  const lazy = () => import(`./pages/Home`);\n  const dynamic = import(name);\n}\n",
        );
        let specs: Vec<&str> = f.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specs, vec!["@/auth/tokenStore", "./pages/Home"], "{f:?}");
        assert_eq!(f.imports[0].names, vec!["*"]);
        assert_eq!(f.imports[0].line, 2);
    }

    #[test]
    fn calls_and_jsx_uses_are_named() {
        let f = facts();
        let callees: Vec<&str> = f.calls.iter().map(|c| c.callee.as_str()).collect();
        for want in [
            "useDeskVerb",
            "useState",
            "api.get",
            "RecordSurface",
            "str",
            "String",
        ] {
            assert!(callees.contains(&want), "missing {want}: {callees:?}");
        }
    }

    #[test]
    fn path_literals_and_string_constants_are_read() {
        let f = extract(
            r#"import { x } from "./a/b";
const P = "api/v1/performance";
export const PERF = {
  open: (id: string) => `${P}/cycles/${id}/open`,
  rate: (r: Row) => `${P}/appraisals/${r.id}/rate`,
};
const lazy = import("./lazy/page");
const when = "dd/MM/yyyy hh";
const icon = "/img/logo.svg";
"#,
            false,
        );
        let strings: Vec<&str> = f.strings.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            strings,
            [
                "api/v1/performance",
                "${P}/cycles/${id}/open",
                "${P}/appraisals/${r.id}/rate"
            ]
        );
        assert_eq!(
            f.consts[0],
            CodeConst {
                name: "P".into(),
                value: "api/v1/performance".into()
            }
        );
    }
}
