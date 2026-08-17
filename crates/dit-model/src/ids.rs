//! Identity value objects.
//!
//! Crockford base32 / ULID handling is implemented here rather than pulled from
//! the `ulid` crate. That is a deliberate consequence of invariant I4: `ulid`'s
//! serde support requires its `std` feature, `std` pulls in `rand` -> `getrandom`,
//! and `getrandom` does not build for `wasm32-unknown-unknown` without an
//! explicit backend. There is no feature combination that gives serde without
//! rand.
//!
//! Owning ~60 lines of base32 is cheaper than a dependency the pure core cannot
//! actually depend on. `dit-model` has no non-derive dependencies, and should
//! stay that way.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Crockford base32. Excludes I, L, O and U to avoid transcription ambiguity.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

const ULID_LEN: usize = 26;
/// Characters 0..10 encode the 48-bit millisecond timestamp.
const TIME_LEN: usize = 10;
/// The short ref is taken from here — inside the 80-bit random component.
const SHORT_START: usize = 10;
const SHORT_LEN: usize = 7;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("a ULID must be {ULID_LEN} characters, got {0}")]
    Length(usize),
    #[error("`{0}` is not a Crockford base32 character")]
    BadChar(char),
    #[error("ULID overflows 128 bits (first character must be 0-7)")]
    Overflow,
    #[error("a short ref must be exactly {SHORT_LEN} characters, got {0}")]
    ShortRefLength(usize),
    #[error("a slug must be non-empty and contain only [a-z0-9-]")]
    BadSlug,
}

/// Decodes one Crockford character, accepting the documented aliases
/// (`I`/`L` -> `1`, `O` -> `0`) and lowercase input.
fn decode_char(c: char) -> Result<u8, IdError> {
    let u = c.to_ascii_uppercase();
    let normalized = match u {
        'I' | 'L' => '1',
        'O' => '0',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|&a| a == normalized as u8)
        .map(|p| p as u8)
        .ok_or(IdError::BadChar(c))
}

/// Canonical issue identity: a 26-character Crockford base32 ULID.
///
/// Note there is no `new()`. Minting an ID needs entropy, entropy is I/O, and
/// I/O does not belong in the pure core — generation lives in `dit-store`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IssueId([u8; ULID_LEN]);

/// A 7-character reference drawn from the ULID's **random** component.
///
/// Deliberately not a prefix. The first 10 characters of a ULID are pure
/// timestamp, so any two issues minted in the same millisecond share them —
/// a prefix-based ref would collide systematically, not rarely.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShortRef(String);

/// Monotonic topological position in the commit DAG; orders `field_events`.
/// Never derived from wall-clock time: two machines with skewed clocks would
/// otherwise produce orderings that contradict each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seq(i64);

/// A filesystem-safe slug, snapshotted at creation and never renamed: git
/// does not invoke a merge driver for rename/modify conflicts, so a folder
/// rename would silently drop the other side's edits instead of merging them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

impl IssueId {
    pub fn parse(s: &str) -> Result<Self, IdError> {
        if s.len() != ULID_LEN {
            return Err(IdError::Length(s.len()));
        }
        let mut out = [0u8; ULID_LEN];
        for (i, c) in s.chars().enumerate() {
            let v = decode_char(c)?;
            // 26 * 5 = 130 bits for a 128-bit value, so the leading character
            // may only carry 3 bits.
            if i == 0 && v > 7 {
                return Err(IdError::Overflow);
            }
            out[i] = ALPHABET[v as usize];
        }
        Ok(IssueId(out))
    }

    pub fn timestamp_ms(&self) -> u64 {
        self.as_str().chars().take(TIME_LEN).fold(0u64, |acc, c| {
            // Every character already validated at parse time.
            acc << 5 | decode_char(c).unwrap_or(0) as u64
        })
    }

    pub fn short_ref(&self) -> ShortRef {
        ShortRef(self.as_str()[SHORT_START..SHORT_START + SHORT_LEN].to_owned())
    }

    pub fn as_str(&self) -> &str {
        // Every byte came from ALPHABET, which is ASCII.
        std::str::from_utf8(&self.0).unwrap_or("")
    }

    /// Build an ID from a millisecond timestamp and 80 random bits.
    ///
    /// The entropy itself comes from the caller — this function only encodes,
    /// so the pure core stays free of any I/O. All 80 random bits are kept:
    /// dropping any would shrink the collision resistance of the whole scheme.
    pub fn from_parts(timestamp_ms: u64, random: [u8; 10]) -> IssueId {
        let mut entropy: u128 = 0;
        for byte in random {
            entropy = (entropy << 8) | byte as u128;
        }
        // 48 bits of time above 80 bits of randomness, big-endian, encoded
        // into 26 base32 characters (130 bits of text for a 128-bit value —
        // so the leading character stays within 0-7).
        let mut value = (timestamp_ms as u128) << 80 | entropy;
        let mut out = [0u8; ULID_LEN];
        for i in (0..ULID_LEN).rev() {
            out[i] = ALPHABET[(value & 0x1F) as usize];
            value >>= 5;
        }
        IssueId(out)
    }
}

impl ShortRef {
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let n = s.chars().count();
        if n != SHORT_LEN {
            return Err(IdError::ShortRefLength(n));
        }
        for c in s.chars() {
            decode_char(c)?;
        }
        Ok(ShortRef(s.to_ascii_uppercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Seq {
    pub fn new(n: i64) -> Self {
        Seq(n)
    }
    pub fn get(self) -> i64 {
        self.0
    }
}

impl Slug {
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let ok = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok {
            Ok(Slug(s.to_owned()))
        } else {
            Err(IdError::BadSlug)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derive a slug from a title: lowercase, alphanumeric runs kept,
    /// everything else becomes a single dash. The slug is a creation-time
    /// snapshot used in folder names, so it needs a length bound and a
    /// fallback for titles that contain nothing usable.
    pub fn from_title(title: &str) -> Slug {
        const MAX: usize = 60;
        let mut out = String::new();
        let mut dash_pending = false;
        for c in title.chars() {
            if c.is_ascii_alphanumeric() {
                if dash_pending && !out.is_empty() {
                    out.push('-');
                }
                dash_pending = false;
                // Stop before producing a slug longer than the bound.
                if out.len() >= MAX {
                    break;
                }
                out.push(c.to_ascii_lowercase());
            } else {
                dash_pending = true;
            }
        }
        if out.is_empty() {
            out.push_str("issue");
        }
        Slug(out)
    }
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

string_serde!(IssueId, IssueId::parse);
string_serde!(ShortRef, ShortRef::parse);
string_serde!(Slug, Slug::parse);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SAMPLE: &str = "01K3M9ZXQ2R7VN8P4TDBCEFGHJ";

    #[test]
    fn issue_id_roundtrips() {
        assert_eq!(IssueId::parse(SAMPLE).unwrap().to_string(), SAMPLE);
    }

    #[test]
    fn issue_id_rejects_wrong_length_and_bad_chars() {
        assert_eq!(IssueId::parse("nope"), Err(IdError::Length(4)));
        // U is excluded from the Crockford alphabet.
        assert!(matches!(
            IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHU"),
            Err(IdError::BadChar('U'))
        ));
    }

    #[test]
    fn issue_id_rejects_128_bit_overflow() {
        assert_eq!(
            IssueId::parse("81K3M9ZXQ2R7VN8P4TDBCEFGHJ"),
            Err(IdError::Overflow)
        );
    }

    #[test]
    fn crockford_aliases_are_accepted_and_normalized() {
        // O -> 0, I -> 1, and lowercase is fine.
        let with_aliases = "OIK3M9ZXQ2R7VN8P4TDBCEFGHJ".to_lowercase();
        assert_eq!(
            IssueId::parse(&with_aliases).unwrap().to_string(),
            "01K3M9ZXQ2R7VN8P4TDBCEFGHJ"
        );
    }

    /// The property the whole short-ref design exists for: two IDs minted in
    /// the same millisecond share their first 10 characters, so a
    /// prefix-based ref would collide systematically. The random-derived
    /// short ref must not.
    #[test]
    fn short_ref_comes_from_the_random_component() {
        let a = IssueId::parse(SAMPLE).unwrap();
        let b = IssueId::parse("01K3M9ZXQ2ZZZZZZZZZZZZZZZZ").unwrap();

        assert_eq!(a.timestamp_ms(), b.timestamp_ms(), "same millisecond");
        assert_eq!(a.as_str()[..10], b.as_str()[..10], "prefixes collide");
        assert_ne!(a.short_ref(), b.short_ref(), "short refs must not");
    }

    #[test]
    fn short_ref_is_seven_chars() {
        assert_eq!(
            IssueId::parse(SAMPLE).unwrap().short_ref().as_str().len(),
            7
        );
        assert!(ShortRef::parse("toolong").is_ok());
        assert_eq!(ShortRef::parse("short"), Err(IdError::ShortRefLength(5)));
    }

    #[test]
    fn from_parts_encodes_timestamp_and_random_bits() {
        // 2026-08-16T09:12:00Z in milliseconds, random bytes chosen so the
        // expected encoding is hand-checkable: the last 16 characters carry
        // the 80 random bits, the first 10 the timestamp.
        let id = IssueId::from_parts(1_755_336_720_000, [0xFF; 10]);
        assert_eq!(id.as_str().len(), 26);
        assert_eq!(id.timestamp_ms(), 1_755_336_720_000);
        // All-ones randomness encodes as repeated 'Z' (Crockford 31).
        assert!(id.as_str().ends_with("ZZZZZZZZZZZZZZZZ"), "{}", id);
        // Round-trip through parse canonicalizes to the same id.
        assert_eq!(IssueId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn from_parts_bounded_random_bytes_stay_in_range() {
        // Only the low 5 bits of each random byte are meaningful (base32);
        // a caller passing arbitrary bytes must still produce a valid id.
        let id = IssueId::from_parts(0, [0xAB; 10]);
        assert!(IssueId::parse(id.as_str()).is_ok());
    }

    #[test]
    fn slug_from_title_ascii_folds_and_dashes() {
        assert_eq!(
            Slug::from_title("Login Timeout on SLOW Networks!").as_str(),
            "login-timeout-on-slow-networks"
        );
        // Punctuation collapses to a single dash; edges are trimmed.
        assert_eq!(
            Slug::from_title("  --Fix: the (login) bug -- ").as_str(),
            "fix-the-login-bug"
        );
        // Very long titles are cut to a bounded length at a dash boundary.
        let long = "a".repeat(200);
        assert!(Slug::from_title(&long).as_str().len() <= 60);
        // A title with no usable characters still yields a valid slug.
        assert_eq!(Slug::from_title("???!!!").as_str(), "issue");
    }

    #[test]
    fn timestamp_decodes() {
        // 0000000000 -> epoch; 0000000001 -> 1ms.
        assert_eq!(
            IssueId::parse("0000000000ZZZZZZZZZZZZZZZZ")
                .unwrap()
                .timestamp_ms(),
            0
        );
        assert_eq!(
            IssueId::parse("0000000001ZZZZZZZZZZZZZZZZ")
                .unwrap()
                .timestamp_ms(),
            1
        );
    }

    #[test]
    fn slug_rejects_uppercase_and_spaces() {
        assert!(Slug::parse("login-timeout").is_ok());
        assert_eq!(Slug::parse("Login Timeout"), Err(IdError::BadSlug));
        assert_eq!(Slug::parse(""), Err(IdError::BadSlug));
    }
}
