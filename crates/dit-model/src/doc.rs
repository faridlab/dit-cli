//! Doc-layer value objects (DESIGN.md §13). A `DocPath` is a validated,
//! sandboxed path to a plain Markdown page: the doc editor's whole attack
//! surface is a path string from the network, so the same rules live here —
//! in the pure core, wasm-checkable — instead of being re-derived in the
//! store, the facade, and the server.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Content roots the doc editor may write. `issues` is deliberately absent
/// even though ADR 0005 reserves it: issue bodies carry validated frontmatter
/// owned by the issue write path, and a free-form editor on top of them would
/// bypass the schema the merge driver and indexer parse.
pub const DOC_ROOTS: [&str; 4] = ["docs", "notes", "epics", "changelogs"];

const MAX_PATH_LEN: usize = 200;
const MAX_SEGMENTS: usize = 8;
const MAX_FILE_LEN: usize = 80;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DocPathError {
    #[error("a doc path is required")]
    Empty,
    #[error("a doc path may be at most 200 characters, got {0}")]
    TooLong(usize),
    #[error("a doc path may nest at most 8 folders deep, got {0} segments")]
    TooDeep(usize),
    #[error("`{0}` must start with one of: docs, notes, epics, changelogs")]
    BadRoot(String),
    #[error("`{0}` escapes the workspace — absolute paths and `.`/`..` are not doc paths")]
    Traversal(String),
    #[error("folder names may use lowercase letters, digits and dashes: `{0}`")]
    BadFolder(String),
    #[error(
        "`{0}` is not a Markdown file name (lowercase letters, digits, dashes, underscores; must end in .md)"
    )]
    BadFileName(String),
}

/// A validated page path, always `<doc-root>/<folder…>/<name>.md`.
///
/// Like the other identity values in this crate: parse, don't validate —
/// once a `DocPath` exists, every segment is slug-safe and the first one is
/// a doc root, so the store can join it onto the layout's content dir
/// without re-checking anything.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocPath(String);

impl DocPath {
    pub fn parse(s: &str) -> Result<Self, DocPathError> {
        // One trailing slash is tolerated and canonicalized away — a typed
        // "docs/x.md/" means the page, not a new shape.
        let s = s.strip_suffix('/').unwrap_or(s);
        if s.is_empty() {
            return Err(DocPathError::Empty);
        }
        if s.len() > MAX_PATH_LEN {
            return Err(DocPathError::TooLong(s.len()));
        }
        let segments: Vec<&str> = s.split('/').collect();
        // A bare root ("docs" or "docs/") names no page at all; anything
        // else with a single real segment is not under a doc root.
        let real = segments.iter().filter(|seg| !seg.is_empty()).count();
        if real <= 1 {
            return if DOC_ROOTS.contains(&segments[0]) {
                Err(DocPathError::Empty)
            } else {
                Err(DocPathError::BadRoot(segments[0].to_owned()))
            };
        }
        if segments.len() > MAX_SEGMENTS {
            return Err(DocPathError::TooDeep(segments.len()));
        }
        // Traversal is checked before shape: `..`, a leading `/`, or a file
        // trying to look like one ("..md") is an escape attempt, not a typo,
        // and the error should say so.
        if s.starts_with('/') || segments.iter().any(|seg| *seg == "." || *seg == "..") {
            return Err(DocPathError::Traversal(s.to_owned()));
        }
        if !DOC_ROOTS.contains(&segments[0]) {
            return Err(DocPathError::BadRoot(segments[0].to_owned()));
        }
        let last = segments.len() - 1;
        for seg in &segments[..last] {
            let folder_ok = !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !folder_ok {
                return Err(DocPathError::BadFolder((*seg).to_owned()));
            }
        }
        let name = segments[last];
        // A name starting ".." reads as an escape attempt even though it
        // fails the shape rule too — checked first so the error says why.
        if name.starts_with("..") {
            return Err(DocPathError::Traversal(s.to_owned()));
        }
        let name_ok = !name.is_empty()
            && name.len() <= MAX_FILE_LEN
            && !name.starts_with('.')
            && !name.contains("..")
            && name.ends_with(".md")
            && name[..name.len() - 3].chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.'
            });
        if !name_ok {
            return Err(DocPathError::BadFileName(name.to_owned()));
        }
        Ok(DocPath(s.to_owned()))
    }

    /// The content root this page lives under (`docs`, `notes`, …).
    pub fn root(&self) -> &str {
        // A parsed path always has at least two segments.
        self.0.split('/').next().unwrap_or("")
    }

    /// The path below the root, for joining onto the layout's content dir.
    pub fn strip_root(&self) -> &str {
        &self.0[self.0.find('/').map(|i| i + 1).unwrap_or(self.0.len())..]
    }

    /// The file name including the `.md` suffix.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One page in the docs list. `updated_ms` is the file's mtime — display
/// metadata read from the filesystem, not a stored field; the page's real
/// history is git log (§13). Derived data never enters the source of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    pub path: DocPath,
    pub updated_ms: i64,
    pub bytes: u64,
}

macro_rules! string_serde {
    ($t:ty, $parse:path) => {
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                $parse(&raw).map_err(serde::de::Error::custom)
            }
        }
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_serde!(DocPath, DocPath::parse);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_pages_in_every_doc_root() {
        for path in [
            "docs/merge-driver.md",
            "notes/2026-08-17.md",
            "notes/learning-dsa.md",
            "epics/checkout/readme.md",
            "changelogs/2026-08.md",
            "docs/flows/auth-session/readme.md",
            "docs/adr/0001-choose-sqlite/readme.md",
        ] {
            let doc = DocPath::parse(path).unwrap();
            assert_eq!(doc.as_str(), path);
            assert!(
                DOC_ROOTS.contains(&doc.root()),
                "{path} must resolve to a doc root"
            );
        }
    }

    #[test]
    fn parse_accepts_underscore_and_inner_dots_in_file_names() {
        // §13 shows `product/_index.md` as a space index page; dotted stems
        // like `v1.2.md` stay legal. Neither may become traversal.
        assert!(DocPath::parse("docs/product/_index.md").is_ok());
        assert!(DocPath::parse("docs/v1.2-release.md").is_ok());
    }

    #[test]
    fn accessors_split_root_folder_and_file() {
        let doc = DocPath::parse("docs/flows/auth-session/readme.md").unwrap();
        assert_eq!(doc.root(), "docs");
        assert_eq!(doc.strip_root(), "flows/auth-session/readme.md");
        assert_eq!(doc.file_name(), "readme.md");
    }

    #[test]
    fn parse_orders_paths_by_root_then_path() {
        // The list view groups by root; sorting DocPaths directly must give
        // the same order the UI shows.
        let mut docs = [
            DocPath::parse("notes/a.md").unwrap(),
            DocPath::parse("docs/b.md").unwrap(),
            DocPath::parse("docs/a/zz.md").unwrap(),
        ];
        docs.sort();
        let ordered: Vec<&str> = docs.iter().map(|d| d.as_str()).collect();
        assert_eq!(ordered, vec!["docs/a/zz.md", "docs/b.md", "notes/a.md"]);
    }

    #[test]
    fn parse_rejects_empty_and_oversized_paths() {
        assert_eq!(DocPath::parse("").unwrap_err(), DocPathError::Empty);
        assert_eq!(
            DocPath::parse("docs/").unwrap_err(),
            DocPathError::Empty,
            "a bare root is not a page"
        );
        let long = format!("docs/{}.md", "a".repeat(MAX_PATH_LEN));
        assert!(matches!(
            DocPath::parse(&long).unwrap_err(),
            DocPathError::TooLong(_)
        ));
    }

    #[test]
    fn parse_rejects_unknown_roots_and_machinery_dirs() {
        assert!(matches!(
            DocPath::parse("readme/x.md").unwrap_err(),
            DocPathError::BadRoot(_)
        ));
        // `issues` is a reserved content root but not a doc root: issue
        // bodies belong to the issue write path.
        assert!(matches!(
            DocPath::parse("issues/2026/08/x/readme.md").unwrap_err(),
            DocPathError::BadRoot(_)
        ));
        assert!(matches!(
            DocPath::parse(".dit/config.md").unwrap_err(),
            DocPathError::BadRoot(_)
        ));
    }

    #[test]
    fn parse_rejects_traversal_shapes() {
        for path in [
            "../escape.md",
            "docs/../notes/x.md",
            "/etc/passwd.md",
            "docs/..",
            "docs/..md",
            "docs/flows/../x.md",
        ] {
            assert!(
                matches!(DocPath::parse(path), Err(DocPathError::Traversal(_))),
                "{path} must be refused as traversal"
            );
        }
    }

    #[test]
    fn parse_rejects_bad_folders_and_file_names() {
        // Uppercase, spaces, slashes doubled or trailing, backslashes, and
        // non-Markdown suffixes all fail with the specific segment named.
        assert!(matches!(
            DocPath::parse("docs/Flows/x.md").unwrap_err(),
            DocPathError::BadFolder(_)
        ));
        assert!(matches!(
            DocPath::parse("docs//x.md").unwrap_err(),
            DocPathError::BadFolder(_)
        ));
        assert!(matches!(
            DocPath::parse("docs/flows//x.md").unwrap_err(),
            DocPathError::BadFolder(_)
        ));
        // A single trailing slash is canonicalized, not rejected.
        assert_eq!(DocPath::parse("docs/x.md/").unwrap().as_str(), "docs/x.md");
        assert!(matches!(
            DocPath::parse("docs/x.txt").unwrap_err(),
            DocPathError::BadFileName(_)
        ));
        assert!(matches!(
            DocPath::parse("docs/X.md").unwrap_err(),
            DocPathError::BadFileName(_)
        ));
        assert!(matches!(
            DocPath::parse("docs/.hidden/x.md").unwrap_err(),
            DocPathError::BadFolder(_)
        ));
        assert!(matches!(
            DocPath::parse("docs/.secret.md").unwrap_err(),
            DocPathError::BadFileName(_)
        ));
        assert!(matches!(
            DocPath::parse("docs/a..md").unwrap_err(),
            DocPathError::BadFileName(_)
        ));
    }

    #[test]
    fn parse_caps_nesting_depth() {
        let deep = format!("docs/{}/x.md", ["f"; MAX_SEGMENTS].join("/"));
        assert!(matches!(
            DocPath::parse(&deep).unwrap_err(),
            DocPathError::TooDeep(_)
        ));
    }
}
