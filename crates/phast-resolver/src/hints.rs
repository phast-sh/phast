//! User-facing error messages for resolution failures.
//!
//! Centralized here so all resolver errors use the same wording and tone.

/// The message for requirements nothing could satisfy.
///
/// One name gets a single sentence; several get a counted list, so a wide
/// failure stays readable instead of running together into one paragraph.
pub fn unresolved_requirements(names: &[String]) -> String {
    if let [name] = names {
        return format!(
            "'{name}' was not resolved: no configured registry offers a version of it. \
             Check the name, the repositories you have configured, and your minimum-stability."
        );
    }

    let listed = names
        .iter()
        .map(|name| format!("\n  {name}"))
        .collect::<String>();

    format!(
        "{} requirements were not resolved:{listed}\n\n  \
         hint: check the names, the repositories you have configured, \
         and your minimum-stability.",
        names.len(),
    )
}
