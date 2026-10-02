//! Image attachments (ADR 0026): files in plain git beside the page or issue
//! that shows them. Like `DocPath`, the whole attack surface is a path string
//! and a byte buffer from the network, so the rules live here — pure,
//! wasm-checkable — and every caller below the facade receives values that
//! already obey them.

use crate::DocPath;

/// §8: plain git below 1 MB; above that, refuse and ask for a link.
pub const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024;

const MAX_PATH_LEN: usize = 240;
const MAX_SEGMENTS: usize = 10;
const MAX_WORDS_LEN: usize = 40;

/// The roots an attachment may live under: the doc roots and the issues.
const ATTACHMENT_ROOTS: [&str; 5] = ["docs", "notes", "epics", "changelogs", "issues"];

/// The images DIT stores. SVG is absent on purpose: it can carry script, and
/// vector drawings are `dit-diagram` fences, sanitized (ADR 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageKind {
    /// Identify an image by its first bytes. A name or a browser's content
    /// type is a claim; the bytes are the fact.
    pub fn sniff(bytes: &[u8]) -> Option<ImageKind> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(ImageKind::Png)
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some(ImageKind::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(ImageKind::Gif)
        } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            Some(ImageKind::Webp)
        } else {
            None
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ImageKind::Png => "png",
            ImageKind::Jpeg => "jpg",
            ImageKind::Gif => "gif",
            ImageKind::Webp => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            ImageKind::Png => "image/png",
            ImageKind::Jpeg => "image/jpeg",
            ImageKind::Gif => "image/gif",
            ImageKind::Webp => "image/webp",
        }
    }

    fn from_extension(ext: &str) -> Option<ImageKind> {
        match ext {
            "png" => Some(ImageKind::Png),
            "jpg" | "jpeg" => Some(ImageKind::Jpeg),
            "gif" => Some(ImageKind::Gif),
            "webp" => Some(ImageKind::Webp),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AttachmentError {
    #[error("an attachment path is required")]
    Empty,
    #[error("an attachment path may be at most 240 characters, got {0}")]
    TooLong(usize),
    #[error("an attachment path may nest at most 10 folders deep, got {0} segments")]
    TooDeep(usize),
    #[error("`{0}` escapes the workspace — absolute paths and `.`/`..` are not attachment paths")]
    Traversal(String),
    #[error("`{0}` must start with one of: docs, notes, epics, changelogs, issues")]
    BadRoot(String),
    #[error("`{0}` is not inside an `attachments/` folder")]
    NotInAttachments(String),
    #[error("`{0}` is not an image name DIT stores (png, jpg, jpeg, gif or webp)")]
    BadFileName(String),
    #[error("the file is {0} bytes; attachments are kept in git only up to 1 MB — link to a larger file instead")]
    TooLarge(usize),
    #[error("this is not a PNG, JPEG, GIF or WebP image (SVG is not accepted — draw it as a diagram block)")]
    NotAnImage,
}

/// A validated path to an attachment, relative to the content root:
/// `<root>/<folders…>/attachments/<name>.<image ext>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttachmentPath(String);

impl AttachmentPath {
    pub fn parse(s: &str) -> Result<Self, AttachmentError> {
        if s.is_empty() {
            return Err(AttachmentError::Empty);
        }
        if s.len() > MAX_PATH_LEN {
            return Err(AttachmentError::TooLong(s.len()));
        }
        let segments: Vec<&str> = s.split('/').collect();
        // Traversal first: an escape attempt should be named as one, not as
        // a misshapen path.
        if s.starts_with('/') || segments.iter().any(|seg| *seg == "." || *seg == "..") {
            return Err(AttachmentError::Traversal(s.to_owned()));
        }
        if segments.len() > MAX_SEGMENTS {
            return Err(AttachmentError::TooDeep(segments.len()));
        }
        if !ATTACHMENT_ROOTS.contains(&segments[0]) {
            return Err(AttachmentError::BadRoot(segments[0].to_owned()));
        }
        let last = segments.len() - 1;
        if last < 2 || segments[last - 1] != "attachments" {
            return Err(AttachmentError::NotInAttachments(s.to_owned()));
        }
        // Folders are what the layouts produce: issue folders carry
        // uppercase ULIDs, doc folders are slugs. Nothing else may appear.
        for seg in &segments[..last] {
            let ok = !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !ok {
                return Err(AttachmentError::Traversal(s.to_owned()));
            }
        }
        let name = segments[last];
        let shape_ok = name.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && stem
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
                && ImageKind::from_extension(ext).is_some()
        });
        if !shape_ok {
            return Err(AttachmentError::BadFileName(name.to_owned()));
        }
        Ok(AttachmentPath(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The kind its extension names. The bytes are sniffed separately.
    pub fn kind(&self) -> Option<ImageKind> {
        self.0
            .rsplit_once('.')
            .and_then(|(_, ext)| ImageKind::from_extension(ext))
    }
}

/// Where a page's attachments go and what its files are prefixed with
/// (ADR 0026): the page's folder's `attachments/`, under the page's own
/// name. A `DocPath` is always a loose `<name>.md` (folder `README.md`
/// pages are not editor-addressable), so the prefix keeps two pages in one
/// folder from mixing their pictures.
pub fn doc_attachment_dir(doc: &DocPath) -> (String, String) {
    let (folder, name) = doc.as_str().rsplit_once('/').unwrap_or(("", doc.as_str()));
    (
        format!("{folder}/attachments"),
        name.trim_end_matches(".md").to_owned(),
    )
}

/// The file name an attachment is stored under:
/// `[<prefix>-]<words of the original name>-<8 hex of the blob id>.<ext>`.
pub fn attachment_file_name(
    prefix: Option<&str>,
    original: &str,
    blob_id: &str,
    kind: ImageKind,
) -> String {
    // The extension is the sniffed kind's, so the uploaded name's own one
    // goes before its words are taken.
    let stem = original.rsplit_once('.').map_or(original, |(stem, _)| stem);
    let mut words = String::new();
    for part in [prefix.unwrap_or(""), stem] {
        let mut dash_pending = !words.is_empty();
        for c in part.chars() {
            if c.is_ascii_alphanumeric() {
                if dash_pending && !words.is_empty() {
                    words.push('-');
                }
                dash_pending = false;
                if words.len() >= MAX_WORDS_LEN {
                    break;
                }
                words.push(c.to_ascii_lowercase());
            } else {
                dash_pending = true;
            }
        }
    }
    let words = words.trim_end_matches('-');
    let words = if words.is_empty() || Some(words) == prefix {
        match prefix {
            Some(p) if !p.is_empty() => format!("{p}-image"),
            _ => "image".to_owned(),
        }
    } else {
        words.to_owned()
    };
    let digest: String = blob_id
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(8)
        .collect();
    format!(
        "{words}-{}.{}",
        digest.to_ascii_lowercase(),
        kind.extension()
    )
}

/// Check bytes before they are staged: within the cap and a stored kind.
pub fn check_attachment_bytes(bytes: &[u8]) -> Result<ImageKind, AttachmentError> {
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(AttachmentError::TooLarge(bytes.len()));
    }
    ImageKind::sniff(bytes).ok_or(AttachmentError::NotAnImage)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 ";

    #[test]
    fn images_are_recognised_by_their_bytes() {
        assert_eq!(ImageKind::sniff(PNG), Some(ImageKind::Png));
        assert_eq!(ImageKind::sniff(JPEG), Some(ImageKind::Jpeg));
        assert_eq!(ImageKind::sniff(GIF), Some(ImageKind::Gif));
        assert_eq!(ImageKind::sniff(b"GIF87a\x01\0"), Some(ImageKind::Gif));
        assert_eq!(ImageKind::sniff(WEBP), Some(ImageKind::Webp));
        assert_eq!(ImageKind::Png.mime(), "image/png");
        assert_eq!(ImageKind::Jpeg.extension(), "jpg");
    }

    #[test]
    fn svg_text_and_truncated_headers_are_not_images() {
        assert_eq!(
            ImageKind::sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script/>"),
            None
        );
        assert_eq!(ImageKind::sniff(b"hello world"), None);
        assert_eq!(ImageKind::sniff(b"\x89PN"), None);
        assert_eq!(
            ImageKind::sniff(b"RIFF\x24\0\0\0WAVE"),
            None,
            "a RIFF that is not WebP"
        );
        assert_eq!(ImageKind::sniff(b""), None);
    }

    #[test]
    fn bytes_over_the_cap_or_not_an_image_are_refused() {
        assert_eq!(check_attachment_bytes(PNG), Ok(ImageKind::Png));
        let mut big = PNG.to_vec();
        big.resize(MAX_ATTACHMENT_BYTES + 1, 0);
        assert_eq!(
            check_attachment_bytes(&big),
            Err(AttachmentError::TooLarge(MAX_ATTACHMENT_BYTES + 1))
        );
        let mut exact = PNG.to_vec();
        exact.resize(MAX_ATTACHMENT_BYTES, 0);
        assert_eq!(check_attachment_bytes(&exact), Ok(ImageKind::Png));
        assert_eq!(
            check_attachment_bytes(b"<svg/>"),
            Err(AttachmentError::NotAnImage)
        );
    }

    #[test]
    fn a_path_inside_an_attachments_folder_parses() {
        for ok in [
            "docs/attachments/guide-shot-0a1b2c3d.png",
            "docs/flows/auth/attachments/login-0a1b2c3d.jpg",
            "issues/2026/10/01M3YQCD0X-FKZK-checklist/attachments/screen-0a1b2c3d.webp",
            "notes/attachments/board-0a1b2c3d.gif",
            "docs/attachments/x.jpeg",
        ] {
            let path = AttachmentPath::parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
            assert_eq!(path.as_str(), ok);
        }
        assert_eq!(
            AttachmentPath::parse("docs/attachments/x.jpeg")
                .unwrap()
                .kind(),
            Some(ImageKind::Jpeg)
        );
    }

    #[test]
    fn escapes_foreign_roots_and_non_images_are_refused() {
        type Expected = fn(&AttachmentError) -> bool;
        use AttachmentError as E;
        let cases: &[(&str, Expected)] = &[
            ("", |e| *e == E::Empty),
            ("/etc/attachments/x.png", |e| matches!(e, E::Traversal(_))),
            ("docs/../attachments/x.png", |e| {
                matches!(e, E::Traversal(_))
            }),
            ("docs/attachments/../../x.png", |e| {
                matches!(e, E::Traversal(_))
            }),
            (".dit/attachments/x.png", |e| matches!(e, E::BadRoot(_))),
            ("src/attachments/x.png", |e| matches!(e, E::BadRoot(_))),
            ("docs/x.png", |e| matches!(e, E::NotInAttachments(_))),
            ("docs/images/x.png", |e| matches!(e, E::NotInAttachments(_))),
            ("docs/attachments/x.svg", |e| matches!(e, E::BadFileName(_))),
            ("docs/attachments/x.md", |e| matches!(e, E::BadFileName(_))),
            ("docs/attachments/.png", |e| matches!(e, E::BadFileName(_))),
            ("docs/attachments/x y.png", |e| {
                matches!(e, E::BadFileName(_))
            }),
            ("docs/attachments/x\\y.png", |e| {
                matches!(e, E::BadFileName(_))
            }),
        ];
        for (input, expected) in cases {
            let err = AttachmentPath::parse(input).expect_err(input);
            assert!(expected(&err), "{input}: got {err:?}");
        }
        let deep = format!("docs/{}attachments/x.png", "a/".repeat(10));
        assert!(matches!(AttachmentPath::parse(&deep), Err(E::TooDeep(_))));
    }

    #[test]
    fn a_page_shares_its_folder_s_attachments_under_its_own_name() {
        let page = DocPath::parse("docs/guide.md").unwrap();
        assert_eq!(
            doc_attachment_dir(&page),
            ("docs/attachments".to_owned(), "guide".to_owned())
        );
        let nested = DocPath::parse("docs/flows/auth-session.md").unwrap();
        assert_eq!(
            doc_attachment_dir(&nested),
            (
                "docs/flows/attachments".to_owned(),
                "auth-session".to_owned()
            )
        );
    }

    #[test]
    fn a_file_name_keeps_words_of_the_original_and_the_blob_id() {
        let blob = "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567";
        assert_eq!(
            attachment_file_name(
                None,
                "Screen Shot 2026-10-03 at 10.15.png",
                blob,
                ImageKind::Png
            ),
            "screen-shot-2026-10-03-at-10-15-0a1b2c3d.png"
        );
        // The extension is the sniffed kind's, never the uploaded name's.
        assert_eq!(
            attachment_file_name(Some("guide"), "photo.PNG", blob, ImageKind::Jpeg),
            "guide-photo-0a1b2c3d.jpg"
        );
        // Nothing usable in the name: the kind stands in.
        assert_eq!(
            attachment_file_name(None, "图片.png", blob, ImageKind::Png),
            "image-0a1b2c3d.png"
        );
        let long = attachment_file_name(None, &"word ".repeat(30), blob, ImageKind::Gif);
        assert!(long.len() <= 40 + "-0a1b2c3d.gif".len(), "{long}");
        // Every name the rule produces is one the path parser accepts.
        for name in [
            long,
            attachment_file_name(Some("guide"), "a.b.c", blob, ImageKind::Webp),
        ] {
            AttachmentPath::parse(&format!("docs/attachments/{name}"))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
