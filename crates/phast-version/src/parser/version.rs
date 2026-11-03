//! PHP version string parsing.
//!
//! ## Steps
//!
//! 1. Strip optional leading `v` or `V`
//! 2. Check for `dev-{branch}` -> Branch variant
//! 3. Check for `{rest}-dev` -> Branch(rest), always (including `9999999-dev`)
//! 4. Split on first `-` followed by alpha to separate numeric part from suffix
//! 5. If no hyphen suffix, scan for inline stability label (e.g. `1.0.0beta1`)
//! 6. Normalize segments to [u64; 4], padding with zeros
//! 7. Parse stability suffix and sequence number

use crate::stability::Stability;
use crate::version::{BRANCH_ALIAS_SENTINEL, NumericVersion, Version, VersionError};

/// Parse a PHP version string into a `Version`.
///
/// Accepts all formats seen in the wild:
/// `1.2.3`, `v1.2.3`, `1.2.3.4`, `1.0.0-beta1`, `1.0.0-beta.1`, `1.0.0beta1`,
/// `dev-main`, `dev-master`, `dev-feature/foo`, `feature-dev`, `9999999-dev`.
pub fn parse_version(input: &str) -> Result<Version, VersionError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(VersionError::Empty);
    }

    let input = input.strip_prefix(['v', 'V']).unwrap_or(input);

    // `dev-{branch}`: canonical branch form.
    if let Some(branch) = input.strip_prefix("dev-") {
        return Ok(Version::Branch(branch.to_string()));
    }

    // `{rest}-dev`: legacy branch form. Always treated as a branch,
    // including `9999999-dev` (the bleeding-edge alias).
    if let Some(prefix) = input.strip_suffix("-dev") {
        return Ok(Version::Branch(prefix.to_string()));
    }

    parse_numeric(input).map(Version::Numeric)
}

/// Parse a `branch-alias` input (e.g. `1.2.x-dev`)
/// into the top-of-series dev version it denotes.
pub fn parse_branch_alias(input: &str) -> Result<NumericVersion, VersionError> {
    let series = input
        .trim()
        .strip_suffix("-dev")
        .filter(|i| !i.is_empty())
        .ok_or_else(|| VersionError::Unrecognized(input.to_string()))?;

    // A series alias means the top of its series. `2.9` is shorthand for `2.9.x`,
    // so append the placeholder when it is implicit; this is what makes `2.9-dev`
    // satisfy a numeric `^2.9` rather than sorting below the stable `2.9.0`.
    let series = if series.contains(['x', 'X']) {
        series.to_owned()
    } else {
        format!("{series}.x")
    };

    let (segments, precision) = parse_segments(&series, Some(BRANCH_ALIAS_SENTINEL))?;

    Ok(NumericVersion {
        segments,
        stability: Stability::Dev,
        stability_seq: 0,
        precision,
    })
}

/// Parse a *declared* version pin, such as an `options.versions` entry or an
/// inline package's `version`.
///
/// A numeric branch-alias form (`1.2.x-dev`) denotes the top-of-series dev version,
/// so it satisfies a numeric constraint like `^1.2`; other forms (concrete versions,
/// `dev-<branch>` branches) defer to [`parse_version`], which would otherwise read
/// `1.2.x-dev` as a branch named `1.2.x`.
#[rustfmt::skip]
pub fn parse_declared_version(input: &str) -> Result<Version, VersionError> {
    match parse_branch_alias(input) {
        Ok(n)  => Ok(Version::Numeric(n)),
        Err(_) => parse_version(input),
    }
}

/// Parse a non-branch version string into a `NumericVersion`.
#[rustfmt::skip]
fn parse_numeric(input: &str) -> Result<NumericVersion, VersionError> {
    // Split `1.0.0-beta1` into `1.0.0` and `beta1`.
    let (numeric_part, suffix) = split_numeric_and_suffix(input);
    let (segments, precision)  = parse_segments(numeric_part, None)?;

    // Parse the stability suffix (e.g. `beta1` -> Stability::Beta, seq 1).
    let (stability, stability_seq) = parse_suffix(suffix);

    Ok(NumericVersion {
        segments,
        stability,
        stability_seq,
        precision,
    })
}

/// Parse a dot-separated numeric part into a fixed `[u64; 4]` plus the number
/// of segments present (the `precision`).
///
/// `placeholder` maps an `x`/`X` segment to a value, for branch-alias targets
/// like `1.2.x-dev`; `None` rejects any non-numeric segment, for plain versions.
#[rustfmt::skip]
fn parse_segments(numeric: &str, placeholder: Option<u64>) -> Result<([u64; 4], u8), VersionError> {
    let parts: Vec<&str> = numeric.split('.').collect();
    if parts.len() > 4 {
        return Err(VersionError::TooManySegments(numeric.to_string()));
    }

    // Fill a fixed-size [u64; 4] array, padding unused positions with zero.
    let mut segments = [0u64; 4];
    for (index, part) in parts.iter().enumerate() {
        segments[index] = match (placeholder, *part) {
            (Some(sentinel), "x" | "X")
              => sentinel,
            _ => part
                .parse::<u64>()
                .map_err(|_| VersionError::InvalidSegment(part.to_string()))?,
        };
    }

    // TooManySegments rejects len > 4 above,
    // so parts.len() fits in u8.
    Ok((segments, parts.len() as u8))
}

/// Split a version string into its numeric portion and optional stability suffix.
///
/// Handles two forms:
/// - Hyphen-separated: `1.0.0-beta1` -> (`1.0.0`, `beta1`)
/// - Inline:           `1.0.0beta1`  -> (`1.0.0`, `beta1`)
fn split_numeric_and_suffix(input: &str) -> (&str, &str) {
    // Hyphen followed by alpha: `1.0.0-beta1`, `1.0.0-RC2`.
    if let Some(position) = input.find('-') {
        let after = &input[position + 1..];
        if after.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return (&input[..position], after);
        }
    }

    // Inline suffix: `1.0.0beta1`, find where the digit/dot section ends
    // and an alphabetic character begins.
    if let Some(position) = input.find(|c: char| c.is_ascii_alphabetic()) {
        return (&input[..position], &input[position..]);
    }

    (input, "")
}

/// Parse a stability suffix string into `(Stability, sequence_number)`.
///
/// Handles: `beta1`, `beta.1`, `beta`, `RC2`, `alpha3`, `dev`, `patch`, `pl`.
///
/// A label it does not recognize falls back to `Stable` rather than failing, while
/// trailing digits are still read as the sequence. `1.0.0-xyz1` therefore becomes
/// a stable `1.0.0` carrying sequence 1, which does not compare equal to a plain
/// `1.0.0` and sorts above it.
fn parse_suffix(suffix: &str) -> (Stability, u32) {
    if suffix.is_empty() {
        return (Stability::Stable, 0);
    }

    // Dot-separated sequence: `beta.1`, try the part after the dot as a number.
    if let Some(dot_position) = suffix.rfind('.') {
        let (label, sequence) = match suffix[dot_position + 1..].parse::<u32>() {
            Ok(n) => (&suffix[..dot_position], n),
            Err(_) => (suffix, 0),
        };

        return (
            Stability::from_label(label).unwrap_or(Stability::Stable),
            sequence,
        );
    }

    // Inline digits: `beta1`, `RC2`, find where the alphabetic label ends.
    let label_end = suffix
        .rfind(|c: char| c.is_ascii_alphabetic())
        .map(|i| i + 1)
        .unwrap_or(suffix.len());

    let (label, sequence_str) = suffix.split_at(label_end);
    // Sequence numbers beyond u32::MAX are treated as 0;
    // no real package reaches this.
    let sequence = sequence_str.parse::<u32>().unwrap_or(0);

    (
        Stability::from_label(label).unwrap_or(Stability::Stable),
        sequence,
    )
}
