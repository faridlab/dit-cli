//! Document templates (ADR 0031): the kinds of document a project may need,
//! built in and overridable per workspace, and a page made from one.
//!
//! A template is a Markdown file whose frontmatter describes the template —
//! `name`, `summary`, `folder` — and whose body is the page to write, with
//! `{{title}}`, `{{date}}` and `{{author}}` filled in. The page gets its own
//! frontmatter: `kind` and `title`, both authored facts (I5).

use std::path::PathBuf;

use dit_model::DocPath;
use dit_parse::{serialize_scalar, Document};
use time::OffsetDateTime;

use crate::{Dit, DitError, Transaction};

/// Where a workspace keeps its own templates, inside its docs root.
pub const DOC_TEMPLATE_DIR: &str = ".templates";

/// The built-in kinds, in the order a project usually writes them.
const BUILT_IN: [(&str, &str); 11] = [
    ("brd", include_str!("../templates/docs/brd.md")),
    ("prd", include_str!("../templates/docs/prd.md")),
    ("srs", include_str!("../templates/docs/srs.md")),
    ("fsd", include_str!("../templates/docs/fsd.md")),
    (
        "business-flow",
        include_str!("../templates/docs/business-flow.md"),
    ),
    ("tsd", include_str!("../templates/docs/tsd.md")),
    (
        "data-model",
        include_str!("../templates/docs/data-model.md"),
    ),
    (
        "api-contract",
        include_str!("../templates/docs/api-contract.md"),
    ),
    ("adr", include_str!("../templates/docs/adr.md")),
    ("test-plan", include_str!("../templates/docs/test-plan.md")),
    (
        "release-notes",
        include_str!("../templates/docs/release-notes.md"),
    ),
];

/// One kind of document a page can be made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocTemplate {
    pub id: String,
    pub name: String,
    /// What the document answers, in a sentence.
    pub summary: String,
    /// Where a page of this kind is placed, e.g. `docs/business`.
    pub folder: String,
    /// DIT ships this kind.
    pub built_in: bool,
    /// The workspace's own file replaces the built-in.
    pub overridden: bool,
}

/// A template with the page body it writes.
struct Loaded {
    meta: DocTemplate,
    body: String,
}

fn load(id: &str, text: &str, built_in: bool, overridden: bool) -> Result<Loaded, DitError> {
    let bad = |why: String| {
        DitError::Refuse(format!(
            "the `{id}` document template cannot be used: {why}"
        ))
    };
    let doc = Document::parse(text).map_err(|e| bad(e.to_string()))?;
    let field = |key: &str| doc.get_str(key).flatten().filter(|v| !v.trim().is_empty());
    let folder = field("folder").unwrap_or_else(|| "docs".to_owned());
    let folder = folder.trim_end_matches('/').to_owned();
    // The folder must be a place a page may live — checked once, here.
    DocPath::parse(&format!("{folder}/page.md"))
        .map_err(|e| bad(format!("folder `{folder}`: {e}")))?;
    Ok(Loaded {
        meta: DocTemplate {
            id: id.to_owned(),
            name: field("name").unwrap_or_else(|| id.to_owned()),
            summary: field("summary").unwrap_or_default(),
            folder,
            built_in,
            overridden,
        },
        body: doc.body().to_owned(),
    })
}

/// A template id is a file name a person can type: lowercase words.
fn is_template_id(id: &str) -> bool {
    !id.is_empty()
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A page's file name from its title: `Checkout: the shopper's way` →
/// `checkout-the-shopper-s-way`.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out = out.trim_end_matches('-').to_owned();
    out.truncate(80);
    out.trim_end_matches('-').to_owned()
}

impl Dit {
    fn doc_template_dir(&self) -> PathBuf {
        self.store.layout().docs_dir().join(DOC_TEMPLATE_DIR)
    }

    /// The workspace's own template files, by id.
    fn own_templates(&self) -> Result<Vec<(String, String)>, DitError> {
        let dir = self.doc_template_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut own = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let Some(id) = path
                .extension()
                .filter(|e| *e == "md")
                .and_then(|_| path.file_stem())
                .and_then(|s| s.to_str())
            else {
                continue;
            };
            if is_template_id(id) {
                own.push((id.to_owned(), std::fs::read_to_string(&path)?));
            }
        }
        own.sort();
        Ok(own)
    }

    fn loaded_templates(&self) -> Result<Vec<Loaded>, DitError> {
        let own = self.own_templates()?;
        let mut all = Vec::new();
        for (id, text) in BUILT_IN {
            match own.iter().find(|(o, _)| o == id) {
                Some((_, mine)) => all.push(load(id, mine, true, true)?),
                None => all.push(load(id, text, true, false)?),
            }
        }
        for (id, text) in &own {
            if !BUILT_IN.iter().any(|(b, _)| b == id) {
                all.push(load(id, text, false, false)?);
            }
        }
        Ok(all)
    }

    /// Every kind of document a page can be made from: the built-ins in the
    /// order a project writes them (each replaced by the workspace's own
    /// `docs/.templates/<id>.md` when there is one), then the workspace's
    /// own kinds.
    pub fn doc_templates(&self) -> Result<Vec<DocTemplate>, DitError> {
        Ok(self
            .loaded_templates()?
            .into_iter()
            .map(|l| l.meta)
            .collect())
    }
}

impl Transaction<'_> {
    /// Write a new page of kind `kind` titled `title`, at
    /// `<folder>/<slug of title>.md`, and answer its path. A page already
    /// there is never overwritten.
    pub fn write_doc_from_template(&mut self, kind: &str, title: &str) -> Result<String, DitError> {
        let title = title.trim();
        let template = self
            .dit
            .loaded_templates()?
            .into_iter()
            .find(|t| t.meta.id == kind)
            .ok_or_else(|| {
                DitError::Missing(format!(
                    "document template `{kind}` — `dit docs templates` lists them"
                ))
            })?;
        let name = slug(title);
        if name.is_empty() {
            return Err(DitError::Refuse(
                "a document title needs letters or digits".into(),
            ));
        }
        let path = format!("{}/{name}.md", template.meta.folder);
        DocPath::parse(&path)?;
        match self.dit.read_doc(&path) {
            Ok(_) => {
                return Err(DitError::Refuse(format!(
                    "a page already exists at `{path}` — open it, or choose another title"
                )))
            }
            Err(DitError::Missing(_)) => {}
            Err(e) => return Err(e),
        }
        let today = OffsetDateTime::now_utc().date().to_string();
        let body = template
            .body
            .replace("{{title}}", title)
            .replace("{{date}}", &today)
            .replace("{{author}}", self.store_tx.author());
        let mut page = Document::parse("").map_err(|e| DitError::Refuse(e.to_string()))?;
        page.set_raw("kind", &serialize_scalar(kind));
        page.set_raw("title", &serialize_scalar(title));
        page.set_body(body);
        self.write_doc(&path, &page.serialize())?;
        Ok(path)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_title_becomes_a_file_name() {
        assert_eq!(
            slug("Checkout: the shopper's way"),
            "checkout-the-shopper-s-way"
        );
        assert_eq!(slug("  Q3 — Plan  "), "q3-plan");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn every_built_in_template_loads() {
        for (id, text) in BUILT_IN {
            let loaded = load(id, text, true, false).unwrap();
            assert!(loaded.body.contains("{{title}}"), "{id}");
        }
    }
}
