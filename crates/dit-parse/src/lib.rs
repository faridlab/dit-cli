//! Parsing and serialization for DIT's file formats.
//!
//! Everything here is pure: bytes in, bytes or typed values out, no I/O.
//! Two parsers live side by side on purpose:
//!
//! - [`frontmatter::Document`] — line-oriented, surgical, byte-preserving.
//!   The write path. Issue files must never be re-serialized from scratch:
//!   unknown fields and human comments would be destroyed, and the merge
//!   driver's per-field decisions would be meaningless.
//! - [`yaml`] — a small typed-tree parser for the schema files
//!   (`workflow.yaml`, `config.yaml`), which DIT reads but never rewrites.
//!
//! [`fmt`] is `dit fmt`: canonical markdown formatting over the body only.

pub mod comment;
pub mod fmt;
pub mod frontmatter;
pub mod html;
pub mod issue;
pub mod prosemirror;
pub mod schema;
pub mod yaml;

pub use comment::{parse_comment, serialize_comment, CommentError};
pub use frontmatter::{serialize_scalar, serialize_seq, Document, FrontmatterError, Value};
pub use html::render_html;
pub use issue::{
    apply_patch, issue_from_document, parse_issue, serialize_new_issue, IssueParseError,
};
pub use prosemirror::{doc_to_markdown, markdown_to_doc, ProseMirrorError};
pub use schema::{parse_config, parse_workflow, write_config, write_workflow, SchemaError};
pub use yaml::{parse as parse_yaml, Yaml, YamlError};
