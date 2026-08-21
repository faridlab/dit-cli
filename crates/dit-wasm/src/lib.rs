//! wasm-bindgen wrapper over dit-model + dit-parse + dit-query.
//!
//! The browser editor's bridge to `dit fmt` (DESIGN.md §12.2): the same
//! Rust serializer that canonicalizes files on disk also converts between
//! markdown bytes and ProseMirror JSON in the tab, so an editor save is
//! byte-identical to a CLI save.
//!
//! The surface is deliberately two functions taking and returning strings.
//! No `js-sys`, no `web-sys`: everything that crosses the boundary is plain
//! text, error messages included — they are written to be shown in the UI.
//! The exports are one-line wrappers over `*_text` functions so the boundary
//! contract itself is testable on the native target.

use wasm_bindgen::prelude::*;

/// Parse markdown into a ProseMirror document, serialized as JSON text.
///
/// Refuses input containing git conflict markers (outside fenced code) with
/// the same message the CLI prints.
#[wasm_bindgen]
pub fn markdown_to_doc(markdown: &str) -> Result<String, JsError> {
    markdown_to_doc_text(markdown).map_err(|e| JsError::new(&e))
}

/// Serialize a ProseMirror document (JSON text) back to canonical markdown.
///
/// The output is exactly what `dit fmt` would write for the equivalent AST —
/// the round trip `doc_to_markdown(markdown_to_doc(x)) == format_body(x)` is
/// pinned by tests in `dit-parse`.
#[wasm_bindgen]
pub fn doc_to_markdown(doc_json: &str) -> Result<String, JsError> {
    doc_to_markdown_text(doc_json).map_err(|e| JsError::new(&e))
}

fn markdown_to_doc_text(markdown: &str) -> Result<String, String> {
    let doc = dit_parse::markdown_to_doc(markdown).map_err(|e| e.to_string())?;
    serde_json::to_string(&doc).map_err(|e| format!("serializing the document failed: {e}"))
}

fn doc_to_markdown_text(doc_json: &str) -> Result<String, String> {
    let doc: serde_json::Value =
        serde_json::from_str(doc_json).map_err(|e| format!("invalid JSON: {e}"))?;
    dit_parse::doc_to_markdown(&doc).map_err(|e| e.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // The boundary contract: text in, canonical text out, errors as strings
    // that make sense in the UI. These run on the native target against the
    // same functions the browser calls.

    #[test]
    fn string_boundary_round_trips() {
        let doc = markdown_to_doc_text("# Title\n\n- [ ] a task\n").unwrap();
        let md = doc_to_markdown_text(&doc).unwrap();
        assert_eq!(md, "# Title\n\n- [ ] a task\n");
    }

    #[test]
    fn conflict_markers_refuse_with_a_message() {
        let err = markdown_to_doc_text("x\n<<<<<<< HEAD\n").unwrap_err();
        assert!(err.contains("conflict markers"), "{err}");
        // A paragraph *typed as* a marker refuses on the way out, too.
        let conflicted = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"<<<<<<< HEAD"}]}]}"#;
        let err = doc_to_markdown_text(conflicted).unwrap_err();
        assert!(err.contains("conflict markers"), "{err}");
    }

    #[test]
    fn hostile_json_is_a_string_error_not_a_crash() {
        let err = doc_to_markdown_text(r#"{"type":"nope"}"#).unwrap_err();
        assert!(err.contains("doc"), "{err}");
        let err = doc_to_markdown_text("not json").unwrap_err();
        assert!(err.contains("invalid JSON"), "{err}");
    }
}
