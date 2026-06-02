//! PROV-O IRI constants and timestamp helper for the v3 substrate.
//!
//! All three IRI constants are the canonical W3C PROV-O IRIs. They are
//! written as full string literals here rather than being concatenated
//! from [`crate::jsonld::PROV_NS`] so that they can be used in const
//! contexts without any runtime cost.

use chrono::Utc;

/// Full IRI for `prov:generatedAtTime`.
pub const GENERATED_AT_TIME: &str = "http://www.w3.org/ns/prov#generatedAtTime";

/// Full IRI for `prov:wasAttributedTo`.
pub const WAS_ATTRIBUTED_TO: &str = "http://www.w3.org/ns/prov#wasAttributedTo";

/// Full IRI for `prov:wasRevisionOf`.
pub const WAS_REVISION_OF: &str = "http://www.w3.org/ns/prov#wasRevisionOf";

/// Return the current UTC time formatted as an `xsd:dateTime` string
/// with millisecond precision and a trailing `Z`.
///
/// Example output: `"2026-05-29T01:00:00.123Z"`
///
/// Millisecond precision matches the v2.5 `asserted_at` format; the
/// migration tool relies on exactly three decimal digits.
pub fn now_iso() -> String {
    let now = Utc::now();
    now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_at_time_is_canonical_prov_o_iri() {
        assert_eq!(
            GENERATED_AT_TIME,
            "http://www.w3.org/ns/prov#generatedAtTime"
        );
    }

    #[test]
    fn was_attributed_to_is_canonical_prov_o_iri() {
        assert_eq!(
            WAS_ATTRIBUTED_TO,
            "http://www.w3.org/ns/prov#wasAttributedTo"
        );
    }

    #[test]
    fn was_revision_of_is_canonical_prov_o_iri() {
        assert_eq!(WAS_REVISION_OF, "http://www.w3.org/ns/prov#wasRevisionOf");
    }

    #[test]
    fn now_iso_is_rfc3339_with_millisecond_precision_and_z() {
        let ts = now_iso();

        // Must end with Z.
        assert!(ts.ends_with('Z'), "timestamp must end with Z: {ts}");

        // Must parse as RFC 3339 (chrono validates the full format).
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts)
            .unwrap_or_else(|e| panic!("not valid RFC 3339: {ts} -- {e}"));

        // The parsed value must round-trip to the same string (ensures
        // no excess precision was silently truncated).
        let round_tripped = parsed.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        assert_eq!(ts, round_tripped, "round-trip mismatch");

        // Exactly three digits after the decimal point, before the Z.
        // Format: ...SS.mmmZ  -- the dot is at position len-5.
        let before_z = ts.trim_end_matches('Z');
        let dot_pos = before_z
            .rfind('.')
            .expect("timestamp must contain a decimal point");
        let frac = &before_z[dot_pos + 1..];
        assert_eq!(
            frac.len(),
            3,
            "expected exactly 3 fractional digits, got {}: {ts}",
            frac.len()
        );
        assert!(
            frac.chars().all(|c| c.is_ascii_digit()),
            "fractional part must be digits: {ts}"
        );
    }
}
