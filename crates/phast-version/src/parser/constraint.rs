//! Constraint expression parsing.
//!
//! ## Steps
//!
//! 1. Split off a trailing `@stability` flag (`^2.0@dev`): a per-package stability
//!    override, ignored here and captured only by root requirements; a bare `@dev`
//!    (no version) means "any version at that stability"
//! 2. `*` -> `Any`
//! 3. Split on `||` / `|` (OR) -> parse each arm
//! 4. Hyphen range `1.0 - 2.0` -> inclusive lower bound, upper bound per right-side precision
//! 5. Split on `,` / whitespace (AND) -> parse each term
//! 6. Each term: `x` and `X` normalized to `*` first, then caret `^`, tilde `~`,
//!    wildcard `1.*`, comparison (`>=`, `<`, ...), branch `dev-*`, or exact version

use super::version::parse_version;

use crate::constraint::{Constraint, ConstraintError, Op};
use crate::stability::Stability;
use crate::version::{Version, VersionError};

/// Parse a constraint expression into a `Constraint`.
///
/// A trailing `@stability` flag (`^2.0@dev`, `>=1.0@RC`) is stripped and ignored:
/// it sets a per-package stability floor, not a version bound. Stripping it here,
/// before any operator dispatch, keeps every constraint form (caret, tilde,
/// wildcard, comparison, exact) tolerant of the flag. Where the flag matters
/// (root requirements) use [`parse_requirement`], which returns it.
pub fn parse_constraint(input: &str) -> Result<Constraint, ConstraintError> {
    let (input, flag) = split_stability_flag(input.trim());
    let input = input.trim();
    // A bare `@dev` means "any version at that stability";
    // an empty string with no flag is genuinely empty.
    if input.is_empty() {
        return match flag {
            Some(_) => Ok(Constraint::Any),
            None => Err(ConstraintError::Empty),
        };
    }

    if input == "*" {
        return Ok(Constraint::Any);
    }

    // Split on `||` or `|` (lowest precedence, both are valid OR separators).
    let or_arms: Vec<&str> = input
        .split("||")
        .flat_map(|s| s.split('|'))
        .map(str::trim)
        .collect();
    if or_arms.len() > 1 {
        let arms = or_arms
            .iter()
            .map(|arm| parse_constraint(arm))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Constraint::Or(arms));
    }

    // Hyphen range: `1.0 - 2.0` (space-dash-space separates two version tokens).
    // Lower bound is inclusive; upper bound is inclusive when the right side
    // specifies a patch segment, exclusive (next minor) otherwise.
    if let Some(idx) = input.find(" - ") {
        return parse_hyphen_range(input[..idx].trim(), input[idx + 3..].trim());
    }

    // Split on `,` or whitespace (AND, higher precedence than `||`).
    // Short-circuit for the common single-term case to avoid allocating a Vec.
    if !input.contains(',') && !input.contains(' ') {
        return parse_single_term(input);
    }

    let and_terms = split_and_terms(input);
    if and_terms.len() == 1 {
        return parse_single_term(input);
    }

    let terms = and_terms
        .iter()
        .map(|t| parse_single_term(t))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Constraint::And(terms))
}

/// Parse a requirement value (a `require` / `require-dev` entry) into
/// its constraint and any per-package `@stability` flag:
/// - `^2.0@dev` yields `(Caret(2.0), Some(Dev))`
/// - `^2.0` yields `(Caret(2.0), None)`
///
/// Same parse as [`parse_constraint`], but also returns the flag that
/// `parse_constraint` discards (root requirements).
pub fn parse_requirement(input: &str) -> Result<(Constraint, Option<Stability>), ConstraintError> {
    let (_, min_stability) = split_stability_flag(input.trim());
    Ok((parse_constraint(input)?, min_stability))
}

/// The stability a requirement implies by naming an unstable version, when it
/// carries no `@flag` of its own:
/// - `dev-master` and `2.x-dev` imply `Dev`
/// - `1.0.0-beta2` and `^1.0-beta` imply `Beta`
/// - `^1.0`, `>=1.0 <2.0` and `1.0 - 2.0` imply nothing
///
/// Read from the text term by term, never from the parsed constraint: parsing
/// gives an exclusive upper bound a `-dev` sentinel (`1.0 - 2.0` becomes
/// `<2.1-dev`), which would read as a dev requirement nobody wrote. The least
/// stable term wins.
pub fn implied_stability(input: &str) -> Option<Stability> {
    if input.contains('@') {
        return None;
    }

    input
        .split("||")
        .flat_map(|arm| arm.split('|'))
        .flat_map(|arm| arm.split([',', ' ']))
        .filter_map(parse_stability_term)
        .min()
        .filter(|stability| *stability != Stability::Stable)
}

/// The stability one term of a requirement names: a branch is `Dev`, anything
/// else the parsed version's own. A term that is not a version (an operator on
/// its own, a wildcard, the `-` of a hyphen range) names none.
fn parse_stability_term(input: &str) -> Option<Stability> {
    let term = input
        .trim_start_matches(is_constraint_operator)
        .to_ascii_lowercase();

    if term.starts_with("dev-") || term.ends_with("-dev") {
        return Some(Stability::Dev);
    }

    match parse_version(&term).ok()? {
        Version::Numeric(version) => Some(version.stability),
        Version::Branch(_) => Some(Stability::Dev),
    }
}

/// Parse a single constraint term with no `||` or AND separators.
///
/// Dispatches on the leading character(s) to the appropriate sub-parser:
/// `^`, `~`, `*`, comparison operators, `dev-` branches, or an exact version.
#[rustfmt::skip]
fn parse_single_term(input: &str) -> Result<Constraint, ConstraintError> {
    let map_err = |e: VersionError| ConstraintError::Invalid(
        input.to_string(),
        e.to_string(),
    );

    // Normalize the `x`/`X` wildcard alias to `*` before dispatch.
    // `5.3.x` and `5.3.X` are identical to `5.3.*`; bare `x`/`X` means any version.
    let owned;
    let input: &str = if input.ends_with(".x") || input.ends_with(".X") {
        owned = format!("{}.*", &input[..input.len() - 2]);
        &owned
    } else if input == "x" || input == "X" {
        "*"
    } else {
        input
    };

    // `^1.2` is a caret range: compatible with the given version up to the next major.
    if let Some(rest) = input.strip_prefix('^') {
        let version = parse_version(rest).map_err(map_err)?;
        return Ok(Constraint::Caret(version));
    }

    // `~1.2` is a tilde range: compatible up to the next minor (or major if no minor given).
    if let Some(rest) = input.strip_prefix('~') {
        let version = parse_version(rest).map_err(map_err)?;
        return Ok(Constraint::Tilde(version));
    }

    // `1.*` or `1.2.*` is a wildcard: any version matching the given prefix.
    if input.ends_with(".*") || input == "*" {
        return parse_wildcard(input);
    }

    // `>=1.0`, `<2.0`, `!=1.5` and friends are explicit comparison operators.
    // Longer prefixes are checked first to avoid `>` matching `>=`.
    for (prefix, op) in [
        (">=", Op::Gte),
        ("<=", Op::Lte),
        ("!=", Op::Ne),
        (">",  Op::Gt),
        ("<",  Op::Lt),
    ] {
        let Some(rest) = input.strip_prefix(prefix) else {
            continue;
        };

        let version = parse_version(rest.trim()).map_err(map_err)?;
        return Ok(Constraint::Comparison(op, version));
    }

    // `dev-main` and `dev-feature/foo` are explicit branch constraints.
    if let Some(branch) = input.strip_prefix("dev-") {
        return Ok(Constraint::Branch(branch.to_string()));
    }

    // Fallback: treat as an exact version. Any `@stability` flag was already
    // removed by `parse_constraint`, so the term is a bare version.
    let version = parse_version(input).map_err(map_err)?;
    Ok(Constraint::Exact(version))
}

/// Parse a wildcard constraint (`1.*` or `1.2.*`) into a `Constraint::Wildcard`.
fn parse_wildcard(input: &str) -> Result<Constraint, ConstraintError> {
    if input == "*" {
        return Ok(Constraint::Any);
    }

    // Strip the trailing `.*` to get the numeric prefix, e.g. `1.2.*` -> `1.2`.
    // The unwrap is safe: the caller only reaches here when input ends with `.*`.
    let without_star = input.strip_suffix(".*").unwrap();

    // Parse one numeric segment, attaching the original input to the error for context.
    let parse_segment = |s: &str, label: &str| -> Result<u64, ConstraintError> {
        s.parse::<u64>().map_err(|_| {
            ConstraintError::Invalid(input.to_string(), format!("invalid {label} segment"))
        })
    };

    // Match on the number of dot-separated parts to distinguish `1.*` from `1.2.*`.
    match without_star.split('.').collect::<Vec<_>>().as_slice() {
        [major] => Ok(Constraint::Wildcard {
            major: parse_segment(major, "major")?,
            minor: None,
        }),
        [major, minor] => Ok(Constraint::Wildcard {
            major: parse_segment(major, "major")?,
            minor: Some(parse_segment(minor, "minor")?),
        }),
        _ => Err(ConstraintError::Invalid(
            input.to_string(),
            "wildcard must be 1.* or 1.2.*".to_string(),
        )),
    }
}

/// Parse a hyphen range `A - B` into an AND of two comparison constraints.
///
/// Lower bound: `>=A` (always inclusive).
/// Upper bound: if B specifies a patch segment (three or more dot-separated numeric
/// parts), `<=B`; otherwise `<(B with next minor)`.
///
/// Examples:
/// - `1.0 - 2.0`     -> `>=1.0 <2.1-dev`  (right has no patch)
/// - `1.0.0 - 2.1.0` -> `>=1.0 <=2.1`     (right has patch)
/// - `8.1 - 8.5`     -> `>=8.1 <8.6-dev`
///
/// The exclusive bound carries a `-dev` sentinel, so `1.0 - 2.0` shuts out a
/// pre-release of 2.1 rather than admitting `2.1.0-beta1` for sorting below `2.1.0`.
#[rustfmt::skip]
fn parse_hyphen_range(left: &str, right: &str) -> Result<Constraint, ConstraintError> {
    let map_err = |e: VersionError| ConstraintError::Invalid(
        format!("{left} - {right}"),
        e.to_string(),
    );

    let lower = parse_version(left).map_err(map_err)?;
    let upper = parse_version(right).map_err(map_err)?;

    // Use the parsed precision rather than counting raw dots in the string.
    // Dot-notation stability suffixes (e.g. "2.0-beta.1") inflate the raw dot count.
    let upper_precision = match &upper {
        Version::Numeric(v) => v.precision,
        Version::Branch(_) => {
            return Err(ConstraintError::Invalid(
                format!("{left} - {right}"),
                "branch version is not valid as a hyphen range bound".to_string(),
            ));
        }
    };

    if upper_precision >= 3 {
        return Ok(Constraint::And(vec![
            Constraint::Comparison(Op::Gte, lower),
            Constraint::Comparison(Op::Lte, upper),
        ]));
    }

    // No patch segment (e.g. `2.0` or `2`): upper bound is exclusive `<` at the
    // next minor, so `1.0 - 2.0` becomes `>=1.0, <2.1`.
    let next = match &upper {
        Version::Numeric(v) => Version::Numeric(v.next_minor()),
        Version::Branch(_) => unreachable!(),
    };

    Ok(Constraint::And(vec![
        Constraint::Comparison(Op::Gte, lower),
        Constraint::Comparison(Op::Lt, next),
    ]))
}

/// Split a trailing `@stability` flag (e.g. `^2.0@dev`) off a constraint string,
/// returning the constraint expression (input minus the flag, still unparsed)
/// and the parsed stability. Returns `(input, None)` when there is no trailing `@`
/// followed by a recognized stability label.
///
/// The flag is a per-package stability override; `parse_constraint` strips
/// and ignores it here, and a root requirement captures it to apply it.
fn split_stability_flag(input: &str) -> (&str, Option<Stability>) {
    // No `@` means no stability flag; return the input unchanged.
    let Some(at_position) = input.rfind('@') else {
        return (input, None);
    };

    // Everything after `@` is the stability label, e.g. `beta` in `1.0@beta`.
    let label = &input[at_position + 1..];

    // If the label isn't a known stability, treat the `@` as part of the version
    // and return the input unchanged.
    let Some(stability) = Stability::from_label(label) else {
        return (input, None);
    };

    // Return the constraint text before `@` and the parsed stability separately.
    (&input[..at_position], Some(stability))
}

/// Split on `,` or whitespace (AND terms), ignoring spaces within a single
/// operator+version token like `>= 1.0`.
fn split_and_terms(input: &str) -> Vec<String> {
    // Comma-separated terms take priority: `>=1.0,<2.0` -> [`>=1.0`, `<2.0`].
    let by_comma: Vec<String> = input
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    if by_comma.len() > 1 {
        return by_comma;
    }

    // Fall back to whitespace splitting: `>=1.0 <2.0` -> [`>=1.0`, `<2.0`].
    // Bare comparison operators like `>=` or `<` may be separated from their
    // version by a space (e.g. `>= 1.0`). Re-join them into a single token.
    let mut terms: Vec<String> = Vec::new();
    let mut tokens = input.split_whitespace();
    while let Some(token) = tokens.next() {
        if !is_comparison_operator(token) {
            terms.push(token.to_string());
            continue;
        }

        // Consume the next token and merge: `>=` + `1.0` -> `>=1.0`.
        if let Some(next) = tokens.next() {
            terms.push(format!("{}{}", token, next));
        }
    }

    terms
}

/// Return `true` if `term` is a bare comparison operator token (`>=`, `<=`, etc.).
fn is_comparison_operator(term: &str) -> bool {
    matches!(term, ">=" | "<=" | "!=" | ">" | "<")
}

/// Return `true` if `character` belongs to a constraint operator: a comparison
/// (`>=`, `!=`, ...), or the caret and tilde ranges (`^`, `~`).
fn is_constraint_operator(character: char) -> bool {
    matches!(character, '<' | '>' | '=' | '!' | '^' | '~')
}
