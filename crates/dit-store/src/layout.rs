//! Where things live on disk.
//!
//! Every path in the workspace is derived here from typed inputs (validated
//! ids, slugs, aliases), so no caller can assemble a path out of raw user
//! text. Issue folders are `<10 time chars>-<4 random chars>-<slug>` under a
//! monthly shard derived from the id's own timestamp — a ULID encodes its
//! creation time, so no separate lookup is needed to find an issue's folder.

use std::fs;
use std::path::{Path, PathBuf};

use dit_model::{DataLayout, IssueId, Slug, ISSUE_BODY_FILE, LEGACY_ISSUE_BODY_FILE};
use time::OffsetDateTime;

use crate::store::StoreError;

/// Millisecond-exact conversions. The `time` crate's public constructors
/// stop at whole seconds, so the remainder is carried by hand.
pub(crate) fn datetime_from_ms(ms: i64) -> OffsetDateTime {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000) as i32;
    OffsetDateTime::from_unix_timestamp(secs).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::milliseconds(millis as i64)
}

pub(crate) fn datetime_to_ms(t: OffsetDateTime) -> i64 {
    t.unix_timestamp() * 1000 + i64::from(t.millisecond())
}

#[derive(Debug, Clone)]
pub struct Layout {
    /// The workspace root: the git repository that contains `.dit/`.
    root: PathBuf,
    /// Where the content roots sit (ADR 0005). Resolved once at open from
    /// `config.yaml` — every path below derives from this one bit.
    kind: DataLayout,
}

impl Layout {
    /// Resolve the layout for a workspace root.
    ///
    /// Precedence: an explicit `layout:` in `.dit/config.yaml` wins. Absent
    /// that, a `.dit/issues/` directory on disk means a workspace created
    /// before ADR 0005 — it stays `dotdir` until migrated. Everything else
    /// (including a workspace with no `.dit/` yet, i.e. `dit init`) is `root`.
    pub fn detect(root: impl Into<PathBuf>) -> Layout {
        let root = root.into();
        let config = root.join(".dit").join("config.yaml");
        let text = fs::read_to_string(&config).unwrap_or_default();
        if config_states_layout(&text) {
            if let Ok(cfg) = dit_parse::parse_config(&text) {
                return Layout {
                    root,
                    kind: cfg.layout,
                };
            }
        }
        let legacy = root.join(".dit").join("issues").is_dir();
        Layout {
            root,
            kind: if legacy {
                DataLayout::DotDir
            } else {
                DataLayout::Root
            },
        }
    }

    /// Tests and `dit init` (which has not written config yet) pin the kind.
    pub fn with_kind(root: impl Into<PathBuf>, kind: DataLayout) -> Layout {
        Layout {
            root: root.into(),
            kind,
        }
    }

    pub fn kind(&self) -> DataLayout {
        self.kind
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dit_dir(&self) -> PathBuf {
        self.root.join(".dit")
    }

    /// A visible content root (`issues`, `docs`, `notes`, …) at the place
    /// this layout puts it: the tree root, or under `.dit/` in `dotdir`.
    pub fn content_dir(&self, name: &str) -> PathBuf {
        match self.kind {
            DataLayout::Root => self.root.join(name),
            DataLayout::DotDir => self.dit_dir().join(name),
        }
    }

    /// Repo-relative form of a content root, as git paths spell it (`/`,
    /// no leading `./`) — for tree walks and diff pathspecs.
    pub fn content_root_rel(&self, name: &str) -> String {
        self.kind.content_root(name)
    }

    pub fn issues_dir(&self) -> PathBuf {
        self.content_dir("issues")
    }

    pub fn epics_dir(&self) -> PathBuf {
        self.content_dir("epics")
    }

    pub fn docs_dir(&self) -> PathBuf {
        self.content_dir("docs")
    }

    /// The file a validated doc page (§13) lives at. `DocPath::parse` has
    /// already confined the segments to a doc root and slug-safe names, so
    /// this join cannot leave the workspace.
    pub fn doc_file(&self, path: &dit_model::DocPath) -> PathBuf {
        self.content_dir(path.root()).join(path.strip_root())
    }

    pub fn notes_dir(&self) -> PathBuf {
        self.content_dir("notes")
    }

    pub fn changelogs_dir(&self) -> PathBuf {
        self.content_dir("changelogs")
    }

    pub fn people_dir(&self) -> PathBuf {
        self.dit_dir().join("people")
    }

    pub fn workflow_yaml(&self) -> PathBuf {
        self.dit_dir().join("schema").join("workflow.yaml")
    }

    pub fn config_yaml(&self) -> PathBuf {
        self.dit_dir().join("config.yaml")
    }

    /// `.dit/templates/` — issue-body templates seeded by `dit init`.
    pub fn templates_dir(&self) -> PathBuf {
        self.dit_dir().join("templates")
    }

    /// The merge-driver attributes file sits where this layout's tree root
    /// is: the tree root itself when DIT owns it, `.dit/` when it is a guest
    /// (ADR 0005).
    pub fn gitattributes(&self) -> PathBuf {
        match self.kind {
            DataLayout::Root => self.root.join(".gitattributes"),
            DataLayout::DotDir => self.dit_dir().join(".gitattributes"),
        }
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(".dit-cache")
    }

    pub fn write_lock(&self) -> PathBuf {
        self.cache_dir().join("write.lock")
    }

    /// The monthly shard an issue lives in, derived from its id's timestamp.
    fn month_dir(&self, id: &IssueId) -> PathBuf {
        // A ULID timestamp is bounded to years 1970..~8900; a hand-forged id
        // outside that range falls back to the epoch shard rather than
        // panicking.
        let t = datetime_from_ms(id.timestamp_ms() as i64);
        self.issues_dir()
            .join(t.year().to_string())
            .join(format!("{:02}", u8::from(t.month())))
    }

    /// Folder name for an issue: time prefix, random prefix, then the slug
    /// snapshot. Never renamed after creation.
    pub fn issue_folder_name(id: &IssueId, slug: &Slug) -> String {
        format!(
            "{}-{}-{}",
            &id.as_str()[..10],
            &id.as_str()[10..14],
            slug.as_str()
        )
    }

    pub fn issue_dir(&self, id: &IssueId, slug: &Slug) -> PathBuf {
        self.month_dir(id).join(Self::issue_folder_name(id, slug))
    }

    /// Find an existing issue's folder on disk. The slug is not known from
    /// the id alone, so the month shard is scanned for the id's unique
    /// `<time>-<random>` prefix. `Ok(None)` when no such issue exists.
    pub fn issue_dir_for(&self, id: &IssueId) -> Result<Option<PathBuf>, StoreError> {
        let prefix = format!("{}-{}", &id.as_str()[..10], &id.as_str()[10..14]);
        let month = self.month_dir(id);
        let entries = match fs::read_dir(&month) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) {
                return Ok(Some(entry.path()));
            }
        }
        Ok(None)
    }

    /// Path of an existing issue's body file. `README.md` since ADR 0006;
    /// folders created before then keep their `issue.md` until migrated —
    /// reads prefer the new name, writes leave the old one in place.
    pub fn issue_body(&self, id: &IssueId) -> Result<PathBuf, StoreError> {
        let dir = self
            .issue_dir_for(id)?
            .ok_or_else(|| StoreError::NotFound(id.as_str().to_owned()))?;
        Self::body_file_in(&dir).ok_or_else(|| StoreError::NotFound(id.as_str().to_owned()))
    }

    /// The body file inside a known issue folder: `README.md` when present
    /// (or when the folder is new and has neither), the legacy `issue.md`
    /// when that is all there is.
    pub fn body_file_in(dir: &Path) -> Option<PathBuf> {
        let readme = dir.join(ISSUE_BODY_FILE);
        if readme.is_file() {
            return Some(readme);
        }
        let legacy = dir.join(LEGACY_ISSUE_BODY_FILE);
        if legacy.is_file() {
            return Some(legacy);
        }
        Some(readme)
    }

    /// File name for a comment: 14 id characters (timestamp + 4 random —
    /// the same shape as issue folders) plus the author alias. The random
    /// part matters: a timestamp-only name would collide for two comments by
    /// the same author within the same half-minute.
    pub fn comment_file_name(comment_id: &IssueId, author: &str) -> Result<String, StoreError> {
        if author.is_empty()
            || !author
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(StoreError::BadAlias(author.to_owned()));
        }
        Ok(format!(
            "{}-{}-{author}.md",
            &comment_id.as_str()[..10],
            &comment_id.as_str()[10..14],
        ))
    }

    /// Path of an existing comment file.
    pub fn comment_md(
        &self,
        id: &IssueId,
        comment_id: &IssueId,
        author: &str,
    ) -> Result<PathBuf, StoreError> {
        let name = Self::comment_file_name(comment_id, author)?;
        self.issue_dir_for(id)?
            .map(|d| d.join("comments").join(name))
            .ok_or_else(|| StoreError::NotFound(id.as_str().to_owned()))
    }
}

/// True when the config text carries an explicit `layout:` key. Only the
/// canonical single-line form is recognized — the file is `write_config`'s
/// output or a hand edit of it, never arbitrary YAML.
fn config_states_layout(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with("layout:"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dit-layout-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn comment_names_reject_path_characters() {
        let cid = IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        assert!(Layout::comment_file_name(&cid, "farid").is_ok());
        assert!(Layout::comment_file_name(&cid, "../escape").is_err());
        assert!(Layout::comment_file_name(&cid, "").is_err());
        assert!(Layout::comment_file_name(&cid, "Farid").is_err());
    }

    #[test]
    fn detect_defaults_to_root_when_nothing_exists() {
        let dir = tmp("root-default");
        assert_eq!(Layout::detect(&dir).kind(), DataLayout::Root);
    }

    #[test]
    fn detect_honors_an_explicit_layout_key() {
        let dir = tmp("explicit");
        std::fs::create_dir_all(dir.join(".dit")).unwrap();
        std::fs::write(
            dir.join(".dit").join("config.yaml"),
            "schema_version: 1\nlayout: dotdir\nnumbering: local\n",
        )
        .unwrap();
        assert_eq!(Layout::detect(&dir).kind(), DataLayout::DotDir);
    }

    #[test]
    fn detect_treats_a_legacy_dotdir_workspace_as_dotdir() {
        // A workspace created before ADR 0005: config carries no `layout:`,
        // and its data already lives under `.dit/issues/`.
        let dir = tmp("legacy");
        std::fs::create_dir_all(dir.join(".dit/issues")).unwrap();
        std::fs::write(dir.join(".dit").join("config.yaml"), "schema_version: 1\n").unwrap();
        assert_eq!(Layout::detect(&dir).kind(), DataLayout::DotDir);
    }

    #[test]
    fn content_roots_follow_the_layout() {
        let root = Layout::with_kind("/tmp/ws", DataLayout::Root);
        assert_eq!(root.issues_dir(), PathBuf::from("/tmp/ws/issues"));
        assert_eq!(root.notes_dir(), PathBuf::from("/tmp/ws/notes"));
        assert_eq!(
            root.templates_dir(),
            PathBuf::from("/tmp/ws/.dit/templates")
        );
        assert_eq!(
            root.gitattributes(),
            PathBuf::from("/tmp/ws/.gitattributes")
        );

        let dot = Layout::with_kind("/tmp/ws", DataLayout::DotDir);
        assert_eq!(dot.issues_dir(), PathBuf::from("/tmp/ws/.dit/issues"));
        assert_eq!(
            dot.gitattributes(),
            PathBuf::from("/tmp/ws/.dit/.gitattributes")
        );
        // Machinery never moves.
        assert_eq!(dot.config_yaml(), PathBuf::from("/tmp/ws/.dit/config.yaml"));
    }

    #[test]
    fn body_file_prefers_readme_and_falls_back_to_legacy() {
        let dir = tmp("body-file");
        let folder = dir.join("01K3M9ZXQ2-R7VN-x");
        std::fs::create_dir_all(&folder).unwrap();

        // Neither file: a new folder gets README.md.
        assert_eq!(
            Layout::body_file_in(&folder),
            Some(folder.join("README.md"))
        );

        // Legacy only: keep editing the file that exists.
        std::fs::write(folder.join("issue.md"), "---\nid: x\n---\n").unwrap();
        assert_eq!(Layout::body_file_in(&folder), Some(folder.join("issue.md")));

        // Both: the new name wins.
        std::fs::write(folder.join("README.md"), "---\nid: x\n---\n").unwrap();
        assert_eq!(
            Layout::body_file_in(&folder),
            Some(folder.join("README.md"))
        );
    }
}
