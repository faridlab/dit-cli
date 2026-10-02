//! Image attachments (ADR 0026): a picture committed beside the page or the
//! issue that shows it, linked relatively, read back file-backed like a doc
//! page (ADR 0010). The rules — what is an image, where it may live, what it
//! is called — are `dit-model`'s; this is where they meet the store and git.

use dit_model::{
    attachment_file_name, check_attachment_bytes, doc_attachment_dir, AttachmentError,
    AttachmentPath, DocPath, ImageKind, IssueId,
};

use crate::{rel_to_root, Dit, DitError, Transaction};

/// What a picture is attached to. A comment's pictures live in its issue's
/// `attachments/`; only the link differs, because a comment file sits one
/// folder further down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachTarget {
    Doc(String),
    Issue(IssueId),
    Comment(IssueId),
}

/// Where an attachment landed, and the link to write into the markdown —
/// relative to the file that will hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    pub path: String,
    pub link: String,
}

/// An attachment's bytes as read back, with the kind its bytes say it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentBytes {
    pub kind: ImageKind,
    pub bytes: Vec<u8>,
}

impl Transaction<'_> {
    /// Stage a picture beside `target`. The bytes decide the kind and the
    /// name carries their blob id, so the same picture twice is the same file
    /// and staging it again writes nothing.
    pub fn add_attachment(
        &mut self,
        target: &AttachTarget,
        original_name: &str,
        bytes: &[u8],
    ) -> Result<Attached, DitError> {
        let kind = check_attachment_bytes(bytes)?;
        let blob = self.dit.repo.blob_id(bytes)?;
        let (dir, prefix, up) = match target {
            AttachTarget::Doc(page) => {
                let page = DocPath::parse(page)?;
                let (dir, prefix) = doc_attachment_dir(&page);
                (dir, Some(prefix), "")
            }
            AttachTarget::Issue(id) => (self.issue_attachment_dir(id)?, None, ""),
            AttachTarget::Comment(id) => (self.issue_attachment_dir(id)?, None, "../"),
        };
        let name = attachment_file_name(prefix.as_deref(), original_name, &blob, kind);
        let path = AttachmentPath::parse(&format!("{dir}/{name}"))?;
        let file = self.dit.store.layout().attachment_file(&path);
        // Same name, same blob id: the picture is already there.
        let already = std::fs::read(&file).is_ok_and(|existing| existing == bytes);
        if !already {
            self.store_tx.write_attachment(&path, bytes.to_vec());
        }
        Ok(Attached {
            link: format!("{up}attachments/{name}"),
            path: path.as_str().to_owned(),
        })
    }

    /// `issues/…/<folder>/attachments`, relative to the content root.
    fn issue_attachment_dir(&self, id: &IssueId) -> Result<String, DitError> {
        let layout = self.dit.store.layout();
        let folder = layout
            .issue_dir_for(id)?
            .ok_or_else(|| DitError::NotFound(id.as_str().to_owned()))?;
        let below = rel_to_root(&layout.issues_dir(), &folder);
        Ok(format!("issues/{below}/attachments"))
    }
}

impl Dit {
    /// Read an attachment back (ADR 0026). File-backed like `read_doc`
    /// (ADR 0010), through the same kind of sandbox — and the bytes are
    /// sniffed on the way out, so a text file wearing an image's name is
    /// never handed to a browser as a picture.
    pub fn read_attachment(&self, path: &str) -> Result<AttachmentBytes, DitError> {
        let path = AttachmentPath::parse(path)?;
        let file = self.store.layout().attachment_file(&path);
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(DitError::NotFound(path.as_str().to_owned()))
            }
            Err(e) => return Err(e.into()),
        };
        // The kind is the bytes', not the extension's: a PNG named `.jpg`
        // is served as the PNG it is.
        let kind = check_attachment_bytes(&bytes)?;
        Ok(AttachmentBytes { kind, bytes })
    }
}

impl From<AttachmentError> for DitError {
    fn from(err: AttachmentError) -> Self {
        DitError::Attachment(err)
    }
}
