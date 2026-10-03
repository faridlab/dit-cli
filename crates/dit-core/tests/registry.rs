//! The per-machine workspace registry (ADR 0028), against real directories
//! and real `dit init`s in a tempdir — never the person's own config.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dit_core::{DitError, Registry};

fn exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap()
}

#[test]
fn a_new_workspace_is_a_name_and_becomes_an_initialised_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let root = tmp.path().join("Documents/DIT");
    let mut registry = Registry::load(&config).unwrap();
    assert!(registry.workspaces().is_empty());

    let path = registry.create("acme", &root, &exe()).unwrap();
    assert_eq!(path, root.join("acme").canonicalize().unwrap());
    assert!(
        dit_core::Dit::is_workspace(&path).unwrap(),
        "dit init ran there"
    );
    assert_eq!(
        registry.default_entry().unwrap().name,
        "acme",
        "the first one is the default"
    );

    // It survives a reload, from the file.
    let reloaded = Registry::load(&config).unwrap();
    assert_eq!(reloaded.get("acme").unwrap().path, path);
    assert!(std::fs::read_to_string(config.join("workspaces.yaml"))
        .unwrap()
        .contains("acme"));
}

#[test]
fn names_are_words_and_unique_and_a_folder_is_registered_once() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let root = tmp.path().join("ws");
    let mut registry = Registry::load(&config).unwrap();
    registry.create("acme", &root, &exe()).unwrap();
    for bad in ["", "Acme Corp", "../escape", "a/b", "-dash"] {
        assert!(
            matches!(
                registry.create(bad, &root, &exe()),
                Err(DitError::Refuse(_))
            ),
            "{bad:?}"
        );
    }
    assert!(
        matches!(
            registry.create("acme", &root, &exe()),
            Err(DitError::Refuse(_))
        ),
        "taken"
    );

    // Adding the same folder under another name returns the name it has.
    let again = registry.add(None, &root.join("acme"), true).unwrap();
    assert_eq!(again, "acme");
    assert_eq!(registry.workspaces().len(), 1);
}

#[test]
fn adding_a_folder_names_it_after_itself_and_can_insist_on_a_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let mut registry = Registry::load(&config).unwrap();
    let plain = tmp.path().join("Side Project");
    std::fs::create_dir_all(&plain).unwrap();
    // The browser path: only a folder that already is a workspace.
    assert!(matches!(
        registry.add(None, &plain, true),
        Err(DitError::Refuse(_))
    ));
    // The terminal path may register any folder; the name comes from it.
    assert_eq!(registry.add(None, &plain, false).unwrap(), "side-project");
    assert!(matches!(
        registry.add(None, &tmp.path().join("missing"), false),
        Err(DitError::Refuse(_))
    ));
}

#[test]
fn removing_a_workspace_leaves_its_files_and_moves_the_default() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let root = tmp.path().join("ws");
    let mut registry = Registry::load(&config).unwrap();
    let acme = registry.create("acme", &root, &exe()).unwrap();
    registry.create("home", &root, &exe()).unwrap();
    registry.set_default("home").unwrap();
    assert_eq!(registry.default_entry().unwrap().name, "home");
    registry.remove("home").unwrap();
    assert!(
        root.join("home/.dit/config.yaml").exists(),
        "files untouched"
    );
    assert_eq!(
        registry.default_entry().unwrap().name,
        "acme",
        "the default falls back to what is left"
    );
    assert!(acme.exists());
    assert!(matches!(registry.remove("nope"), Err(DitError::Missing(_))));
    assert!(matches!(
        registry.set_default("nope"),
        Err(DitError::Missing(_))
    ));
}

#[test]
fn a_workspace_is_named_by_flag_then_environment_then_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let root = tmp.path().join("ws");
    let mut registry = Registry::load(&config).unwrap();
    registry.create("acme", &root, &exe()).unwrap();
    registry.create("home", &root, &exe()).unwrap();
    assert_eq!(
        registry
            .resolve(Some("home"), Some("acme"))
            .unwrap()
            .unwrap()
            .name,
        "home"
    );
    assert_eq!(
        registry.resolve(None, Some("acme")).unwrap().unwrap().name,
        "acme"
    );
    assert!(
        registry.resolve(None, None).unwrap().is_none(),
        "nothing named: the directory decides"
    );
    let ghost = registry.resolve(Some("ghost"), None).unwrap_err();
    assert!(matches!(ghost, DitError::Missing(_)), "{ghost:?}");
    // A workspace is not an issue, and the message must not say it is.
    assert!(
        ghost.to_string().starts_with("no workspace `ghost`"),
        "{ghost}"
    );
}

#[test]
fn a_path_with_quotes_and_spaces_survives_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let mut registry = Registry::load(&config).unwrap();
    // Both kinds of quote cannot be written faithfully; say so rather than
    // corrupt. Only where such a folder can exist: Windows forbids `"` in a
    // file name.
    #[cfg(unix)]
    {
        let odd = tmp.path().join("it's a \"project\" dir");
        std::fs::create_dir_all(&odd).unwrap();
        let name = registry.add(Some("odd"), &odd, false);
        assert!(matches!(name, Err(DitError::Refuse(_))));
    }
    let fine = tmp.path().join("it's fine");
    std::fs::create_dir_all(&fine).unwrap();
    registry.add(Some("fine"), &fine, false).unwrap();
    assert_eq!(
        Registry::load(&config).unwrap().get("fine").unwrap().path,
        fine.canonicalize().unwrap()
    );
}
