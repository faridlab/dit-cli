//! The code map's extractor (ADR 0025): one source file in, the facts the
//! index stores out — the symbols it defines, what it imports and from where,
//! what it re-exports, what it calls, and what its types extend.
//!
//! Pure over its input: the caller reads the file through `dit-vcs` (I3) and
//! hands the text here; nothing in this crate touches the disk or the network.

mod kotlin;
mod resolve;
mod rust;
mod ts;

use dit_model::{CodeLang, FileFacts};

pub use resolve::{resolve, KotlinIndex, Resolved, RootIndex};

/// Bumped whenever extraction changes what it reads out of a file. The index
/// cache is keyed by blob, so without this a fixed extractor would never
/// reach a file that did not change.
pub const EXTRACTOR_VERSION: &str = "4";

/// Extract one file's facts. A file that fails to parse cleanly still yields
/// whatever the grammar recovered — tree-sitter parses around errors — so a
/// half-written file never drops out of the map.
pub fn extract(lang: CodeLang, text: &str) -> FileFacts {
    match lang {
        CodeLang::TypeScript => ts::extract(text, false),
        CodeLang::Tsx => ts::extract(text, true),
        CodeLang::Rust => rust::extract(text),
        CodeLang::Kotlin => kotlin::extract(text),
    }
}
