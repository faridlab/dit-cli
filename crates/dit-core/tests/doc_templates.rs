//! Document templates (ADR 0031): eleven kinds built in, placed by stage,
//! overridable per workspace, and a page made from one through a
//! transaction — against a real workspace in a tempdir.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dit_core::{Dit, DitError};

fn workspace() -> (Dit, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();
    (Dit::open(tmp.path()).unwrap(), tmp)
}

const BUILT_IN: [(&str, &str); 11] = [
    ("brd", "docs/business"),
    ("prd", "docs/business"),
    ("srs", "docs/requirements"),
    ("fsd", "docs/requirements"),
    ("business-flow", "docs/requirements"),
    ("tsd", "docs/technical"),
    ("data-model", "docs/technical"),
    ("api-contract", "docs/technical"),
    ("adr", "docs/adr"),
    ("test-plan", "docs/testing"),
    ("release-notes", "changelogs"),
];

#[test]
fn eleven_kinds_are_built_in_in_lifecycle_order_each_placed_by_stage() {
    let (dit, _tmp) = workspace();
    let templates = dit.doc_templates().unwrap();
    let got: Vec<(&str, &str)> = templates
        .iter()
        .map(|t| (t.id.as_str(), t.folder.as_str()))
        .collect();
    assert_eq!(got, BUILT_IN);
    for t in &templates {
        assert!(!t.name.is_empty() && !t.summary.is_empty(), "{t:?}");
        assert!(t.built_in && !t.overridden, "{t:?}");
    }
}

#[test]
fn every_kind_makes_a_page_with_its_title_date_and_no_placeholder_left() {
    let (mut dit, _tmp) = workspace();
    for (id, folder) in BUILT_IN {
        let mut tx = dit.transaction("farid").unwrap();
        let path = tx
            .write_doc_from_template(id, &format!("Checkout {id}"))
            .unwrap();
        tx.commit(&format!("dit docs new: {path}")).unwrap();
        assert_eq!(path, format!("{folder}/checkout-{id}.md"));
        let page = dit.read_doc(&path).unwrap();
        assert!(page.contains(&format!("# Checkout {id}")), "{page}");
        assert!(page.contains(&format!("kind: {id}")), "{page}");
        assert!(
            !page.contains("{{"),
            "a placeholder survived in {id}:\n{page}"
        );
        // Template metadata stays in the template, not the page.
        assert!(!page.contains("summary:"), "{page}");
        // A guiding prompt under the headings, for the writer to replace
        // (italic; `dit fmt` writes emphasis with `*`).
        assert!(page.contains("\n*"), "{id} has no prompts:\n{page}");
    }
}

#[test]
fn a_title_with_punctuation_survives_the_frontmatter() {
    let (mut dit, _tmp) = workspace();
    let mut tx = dit.transaction("farid").unwrap();
    let path = tx
        .write_doc_from_template("prd", "Checkout: the shopper's way")
        .unwrap();
    tx.commit("dit docs new").unwrap();
    assert_eq!(path, "docs/business/checkout-the-shopper-s-way.md");
    let page = dit.read_doc(&path).unwrap();
    let doc = dit_parse::Document::parse(&page).unwrap();
    assert_eq!(
        doc.get_str("title"),
        Some(Some("Checkout: the shopper's way".to_owned()))
    );
}

#[test]
fn a_page_that_exists_is_never_overwritten_and_an_unknown_kind_is_named() {
    let (mut dit, _tmp) = workspace();
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc_from_template("prd", "Checkout").unwrap();
    tx.commit("first").unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    assert!(matches!(
        tx.write_doc_from_template("prd", "Checkout"),
        Err(DitError::Refuse(_))
    ));
    assert!(matches!(
        tx.write_doc_from_template("memo", "Anything"),
        Err(DitError::Missing(_))
    ));
    assert!(matches!(
        tx.write_doc_from_template("prd", "!!!"),
        Err(DitError::Refuse(_))
    ));
}

#[test]
fn a_workspace_template_overrides_a_built_in_and_a_new_one_adds_a_kind() {
    let (mut dit, tmp) = workspace();
    let dir = tmp.path().join("docs/.templates");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("prd.md"),
        "---\nname: Our PRD\nsummary: The one we use.\nfolder: docs/product\n---\n# {{title}}\n\n_Our own prompt._\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("runbook.md"),
        "---\nname: Runbook\nsummary: How to operate a service.\n---\n# {{title}}\n\nWritten {{date}} by {{author}}.\n",
    )
    .unwrap();

    let templates = dit.doc_templates().unwrap();
    let prd = templates.iter().find(|t| t.id == "prd").unwrap();
    assert_eq!(prd.name, "Our PRD");
    assert!(prd.built_in && prd.overridden);
    assert_eq!(prd.folder, "docs/product");
    let runbook = templates.iter().find(|t| t.id == "runbook").unwrap();
    assert!(!runbook.built_in);
    assert_eq!(runbook.folder, "docs", "no folder named: docs/");
    assert_eq!(
        templates.last().unwrap().id,
        "runbook",
        "own kinds come last"
    );

    let mut tx = dit.transaction("farid").unwrap();
    let path = tx.write_doc_from_template("runbook", "Payments").unwrap();
    tx.commit("dit docs new").unwrap();
    assert_eq!(path, "docs/payments.md");
    let page = dit.read_doc(&path).unwrap();
    assert!(page.contains("by farid."), "{page}");
    assert!(!page.contains("{{date}}"), "{page}");
}
