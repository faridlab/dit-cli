//! RFC3339 timestamps — the file format's only time representation.
//!
//! Pure parsing/formatting only. Reading the wall clock does NOT live here:
//! `now()` would make the pure core non-deterministic and non-wasm-safe, so
//! the clock is injected by adapters (`dit-store`).

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TimeError {
    #[error("not an RFC3339 timestamp: `{0}` (expected e.g. 2026-08-16T09:12:00Z)")]
    NotRfc3339(String),
    #[error("not a `YYYY-MM-DD` date: `{0}`")]
    NotDate(String),
}

/// Parse an RFC3339 timestamp from the frontmatter.
pub fn parse_rfc3339(s: &str) -> Result<OffsetDateTime, TimeError> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(|_| TimeError::NotRfc3339(s.to_owned()))
}

/// Format in the canonical form DIT writes: UTC, `Z` suffix, second precision.
/// Milliseconds are dropped deliberately — two writers in the same second
/// should produce identical bytes, so `dit fmt` output is stable.
pub fn format_rfc3339(t: OffsetDateTime) -> String {
    let utc = t.to_utc();
    let (y, mo, d, h, mi, s) = (
        utc.year(),
        utc.month() as u8,
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second(),
    );
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Validate a `YYYY-MM-DD` date (the `due` field).
pub fn validate_date(s: &str) -> Result<(), TimeError> {
    let bytes = s.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !shape {
        return Err(TimeError::NotDate(s.to_owned()));
    }
    let month = s[5..7].parse::<u8>().unwrap_or(0);
    let day = s[8..10].parse::<u8>().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(TimeError::NotDate(s.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_reformats_canonically() {
        let t = parse_rfc3339("2026-08-16T09:12:00Z").unwrap();
        assert_eq!(format_rfc3339(t), "2026-08-16T09:12:00Z");
    }

    #[test]
    fn fractional_seconds_and_offsets_normalize_to_utc_z() {
        let t = parse_rfc3339("2026-08-16T16:12:00.500+07:00").unwrap();
        assert_eq!(format_rfc3339(t), "2026-08-16T09:12:00Z");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_rfc3339("yesterday").is_err());
        assert!(parse_rfc3339("").is_err());
    }

    #[test]
    fn validates_due_dates() {
        assert!(validate_date("2026-08-30").is_ok());
        assert!(validate_date("2026-13-01").is_err());
        assert!(validate_date("2026-8-30").is_err());
        assert!(validate_date("not-a-date").is_err());
    }
}
