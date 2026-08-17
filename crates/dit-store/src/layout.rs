//! Where things live on disk.
//!
//! Every path in the workspace is derived here from typed inputs (validated
//! ids, slugs, aliases), so no caller can assemble a path out of raw user
//! text. Issue folders are `<10 time chars>-<4 random chars>-<slug>` under a
//! monthly shard derived from the id's own timestamp — a ULID encodes its
//! creation time, so no separate lookup is needed to find an issue's folder.

use std::fs;
use std::path::PathBuf;

use dit_model::{IssueId, Slug};
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
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Layout {
        Layout { root: root.into() }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn dit_dir(&self) -> PathBuf {
        self.root.join(".dit")
    }

    pub fn issues_dir(&self) -> PathBuf {
        self.dit_dir().join("issues")
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

    pub fn gitattributes(&self) -> PathBuf {
        self.dit_dir().join(".gitattributes")
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

    /// Path of an existing issue's `issue.md`.
    pub fn issue_md(&self, id: &IssueId) -> Result<PathBuf, StoreError> {
        self.issue_dir_for(id)?
            .map(|d| d.join("issue.md"))
            .ok_or_else(|| StoreError::NotFound(id.as_str().to_owned()))
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn comment_names_reject_path_characters() {
        let cid = IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        assert!(Layout::comment_file_name(&cid, "farid").is_ok());
        assert!(Layout::comment_file_name(&cid, "../escape").is_err());
        assert!(Layout::comment_file_name(&cid, "").is_err());
        assert!(Layout::comment_file_name(&cid, "Farid").is_err());
    }
}
