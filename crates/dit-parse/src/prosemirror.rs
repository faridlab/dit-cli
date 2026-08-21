//! The ProseMirror bridge — the editor's half of DESIGN.md §12.2.
//!
//! There is one markdown serializer in DIT and it lives in Rust: the editor
//! receives a ProseMirror JSON document produced here from the file's bytes,
//! and hands back a document that is serialized here into the same bytes
//! `dit fmt` would write. Byte-identity is structural — [`doc_to_markdown`]
//! builds a comrak AST and runs the exact [`crate::fmt::format_ast`]
//! pipeline — so the CLI, the editor, the AI and the merge driver cannot
//! produce different bytes (Risk #12).
//!
//! The ProseMirror schema is a strict subset of CommonMark + GFM plus the
//! `dit-*` fenced blocks (§12.5): every construct here has a markdown home.
//! Raw HTML has no rendering equivalent, so `htmlBlock` / `htmlInline`
//! carry their literal verbatim — bytes in, bytes out, never rendered.
//!
//! Incoming JSON is hostile surface (§17): unknown node types are refused,
//! never dropped, and depth/node caps keep adversarial documents from
//! exhausting the stack.

use comrak::nodes::{
    AstNode, LineColumn, ListDelimType, ListType, NodeCode, NodeCodeBlock, NodeHeading,
    NodeHtmlBlock, NodeLink, NodeList, NodeTable, NodeTaskItem, NodeValue, NodeWikiLink, Sourcepos,
    TableAlignment,
};
use comrak::{parse_document, Arena};
use serde_json::{json, Value};

use crate::fmt::{format_ast, has_conflict_markers};

/// Nesting deeper than this is refused, not recursed into.
pub const MAX_DEPTH: u32 = 64;
/// More nodes than this in one document is refused (hostile-input cap).
pub const MAX_NODES: u32 = 100_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProseMirrorError {
    #[error("unknown node type `{kind}` — this document needs a newer editor")]
    UnknownNode { kind: String },
    #[error("unknown mark type `{kind}`")]
    UnknownMark { kind: String },
    #[error("{path}: {reason}")]
    UnexpectedShape { path: String, reason: String },
    #[error("node `{node}` attr `{attr}`: {reason}")]
    InvalidAttr {
        node: String,
        attr: String,
        reason: String,
    },
    #[error("document nesting exceeds {limit} levels")]
    TooDeep { limit: u32 },
    #[error("document exceeds {limit} nodes")]
    TooLarge { limit: u32 },
    #[error("the built tree failed comrak's structural validation")]
    IllFormedAst,
    #[error(transparent)]
    Fmt(#[from] crate::fmt::FmtError),
}

/// Markdown bytes → ProseMirror JSON. Accepts any parseable markdown (the
/// file may not be canonical yet); [`doc_to_markdown`] on the result yields
/// the canonical form.
pub fn markdown_to_doc(markdown: &str) -> Result<Value, ProseMirrorError> {
    if has_conflict_markers(markdown) {
        return Err(crate::fmt::FmtError::ConflictMarkers.into());
    }
    let options = crate::fmt::dit_options();
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options);
    let blocks: Vec<&AstNode> = root.children().collect();
    let mut ctx = Count::default();
    let content = blocks_to_json(&blocks, 0, &mut ctx)?;
    Ok(json!({ "type": "doc", "content": content }))
}

/// ProseMirror JSON → canonical markdown, byte-identical to `dit fmt`.
pub fn doc_to_markdown(doc: &Value) -> Result<String, ProseMirrorError> {
    if doc.get("type").and_then(Value::as_str) != Some("doc") {
        return Err(ProseMirrorError::UnexpectedShape {
            path: "/".into(),
            reason: "the root must be {\"type\": \"doc\"}".into(),
        });
    }
    let arena = Arena::new();
    let root: &AstNode = arena.alloc(NodeValue::Document.into());
    let mut ctx = Count::default();
    let children = doc
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for (i, child) in children.iter().enumerate() {
        let block = build_block(child, &format!("/content/{i}"), 1, &arena, &mut ctx)?;
        root.append(block);
    }
    root.validate()
        .map_err(|_| ProseMirrorError::IllFormedAst)?;
    let out = format_ast(root)?;
    if has_conflict_markers(&out) {
        return Err(crate::fmt::FmtError::ConflictMarkers.into());
    }
    Ok(out)
}

/// Node budget shared by both directions — one counter, one cap.
#[derive(Default)]
struct Count {
    nodes: u32,
}

impl Count {
    fn tick(&mut self) -> Result<(), ProseMirrorError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ProseMirrorError::TooLarge { limit: MAX_NODES });
        }
        Ok(())
    }
}

fn depth_guard(depth: u32) -> Result<(), ProseMirrorError> {
    if depth > MAX_DEPTH {
        return Err(ProseMirrorError::TooDeep { limit: MAX_DEPTH });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// comrak AST → ProseMirror JSON
// ---------------------------------------------------------------------------

fn blocks_to_json<'a>(
    nodes: &[&'a AstNode<'a>],
    depth: u32,
    ctx: &mut Count,
) -> Result<Vec<Value>, ProseMirrorError> {
    depth_guard(depth)?;
    nodes.iter().map(|n| block_to_json(n, depth, ctx)).collect()
}

fn block_to_json<'a>(
    node: &'a AstNode<'a>,
    depth: u32,
    ctx: &mut Count,
) -> Result<Value, ProseMirrorError> {
    ctx.tick()?;
    let value = node.data().value.clone();
    let children: Vec<&AstNode> = node.children().collect();
    match value {
        NodeValue::Paragraph => inline_container_json("paragraph", node, &[], depth, ctx),
        NodeValue::Heading(h) => Ok(json!({
            "type": "heading",
            "attrs": { "level": h.level },
            "content": inlines_to_json(&children, &[], depth, ctx)?,
        })),
        NodeValue::BlockQuote => Ok(json!({
            "type": "blockquote",
            "content": blocks_to_json(&children, depth + 1, ctx)?,
        })),
        NodeValue::ThematicBreak => Ok(json!({ "type": "horizontalRule" })),
        NodeValue::CodeBlock(cb) => Ok(json!({
            "type": "codeBlock",
            "attrs": { "language": cb.info },
            "content": [{ "type": "text", "text": cb.literal }],
        })),
        NodeValue::HtmlBlock(hb) => Ok(json!({
            "type": "htmlBlock",
            "attrs": { "literal": hb.literal },
        })),
        NodeValue::List(l) => {
            let (kind, attrs) = match l.list_type {
                ListType::Bullet => ("bulletList", json!({ "tight": l.tight })),
                ListType::Ordered => {
                    let mut a = json!({ "tight": l.tight });
                    a["start"] = json!(l.start);
                    ("orderedList", a)
                }
            };
            let items: Vec<Value> = children
                .iter()
                .map(|item| item_to_json(item, depth, ctx))
                .collect::<Result<_, _>>()?;
            Ok(json!({ "type": kind, "attrs": attrs, "content": items }))
        }
        NodeValue::Table(t) => {
            let alignments: Vec<&str> = t
                .alignments
                .iter()
                .map(|a| match a {
                    TableAlignment::None => "none",
                    TableAlignment::Left => "left",
                    TableAlignment::Center => "center",
                    TableAlignment::Right => "right",
                })
                .collect();
            let rows: Vec<Value> = children
                .iter()
                .map(|row| {
                    ctx.tick()?;
                    let is_header = match row.data().value {
                        NodeValue::TableRow(h) => h,
                        ref other => {
                            return Err(ProseMirrorError::UnexpectedShape {
                                path: "/".into(),
                                reason: format!("table contains {}", variant_name(other)),
                            })
                        }
                    };
                    let cells: Vec<Value> = row
                        .children()
                        .map(|cell| {
                            ctx.tick()?;
                            Ok(json!({
                                "type": if is_header { "tableHeader" } else { "tableCell" },
                                "content": [
                                    inline_container_json("paragraph", cell, &[], depth, ctx)?
                                ],
                            }))
                        })
                        .collect::<Result<_, ProseMirrorError>>()?;
                    Ok(json!({
                        "type": "tableRow",
                        "attrs": { "isHeader": is_header },
                        "content": cells,
                    }))
                })
                .collect::<Result<_, ProseMirrorError>>()?;
            Ok(json!({
                "type": "table",
                "attrs": { "alignments": alignments },
                "content": rows,
            }))
        }
        // Bodies never parse into these with `dit_options()`; hitting one
        // means the option set and this module drifted apart.
        other => Err(ProseMirrorError::UnexpectedShape {
            path: "/".into(),
            reason: format!(
                "no ProseMirror home for comrak {} — is dit_options out of sync?",
                variant_name(&other)
            ),
        }),
    }
}

/// A list item; task-ness is per item because GFM allows `- [ ] a` next to
/// `- b` in one list (§12.5 — the editor must round-trip that).
fn item_to_json<'a>(
    item: &'a AstNode<'a>,
    depth: u32,
    ctx: &mut Count,
) -> Result<Value, ProseMirrorError> {
    ctx.tick()?;
    let task = match item.data().value {
        NodeValue::TaskItem(ref t) => match t.symbol {
            Some(c) => json!(c.to_string()),
            // An unchecked `- [ ]` item is a task, not a plain item; `false`
            // keeps it distinct from `null` (no checkbox at all).
            None => json!(false),
        },
        NodeValue::Item(_) => json!(null),
        ref other => {
            return Err(ProseMirrorError::UnexpectedShape {
                path: "/".into(),
                reason: format!("list contains {}", variant_name(other)),
            })
        }
    };
    let blocks: Vec<&AstNode> = item.children().collect();
    Ok(json!({
        "type": "listItem",
        "attrs": { "task": task },
        "content": blocks_to_json(&blocks, depth + 1, ctx)?,
    }))
}

fn inline_container_json<'a>(
    kind: &str,
    node: &'a AstNode<'a>,
    marks: &[Value],
    depth: u32,
    ctx: &mut Count,
) -> Result<Value, ProseMirrorError> {
    let inlines: Vec<&AstNode> = node.children().collect();
    let content = inlines_to_json(&inlines, marks, depth, ctx)?;
    let mut v = json!({ "type": kind });
    if !content.is_empty() {
        v["content"] = Value::Array(content);
    }
    Ok(v)
}

/// Walk inline nodes, accumulating ancestor marks outermost-first — the
/// marks array order IS the comrak nesting order, and `build_inline_run`
/// rebuilds it from that order.
fn inlines_to_json<'a>(
    nodes: &[&'a AstNode<'a>],
    marks: &[Value],
    depth: u32,
    ctx: &mut Count,
) -> Result<Vec<Value>, ProseMirrorError> {
    depth_guard(depth)?;
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        ctx.tick()?;
        let value = node.data().value.clone();
        let with_marks = |mut v: Value, marks: &[Value]| {
            if !marks.is_empty() {
                v["marks"] = Value::Array(marks.to_vec());
            }
            v
        };
        match value {
            NodeValue::Text(t) => out.push(with_marks(json!({ "type": "text", "text": t }), marks)),
            NodeValue::Code(c) => {
                let mut m = marks.to_vec();
                m.push(json!({ "type": "code" }));
                out.push(json!({ "type": "text", "text": c.literal, "marks": m }));
            }
            NodeValue::SoftBreak => out.push(with_marks(
                json!({ "type": "hardBreak", "attrs": { "soft": true } }),
                marks,
            )),
            NodeValue::LineBreak => out.push(with_marks(
                json!({ "type": "hardBreak", "attrs": { "soft": false } }),
                marks,
            )),
            NodeValue::Emph | NodeValue::Strong | NodeValue::Strikethrough => {
                let mark = match value {
                    NodeValue::Emph => json!({ "type": "italic" }),
                    NodeValue::Strong => json!({ "type": "bold" }),
                    _ => json!({ "type": "strike" }),
                };
                let mut m = marks.to_vec();
                m.push(mark);
                let inner: Vec<&AstNode> = node.children().collect();
                out.extend(inlines_to_json(&inner, &m, depth + 1, ctx)?);
            }
            NodeValue::Link(l) => {
                let mut m = marks.to_vec();
                m.push(json!({ "type": "link", "attrs": { "href": l.url, "title": l.title } }));
                let inner: Vec<&AstNode> = node.children().collect();
                let produced = inlines_to_json(&inner, &m, depth + 1, ctx)?;
                if produced.is_empty() {
                    // `[](url)` — a link over no text still exists in the
                    // bytes; keep it as an empty text node carrying the mark.
                    out.push(json!({ "type": "text", "text": "", "marks": m }));
                } else {
                    out.extend(produced);
                }
            }
            NodeValue::Image(l) => {
                let mut v = json!({
                    "type": "image",
                    "attrs": { "src": l.url, "title": l.title },
                });
                let inner: Vec<&AstNode> = node.children().collect();
                // Alt text is its own inline run — marks do not leak in.
                let alt = inlines_to_json(&inner, &[], depth + 1, ctx)?;
                if !alt.is_empty() {
                    v["content"] = Value::Array(alt);
                }
                out.push(with_marks(v, marks));
            }
            NodeValue::WikiLink(w) => {
                let mut v = json!({
                    "type": "wikiLink",
                    "attrs": { "target": w.url },
                });
                let inner: Vec<&AstNode> = node.children().collect();
                let label = inlines_to_json(&inner, &[], depth + 1, ctx)?;
                if !label.is_empty() {
                    v["content"] = Value::Array(label);
                }
                out.push(with_marks(v, marks));
            }
            NodeValue::HtmlInline(s) => out.push(with_marks(
                json!({ "type": "htmlInline", "attrs": { "literal": s } }),
                marks,
            )),
            other => {
                return Err(ProseMirrorError::UnexpectedShape {
                    path: "/".into(),
                    reason: format!(
                        "no ProseMirror home for comrak {} — is dit_options out of sync?",
                        variant_name(&other)
                    ),
                })
            }
        }
    }
    Ok(out)
}

fn zero_sourcepos() -> Sourcepos {
    Sourcepos {
        start: LineColumn { line: 0, column: 0 },
        end: LineColumn { line: 0, column: 0 },
    }
}

fn variant_name(v: &NodeValue) -> String {
    let s = format!("{v:?}");
    s.split(['(', ' ']).next().unwrap_or(&s).to_string()
}

// ---------------------------------------------------------------------------
// ProseMirror JSON → comrak AST
// ---------------------------------------------------------------------------

fn get_str<'v>(
    v: &'v Value,
    node: &str,
    attr: &str,
    path: &str,
) -> Result<&'v str, ProseMirrorError> {
    v.get(attr)
        .and_then(Value::as_str)
        .ok_or(ProseMirrorError::InvalidAttr {
            node: node.into(),
            attr: attr.into(),
            reason: format!("must be a string ({path})"),
        })
}

/// Zip inline children with their (shape-checked) marks for the grouping
/// walk. Clones the JSON values — editor documents are small next to the
/// tree we are about to allocate for them.
fn run_items(children: &[Value], path: &str) -> Result<Vec<(Value, Vec<Value>)>, ProseMirrorError> {
    let mut items = Vec::with_capacity(children.len());
    for (i, child) in children.iter().enumerate() {
        let marks = child
            .get("marks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for m in &marks {
            if m.get("type").and_then(Value::as_str).is_none() {
                return Err(ProseMirrorError::UnexpectedShape {
                    path: format!("{path}/content/{i}"),
                    reason: "every mark must be an object with a string `type`".into(),
                });
            }
        }
        items.push((child.clone(), marks));
    }
    Ok(items)
}

fn build_block<'a>(
    v: &Value,
    path: &str,
    depth: u32,
    arena: &'a Arena<'a>,
    ctx: &mut Count,
) -> Result<&'a AstNode<'a>, ProseMirrorError> {
    ctx.tick()?;
    depth_guard(depth)?;
    let Some(kind) = v.get("type").and_then(Value::as_str) else {
        return Err(ProseMirrorError::UnexpectedShape {
            path: path.into(),
            reason: "every node must be an object with a string `type`".into(),
        });
    };
    let children: Vec<Value> = v
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let attrs = v.get("attrs").cloned().unwrap_or(Value::Null);

    let node: &AstNode = match kind {
        "paragraph" => {
            let n = arena.alloc(NodeValue::Paragraph.into());
            for inline in build_inline_run(&run_items(&children, path)?, arena, ctx, depth + 1)? {
                n.append(inline);
            }
            n
        }
        "heading" => {
            let level = attrs.get("level").and_then(Value::as_u64).ok_or(
                ProseMirrorError::InvalidAttr {
                    node: "heading".into(),
                    attr: "level".into(),
                    reason: "must be an integer 1-6".into(),
                },
            )?;
            if !(1..=6).contains(&level) {
                return Err(ProseMirrorError::InvalidAttr {
                    node: "heading".into(),
                    attr: "level".into(),
                    reason: format!("heading level {level} is outside 1-6"),
                });
            }
            let n = arena.alloc(
                NodeValue::Heading(NodeHeading {
                    level: level as u8,
                    setext: false,
                    closed: true,
                })
                .into(),
            );
            for inline in build_inline_run(&run_items(&children, path)?, arena, ctx, depth + 1)? {
                n.append(inline);
            }
            n
        }
        "blockquote" => {
            let n = arena.alloc(NodeValue::BlockQuote.into());
            for (i, child) in children.iter().enumerate() {
                n.append(build_block(
                    child,
                    &format!("{path}/content/{i}"),
                    depth + 1,
                    arena,
                    ctx,
                )?);
            }
            n
        }
        "horizontalRule" => arena.alloc(NodeValue::ThematicBreak.into()),
        "codeBlock" => {
            let language = get_str(&attrs, "codeBlock", "language", path)?;
            if language.contains(['`', '\n', '\r']) {
                return Err(ProseMirrorError::InvalidAttr {
                    node: "codeBlock".into(),
                    attr: "language".into(),
                    reason: "info strings cannot contain backticks or newlines".into(),
                });
            }
            let mut literal = String::new();
            for (i, child) in children.iter().enumerate() {
                let text = child.get("text").and_then(Value::as_str).ok_or(
                    ProseMirrorError::UnexpectedShape {
                        path: format!("{path}/content/{i}"),
                        reason: "code blocks may only contain text".into(),
                    },
                )?;
                literal.push_str(text);
            }
            arena.alloc(
                NodeValue::CodeBlock(Box::new(NodeCodeBlock {
                    // The formatter derives fence char and length from the
                    // info string and the literal itself; these parse-time
                    // fields just record a faithful default.
                    fenced: true,
                    fence_char: b'`',
                    fence_length: 3,
                    fence_offset: 0,
                    info: language.to_string(),
                    literal,
                    closed: true,
                }))
                .into(),
            )
        }
        "htmlBlock" => {
            let literal = get_str(&attrs, "htmlBlock", "literal", path)?;
            if literal.is_empty() {
                return Err(ProseMirrorError::InvalidAttr {
                    node: "htmlBlock".into(),
                    attr: "literal".into(),
                    reason: "must be a non-empty string".into(),
                });
            }
            arena.alloc(
                NodeValue::HtmlBlock(NodeHtmlBlock {
                    block_type: 6,
                    literal: literal.to_string(),
                })
                .into(),
            )
        }
        "bulletList" | "orderedList" => {
            let ordered = kind == "orderedList";
            let tight = attrs.get("tight").and_then(Value::as_bool).unwrap_or(true);
            let start = attrs.get("start").and_then(Value::as_u64).unwrap_or(1);
            let list_data = NodeList {
                list_type: if ordered {
                    ListType::Ordered
                } else {
                    ListType::Bullet
                },
                marker_offset: 0,
                padding: 0,
                start: start as usize,
                delimiter: ListDelimType::Period,
                bullet_char: b'-',
                tight,
                is_task_list: false,
            };
            if children.is_empty() {
                return Err(ProseMirrorError::UnexpectedShape {
                    path: path.into(),
                    reason: "a list needs at least one item".into(),
                });
            }
            let n = arena.alloc(NodeValue::List(list_data).into());
            let mut any_task = false;
            for (i, child) in children.iter().enumerate() {
                let item_path = format!("{path}/content/{i}");
                if child.get("type").and_then(Value::as_str) != Some("listItem") {
                    return Err(ProseMirrorError::UnexpectedShape {
                        path: item_path,
                        reason: "lists may only contain listItem".into(),
                    });
                }
                let item: &AstNode = match child.get("attrs").and_then(|a| a.get("task")) {
                    None | Some(Value::Null) => arena.alloc(NodeValue::Item(list_data).into()),
                    Some(Value::Bool(false)) => {
                        any_task = true;
                        arena.alloc(
                            NodeValue::TaskItem(NodeTaskItem {
                                symbol: None,
                                symbol_sourcepos: zero_sourcepos(),
                            })
                            .into(),
                        )
                    }
                    Some(Value::String(s)) if s == "x" || s == "X" => {
                        any_task = true;
                        arena.alloc(
                            NodeValue::TaskItem(NodeTaskItem {
                                symbol: s.chars().next(),
                                symbol_sourcepos: zero_sourcepos(),
                            })
                            .into(),
                        )
                    }
                    Some(other) => {
                        return Err(ProseMirrorError::InvalidAttr {
                            node: "listItem".into(),
                            attr: "task".into(),
                            reason: format!("must be null, false, \"x\" or \"X\", got {other}"),
                        })
                    }
                };
                let item_blocks: Vec<Value> = child
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if item_blocks.is_empty() {
                    return Err(ProseMirrorError::UnexpectedShape {
                        path: item_path,
                        reason: "a list item needs at least one block".into(),
                    });
                }
                for (j, block) in item_blocks.iter().enumerate() {
                    item.append(build_block(
                        block,
                        &format!("{item_path}/content/{j}"),
                        depth + 1,
                        arena,
                        ctx,
                    )?);
                }
                n.append(item);
            }
            if any_task {
                if let NodeValue::List(ref mut l) = n.data_mut().value {
                    l.is_task_list = true;
                }
            }
            n
        }
        "table" => {
            let alignments_json = attrs
                .get("alignments")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut alignments = Vec::with_capacity(alignments_json.len());
            for (i, a) in alignments_json.iter().enumerate() {
                alignments.push(match a.as_str() {
                    Some("none") => TableAlignment::None,
                    Some("left") => TableAlignment::Left,
                    Some("center") => TableAlignment::Center,
                    Some("right") => TableAlignment::Right,
                    other => {
                        return Err(ProseMirrorError::InvalidAttr {
                            node: "table".into(),
                            attr: format!("alignments[{i}]"),
                            reason: format!(
                                "must be \"none\"|\"left\"|\"center\"|\"right\", got {other:?}"
                            ),
                        })
                    }
                });
            }
            let width = alignments.len();
            if children.is_empty() {
                return Err(ProseMirrorError::UnexpectedShape {
                    path: path.into(),
                    reason: "a table needs at least its header row".into(),
                });
            }
            let n = arena.alloc(
                NodeValue::Table(Box::new(NodeTable {
                    num_columns: width,
                    num_rows: children.len(),
                    num_nonempty_cells: 0,
                    alignments,
                }))
                .into(),
            );
            let mut nonempty = 0usize;
            for (i, row) in children.iter().enumerate() {
                let row_path = format!("{path}/content/{i}");
                if row.get("type").and_then(Value::as_str) != Some("tableRow") {
                    return Err(ProseMirrorError::UnexpectedShape {
                        path: row_path,
                        reason: "tables may only contain tableRow".into(),
                    });
                }
                let is_header = row
                    .get("attrs")
                    .and_then(|a| a.get("isHeader"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                // comrak's parser marks exactly the first row as the header;
                // the formatter's delimiter row is written from that flag.
                if is_header != (i == 0) {
                    return Err(ProseMirrorError::UnexpectedShape {
                        path: row_path,
                        reason: "exactly the first row may be the header row".into(),
                    });
                }
                let row_node = arena.alloc(NodeValue::TableRow(is_header).into());
                let cells: Vec<Value> = row
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if cells.len() != width {
                    return Err(ProseMirrorError::UnexpectedShape {
                        path: format!("{row_path}/content"),
                        reason: format!(
                            "row has {} cells but the table has {width} columns",
                            cells.len()
                        ),
                    });
                }
                for (j, cell) in cells.iter().enumerate() {
                    let cell_path = format!("{row_path}/content/{j}");
                    let wanted = if is_header {
                        "tableHeader"
                    } else {
                        "tableCell"
                    };
                    if cell.get("type").and_then(Value::as_str) != Some(wanted) {
                        return Err(ProseMirrorError::UnexpectedShape {
                            path: cell_path,
                            reason: format!(
                                "a {} row may only contain {wanted}",
                                if is_header { "header" } else { "body" }
                            ),
                        });
                    }
                    let cell_node = arena.alloc(NodeValue::TableCell.into());
                    let cell_blocks: Vec<Value> = cell
                        .get("content")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    // The schema gives a cell exactly one paragraph.
                    if cell_blocks.len() > 1 {
                        return Err(ProseMirrorError::UnexpectedShape {
                            path: format!("{cell_path}/content"),
                            reason: "a table cell is exactly one paragraph".into(),
                        });
                    }
                    if let Some(par) = cell_blocks.first() {
                        if par.get("type").and_then(Value::as_str) != Some("paragraph") {
                            return Err(ProseMirrorError::UnexpectedShape {
                                path: format!("{cell_path}/content/0"),
                                reason: "a table cell is exactly one paragraph".into(),
                            });
                        }
                        let par_children: Vec<Value> = par
                            .get("content")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        if !par_children.is_empty() {
                            nonempty += 1;
                        }
                        for inline in build_inline_run(
                            &run_items(&par_children, &cell_path)?,
                            arena,
                            ctx,
                            depth + 2,
                        )? {
                            cell_node.append(inline);
                        }
                    }
                    row_node.append(cell_node);
                }
                n.append(row_node);
            }
            if let NodeValue::Table(ref mut t) = n.data_mut().value {
                t.num_nonempty_cells = nonempty;
            }
            n
        }
        "doc" => {
            return Err(ProseMirrorError::UnexpectedShape {
                path: path.into(),
                reason: "the doc node may only appear at the root".into(),
            })
        }
        other => return Err(ProseMirrorError::UnknownNode { kind: other.into() }),
    };
    Ok(node)
}

/// Convert a flat run of marked inline PM nodes into comrak inline nodes.
/// ProseMirror flattens nesting into mark arrays (outermost first); comrak
/// nests — so consecutive children sharing an outermost mark are regrouped
/// under one mark node, recursively.
fn build_inline_run<'a>(
    items: &[(Value, Vec<Value>)],
    arena: &'a Arena<'a>,
    ctx: &mut Count,
    depth: u32,
) -> Result<Vec<&'a AstNode<'a>>, ProseMirrorError> {
    ctx.tick()?;
    depth_guard(depth)?;
    let mut out: Vec<&AstNode> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let (v, marks) = &items[i];
        let Some(kind) = v.get("type").and_then(Value::as_str) else {
            return Err(ProseMirrorError::UnexpectedShape {
                path: "/".into(),
                reason: "every inline node needs a string `type`".into(),
            });
        };
        if marks.is_empty() {
            out.push(build_inline_leaf(v, kind, arena, ctx, depth)?);
            i += 1;
            continue;
        }
        let outer = marks[0].clone();
        let outer_kind = outer.get("type").and_then(Value::as_str).unwrap_or("");
        let mut run: Vec<(Value, Vec<Value>)> = Vec::new();
        while i < items.len() && items[i].1.first() == Some(&outer) {
            let (rv, rm) = &items[i];
            run.push((rv.clone(), rm[1..].to_vec()));
            i += 1;
        }
        match outer_kind {
            // Code is a leaf: its run is one literal, never children.
            "code" => {
                let mut literal = String::new();
                for (rv, remaining) in &run {
                    if !remaining.is_empty() {
                        return Err(ProseMirrorError::UnexpectedShape {
                            path: "/".into(),
                            reason: "the code mark must be the innermost mark".into(),
                        });
                    }
                    let text = rv.get("text").and_then(Value::as_str).ok_or(
                        ProseMirrorError::UnexpectedShape {
                            path: "/".into(),
                            reason: "code marks may only cover text".into(),
                        },
                    )?;
                    literal.push_str(text);
                }
                out.push(
                    arena.alloc(
                        NodeValue::Code(NodeCode {
                            num_backticks: 1,
                            literal,
                        })
                        .into(),
                    ),
                );
            }
            "bold" | "italic" | "strike" | "link" => {
                let children = build_inline_run(&run, arena, ctx, depth + 1)?;
                let n: &AstNode = match outer_kind {
                    "bold" => arena.alloc(NodeValue::Strong.into()),
                    "italic" => arena.alloc(NodeValue::Emph.into()),
                    "strike" => arena.alloc(NodeValue::Strikethrough.into()),
                    _ => {
                        let link_attrs = outer.get("attrs").cloned().unwrap_or(Value::Null);
                        let href = link_attrs
                            .get("href")
                            .and_then(Value::as_str)
                            .ok_or(ProseMirrorError::InvalidAttr {
                                node: "link".into(),
                                attr: "href".into(),
                                reason: "the link mark needs a string href".into(),
                            })?
                            .to_string();
                        let title = link_attrs
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        arena.alloc(NodeValue::Link(Box::new(NodeLink { url: href, title })).into())
                    }
                };
                for child in children {
                    n.append(child);
                }
                out.push(n);
            }
            other => return Err(ProseMirrorError::UnknownMark { kind: other.into() }),
        }
    }
    Ok(out)
}

fn build_inline_leaf<'a>(
    v: &Value,
    kind: &str,
    arena: &'a Arena<'a>,
    ctx: &mut Count,
    depth: u32,
) -> Result<&'a AstNode<'a>, ProseMirrorError> {
    ctx.tick()?;
    depth_guard(depth)?;
    match kind {
        "text" => {
            let text =
                v.get("text")
                    .and_then(Value::as_str)
                    .ok_or(ProseMirrorError::UnexpectedShape {
                        path: "/".into(),
                        reason: "text nodes need a string `text` field".into(),
                    })?;
            Ok(arena.alloc(NodeValue::Text(text.to_string().into()).into()))
        }
        "hardBreak" => {
            let soft = v
                .get("attrs")
                .and_then(|a| a.get("soft"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Ok(arena.alloc(
                (if soft {
                    NodeValue::SoftBreak
                } else {
                    NodeValue::LineBreak
                })
                .into(),
            ))
        }
        "image" => {
            let attrs = v.get("attrs").cloned().unwrap_or(Value::Null);
            let src = get_str(&attrs, "image", "src", "/")?.to_string();
            let title = attrs
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let n = arena.alloc(NodeValue::Image(Box::new(NodeLink { url: src, title })).into());
            let alt: Vec<Value> = v
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for child in build_inline_run(&run_items(&alt, "/")?, arena, ctx, depth + 1)? {
                n.append(child);
            }
            Ok(n)
        }
        "wikiLink" => {
            let attrs = v.get("attrs").cloned().unwrap_or(Value::Null);
            let target = get_str(&attrs, "wikiLink", "target", "/")?;
            if target.is_empty() || target.contains(['[', ']', '|', '\n', '\r']) {
                return Err(ProseMirrorError::InvalidAttr {
                    node: "wikiLink".into(),
                    attr: "target".into(),
                    reason: "must be non-empty and free of brackets, pipes and newlines".into(),
                });
            }
            let label: Vec<Value> = v
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if label.is_empty() {
                return Err(ProseMirrorError::InvalidAttr {
                    node: "wikiLink".into(),
                    attr: "target".into(),
                    reason: "a wiki-link needs label text (write [[target|label]])".into(),
                });
            }
            let n = arena.alloc(
                NodeValue::WikiLink(NodeWikiLink {
                    url: target.to_string(),
                })
                .into(),
            );
            for child in build_inline_run(&run_items(&label, "/")?, arena, ctx, depth + 1)? {
                n.append(child);
            }
            Ok(n)
        }
        "htmlInline" => {
            let attrs = v.get("attrs").cloned().unwrap_or(Value::Null);
            let literal = get_str(&attrs, "htmlInline", "literal", "/")?;
            if literal.is_empty() {
                return Err(ProseMirrorError::InvalidAttr {
                    node: "htmlInline".into(),
                    attr: "literal".into(),
                    reason: "must be a non-empty string".into(),
                });
            }
            Ok(arena.alloc(NodeValue::HtmlInline(literal.to_string()).into()))
        }
        other => Err(ProseMirrorError::UnexpectedShape {
            path: "/".into(),
            reason: format!("`{other}` is a block node and cannot appear inline"),
        }),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The §12.2 invariant: a document that went markdown → PM → markdown
    /// must come back as exactly the canonical (`dit fmt`) bytes.
    fn round_trips(input: &str) {
        let canonical = crate::fmt::format_body(input).unwrap();
        let doc = markdown_to_doc(&canonical).unwrap();
        let out = doc_to_markdown(&doc).unwrap();
        assert_eq!(
            out, canonical,
            "\n--- canonical ---\n{canonical}\n--- bridge ---\n{out}"
        );
    }

    /// The other direction: once markdown is a PM document, the bridge must
    /// not lose or reshuffle structure — PM → md → PM is the identity.
    fn pm_is_stable(input: &str) {
        let canonical = crate::fmt::format_body(input).unwrap();
        let doc = markdown_to_doc(&canonical).unwrap();
        let back = markdown_to_doc(&doc_to_markdown(&doc).unwrap()).unwrap();
        assert_eq!(back, doc, "\n{doc}\nvs\n{back}");
    }

    fn both(input: &str) {
        round_trips(input);
        pm_is_stable(input);
    }

    #[test]
    fn plain_text_and_emphasis() {
        both("Some  *bold*   text\n");
        both("**strong**, *em*, `code`, ~~gone~~\n");
    }

    #[test]
    fn headings_one_to_six() {
        both("# one\n\n## two\n\n### three\n\n#### four\n\n##### five\n\n###### six\n");
        // A closed ATX heading and a setext heading normalize first; the
        // bridge must round-trip whatever the canonical form is.
        both("# trailing hashes #\n");
        both("Setext\n======\n\nSetext two\n---\n");
    }

    #[test]
    fn nested_lists_tight_and_loose() {
        both("- a\n- b\n  - b1\n  - b2\n    - deep\n- c\n");
        both("1. one\n2. two\n   1. nested\n   2. deeper\n3. three\n");
        // Loose: blank lines between items.
        both("- a\n\n- b\n\n- c\n");
        // Mixed tight outer, loose inner.
        both("- a\n  - a1\n\n  - a2\n- b\n");
        both("3. three\n4. four\n");
    }

    #[test]
    fn mixed_task_and_plain_items() {
        let input = "- [ ] a\n- b\n- [X] c\n- [x] d\n";
        both(input);
        // The task flag is per item, so the JSON must record it per item.
        let canonical = crate::fmt::format_body(input).unwrap();
        let doc = markdown_to_doc(&canonical).unwrap();
        let list = &doc["content"][0];
        assert_eq!(list["type"], "bulletList");
        let items = list["content"].as_array().unwrap();
        assert_eq!(items[0]["type"], "listItem");
        assert_eq!(items[0]["attrs"]["task"], json!(false));
        assert_eq!(items[1]["attrs"]["task"], json!(null));
        assert_eq!(items[2]["attrs"]["task"], "X");
        assert_eq!(items[3]["attrs"]["task"], "x");
        // Tasks can hang off ordered lists too.
        both("1. [ ] first\n2. [x] second\n");
    }

    #[test]
    fn blockquote_nested_and_in_list() {
        both("> quoted\n> more\n");
        both("> > double\n");
        // The §12.3 case: a blockquote inside a list item.
        both("- item\n\n  > quoted inside\n");
        both("> - list inside quote\n> - second\n");
    }

    #[test]
    fn list_followed_by_fence() {
        // cm.rs inserts an <!-- end list --> comment between a tight list and
        // a fence; it must survive the bridge like any other HTML block.
        both("- a\n- b\n\n```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn hard_and_soft_breaks() {
        // Canonical hard break is the backslash form (fmt.rs pins it).
        both("line one\\\nline two\n");
        both("line one  \nline two\n");
        // Soft break: single newline inside a paragraph.
        both("line one\nline two\n");
        // Breaks inside emphasis keep their marks.
        both("*line one\\\nline two*\n");
    }

    #[test]
    fn code_blocks_with_info_strings() {
        both("```\nplain fence\n```\n");
        both("```rust\nfn main() {}\n```\n");
        both("```dit:query\nstatus = \"todo\"\n```\n");
        both("```mermaid\ngraph TD; A-->B;\n```\n");
        // Info strings keep everything after the fence, verbatim.
        both("```rust ignore\nfn main() {}\n```\n");
        // Indented code normalizes first; canonical form must round-trip.
        both("    indented code\n");
        both("```mermaid\n<<<<<<< HEAD\nlooks conflicted\n```\n");
        // A fence containing fence lines must come back at the same length.
        both("````\ninner ``` fence\n````\n");
        both("````\ninner ``` fence\ninner ```` fence\n`````\n");
    }

    #[test]
    fn empty_code_block() {
        both("```\n```\n");
    }

    #[test]
    fn wikilinks_short_and_titled() {
        both("see [[docs/flows/auth-session]] here\n");
        both("see [[docs/flows/auth-session|the auth flow]] here\n");
        both("[[x|x]] stays collapsed\n");
        // A wikilink next to plain brackets (not a wikilink).
        both("[plain link text](https://example.com) and [[wiki|title]]\n");
    }

    #[test]
    fn links_images_autolinks() {
        both("[title](https://example.com)\n");
        both("[title](https://example.com \"with title\")\n");
        both("<https://example.com>\n");
        both("bare https://example.com in text\n");
        both("<someone@example.com>\n");
        both("![alt text](https://example.com/pic.png)\n");
        both("![alt text](https://example.com/pic.png \"title\")\n");
        both("[](https://example.com/empty-alt)\n");
        both("[![image in link](https://x/i.png)](https://x)\n");
        both("[`code in link`](https://example.com)\n");
        // Parenthesised and percent-encoded destinations.
        both("[wiki](https://example.com/Foo_\\(bar\\))\n");
    }

    #[test]
    fn tables_all_alignments() {
        let input = "| a | b | c | d |\n|---|:--|:-:|--:|\n| 1 | 2 | 3 | 4 |\n| x | y | z | w |\n";
        both(input);
        let canonical = crate::fmt::format_body(input).unwrap();
        let doc = markdown_to_doc(&canonical).unwrap();
        let table = &doc["content"][0];
        assert_eq!(table["type"], "table");
        assert_eq!(
            table["attrs"]["alignments"],
            json!(["none", "left", "center", "right"])
        );
        let rows = table["content"].as_array().unwrap();
        assert_eq!(rows[0]["type"], "tableRow");
        assert_eq!(rows[0]["attrs"]["isHeader"], true);
        assert_eq!(rows[0]["content"][0]["type"], "tableHeader");
        assert_eq!(rows[1]["content"][0]["type"], "tableCell");
        // Single-column and single-row tables.
        both("| only |\n|---|\n| one |\n");
        both("| a | b |\n|---|---|\n");
    }

    #[test]
    fn raw_html_block_and_inline() {
        both("<div class=\"x\">\n  <span>raw</span>\n</div>\n");
        both("<!-- a comment block -->\n");
        both("text with <br> inline html and <b>bold</b> tags\n");
        both("<p>\nparagraph block\n</p>\n");
    }

    #[test]
    fn entities_and_escapes() {
        both("amp &amp; &#65; &notanentity; plain &\n");
        both("\\*not emphasis\\* \\_ \\# \\[ \\]\n");
        both("5 \\* 3 \\* 2\n");
    }

    #[test]
    fn degenerate_inputs() {
        both("");
        both("\n\n\n");
        both("   \n  \n");
        // CRLF normalizes first; canonical LF form must round-trip.
        both("line one\r\nline two\r\n");
        // Unicode, emoji, CJK.
        both("h\u{e9}llo \u{2014} na\u{ef}ve \u{65e5}\u{672c}\u{8a9e} \u{1f389} text\n");
        // Whitespace-only code span and long inline.
        both("a ` ` b\n");
    }

    #[test]
    fn horizontal_rule_forms() {
        both("---\n");
        both("***\n");
        both("___\n");
        both("text\n\n---\n\nmore text\n");
    }

    #[test]
    fn kitchen_sink() {
        both(concat!(
            "# Kitchen sink\n",
            "\n",
            "A paragraph with *emphasis*, **strong**, `code`, ~~strike~~, ",
            "[a link](https://example.com \"t\"), an ![image](i.png), ",
            "[[wiki-link|titled]], [[short]], and <br> inline HTML.\n",
            "\n",
            "## Lists\n",
            "\n",
            "- [ ] a task\n",
            "- plain item\n",
            "  1. nested ordered\n",
            "  2. second\n",
            "- [X] done task\n",
            "\n",
            "> a quote\n",
            "> with two lines\\\n",
            "> and a hard break\n",
            "\n",
            "```dit:query\n",
            "status = \"todo\" AND assignee = @me\n",
            "```\n",
            "\n",
            "| h1 | h2 |\n",
            "|:--|--:|\n",
            "| a  | b  |\n",
            "\n",
            "---\n",
            "\n",
            "<div>\n",
            "html block\n",
            "</div>\n",
            "\n",
            "Trailing text &amp; entities.\n",
        ));
    }

    #[test]
    fn conflicted_markdown_is_refused_not_mangled() {
        let conflicted = "text\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\n";
        assert!(matches!(
            markdown_to_doc(conflicted),
            Err(ProseMirrorError::Fmt(crate::fmt::FmtError::ConflictMarkers))
        ));
    }

    #[test]
    fn typed_in_conflict_markers_refuse_on_serialize() {
        // A paragraph whose text starts like a marker must be refused on the
        // way out, or saving would silently fuse a conflicted file.
        let doc = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [
                    {"type": "text", "text": "before\n"},
                    {"type": "hardBreak", "attrs": {"soft": true}},
                    {"type": "text", "text": "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> f\n"}
                ]
            }]
        });
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::Fmt(crate::fmt::FmtError::ConflictMarkers))
        ));
    }

    #[test]
    fn unknown_node_type_is_refused_never_dropped() {
        let doc = json!({
            "type": "doc",
            "content": [{"type": "novel-block", "attrs": {"x": 1}}]
        });
        assert_eq!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::UnknownNode {
                kind: "novel-block".into()
            })
        );
        // Unknown *attrs* on a known node are fine — they cannot introduce
        // a construct the schema has no home for.
        let doc = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "attrs": {"data-track-id": 7},
                "content": [{"type": "text", "text": "ok"}]
            }]
        });
        assert!(doc_to_markdown(&doc).is_ok());
    }

    #[test]
    fn unknown_mark_type_is_refused() {
        let doc = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "x", "marks": [{"type": "glitter"}]}]
            }]
        });
        assert_eq!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::UnknownMark {
                kind: "glitter".into()
            })
        );
    }

    #[test]
    fn hostile_depth_is_cut_off_not_crashed() {
        let mut doc = json!({
            "type": "doc",
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "deep"}]}]
        });
        for _ in 0..200 {
            doc = json!({
                "type": "doc",
                "content": [{
                    "type": "blockquote",
                    "content": doc["content"].as_array().unwrap().clone()
                }]
            });
        }
        assert_eq!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::TooDeep { limit: MAX_DEPTH })
        );
        // A genuinely deep-but-legal document under the cap still works.
        let mut nested = String::new();
        for _ in 0..40 {
            nested.push_str("> ");
        }
        nested.push_str("forty levels\n");
        round_trips(&nested);
    }

    #[test]
    fn hostile_node_count_is_cut_off() {
        let paragraphs: Vec<Value> = (0..(MAX_NODES + 10))
            .map(|_| json!({"type": "paragraph"}))
            .collect();
        let doc = json!({"type": "doc", "content": paragraphs});
        assert_eq!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::TooLarge { limit: MAX_NODES })
        );
    }

    #[test]
    fn bad_shapes_are_named_not_panicked() {
        // Heading level out of range.
        let doc = json!({
            "type": "doc",
            "content": [{"type": "heading", "attrs": {"level": 9}}]
        });
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::InvalidAttr { .. })
        ));
        // Language that would break the fence.
        let doc = json!({
            "type": "doc",
            "content": [{"type": "codeBlock", "attrs": {"language": "a`b"}, "content": []}]
        });
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::InvalidAttr { .. })
        ));
        // A block where an inline belongs.
        let doc = json!({
            "type": "doc",
            "content": [{"type": "paragraph", "content": [{"type": "blockquote"}]}]
        });
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::UnexpectedShape { .. })
        ));
        // A table row wider than the alignments.
        let doc = json!({
            "type": "doc",
            "content": [{
                "type": "table",
                "attrs": {"alignments": ["none"]},
                "content": [{
                    "type": "tableRow",
                    "attrs": {"isHeader": true},
                    "content": [
                        {"type": "tableHeader", "content": []},
                        {"type": "tableHeader", "content": []}
                    ]
                }]
            }]
        });
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::UnexpectedShape { .. })
        ));
        // A wiki-link target that would break the [[ ]] syntax.
        let doc = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "wikiLink",
                    "attrs": {"target": "a]b"},
                    "content": [{"type": "text", "text": "t"}]
                }]
            }]
        });
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::InvalidAttr { .. })
        ));
        // Non-object content.
        let doc = json!({"type": "doc", "content": ["just a string"]});
        assert!(matches!(
            doc_to_markdown(&doc),
            Err(ProseMirrorError::UnexpectedShape { .. })
        ));
    }

    #[test]
    fn every_markdown_file_in_the_repo_round_trips() {
        // The v0.4 exit criterion in miniature: the corpus of real markdown
        // this repo carries (design docs, ADRs) must cross the bridge without
        // losing a byte.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = vec![
            root.join("README.md"),
            root.join("DESIGN.md"),
            root.join("ARCHITECTURE.md"),
            root.join("CLAUDE.md"),
        ];
        fn walk_md(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    walk_md(&path, out);
                } else if path.extension().is_some_and(|x| x == "md") {
                    out.push(path);
                }
            }
        }
        walk_md(&root.join("docs"), &mut files);
        let mut checked = 0usize;
        for path in &files {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            // Skip only genuinely conflicted files (defense #5); everything
            // else must round-trip.
            let Ok(canonical) = crate::fmt::format_body(&text) else {
                continue;
            };
            let doc = markdown_to_doc(&canonical)
                .unwrap_or_else(|e| panic!("{}: to-doc failed: {e}", path.display()));
            let out = doc_to_markdown(&doc)
                .unwrap_or_else(|e| panic!("{}: to-md failed: {e}", path.display()));
            assert_eq!(
                out,
                canonical,
                "{} leaked through the bridge",
                path.display()
            );
            checked += 1;
        }
        assert!(checked >= 5, "corpus sweep checked only {checked} files");
    }

    #[test]
    fn a_crlf_document_round_trips_like_a_second_fmt_pass() {
        // A Windows clone can smudge a file to CRLF, and format_body is not
        // a fixed point on such input: when a line wrap lands inside a code
        // span, comrak keeps the CR in the span's literal on the first pass
        // and normalizes it away on the next parse. The bridge re-parses, so
        // its output matches a second fmt pass, not the first — which is why
        // the corpus test above needs checkouts to stay LF (.gitattributes
        // pins that) rather than comparing against mutated bytes.
        let crlf = "run `cargo install\r\njust wasm-pack` now\r\n";
        let once = crate::fmt::format_body(crlf).unwrap();
        let twice = crate::fmt::format_body(&once).unwrap();
        assert_ne!(
            once, twice,
            "premise changed: fmt became idempotent on CRLF input"
        );
        let doc = markdown_to_doc(&once).unwrap();
        let out = doc_to_markdown(&doc).unwrap();
        assert_eq!(out, twice);
    }
}
